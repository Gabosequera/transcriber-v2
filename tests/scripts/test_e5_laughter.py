"""Laughter units and explicit real silence/SAPI negatives; no invented positive.

Probability arrays in units test deterministic pooling, never model inference.
No V1 imports, upstream training class, PyAV or project mutation.
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
import struct
import subprocess
import sys
import threading
import time
import unittest
import wave

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("laughter_worker", REPO / "workers/python/laughter_worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
OPTIONS = None
PARAMS = None
LOG = []
MEMORY = []
OUTCOMES = []

class Client:
    def __init__(self, root):
        self.child = subprocess.Popen([sys.executable, "-I", "-u", str(REPO / "workers/python/laughter_worker.py"), "--work-root", str(root)],
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
            self.queue.put(EOFError("Laughter worker closed"))
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
                raise AssertionError("Invalid laughter protocol")
            if value.get("event"):
                LOG.append(value)
                if event:
                    event(value)
                continue
            if value.get("id") == "cancel":
                LOG.append(value)
                continue
            if value.get("id") != identifier:
                raise AssertionError("Uncorrelated laughter reply: " + repr(value))
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





def wav(path, samples, channels=1):
    with wave.open(str(path), "wb") as audio:
        audio.setparams((channels, 2, 16000, 0, "NONE", "not compressed"))
        audio.writeframes(struct.pack("<"+"h"*len(samples), *samples))


class IndependentTests(unittest.TestCase):
    def test_01_runtime_lazy_hello_and_protocol(self):
        self.assertNotIn("torch", sys.modules)
        self.assertNotIn("numpy", sys.modules)
        client = Client(OPTIONS.work_root / "hello")
        try:
            client.send("early", "run", PARAMS)
            self.assertEqual(client.reply("early")["error"]["code"], "E_PROTOCOL")
            hello = client.initialize()
            self.assertEqual(hello["runtime"]["python"], "3.11.15")
            self.assertEqual(hello["runtime"]["dependencies"]["torch"], "2.1.2+cpu")
            self.assertEqual(hello["capabilities"]["laughter"]["runtime_state"], "not_loaded")
            self.assertFalse(hello["capabilities"]["gui_integration"])
            client.send("invalid", "run", {"job_id": "../escape"})
            self.assertEqual(client.reply("invalid")["error"]["code"], "E_ARGUMENT")
        finally:
            client.close()

    def test_02_closed_bounds_and_paths(self):
        worker.validate_params(PARAMS)
        for key, value in (("audio_offset_ticks", -1), ("revision", True), ("model_sha256", "x"*64), ("extra", True)):
            params = copy.deepcopy(PARAMS)
            params[key] = value
            with self.assertRaises(worker.LaughterError):
                worker.validate_params(params)
        for name, value in (("threads", 4), ("threshold", math.nan), ("batch_size", 3), ("amplitude_boost", 1)):
            params = copy.deepcopy(PARAMS)
            params["parameters"][name] = value
            with self.assertRaises(worker.LaughterError):
                worker.validate_params(params)
        for relative in ("../escape", "/root", "a\\b", "a:stream", "a//b"):
            with self.assertRaises(worker.LaughterError):
                worker.inside(OPTIONS.work_root, relative)

    def test_03_pcm_silence_streaming_boost(self):
        path = OPTIONS.work_root / "silence.wav"
        wav(path, [0]*32000)
        self.assertEqual(worker.pcm_info(path), 32000)
        ranges, peak = worker.boost_plan(path, 32000, threading.Event())
        self.assertEqual(ranges, [(0, 32000)])
        self.assertEqual(peak, 1)
        self.assertEqual(worker.transform_samples([0]*10, 0, 32000, ranges, peak), [0]*10)
        path = OPTIONS.work_root / "loud.wav"
        wav(path, [12000, -12000]*16000)
        self.assertEqual(worker.silence_ranges(path, 32000, threading.Event()), [])
        ranges, peak = worker.boost_plan(path, 32000, threading.Event())
        self.assertEqual(worker.transform_samples([12000, -12000], 0, 32000, ranges, peak), [1, -1])
        stereo = OPTIONS.work_root / "stereo.wav"
        wav(stereo, [0]*32000, channels=2)
        with self.assertRaises(worker.LaughterError):
            worker.pcm_info(stereo)

    def test_04_silence_boundary_and_golden_fades(self):
        short = OPTIONS.work_root / "short.wav"
        wav(short, [0]*4319)
        self.assertEqual(worker.silence_ranges(short, 4319, threading.Event()), [(0, 4319)])
        too_short = OPTIONS.work_root / "too-short.wav"
        wav(too_short, [0]*4000)
        self.assertEqual(worker.silence_ranges(too_short, 4000, threading.Event()), [])
        self.assertEqual(worker.amplitude_gain(100, (100, 10000), 11000), 1)
        self.assertEqual(worker.amplitude_gain(2499, (100, 10000), 11000), 5)
        self.assertEqual(worker.amplitude_gain(9999, (100, 10000), 11000), 1)
        self.assertEqual(worker.amplitude_gain(9999, (100, 10000), 10000), 5)
        self.assertEqual(worker.amplitude_gain(101, (100, 4900), 10000), 5)

    def test_05_global_maxpool_overlap_and_invalid_probability(self):
        timeline = [0.0]*8
        worker.pool_window(timeline, [0.1, 0.8, 0.4, 0.5], 0, 0.1)
        worker.pool_window(timeline, [0.9, 0.2, 0.7], 0.2, 0.1)
        self.assertEqual(timeline[:5], [0.1, 0.8, 0.9, 0.5, 0.7])
        for value in (math.nan, math.inf, -0.1, 1.1):
            with self.assertRaises(worker.LaughterError):
                worker.pool_window(timeline, [value], 0, 0.1)

    def test_06_v1_threshold_merge_minimum_confidence(self):
        options = PARAMS["parameters"]
        events = worker.extract_events([0, .6, .8, 0, .9, .7, 0, 0], .1, .8, options, "a1", threading.Event())
        self.assertEqual(len(events), 1)
        self.assertEqual((events[0]["t_ini"], events[0]["t_fin"]), (.1, .6))
        self.assertEqual(events[0]["max_conf"], .9)
        self.assertEqual(events[0]["mean_conf"], .75)
        self.assertEqual(events[0]["event_id"], "a1-laugh-00001")
        self.assertEqual(worker.extract_events([.9, 0, 0], .02, .06, options, "a0", threading.Event()), [])

    def test_07_fractional_clamp_no_event_outside_source(self):
        options = {**PARAMS["parameters"], "min_dur": .02}
        events = worker.extract_events([.9]*20, .02, .3754, options, "a0", threading.Event())
        self.assertEqual(events[0]["t_fin"], .3754)
        self.assertTrue(all(0 <= e["t_ini"] < e["t_fin"] <= .3754 for e in events))
        absolute = worker.absolute_events(copy.deepcopy(events), worker.FLICKS//4, round(.6254*worker.FLICKS))
        self.assertEqual(absolute[0]["t_ini"], .25)
        self.assertLessEqual(absolute[0]["t_fin"], .6254)
        self.assertEqual(absolute[0]["track_id"], "a0")

    def test_08_cancel_and_hash_errors_before_native(self):
        cancel = threading.Event()
        cancel.set()
        with self.assertRaises(worker.LaughterError) as error:
            worker.Analysis(OPTIONS.work_root, PARAMS, cancel, LOG.append).run()
        self.assertEqual(error.exception.code, "E_CANCELLED")
        invalid = {**PARAMS, "normalized_audio_sha256": "0"*64}
        with self.assertRaises(worker.LaughterError) as error:
            worker.Analysis(OPTIONS.work_root, invalid, threading.Event(), LOG.append).run()
        self.assertEqual(error.exception.code, "E_PRECONDITION")
        self.assertNotIn("torch", sys.modules)

    def test_09_checkpoint_exact_artifact_schema_and_sha(self):
        analysis = worker.Analysis(OPTIONS.work_root / "cache", PARAMS, threading.Event(), LOG.append)
        target = analysis.root / worker.OUTPUT
        checkpoint = analysis.root / ".work/manifests/laughter.json"
        worker.atomic_json(target, {"unit": "fixture cache bytes; no model inference"})
        artifacts = {worker.OUTPUT: worker.file_hash(target, threading.Event())}
        valid = {"schema": "tv2-laughter-stage/1", "stage": "laughter", "input_digest": "key", "artifacts": artifacts}
        worker.atomic_json(checkpoint, valid)
        self.assertEqual(analysis.cached("key", target, checkpoint), artifacts)
        for field, value in (("schema", "other"), ("stage", "other"), ("input_digest", "other"), ("artifacts", {"sibling.json": artifacts[worker.OUTPUT]})):
            worker.atomic_json(checkpoint, {**valid, field: value})
            self.assertIsNone(analysis.cached("key", target, checkpoint))
        worker.atomic_json(checkpoint, valid)
        target.write_text("altered", encoding="utf-8")
        self.assertIsNone(analysis.cached("key", target, checkpoint))

    def test_10_protocol_cancel_during_verification_and_eof(self):
        client = Client(OPTIONS.work_root / "cancel")
        try:
            client.initialize()
            client.send("running", "run", PARAMS)
            sent = []
            def cancel(event):
                if not sent:
                    client.send("cancel", "cancel", {"job_id": PARAMS["job_id"]})
                    sent.append(True)
            reply = client.reply("running", event=cancel)
            self.assertEqual(reply["error"]["code"], "E_CANCELLED")
        finally:
            client.close()
        client = Client(OPTIONS.work_root / "eof")
        client.initialize()
        client.child.stdin.close()
        self.assertEqual(client.child.wait(timeout=5), 0)
        client.close()

    def test_11_pipe_partial_multiple_oversize_and_incomplete_eof(self):
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
    def test_12_actual_pinned_model_negatives_resume_and_invalidation(self):
        for name, audio in (("silence", OPTIONS.work_root / "silence.wav"), ("known-sapi", Path(PARAMS["normalized_audio_path"]))):
            params = copy.deepcopy(PARAMS)
            params.update(job_id="real-"+name, normalized_audio_path=str(audio), normalized_audio_sha256=worker.file_hash(audio, threading.Event()))
            with wave.open(str(audio), "rb") as waveform:
                params["source_duration_ticks"] = round(waveform.getnframes()/16000*worker.FLICKS)
            params["source_sha256"] = params["normalized_audio_sha256"]
            client = Client(OPTIONS.work_root / "real")
            before = time.monotonic()
            try:
                client.initialize()
                client.send("run", "run", params)
                reply = client.reply("run", timeout=240)
                client.record_memory()
                self.assertIn("result", reply, reply)
                receipt = reply["result"]
                path = OPTIONS.work_root / "real" / params["job_id"] / worker.OUTPUT
                result = json.loads(path.read_bytes())
                OUTCOMES.append({"fixture": name, "elapsed_seconds": time.monotonic()-before, "events": result["events"], "metadata": result["laughter"]})
                self.assertEqual(receipt["artifacts"][worker.OUTPUT], worker.file_hash(path, threading.Event()))
                self.assertEqual(result["laughter"]["execution_state"], "completed")
                self.assertEqual(result["project_digest"], params["project_digest"])
                self.assertTrue(all(0 <= e["t_ini"] < e["t_fin"] <= params["source_duration_ticks"]/worker.FLICKS for e in result["events"]))
                client.send("resume", "run", params)
                self.assertEqual(client.reply("resume")["result"], receipt)
                self.assertTrue(any(e.get("cached") and e.get("job_id") == params["job_id"] for e in LOG))
                invalid = {**params, "normalized_audio_sha256": "0"*64}
                client.send("invalidated", "run", invalid)
                self.assertEqual(client.reply("invalidated")["error"]["code"], "E_PRECONDITION")
                # Negative results are measured honestly; never fabricate a laugh.
                self.assertEqual(result["events"], [], "False-positive laughter on known negative fixture")
            finally:
                client.close()


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
    manifest = REPO / ".local/models/laughter-omine/model-manifest.json"
    with wave.open(str(audio), "rb") as waveform:
        duration = waveform.getnframes()/16000
    PARAMS = {"job_id": "laughter-fixture", "project_id": "laughter-fixture", "revision": 0, "project_digest": worker.digest({"fixture": "silence and known invented SAPI"}),
              "asset_id": "sapi-audio", "track_id": "a0", "audio_index": 0, "audio_offset_ticks": 0,
              "normalized_audio_path": str(audio), "normalized_audio_sha256": worker.file_hash(audio, threading.Event()),
              "source_sha256": worker.file_hash(audio, threading.Event()), "source_duration_ticks": round(duration*worker.FLICKS), "timebase": "flicks/705600000",
              "model_path": str(manifest.with_name("model.safetensors")), "config_path": str(manifest.with_name("config.json")), "manifest_path": str(manifest),
              "model_sha256": worker.MODEL_SHA, "config_sha256": worker.CONFIG_SHA,
              "parameters": {"device": "cpu", "threads": 2, "threshold": .5, "amplitude_boost": True, "min_dur": .2, "merge_gap": .2, "input_sec": 7, "overlap_sec": 2, "batch_size": 1}}
    before = time.monotonic()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(IndependentTests)
    if OPTIONS.suite == "real":
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(RealTests))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    report = {"protocol": worker.PROTOCOL, "suite": OPTIONS.suite, "asr_exercised": False, "gui_exercised": False, "positive_laughter_fixture": False,
              "real_laughter_requested": OPTIONS.suite == "real", "parameters": PARAMS, "tests_run": result.testsRun, "success": result.wasSuccessful(),
              "elapsed_seconds": time.monotonic()-before, "worker_sha256": worker.file_hash(REPO / "workers/python/laughter_worker.py", threading.Event()),
              "failures": [(str(test), trace) for test, trace in result.failures+result.errors], "events": LOG, "process_memory": MEMORY, "measured_outcomes": OUTCOMES}
    worker.atomic_json(OPTIONS.evidence, report)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
