"""Owned real-process editorial protocol tests; no model inference or downloads."""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKER = ROOT / "workers/python/editorial_worker.py"
PYTHON = ROOT / ".local/e5-editorial-venv/Scripts/python.exe"
PROTOCOL = "tv2-editorial/1"
MAX_LINE = 1024 * 1024
CAP = 256 * 1024 * 1024
OUTPUT = None
WORKER_SHA = None
REPORTS = []


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def child_main(args):
    forbidden = {"torch", "transformers", "numpy", "safetensors", "tokenizers"}
    attempts = []
    def audit(event, values):
        if event == "import" and values[0].split(".")[0] in forbidden:
            attempts.append(values[0])
            raise RuntimeError("Protocol test forbids ML import: " + values[0])
    sys.addaudithook(audit)
    source = WORKER.read_bytes()
    if hashlib.sha256(source).hexdigest() != args.worker_sha:
        raise RuntimeError("Worker changed before child execution")
    sys.argv = [str(WORKER), "--work-root", args.child_root]
    namespace = {"__file__": str(WORKER), "__name__": "__main__"}
    try:
        exec(compile(source, str(WORKER), "exec"), namespace)
    finally:
        loaded = sorted(name for name in sys.modules if name.split(".")[0] in forbidden)
        Path(args.child_root, "imports.json").write_text(json.dumps({"blocked_attempts": attempts, "loaded_ml_modules": loaded,
                                                                     "worker_sha256": args.worker_sha}), encoding="utf-8")


class Limits(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t), ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", wintypes.DWORD), ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD), ("SchedulingClass", wintypes.DWORD)]


class IoCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class ExtendedLimits(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", Limits), ("IoInfo", IoCounters), ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t), ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]


class OwnedWorker:
    def __init__(self, name, timeout=30):
        self.root = OUTPUT / name
        self.root.mkdir()
        self.start = time.monotonic()
        self.deadline = self.start + timeout
        self.report = {"case": name, "deadline_seconds": timeout, "job_memory_limit": CAP, "worker_sha256": WORKER_SHA, "peak_job_memory": 0}
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self.kernel.CreateJobObjectW.restype = wintypes.HANDLE
        self.kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        self.kernel.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
        self.kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.handle = self.kernel.CreateJobObjectW(None, None)
        self.process = self.reader = None
        self.stderr = self.stdout = None
        self.records = queue.Queue()
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())

    def __enter__(self):
        try:
            limits = ExtendedLimits()
            limits.BasicLimitInformation.LimitFlags = 0x2000 | 0x200
            limits.JobMemoryLimit = CAP
            if not self.kernel.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
                raise ctypes.WinError(ctypes.get_last_error())
            self.stderr = (self.root / "stderr.log").open("wb")
            self.stdout = (self.root / "stdout.ndjson").open("wb")
            self.process = subprocess.Popen([str(PYTHON), "-I", str(Path(__file__).resolve()), "--child-root", str(self.root),
                                             "--worker-sha", WORKER_SHA], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                            stderr=self.stderr, creationflags=subprocess.CREATE_NO_WINDOW)
            if not self.kernel.AssignProcessToJobObject(self.handle, wintypes.HANDLE(int(self.process._handle))):
                # No protocol was sent: this exact child cannot start an inference job.
                self.process.terminate()
                raise ctypes.WinError(ctypes.get_last_error())
            self.report["pid"] = self.process.pid
            def read():
                try:
                    while line := self.process.stdout.readline(MAX_LINE + 1):
                        self.stdout.write(line)
                        self.stdout.flush()
                        if len(line) > MAX_LINE:
                            raise RuntimeError("Oversized worker response")
                        self.records.put(json.loads(line))
                except Exception as error:
                    self.records.put(error)
                finally:
                    self.records.put(None)
            self.reader = threading.Thread(target=read, daemon=True)
            self.reader.start()
            return self
        except Exception:
            self.__exit__(*sys.exc_info())
            raise

    def sample(self):
        value = ExtendedLimits()
        if not self.kernel.QueryInformationJobObject(self.handle, 9, ctypes.byref(value), ctypes.sizeof(value), None):
            raise ctypes.WinError(ctypes.get_last_error())
        self.report["peak_job_memory"] = max(self.report["peak_job_memory"], value.PeakJobMemoryUsed)

    def raw(self, data):
        self.sample()
        self.process.stdin.write(data)
        self.process.stdin.flush()

    def send(self, method, params, request_id="request-1", **extra):
        value = {"protocol": PROTOCOL, "id": request_id, "method": method, "params": params, **extra}
        self.raw(json.dumps(value, separators=(",", ":")).encode() + b"\n")

    def receive(self):
        while time.monotonic() < self.deadline:
            self.sample()
            try:
                value = self.records.get(timeout=min(.1, max(.001, self.deadline-time.monotonic())))
            except queue.Empty:
                continue
            if isinstance(value, Exception):
                raise value
            if value is None:
                raise AssertionError("Unexpected worker EOF")
            if value.get("protocol") != PROTOCOL:
                raise AssertionError("Unexpected response protocol")
            return value
        raise TimeoutError("Owned worker deadline exceeded")

    def eof(self):
        if not self.process.stdin.closed:
            self.process.stdin.close()

    def clean_exit(self):
        self.process.wait(timeout=max(.001, self.deadline-time.monotonic()))
        self.sample()
        if self.process.returncode != 0:
            raise AssertionError("Worker exit code " + str(self.process.returncode))

    def shutdown(self):
        self.send("shutdown", {}, "shutdown")
        if self.receive() != {"protocol": PROTOCOL, "id": "shutdown", "result": {"shutdown": True}}:
            raise AssertionError("Missing shutdown acknowledgment")
        self.clean_exit()

    def __exit__(self, kind, error, traceback):
        self.report.update(accepted=kind is None, error=None if error is None else str(error))
        try:
            self.sample()
        finally:
            self.kernel.CloseHandle(self.handle)  # Only this owned process tree.
            if self.process is not None:
                self.process.wait(timeout=10)
                self.report["exit_code"] = self.process.returncode
                self.eof()
            if self.reader:
                self.reader.join(timeout=5)
            if self.process and self.process.stdout:
                self.process.stdout.close()
            if self.stdout:
                self.stdout.close()
            if self.stderr:
                self.stderr.close()
            self.report["duration_seconds"] = time.monotonic()-self.start
            imports = self.root / "imports.json"
            self.report["import_audit"] = json.loads(imports.read_text(encoding="utf-8")) if imports.exists() else None
            self.report["stderr_bytes"] = (self.root / "stderr.log").stat().st_size if self.stderr else None
            (self.root / "process.json").write_text(json.dumps(self.report, indent=2), encoding="utf-8")
            REPORTS.append(self.report)


