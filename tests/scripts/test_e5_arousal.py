"""Independent contracts and optional genuine audEERING inference on SAPI.

Known invented transcript has real MMS alignment; it is never described as ASR.
No test substitutes a fictional ML model for acceptance inference.
"""
import argparse
import copy
import ctypes
from ctypes import wintypes
import importlib.util
import json
import math
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import wave

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("arousal_worker", REPO/"workers/python/arousal_worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
LOG, MEMORY = [], []
DEADLINE = 120
TRACE_AFTER = None


def memory_tree(root_pid):
    if sys.platform != "win32":
        return
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    class Entry(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD), ("pid", wintypes.DWORD), ("heap", ctypes.c_size_t),
                    ("module", wintypes.DWORD), ("threads", wintypes.DWORD), ("parent", wintypes.DWORD),
                    ("priority", wintypes.LONG), ("flags", wintypes.DWORD), ("exe", wintypes.WCHAR*260)]
    class Counters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("faults", wintypes.DWORD)] + [(name, ctypes.c_size_t) for name in
                   ("peak", "current", "peak_paged", "paged", "peak_nonpaged", "nonpaged", "pagefile", "peak_pagefile")]
    kernel.CreateToolhelp32Snapshot.argtypes = (wintypes.DWORD, wintypes.DWORD)
    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel.Process32FirstW.argtypes = (wintypes.HANDLE, ctypes.POINTER(Entry))
    kernel.Process32NextW.argtypes = (wintypes.HANDLE, ctypes.POINTER(Entry))
    kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel.OpenProcess.restype = wintypes.HANDLE
    snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
    if snapshot == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    parents = {}
    try:
        entry = Entry()
        entry.size = ctypes.sizeof(entry)
        more = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            parents[entry.pid] = {"parent": entry.parent, "executable": entry.exe}
            more = kernel.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    owned = {root_pid}
    while children := {pid for pid, info in parents.items() if info["parent"] in owned}-owned:
        owned |= children
    query = ctypes.WinDLL("psapi", use_last_error=True).GetProcessMemoryInfo
    query.argtypes = (wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD)
    query.restype = wintypes.BOOL
    for pid in sorted(owned):
        handle = kernel.OpenProcess(0x0400 | 0x0010, False, pid)
        if not handle:
            continue  # Process can finish between the snapshot and OpenProcess.
        try:
            value = Counters()
            value.cb = ctypes.sizeof(value)
            if query(handle, ctypes.byref(value), value.cb):
                MEMORY.append({"pid": pid, "root_pid": root_pid, **parents.get(pid, {}), "peak_working_set_bytes": value.peak,
                               "working_set_bytes": value.current, "peak_pagefile_bytes": value.peak_pagefile})
        finally:
            kernel.CloseHandle(handle)


