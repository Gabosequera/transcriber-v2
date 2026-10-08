"""MMS contract tests and explicit real alignment of public synthetic SAPI.

Known transcript fixture is invented text, not ASR. V1 is never executed.
"""
import argparse
import copy
import ctypes
from ctypes import wintypes
import importlib.util
import json
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import unittest
import wave

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("alignment_worker", REPO / "workers/python/alignment_worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
OPTIONS = None
PARAMS = None
LOG = []
MEMORY = []
TEXT = "This is a synthetic recording for a local software test. The blue notebook is on the table. We will meet tomorrow at nine."


class Client:
    def __init__(self, root):
        self.child = subprocess.Popen([sys.executable, "-I", "-u", str(REPO / "workers/python/alignment_worker.py"), "--work-root", str(root)],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.queue = queue.Queue()
        self.stderr = bytearray()
        def read():
            for line in self.child.stdout:
                try:
                    self.queue.put(json.loads(line))
                except Exception as error:
                    self.queue.put(error)
            self.queue.put(EOFError("Alignment worker closed"))
        def diagnostic():
            while block := self.child.stderr.read(4096):
                self.stderr.extend(block)
                del self.stderr[:-16384]
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        self.diagnostics = threading.Thread(target=diagnostic, daemon=True)
        self.diagnostics.start()

    def record_memory(self):
        if sys.platform != "win32":
            return
        class Counters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [(name, ctypes.c_size_t) for name in
                         ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
        class Entry(ctypes.Structure):
            _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("pid", wintypes.DWORD),
                        ("heap", ctypes.c_size_t), ("module", wintypes.DWORD), ("threads", wintypes.DWORD),
                        ("parent", wintypes.DWORD), ("priority", wintypes.LONG), ("flags", wintypes.DWORD), ("exe", wintypes.WCHAR*260)]
        kernel.CreateToolhelp32Snapshot.argtypes = (wintypes.DWORD, wintypes.DWORD)
        kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        kernel.Process32FirstW.argtypes = (wintypes.HANDLE, ctypes.POINTER(Entry))
        kernel.Process32NextW.argtypes = (wintypes.HANDLE, ctypes.POINTER(Entry))
        snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
        if snapshot == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        parents = {}
        try:
            entry = Entry()
            entry.dwSize = ctypes.sizeof(entry)
            more = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
            while more:
                parents[entry.pid] = {"parent": entry.parent, "executable": entry.exe}
                more = kernel.Process32NextW(snapshot, ctypes.byref(entry))
        finally:
            kernel.CloseHandle(snapshot)
        owned = {self.child.pid}
        while children := {pid for pid, info in parents.items() if info["parent"] in owned} - owned:
            owned |= children
        query = ctypes.WinDLL("psapi", use_last_error=True).GetProcessMemoryInfo
        query.argtypes = (wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD)
        query.restype = wintypes.BOOL
        for pid in sorted(owned):
            handle = kernel.OpenProcess(0x0400 | 0x0010, False, pid)
            if not handle:
                raise ctypes.WinError(ctypes.get_last_error())
            try:
                counters = Counters()
                counters.cb = ctypes.sizeof(counters)
                if not query(handle, ctypes.byref(counters), counters.cb):
                    raise ctypes.WinError(ctypes.get_last_error())
                MEMORY.append({"pid": pid, "root_pid": self.child.pid, **parents.get(pid, {}), "peak_working_set_bytes": counters.PeakWorkingSetSize,
                               "working_set_bytes": counters.WorkingSetSize, "peak_pagefile_bytes": counters.PeakPagefileUsage})
            finally:
                kernel.CloseHandle(handle)

    def send(self, identifier, method, params=None):
        self.child.stdin.write(worker.canonical({"protocol": worker.PROTOCOL, "id": identifier, "method": method, "params": params or {}})+b"\n")
        self.child.stdin.flush()

    def reply(self, identifier, event=None, timeout=180):
        deadline = time.monotonic()+timeout
        while True:
            value = self.queue.get(timeout=max(0.1, deadline-time.monotonic()))
            if isinstance(value, Exception):
                raise value
            if value.get("protocol") != worker.PROTOCOL:
                raise AssertionError("Invalid alignment protocol")
            if value.get("event"):
                LOG.append(value)
                if event:
                    event(value)
                continue
            if value.get("id") == "cancel":
                LOG.append(value)
                continue
            if value.get("id") != identifier:
                raise AssertionError("Uncorrelated alignment reply: " + repr(value))
            LOG.append(value)
            return value

    def initialize(self):
        self.send("hello", "hello")
        reply = self.reply("hello")
        assert "result" in reply, reply
        return reply["result"]

    def close(self):
        if self.child.poll() is None:
            try:
                self.send("shutdown", "shutdown")
                self.reply("shutdown", timeout=10)
                self.child.stdin.close()
                self.child.wait(timeout=7)
            except Exception:
                self.child.kill()
                self.child.wait(timeout=10)
        self.reader.join(timeout=2)
        self.diagnostics.join(timeout=2)
        for stream in (self.child.stdin, self.child.stdout, self.child.stderr):
            stream.close()


class IndependentTests(unittest.TestCase):
    def test_01_v1_normalization_golden(self):
        self.assertEqual(worker.normalize("¡ÑÁndú café!"), "nandu cafe".replace(" ", ""))
        self.assertEqual(worker.normalize("L'été don't 123"), "l'etedon't")
        self.assertEqual(worker.normalize("123… 😁"), "")
        self.assertNotIn("torch", sys.modules)
        self.assertNotIn("torchaudio", sys.modules)

    def test_02_bounded_batches_and_anomalous_word(self):
        words = [{"t_ini": 0, "t_fin": 10}, {"t_ini": 10, "t_fin": 29}, {"t_ini": 30, "t_fin": 40}]
        groups = list(worker.batches(words, PARAMS["parameters"], 40, threading.Event()))
        self.assertEqual([len(group[1]) for group in groups], [2, 1])
        self.assertLessEqual(max(end-start for _, _, start, end in groups), 31)
        with self.assertRaises(worker.AlignmentError):
            list(worker.batches([{"t_ini": 0, "t_fin": 31}], PARAMS["parameters"], 31, threading.Event()))

    def test_03_interpolation_fallback_ids_and_original(self):
        words = [{"word_id": str(index), "text": text, "t_ini": index, "t_fin": index+0.5, "probability": 0.7}
                 for index, text in enumerate(("uno", "123", "tres"))]
        before = copy.deepcopy(words)
        output = worker.apply_times(words, {0: (0.1, 0.4), 2: (2.1, 2.4)})
        self.assertEqual(output[1]["t_ini"], 0.4)
        self.assertEqual(output[1]["t_fin"], 1.25)
        self.assertEqual(output[1]["alignment_source"], "mms_interpolated")
        self.assertEqual(output[1]["asr_original"], before[1])
        self.assertEqual(words, before)
        self.assertEqual([word["word_id"] for word in output], ["0", "1", "2"])
        self.assertTrue(all(word["alignment_source"] == "whisper_fallback" for word in worker.apply_times(words, {}, "ctc failure")))
        self.assertTrue(all(word["alignment_source"] == "whisper_unalignable" for word in worker.apply_times(words, {})))

    def test_04_schema_shape_and_cancel_before_native(self):
        invalid = copy.deepcopy(PARAMS)
        invalid["parameters"]["threads"] = 4
        with self.assertRaises(worker.AlignmentError):
            worker.validate_params(invalid)
        cancelled = threading.Event()
        cancelled.set()
        with self.assertRaises(worker.AlignmentError) as failed:
            worker.Analysis(OPTIONS.work_root / "pre-cancel", PARAMS, cancelled, LOG.append).run()
        self.assertEqual(failed.exception.code, "E_CANCELLED")
        self.assertNotIn("torch", sys.modules)

    def test_05_hello_and_schema_errors_ndjson_real_process(self):
        client = Client(OPTIONS.work_root / "hello")
        try:
            client.send("nohello", "run", PARAMS)
            self.assertEqual(client.reply("nohello")["error"]["code"], "E_PROTOCOL")
            hello = client.initialize()
            self.assertEqual(hello["runtime"]["dependencies"]["torch"], "2.8.0+cpu")
            self.assertEqual(hello["capabilities"]["forced_alignment"]["runtime_state"], "not_loaded")
            self.assertFalse(hello["capabilities"]["asr"])
            client.send("invalid", "run", {"job_id": "../escape"})
            self.assertEqual(client.reply("invalid")["error"]["code"], "E_ARGUMENT")
        finally:
            client.close()

    def test_06_input_hash_failure_before_native(self):
        params = copy.deepcopy(PARAMS)
        params["normalized_audio_sha256"] = "0"*64
        with self.assertRaises(worker.AlignmentError) as failed:
            worker.Analysis(OPTIONS.work_root / "hash-error", params, threading.Event(), LOG.append).run()
        self.assertEqual(failed.exception.code, "E_PRECONDITION")
        self.assertNotIn("torch", sys.modules)

    def test_07_source_paths_boundaries_and_word_shape(self):
        with self.assertRaises(worker.AlignmentError):
            worker.inside(OPTIONS.work_root, "../escape")
        track = json.loads(Path(PARAMS["transcript_path"]).read_bytes())
        duplicate = copy.deepcopy(track)
        duplicate["words"][1]["word_id"] = duplicate["words"][0]["word_id"]
        with self.assertRaises(worker.AlignmentError):
            worker.validate_track(duplicate, 10)

    def test_08_unalignable_stage_real_cache_corruption_and_cancel_process(self):
        params = copy.deepcopy(PARAMS)
        params["job_id"] = "unalignable-stdlib"
        track = json.loads(Path(params["transcript_path"]).read_bytes())
        for word in track["words"]:
            word["text"] = "123"
        transcript = OPTIONS.work_root / "unalignable-transcript.json"
        worker.atomic_json(transcript, track)
        params["transcript_path"], params["transcript_sha256"] = str(transcript), worker.file_hash(transcript, threading.Event())
        analysis = worker.Analysis(OPTIONS.work_root / "no-native", params, threading.Event(), LOG.append)
        receipt = analysis.run()
        path = analysis.root / worker.OUTPUT
        aligned = json.loads(path.read_bytes())
        self.assertEqual(aligned["alignment"]["execution_state"], "no_alignable_words")
        self.assertEqual(aligned["alignment"]["aligned_words"], 0)
        self.assertEqual(analysis.run(), receipt)
        path.write_text('{"altered":true}', encoding="utf-8")
        self.assertEqual(analysis.run(), receipt)
        manifest_path = analysis.root / ".work/manifests/alignment.json"
        manifest = json.loads(manifest_path.read_bytes())
        manifest["artifacts"] = {"unalignable-transcript.json": params["transcript_sha256"]}
        worker.atomic_json(manifest_path, manifest)
        self.assertEqual(analysis.run(), receipt)
        self.assertEqual(set(json.loads(manifest_path.read_bytes())["artifacts"]), {worker.OUTPUT})
        self.assertNotIn("torch", sys.modules)
        client = Client(OPTIONS.work_root / "cancel-process")
        try:
            client.initialize()
            sent = []
            def event(value):
                if value["stage"] == "verify-inputs" and not sent:
                    client.send("cancel", "cancel", {"job_id": PARAMS["job_id"]})
                    sent.append(True)
            client.send("run", "run", PARAMS)
            self.assertEqual(client.reply("run", event=event)["error"]["code"], "E_CANCELLED")
            self.assertTrue(sent)
            self.assertFalse((OPTIONS.work_root / "cancel-process" / PARAMS["job_id"] / worker.OUTPUT).exists())
        finally:
            client.close()


    def test_09_pipe_partial_multiple_oversize_and_incomplete_eof(self):
        client = Client(OPTIONS.work_root / "partial")
        try:
            envelope = worker.canonical({"protocol": worker.PROTOCOL, "id": "hello", "method": "hello", "params": {}})+b"\n"
            client.child.stdin.write(envelope[:12])
            client.child.stdin.flush()
            time.sleep(.08)
            self.assertTrue(client.queue.empty())
            client.child.stdin.write(envelope[12:]+worker.canonical({"protocol": worker.PROTOCOL, "id": "bad", "method": "unknown", "params": {}})+b"\n")
            client.child.stdin.flush()
            self.assertIn("result", client.reply("hello"))
            self.assertEqual(client.reply("bad")["error"]["code"], "E_METHOD")
        finally:
            client.close()
        client = Client(OPTIONS.work_root / "incomplete")
        client.child.stdin.write(b'{"unfinished":')
        client.child.stdin.close()
        self.assertEqual(client.reply(None)["error"]["code"], "E_LIMIT")
        self.assertEqual(client.child.wait(timeout=5), 0)
        client.close()
        client = Client(OPTIONS.work_root / "oversize")
        client.child.stdin.write(b"x"*(worker.MAX_LINE+1))
        client.child.stdin.flush()
        self.assertEqual(client.reply(None)["error"]["code"], "E_LIMIT")
        self.assertEqual(client.child.wait(timeout=5), 0)
        client.close()


class RealTests(unittest.TestCase):
    def test_08_real_mms_known_sapi_transcript_resume_corruption(self):
        client = Client(OPTIONS.work_root / "real")
        try:
            client.initialize()
            client.send("run", "run", PARAMS)
            reply = client.reply("run", timeout=180)
            self.assertIn("result", reply, reply)
            receipt = reply["result"]
            client.record_memory()
            path = OPTIONS.work_root / "real" / PARAMS["job_id"] / receipt["alignment_path"]
            aligned = json.loads(path.read_bytes())
            source = json.loads(Path(PARAMS["transcript_path"]).read_bytes())
            self.assertEqual([word["word_id"] for word in aligned["words"]], [word["word_id"] for word in source["words"]])
            self.assertGreater(aligned["alignment"]["aligned_words"], 0)
            self.assertEqual(aligned["alignment"]["failed_batches"], [])
            self.assertEqual(aligned["alignment"]["execution_state"], "completed")
            self.assertTrue(all(word["asr_original"] == original for word, original in zip(aligned["words"], source["words"])))
            self.assertTrue(any((word["t_ini"], word["t_fin"]) != (original["t_ini"], original["t_fin"]) for word, original in zip(aligned["words"], source["words"])))
            for artifact, sha in receipt["artifacts"].items():
                self.assertEqual(worker.file_hash(OPTIONS.work_root / "real" / PARAMS["job_id"] / artifact, threading.Event()), sha)
            client.send("resume", "run", PARAMS)
            self.assertEqual(client.reply("resume")["result"], receipt)
            self.assertTrue(any(event.get("cached") and event.get("stage") == "alignment" for event in LOG))
            # Hash validation rejects an altered artifact without native reload.
            # Do not re-run full inference solely to repair this test artifact.
            path.write_text('{"altered":true}', encoding="utf-8")
            manifest = json.loads((path.parent.parent / ".work/manifests/alignment.json").read_bytes())
            self.assertNotEqual(worker.file_hash(path, threading.Event()), manifest["artifacts"][worker.OUTPUT])
            worker.atomic_json(path, aligned)
            self.assertEqual(worker.file_hash(path, threading.Event()), manifest["artifacts"][worker.OUTPUT])
        finally:
            client.close()
        self.assertEqual(worker.file_hash(PARAMS["normalized_audio_path"], threading.Event()), PARAMS["normalized_audio_sha256"])
        self.assertEqual(worker.file_hash(PARAMS["transcript_path"], threading.Event()), PARAMS["transcript_sha256"])


def main():
    global OPTIONS, PARAMS
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", choices=("independent", "real"), default="independent")
    parser.add_argument("--work-root", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    OPTIONS = parser.parse_args()
    OPTIONS.work_root, OPTIONS.evidence = OPTIONS.work_root.resolve(), OPTIONS.evidence.resolve()
    if OPTIONS.work_root.exists() or not OPTIONS.work_root.is_relative_to(REPO) or not OPTIONS.evidence.is_relative_to(REPO):
        parser.error("New V2 work/evidence paths required")
    OPTIONS.work_root.mkdir(parents=True)
    audio = REPO / ".local/e5-python-tests-20260930/fixtures/speech-a.wav"
    manifest = REPO / ".local/models/mms-fa/model-manifest.json"
    metadata = json.loads(manifest.read_bytes())
    with wave.open(str(audio), "rb") as waveform:
        duration = waveform.getnframes()/waveform.getframerate()
    tokens = TEXT.split()
    track = {"track_id": "a0", "words": [{"word_id": f"a0-w-{index+1:06d}", "track_id": "a0", "text": token,
                                           "t_ini": index*duration/len(tokens), "t_fin": (index+1)*duration/len(tokens),
                                           "probability": None, "fixture": "known invented transcript; uniformly placed initial times; not ASR"} for index, token in enumerate(tokens)]}
    transcript = OPTIONS.work_root / "known-transcript.json"
    worker.atomic_json(transcript, track)
    PARAMS = {"job_id": "mms-known-sapi", "project_id": "mms-fixture", "revision": 0, "project_digest": worker.digest({"fixture": "MMS known transcript"}),
              "asset_id": "sapi-audio", "normalized_audio_path": str(audio), "normalized_audio_sha256": worker.file_hash(audio, threading.Event()),
              "source_sha256": worker.file_hash(audio, threading.Event()), "source_duration_ticks": round(duration*worker.FLICKS), "timebase": "flicks/705600000",
              "transcript_path": str(transcript), "transcript_sha256": worker.file_hash(transcript, threading.Event()),
              "model_path": str(manifest.with_name("model.pt")), "manifest_path": str(manifest), "model_sha256": metadata["sha256"],
              "parameters": {"device": "cpu", "threads": 2, "batch_words": 120, "margin": 0.5, "max_batch_seconds": 30}}
    before = time.monotonic()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(IndependentTests)
    if OPTIONS.suite == "real":
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(RealTests))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    report = {"protocol": worker.PROTOCOL, "suite": OPTIONS.suite, "asr_exercised": False, "gui_exercised": False,
              "real_alignment_requested": OPTIONS.suite == "real", "known_transcript": TEXT, "parameters": PARAMS,
              "tests_run": result.testsRun, "success": result.wasSuccessful(), "elapsed_seconds": time.monotonic()-before,
              "worker_sha256": worker.file_hash(REPO / "workers/python/alignment_worker.py", threading.Event()),
              "failures": [(str(test), trace) for test, trace in result.failures+result.errors], "events": LOG, "process_memory": MEMORY}
    worker.atomic_json(OPTIONS.evidence, report)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