class ProtocolTests(unittest.TestCase):
    def error(self, child, code, request_id):
        value = child.receive()
        self.assertEqual(value["id"], request_id)
        self.assertEqual(value["error"]["code"], code)
        return value

    def test_01_invalid_envelopes_recover_and_shutdown(self):
        with OwnedWorker("invalid-recovery") as child:
            child.raw(b'{invalid}\n')
            self.error(child, "E_EDITORIAL", None)
            child.send("unknown", {}, extra=True)
            self.error(child, "E_PROTOCOL", None)
            child.send("unknown", {}, "unknown")
            self.error(child, "E_METHOD", "unknown")
            child.send("shutdown", {"extra": True}, "invalid-shutdown")
            self.error(child, "E_ARGUMENT", "invalid-shutdown")
            child.send("cancel", {"job_id": "job-000000000000"}, "no-job")
            self.error(child, "E_ARGUMENT", "no-job")
            child.send("run", {"job_id": "job-000000000000"}, "uninitialized")
            self.error(child, "E_PROTOCOL", "uninitialized")
            child.raw(b'{"protocol":"tv2-editorial/1","id":"duplicate","id":"other","method":"shutdown","params":{}}\n')
            self.error(child, "E_JSON", None)
            child.raw(b'{"protocol":"tv2-editorial/1","id":"nan","method":"unknown","params":{"x":NaN}}\n')
            self.error(child, "E_EDITORIAL", None)
            child.send("unknown", {}, "bad-protocol", protocol="other")
            self.error(child, "E_PROTOCOL", "bad-protocol")
            child.shutdown()

    def test_02_fragmented_line_does_not_dispatch_before_newline(self):
        with OwnedWorker("fragmented") as child:
            line = json.dumps({"protocol": PROTOCOL, "id": "fragment", "method": "unknown", "params": {}}).encode()
            child.raw(line[:20])
            with self.assertRaises(queue.Empty):
                child.records.get(timeout=.15)
            self.assertIsNone(child.process.poll())
            child.raw(line[20:]+b"\n")
            self.error(child, "E_METHOD", "fragment")
            child.shutdown()

    def test_03_exact_line_limit_is_accepted(self):
        with OwnedWorker("exact-limit") as child:
            line = json.dumps({"protocol": PROTOCOL, "id": "boundary", "method": "unknown", "params": {}}).encode()
            child.raw(line + b" "*(MAX_LINE-len(line)-1) + b"\n")
            self.error(child, "E_METHOD", "boundary")
            child.shutdown()

    def test_04_oversized_unterminated_line_errors_and_exits(self):
        with OwnedWorker("oversized") as child:
            child.raw(b" "*(MAX_LINE+1))
            self.error(child, "E_PROTOCOL", None)
            child.clean_exit()

    def test_05_incomplete_eof_rejected_and_exits(self):
        with OwnedWorker("incomplete-eof") as child:
            child.raw(b'{"protocol":"tv2-editorial/1","id":"partial","method":"shutdown","params":{}}')
            child.eof()
            self.error(child, "E_PROTOCOL", None)
            child.clean_exit()

    def test_06_live_idle_eof_exits_without_response(self):
        with OwnedWorker("idle-eof") as child:
            time.sleep(.15)
            self.assertIsNone(child.process.poll())
            child.eof()
            child.clean_exit()
            self.assertIsNone(child.records.get(timeout=5))

    def test_07_real_hello_cancel_identity_and_live_eof(self):
        with OwnedWorker("hello-cancel-eof", timeout=120) as child:
            hello = {"model": str(ROOT / ".local/models/qwen2.5-1.5b-instruct"),
                     "model_manifest": str(ROOT / ".local/models/qwen2.5-1.5b-instruct/model-manifest.json"),
                     "lock": str(ROOT / "workers/python/requirements-editorial.lock"), "python_path": str(PYTHON)}
            child.send("hello", hello, "hello")
            response = child.receive()
            self.assertEqual(response["id"], "hello")
            result = response["result"]
            self.assertTrue(result["local_only"])
            self.assertEqual(result["identity"]["worker_sha256"], WORKER_SHA)
            self.assertEqual(set(result["tasks"]), {"topics", "layers", "trims", "montage"})
            child.report["hello_identity"] = result["identity"]
            # Every post-hello action has its own bounded 30 s window.
            child.deadline = time.monotonic()+30
            job_id = "job-012345abcdef"
            (child.root / job_id).mkdir()
            params = {"job_id": job_id, "project_id": "synthetic-protocol-only", "revision": 0, "project_digest": "a"*64,
                      "asset_id": "synthetic-asset", "input_digest": "b"*64, "kind": "topics", "request_id": "request-test",
                      "request_digest": "c"*64, "pass": 1, "documents": {}, "backend": result["identity"],
                      "model": hello["model"], "model_manifest": hello["model_manifest"],
                      "parameters": {"temperature": 0.0, "seed": 0, "max_tokens": 1, "timeout_seconds": 1, "trim_mode": "content"},
                      "output": "editorial/proposal.json"}
            # Empty documents prevent inference even if rehash finishes before cancel arrives.
            child.send("run", params, "run")
            child.send("cancel", {"job_id": "job-ffffffffffff"}, "wrong-cancel")
            child.send("cancel", {"job_id": job_id}, "cancel")
            child.send("run", params, "second-run")
            child.send("hello", hello, "late-hello")
            received = {}
            for _ in range(5):
                value = child.receive()
                received[value["id"]] = value
            self.assertEqual(received["wrong-cancel"]["error"]["code"], "E_ARGUMENT")
            self.assertEqual(received["cancel"]["result"], {"job_id": job_id, "cancel_requested": True})
            self.assertEqual(received["run"]["error"]["code"], "E_CANCELLED")
            self.assertEqual(received["second-run"]["error"]["code"], "E_PROTOCOL")
            self.assertEqual(received["late-hello"]["error"]["code"], "E_PROTOCOL")
            child.send("cancel", {"job_id": job_id}, "completed-cancel")
            self.assertEqual(child.receive()["result"], {"job_id": job_id, "cancel_requested": False})
            self.assertIsNone(child.process.poll())
            child.eof()
            child.clean_exit()
            self.assertFalse((child.root / job_id / "editorial/proposal.json").exists())