class Client:
    def __init__(self, root):
        command = [sys.executable, "-I", "-u", str(REPO/"workers/python/arousal_worker.py"), "--work-root", str(root)]
        if TRACE_AFTER:
            command += ["--diagnostic-trace-after", str(TRACE_AFTER)]
        self.child = subprocess.Popen(command,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.queue, self.stderr = queue.Queue(), bytearray()
        def read():
            for line in self.child.stdout:
                try:
                    self.queue.put(json.loads(line))
                except Exception as error:
                    self.queue.put(error)
            self.queue.put(EOFError("Arousal worker closed"))
        def diagnostic():
            while block := self.child.stderr.read(4096):
                self.stderr.extend(block)
                del self.stderr[:-32768]
        self.reader, self.diagnostics = threading.Thread(target=read, daemon=True), threading.Thread(target=diagnostic, daemon=True)
        self.reader.start()
        self.diagnostics.start()

    def send(self, identifier, method, params=None):
        self.child.stdin.write(worker.canonical({"protocol": worker.PROTOCOL, "id": identifier, "method": method, "params": params or {}})+b"\n")
        self.child.stdin.flush()

    def reply(self, identifier, event=None, timeout=None):
        timeout = DEADLINE if timeout is None else timeout
        deadline = time.monotonic()+timeout
        while time.monotonic() < deadline:
            memory_tree(self.child.pid)
            try:
                value = self.queue.get(timeout=min(.2, max(.001, deadline-time.monotonic())))
            except queue.Empty:
                continue
            if isinstance(value, Exception):
                raise value
            assert value.get("protocol") == worker.PROTOCOL, value
            LOG.append(value)
            if value.get("event"):
                if event:
                    event(value)
                continue
            if value.get("id") == "cancel":
                continue
            assert value.get("id") == identifier, value
            return value
        raise TimeoutError("Arousal worker deadline")

    def close(self):
        handles = []
        kernel = None
        if sys.platform == "win32":
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
            kernel.OpenProcess.restype = wintypes.HANDLE
            kernel.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
            kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
            for pid in sorted({item["pid"] for item in MEMORY if item["root_pid"] == self.child.pid}):
                handle = kernel.OpenProcess(0x100000, False, pid)
                if handle:
                    handles.append((pid, handle))
        if self.child.poll() is None:
            try:
                self.send("shutdown", "shutdown")
                self.reply("shutdown", timeout=10)
                self.child.stdin.close()
                self.child.wait(timeout=7)
            except Exception:
                # Venv launcher can have a native child; terminate only own tree.
                subprocess.run(["taskkill", "/PID", str(self.child.pid), "/T", "/F"], capture_output=True, timeout=10)
                self.child.wait(timeout=10)
        self.reader.join(timeout=2)
        self.diagnostics.join(timeout=2)
        for stream in (self.child.stdin, self.child.stdout, self.child.stderr):
            stream.close()
        signaled = []
        try:
            for pid, handle in handles:
                done = kernel.WaitForSingleObject(handle, 2000) == 0
                signaled.append({"pid": pid, "handle_signaled": done})
            LOG.append({"owned_process_exit_handles": signaled})
            assert all(item["handle_signaled"] for item in signaled), signaled
        finally:
            for _, handle in handles:
                kernel.CloseHandle(handle)


class Independent(unittest.TestCase):
    def test_01_lazy_import(self):
        self.assertNotIn("torch", sys.modules)
        self.assertNotIn("transformers", sys.modules)
        self.assertNotIn("av", sys.modules)

    def test_02_regions_merge_pad_cap(self):
        words = [{"t_ini": 4, "t_fin": 4.5}, {"t_ini": 1, "t_fin": 2}, {"t_ini": 2.9, "t_fin": 3}]
        self.assertEqual(worker.speech_regions(words, duration=4.7), [(0.5, 4.7)])
        self.assertEqual(worker.speech_regions([], duration=3), [])

    def test_03_global_hop_grid_and_empty(self):
        self.assertEqual(worker.window_offsets(160000, 32000, [(2.7, 3.3), (3.1, 4.1)]), [32000, 64000])
        self.assertEqual(worker.window_offsets(10, 4, None), [0, 4, 8])
        self.assertEqual(worker.window_offsets(10, 4, []), [])

    def test_04_baseline_population_std_and_zero(self):
        events = [{"arousal": value} for value in (1, 2, 3)]
        self.assertEqual(worker.baseline(events), {"mean": 2., "std": round(math.sqrt(2/3), 6)})
        self.assertEqual([value["arousal_z"] for value in events], [-1.225, 0., 1.225])
        self.assertEqual(worker.baseline([{"arousal": .2}]), {"mean": .2, "std": 1.})
        self.assertEqual(worker.baseline([]), {"mean": 0., "std": 1.})

    def test_05_overlap_half_open_identity_human_copy(self):
        words = [{"word_id": "late", "text": "human", "t_ini": 2, "t_fin": 3, "state": "disabled", "edited": True, "comment": {"human": "keep"}},
                 {"word_id": "early", "text": "first", "t_ini": .5, "t_fin": 1.5}]
        before = copy.deepcopy(words)
        events = [{"t_ini": 0, "t_fin": 1, "arousal": .2, "arousal_z": -1}, {"t_ini": 1, "t_fin": 2, "arousal": .8, "arousal_z": 1}]
        result = worker.associate_arousal(words, events)
        self.assertIsNone(result[0]["arousal"])
        self.assertEqual(result[1]["arousal"], .5)
        self.assertEqual(result[1]["arousal_z"], 0.)
        self.assertEqual(words, before)
        self.assertEqual(result[0]["state"], "disabled")
        result[0]["comment"]["human"] = "changed"
        self.assertEqual(words, before)

    def test_06_pcm_pad_rms_finite(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"audio.wav"
            with wave.open(str(path), "wb") as output:
                output.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
                output.writeframes(b"\x00\x40"*160)
            samples, valid = worker.read_chunk(path, 80, 160, threading.Event())
            self.assertEqual(valid, 80)
            self.assertEqual(samples, [.5]*80+[0.]*80)
            event = worker.event_from_values(0, valid, samples, [.1, .2, .3])
            self.assertEqual(event["rms_dbfs"], -6.021)
            with self.assertRaises(worker.ArousalError):
                worker.event_from_values(0, valid, samples, [float("nan"), .2, .3])

    def test_07_checkpoint_hash_and_corruption(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = worker.inside(root, worker.OUTPUT)
            worker.atomic_json(target, {"genuine_model_result": False, "test": "checkpoint bytes only"})
            artifacts = {worker.OUTPUT: worker.file_hash(target, threading.Event())}
            path = worker.inside(root, ".work/manifests/arousal.json")
            worker.atomic_json(path, {"schema": "tv2-arousal-stage/1", "stage": "arousal", "input_digest": "key", "artifacts": artifacts})
            self.assertEqual(worker.checkpoint(root, "key", threading.Event()), artifacts)
            self.assertIsNone(worker.checkpoint(root, "other", threading.Event()))
            target.write_bytes(b"tampered")
            self.assertIsNone(worker.checkpoint(root, "key", threading.Event()))
            path.write_bytes(b"x"*(worker.MAX_LINE+1))
            self.assertIsNone(worker.checkpoint(root, "key", threading.Event()))

    def test_08_paths_and_cancellation(self):
        with tempfile.TemporaryDirectory() as directory:
            for path in ("../escape", "/absolute", "C:/escape", "a\\b", "a//b"):
                with self.assertRaises(worker.ArousalError):
                    worker.inside(Path(directory), path)
            event = threading.Event()
            event.set()
            with self.assertRaises(worker.ArousalError) as error:
                worker.file_hash(Path(directory)/"missing", event)
            self.assertEqual(error.exception.code, "E_CANCELLED")

    def test_09_invalid_track_times_ids(self):
        for words in ([{"word_id": "a", "text": "x", "t_ini": 1, "t_fin": 0}],
                      [{"word_id": "a", "text": "x", "t_ini": False, "t_fin": 1}],
                      [{"word_id": "a", "text": "x", "t_ini": 0, "t_fin": 1}]*2):
            with self.assertRaises(worker.ArousalError):
                worker.validate_track({"words": words}, 2)

    def test_10_closed_params_invalid_types(self):
        params = {"job_id": "job-000000000001", "project_id": "project", "revision": 1, "project_digest": "1"*64,
                  "asset_id": "asset", "normalized_audio_path": "audio.wav", "normalized_audio_sha256": "2"*64,
                  "transcript_path": "track.json", "transcript_sha256": "3"*64, "model_path": "model", "model_digest": "4"*64,
                  "manifest_path": "manifest.json", "manifest_sha256": "5"*64, "source_sha256": "6"*64,
                  "source_duration_ticks": worker.FLICKS, "timebase": "canonical-normalized-pcm/1",
                  "parameters": {"device": "cpu", "threads": 2, "window_seconds": 4., "hop_seconds": 2., "batch_size": 1,
                                 "gap_seconds": 1., "padding_seconds": .5, "scope": "speech-regions"}}
        worker.validate_params(params)
        for mutation in ({"revision": True}, {"unexpected": 1}, {"job_id": "../escape"}, {"source_sha256": "x"*64},
                         {"source_duration_ticks": 0}, {"timebase": "unknown"}):
            with self.assertRaises(worker.ArousalError):
                worker.validate_params(params | mutation)
        for mutation in ({"threads": True}, {"device": "cuda"}, {"window_seconds": float("nan")}, {"batch_size": 9},
                         {"hop_seconds": 5}, {"scope": "unknown"}, {"unexpected": True}):
            invalid = copy.deepcopy(params)
            invalid["parameters"].update(mutation)
            with self.assertRaises(worker.ArousalError):
                worker.validate_params(invalid)

    def test_11_cancellation_before_inputs_no_checkpoint(self):
        # A pre-cancelled actual Analysis never reads model/audio or publishes.
        with tempfile.TemporaryDirectory() as directory:
            event = threading.Event()
            event.set()
            value = object.__new__(worker.Analysis)
            value.cancel, value.root = event, Path(directory)
            with self.assertRaises(worker.ArousalError) as error:
                value.run()
            self.assertEqual(error.exception.code, "E_CANCELLED")
            self.assertEqual(list(Path(directory).iterdir()), [])


class Protocol(unittest.TestCase):
    def test_12_partial_and_multiple_envelopes(self):
        with tempfile.TemporaryDirectory() as directory:
            client = Client(Path(directory))
            try:
                line = worker.canonical({"protocol": worker.PROTOCOL, "id": "partial", "method": "hello", "params": {}})+b"\n"
                client.child.stdin.write(line[:len(line)//2])
                client.child.stdin.flush()
                with self.assertRaises(queue.Empty):
                    client.queue.get(timeout=.15)
                client.child.stdin.write(line[len(line)//2:])
                client.child.stdin.flush()
                self.assertIn("result", client.reply("partial"))
                first = worker.canonical({"protocol": worker.PROTOCOL, "id": "one", "method": "unknown", "params": {}})+b"\n"
                second = worker.canonical({"protocol": worker.PROTOCOL, "id": "two", "method": "hello", "params": {}})+b"\n"
                client.child.stdin.write(first+second)
                client.child.stdin.flush()
                self.assertEqual(client.reply("one")["error"]["code"], "E_METHOD")
                self.assertIn("result", client.reply("two"))
            finally:
                client.close()

    def test_13_incomplete_eof_and_oversize(self):
        for payload in (b'{"partial":', b"x"*(worker.MAX_LINE+1)):
            with self.subTest(size=len(payload)), tempfile.TemporaryDirectory() as directory:
                client = Client(Path(directory))
                try:
                    client.child.stdin.write(payload)
                    client.child.stdin.flush()
                    client.child.stdin.close()
                    self.assertEqual(client.reply(None)["error"]["code"], "E_LIMIT")
                    self.assertEqual(client.child.wait(timeout=5), 0)
                finally:
                    client.close()


def prepare(root, audio, transcript, model):
    manifest = model/"model-manifest.json"
    model_value = json.loads(manifest.read_bytes())
    hashes = {name: value["sha256"] for name, value in model_value["files"].items()}
    original = json.loads(transcript.read_bytes())
    # Exercise preservation of human editorial state in a proposal artifact.
    original["words"][0].update(edited=True, state="disabled", comment="Human fixture decision remains unchanged")
    track = root/"input-track.json"
    worker.atomic_json(track, original)
    with wave.open(str(audio), "rb") as source:
        duration_ticks = round(source.getnframes()/source.getframerate()*worker.FLICKS)
    return {"job_id": "job-000000000001", "project_id": "synthetic-arousal-project", "revision": 3, "project_digest": "1"*64,
            "asset_id": "synthetic-sapi-asset", "normalized_audio_path": str(audio), "normalized_audio_sha256": worker.file_hash(audio, threading.Event()),
            "transcript_path": str(track), "transcript_sha256": worker.file_hash(track, threading.Event()), "model_path": str(model),
            "model_digest": worker.digest(hashes), "manifest_path": str(manifest), "manifest_sha256": worker.file_hash(manifest, threading.Event()),
            "source_sha256": worker.file_hash(audio, threading.Event()), "source_duration_ticks": duration_ticks, "timebase": "canonical-normalized-pcm/1",
            "parameters": {"device": "cpu", "threads": 2, "window_seconds": 4., "hop_seconds": 2., "batch_size": 1,
                           "gap_seconds": 1., "padding_seconds": .5, "scope": "speech-regions"}}


def real(root, params):
    client = Client(root/"jobs")
    started = time.monotonic()
    try:
        client.send("nohello", "run", params)
        assert client.reply("nohello")["error"]["code"] == "E_PROTOCOL"
        client.send("hello", "hello")
        hello = client.reply("hello")["result"]
        assert hello["capabilities"]["arousal"]["runtime_state"] == "not_loaded"
        invalid = copy.deepcopy(params)
        invalid["unexpected"] = True
        client.send("invalid", "run", invalid)
        assert client.reply("invalid")["error"]["code"] == "E_ARGUMENT"
        client.send("run", "run", params)
        result = client.reply("run")
        if "error" in result:
            raise AssertionError(result)
        receipt = result["result"]
        artifact = worker.inside(root/"jobs"/params["job_id"], receipt["arousal_path"])
        assert worker.file_hash(artifact, threading.Event()) == receipt["artifacts"][worker.OUTPUT]
        output = json.loads(artifact.read_bytes())
        original = json.loads(Path(params["transcript_path"]).read_bytes())
        assert output["arousal_analysis"]["native_inference_executed"] is True
        assert output["arousal_analysis"]["execution_state"] == "completed"
        assert len(output["words"]) == len(original["words"]) == 23
        assert len(output["arousal"]["events"]) > 0
        for old, new in zip(original["words"], output["words"]):
            assert {key: new[key] for key in old} == old
            assert new["arousal"] is not None and math.isfinite(new["arousal"])
        recalculated = worker.associate_arousal(original["words"], output["arousal"]["events"])
        assert recalculated == output["words"]
        before = len(LOG)
        client.send("resume", "run", params)
        assert client.reply("resume")["result"] == receipt
        assert any(event.get("cached") is True for event in LOG[before:])
        assert not any(event.get("stage") == "load-arousal" for event in LOG[before:])
        # Cancel a new real run before loading and require no durable checkpoint.
        cancel_params = copy.deepcopy(params)
        cancel_params["job_id"] = "job-000000000002"
        client.send("cancelled-run", "run", cancel_params)
        sent = False
        def cancel(event):
            nonlocal sent
            if not sent and event.get("stage") == "verify-inputs":
                client.send("cancel", "cancel", {"job_id": cancel_params["job_id"]})
                sent = True
        assert client.reply("cancelled-run", cancel)["error"]["code"] == "E_CANCELLED"
        assert sent
        assert not (root/"jobs"/cancel_params["job_id"]/".work/manifests/arousal.json").exists()
        # SHA tamper must stop a genuine runtime before inference/publishing.
        bad = copy.deepcopy(params)
        bad["job_id"], bad["normalized_audio_sha256"] = "job-000000000003", "f"*64
        client.send("tamper", "run", bad)
        assert client.reply("tamper")["error"]["code"] == "E_PRECONDITION"
        return {"receipt": receipt, "word_count": len(output["words"]), "event_count": len(output["arousal"]["events"]),
                "baseline": output["arousal"]["baseline"], "elapsed_seconds": time.monotonic()-started, "native_inference_executed": True}
    finally:
        client.close()
        LOG.append({"owned_root_pid": client.child.pid, "exit_code": client.child.returncode, "stderr_tail": client.stderr.decode("utf-8", errors="replace")})


def main():
    global DEADLINE, TRACE_AFTER
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--real", action="store_true")
    parser.add_argument("--audio", type=Path)
    parser.add_argument("--transcript", type=Path)
    parser.add_argument("--model", type=Path)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--diagnostic-trace-after", type=int, choices=range(10, 121))
    args = parser.parse_args()
    if not 10 <= args.timeout <= 600:
        parser.error("timeout must be 10–600 seconds")
    DEADLINE, TRACE_AFTER = args.timeout, args.diagnostic_trace_after
    args.root.mkdir(parents=True, exist_ok=True)
    summary = {"asr_exercised": False, "gui_integration": False, "v1_executed": False, "real_inference_requested": args.real}
    success = False
    try:
        suite = unittest.TestSuite([unittest.defaultTestLoader.loadTestsFromTestCase(Independent),
                                   unittest.defaultTestLoader.loadTestsFromTestCase(Protocol)])
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        summary["independent_tests"] = {"run": result.testsRun, "failures": len(result.failures), "errors": len(result.errors)}
        assert result.wasSuccessful(), "Independent contracts failed"
        if args.real:
            params = prepare(args.root.resolve(), args.audio.resolve(), args.transcript.resolve(), args.model.resolve())
            worker.atomic_json(args.root/"request.json", params)
            summary["real"] = real(args.root.resolve(), params)
        success = True
    except Exception as error:
        summary["error"] = {"type": type(error).__name__, "message": str(error)}
        raise
    finally:
        summary.update(success=success, events=LOG, memory=MEMORY)
        worker.atomic_json(args.root/"result.json", summary)


if __name__ == "__main__":
    main()