def main():
    global OUTPUT, WORKER_SHA
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    parser.add_argument("--child-root")
    parser.add_argument("--worker-sha")
    args = parser.parse_args()
    if args.child_root:
        child_main(args)
        return
    if os.name != "nt":
        parser.error("These real Windows pipe/JobObject tests require Windows")
    if args.output is None:
        parser.error("--output must be a fresh owned V2 evidence directory")
    OUTPUT = args.output.resolve()
    allowed = (ROOT / ".local", ROOT / "implementation/evidence/e2/continuacion-14")
    if not any(OUTPUT.is_relative_to(path) and OUTPUT != path for path in allowed) or OUTPUT.exists():
        parser.error("Use a fresh directory inside the owned V2 evidence scope")
    OUTPUT.mkdir(parents=True)
    WORKER_SHA = digest(WORKER)
    started = time.monotonic()
    with (OUTPUT / "tests.log").open("w", encoding="utf-8") as log:
        result = unittest.TextTestRunner(stream=log, verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(ProtocolTests))
    audit_ok = all(value["import_audit"] is not None and not value["import_audit"]["blocked_attempts"]
                   and not value["import_audit"]["loaded_ml_modules"] and value["stderr_bytes"] == 0 for value in REPORTS)
    same_worker = digest(WORKER) == WORKER_SHA
    report = {"accepted": result.wasSuccessful() and audit_ok and same_worker, "tests_run": result.testsRun,
              "failures": len(result.failures), "errors": len(result.errors), "no_ml_imports": audit_ok,
              "worker_sha256": WORKER_SHA, "worker_unchanged": same_worker, "script_sha256": digest(__file__),
              "duration_seconds": time.monotonic()-started, "processes": REPORTS,
              "inference_executed": False, "downloads": False, "gui_exercised": False}
    (OUTPUT / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "processes"}), flush=True)
    raise SystemExit(0 if report["accepted"] else 1)


if __name__ == "__main__":
    main()
