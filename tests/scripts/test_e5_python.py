"""Real offline tiny inference and NDJSON lifecycle/regression checks.

Run using the isolated Python 3.12.13 environment. Sources are invented SAPI
recordings inside V2; no V1 execution, network, credentials or project editing.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import unittest
from unittest.mock import patch
import wave


REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("tv2_worker", REPO / "workers/python/worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
OPTIONS = None
PARAMS = None
EVENT_LOG = []


class Client:
    def __init__(self):
        self.child = subprocess.Popen([sys.executable, "-I", "-u", str(REPO / "workers/python/worker.py"), "--work-root", str(OPTIONS.work_root)],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.lines = queue.Queue()
        self.errors = bytearray()
        def read():
            for line in self.child.stdout:
                try:
                    self.lines.put(json.loads(line))
                except Exception as error:
                    self.lines.put(error)
            self.lines.put(EOFError("Worker closed"))
        def stderr():
            while block := self.child.stderr.read(4096):
                self.errors.extend(block)
                del self.errors[:-16384]
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        self.error_reader = threading.Thread(target=stderr, daemon=True)
        self.error_reader.start()
        self.hello = None

    def send(self, request_id, method, params=None):
        value = {"protocol": worker.PROTOCOL, "id": request_id, "method": method, "params": params or {}}
        self.child.stdin.write(worker.canonical(value) + b"\n")
        self.child.stdin.flush()

    def reply(self, request_id, on_event=None, timeout=120):
        deadline = time.monotonic() + timeout
        while True:
            value = self.lines.get(timeout=max(0.1, deadline-time.monotonic()))
            if isinstance(value, Exception):
                raise value
            if value.get("protocol") != worker.PROTOCOL:
                raise AssertionError("Invalid NDJSON protocol")
            if value.get("event"):
                EVENT_LOG.append(value)
                if on_event:
                    on_event(value)
                continue
            if value.get("id") == "cancel":
                EVENT_LOG.append(value)
                continue
            if value.get("id") != request_id:
                raise AssertionError(f"Uncorrelated worker response: {value}")
            return value

    def initialize(self):
        self.send("hello", "hello")
        self.hello = self.reply("hello")
        assert "result" in self.hello, self.hello
        return self.hello["result"]

    def close(self):
        try:
            if self.child.poll() is None:
                self.send("shutdown", "shutdown")
                self.reply("shutdown", timeout=15)
                self.child.stdin.close()
                self.child.wait(timeout=10)
        finally:
            if self.child.poll() is None:
                self.child.kill()
                self.child.wait()
            self.reader.join(timeout=2)
            self.error_reader.join(timeout=2)
            for pipe in (self.child.stdin, self.child.stdout, self.child.stderr):
                if not pipe.closed:
                    pipe.close()


def request(job):
    value = copy.deepcopy(PARAMS)
    value["job_id"] = job
    return value


def analyze(value):
    client = Client()
    try:
        client.initialize()
        client.send("run", "run", value)
        response = client.reply("run")
        assert "result" in response, response
        return response["result"]
    finally:
        client.close()


class ContractTests(unittest.TestCase):
    def test_01_hello_runtime_and_closed_capabilities(self):
        client = Client()
        try:
            hello = client.initialize()
            self.assertEqual(hello["python"], "3.12.13")
            self.assertEqual(hello["dependencies"], worker.runtime_versions())
            self.assertTrue(hello["capabilities"]["transcribe"]["installed"])
            self.assertEqual(hello["capabilities"]["transcribe"]["runtime_state"], "not_loaded")
            self.assertEqual(hello["capabilities"]["heuristics"]["runtime_state"], "stdlib_ready")
            for step in ("forced_alignment", "laughter", "arousal"):
                self.assertFalse(hello["capabilities"][step])
        finally:
            client.close()

    def test_02_run_requires_hello_and_invalid_request_rejected(self):
        client = Client()
        try:
            client.send("run", "run", request("not-initialized"))
            self.assertEqual(client.reply("run")["error"]["code"], "E_PROTOCOL")
            client.initialize()
            for job, patch, code in (("bad-steps", {"parameters": PARAMS["parameters"] | {"steps": ["extract", "align"]}}, "E_UNSUPPORTED"),
                                      ("bad-path", {"job_id": "../escape"}, "E_PATH"),
                                      ("bad-field", {"unexpected": "field"}, "E_ARGUMENT")):
                client.send(job, "run", request(job) | patch)
                self.assertEqual(client.reply(job)["error"]["code"], code)
            self.assertFalse((OPTIONS.work_root / "not-initialized").exists())
        finally:
            client.close()

    def test_03_hash_preconditions_before_ffmpeg(self):
        client = Client()
        try:
            client.initialize()
            for field in ("source_sha256", "model_digest"):
                value = request("wrong-" + field) | {field: "0" * 64}
                client.send(field, "run", value)
                self.assertEqual(client.reply(field)["error"]["code"], "E_PRECONDITION")
                self.assertFalse((OPTIONS.work_root / value["job_id"] / ".work/audio").exists())
        finally:
            client.close()

    def test_04_real_multistream_and_resume(self):
        before = len(EVENT_LOG)
        value = request("real-multistream")
        result = analyze(value)
        self.assertEqual({key: result[key] for key in ("job_id", "project_id", "revision", "project_digest", "source_sha256", "model_digest")},
                         {key: value[key] for key in ("job_id", "project_id", "revision", "project_digest", "source_sha256", "model_digest")})
        root = OPTIONS.work_root / value["job_id"]
        master = json.loads((root / result["master_path"]).read_text(encoding="utf-8"))
        self.assertEqual(master["schema"], "editorial-master/1")
        self.assertEqual(len(master["tracks"]), 2)
        for track in master["tracks"].values():
            self.assertGreater(len(track["words"]), 5, "Tiny must actually recognize the invented speech")
            self.assertGreater(len(track["utterances"]), 0)
            for record in track["words"] + track["utterances"]:
                self.assertGreater(record["t_fin"], record["t_ini"])
                self.assertGreaterEqual(record["t_ini"], 0)
                self.assertLessEqual(record["t_fin"], master["media"]["duration"])
            self.assertNotIn("laughter", track)
            self.assertEqual(track["asr"]["alignment"], "not_run")
        self.assertAlmostEqual(master["tracks"]["a1"]["offset"], 0.25, places=3)
        for relative, expected in result["artifacts"].items():
            self.assertEqual(worker.file_hash(root / relative, threading.Event()), expected)
        self.assertEqual(worker.file_hash(OPTIONS.source, threading.Event()), value["source_sha256"])
        self.assertTrue(any(event.get("stage") == "transcribe-a0" and event.get("cached") is False for event in EVENT_LOG[before:]))
        again_before = len(EVENT_LOG)
        again = analyze(value)
        self.assertEqual(result, again, "Same inputs must reuse exact artifacts, including master")
        cached = [event for event in EVENT_LOG[again_before:] if event.get("cached") is True]
        self.assertEqual({event["stage"] for event in cached}, {"extract-a0", "extract-a1", "transcribe-a0", "transcribe-a1", "editorial-a0", "editorial-a1", "complete"})

    def test_05_corrupt_checkpoint_and_parameter_invalidation(self):
        value = request("real-multistream")
        root = OPTIONS.work_root / value["job_id"]
        transcript = root / ".work/transcripts/a0.json"
        transcript.write_text('{"corrupt":true}', encoding="utf-8")
        before = len(EVENT_LOG)
        result = analyze(value)
        self.assertTrue(any(event.get("stage") == "transcribe-a0" and event.get("cached") is False for event in EVENT_LOG[before:]))
        self.assertEqual(worker.file_hash(transcript, threading.Event()), result["artifacts"][".work/transcripts/a0.json"])
        value["parameters"]["beam_size"] = 1
        before = len(EVENT_LOG)
        analyze(value)
        self.assertTrue(any(event.get("stage") == "extract-a0" and event.get("cached") is True for event in EVENT_LOG[before:]))
        self.assertTrue(any(event.get("stage") == "transcribe-a0" and event.get("cached") is False for event in EVENT_LOG[before:]), "Changed decoder parameters must invalidate transcription")

    def test_06_cancel_real_decoder_then_resume_verified_extract(self):
        value = request("cancel-decoder")
        client = Client()
        cancelled = False
        def cancel_on_transcription(event):
            nonlocal cancelled
            if event["stage"] == "transcribe-a0" and event["fraction"] == 0 and not cancelled:
                cancelled = True
                client.send("cancel", "cancel", {"job_id": value["job_id"]})
        try:
            client.initialize()
            client.send("run", "run", value)
            response = client.reply("run", on_event=cancel_on_transcription)
            self.assertTrue(cancelled, "Cancellation must reach the real transcription stage")
            self.assertEqual(response["error"]["code"], "E_CANCELLED")
        finally:
            client.close()
        self.assertFalse((OPTIONS.work_root / value["job_id"] / ".work/manifests/transcribe-a0.json").exists())
        before = len(EVENT_LOG)
        analyze(value)
        self.assertTrue(any(event.get("stage") == "extract-a0" and event.get("cached") is True for event in EVENT_LOG[before:]))
        self.assertTrue(any(event.get("stage") == "transcribe-a0" and event.get("cached") is False for event in EVENT_LOG[before:]))

    def test_07_eof_cancels_and_leaves_no_success_master(self):
        client = Client()
        try:
            client.initialize()
            value = request("eof-cancelled")
            client.send("run", "run", value)
            client.child.stdin.close()
            client.child.wait(timeout=15)
            self.assertEqual(client.child.returncode, 0)
            self.assertFalse((OPTIONS.work_root / value["job_id"] / "editorial/analysis.editorial.master.json").exists())
        finally:
            client.close()

    def test_08_paths_reject_traversal_and_existing_escape(self):
        for relative in ("../other", "/absolute", "a\\b", "C:/escape", "a//b"):
            with self.assertRaises(worker.WorkerError):
                worker.inside(OPTIONS.work_root, relative)


class ExtractionTests(unittest.TestCase):
    def test_01_real_multistream_offsets_cache_and_corruption(self):
        value = request("extract-only")
        cancel = threading.Event()
        events = []
        analysis = worker.Analysis(OPTIONS.work_root, value, cancel, events.append)
        outputs = [(stream, analysis.extract(stream)) for stream in value["asset"]["probe"]["audio"]]
        self.assertEqual(len(outputs), 2)
        with wave.open(str(analysis.root / outputs[1][1]), "rb") as audio:
            self.assertEqual(audio.getframerate(), 16000)
            self.assertEqual(audio.getnchannels(), 1)
            self.assertEqual(audio.readframes(4000), bytes(8000), "Positive 250 ms offset must prepend real silence")
        before = len(events)
        for stream, relative in outputs:
            self.assertEqual(analysis.extract(stream), relative)
        self.assertTrue(all(event.get("cached") for event in events[before:]))
        target = analysis.root / outputs[0][1]
        target.write_bytes(b"corrupt normalized audio")
        before = len(events)
        analysis.extract(outputs[0][0])
        self.assertTrue(any(event.get("cache_scope") == "work_root" for event in events[before:]), "Verified shared bytes should repair a corrupt job copy")
        with wave.open(str(target), "rb") as audio:
            self.assertGreater(audio.getnframes(), 0)
        shared = OPTIONS.work_root / f".cache/extract/{analysis.stage_key('extract-a0')}.wav"
        shared.write_bytes(b"corrupt shared audio")
        target.write_bytes(b"corrupt job audio again")
        before = len(events)
        analysis.extract(outputs[0][0])
        self.assertTrue(any(event.get("cached") is False for event in events[before:]), "Both invalid copies require actual re-extraction")
        changed = copy.deepcopy(value)
        changed["parameters"]["beam_size"] = 1
        reconfigured = worker.Analysis(OPTIONS.work_root, changed, cancel, events.append)
        self.assertIsNotNone(reconfigured.cached("extract-a0"), "Decoder parameters must not invalidate normalized extraction")
        reconfigured.extract(outputs[0][0])
        self.assertIsNotNone(reconfigured.cached("extract-a0"))

    def test_02_negative_offset_trims_exact_samples(self):
        value = request("negative-offset")
        value["asset"]["probe"]["start_time"] = round(0.25 * worker.FLICKS)
        events = []
        analysis = worker.Analysis(OPTIONS.work_root, value, threading.Event(), events.append)
        relative = analysis.extract(value["asset"]["probe"]["audio"][0])
        baseline = worker.Analysis(OPTIONS.work_root, request("negative-baseline"), threading.Event(), events.append)
        original = baseline.extract(value["asset"]["probe"]["audio"][0])
        with wave.open(str(baseline.root / original), "rb") as source, wave.open(str(analysis.root / relative), "rb") as trimmed:
            source.setpos(4000)
            self.assertEqual(trimmed.readframes(8000), source.readframes(8000), "Negative 250 ms must trim exactly 4000 samples at 16 kHz")
            self.assertEqual(trimmed.getnframes(), source.getnframes()-4000)

    def test_03_cancel_before_extract_preserves_checkpoint_and_source(self):
        value = request("extract-cancel")
        cancel = threading.Event()
        analysis = worker.Analysis(OPTIONS.work_root, value, cancel, lambda event: None)
        relative = analysis.extract(value["asset"]["probe"]["audio"][0])
        original = worker.file_hash(analysis.root / relative, threading.Event())
        cancel.set()
        with self.assertRaises(worker.WorkerError) as failed:
            analysis.extract(value["asset"]["probe"]["audio"][0])
        self.assertEqual(failed.exception.code, "E_CANCELLED")
        self.assertEqual(worker.file_hash(analysis.root / relative, threading.Event()), original)
        self.assertEqual(worker.file_hash(OPTIONS.source, threading.Event()), value["source_sha256"])

    def test_04_tampered_cache_path_is_ignored(self):
        value = request("tampered-path")
        analysis = worker.Analysis(OPTIONS.work_root, value, threading.Event(), lambda event: None)
        path = analysis.root / ".work/manifests/extract-a0.json"
        path.parent.mkdir(parents=True)
        path.write_bytes(worker.canonical({"schema": "tv2-analysis-stage/1", "stage": "extract-a0", "input_digest": analysis.stage_key("extract-a0"), "artifacts": {"../source": "0"*64}}))
        self.assertIsNone(analysis.cached("extract-a0"))
        # A real verified file must not stand in for the fixed transcript/master
        # path. This tests cache integrity only; no ASR result is fabricated.
        audio = analysis.extract(value["asset"]["probe"]["audio"][0])
        correct_hash = worker.file_hash(analysis.root / audio, threading.Event())
        for stage in ("transcribe-a0", "master"):
            manifest = analysis.root / f".work/manifests/{stage}.json"
            manifest.write_bytes(worker.canonical({"schema": "tv2-analysis-stage/1", "stage": stage, "input_digest": analysis.stage_key("transcribe-a0") if stage != "master" else "0"*64,
                                                  "artifacts": {audio: correct_hash}}))
            self.assertIsNone(analysis.cached(stage), "Valid unrelated artifact must never authorize an unchecked fixed output")
        valid = json.loads(path.read_text(encoding="utf-8"))
        valid["stage"] = "extract-a1"
        path.write_bytes(worker.canonical(valid))
        self.assertIsNone(analysis.cached("extract-a0"), "Stage identity must match its manifest filename")

    def test_05_exact_runtime_mismatch_rejected_without_import(self):
        original = worker.platform.python_version
        worker.platform.python_version = lambda: "3.12.12"
        try:
            with self.assertRaises(worker.WorkerError) as failed:
                worker.runtime_versions()
            self.assertEqual(failed.exception.code, "runtime_mismatch")
        finally:
            worker.platform.python_version = original

    def test_06_cancel_real_ffmpeg_reaps_process_and_cleans_partial(self):
        value = request("cancel-real-ffmpeg")
        value["asset"]["probe"]["start_time"] = worker.FLICKS // 1000
        cancel = threading.Event()
        analysis = worker.Analysis(OPTIONS.work_root, value, cancel, lambda event: None)
        launch = subprocess.Popen
        children = []
        def launch_then_cancel(*args, **kwargs):
            child = launch(*args, **kwargs)
            children.append(child)
            self.assertIsNone(child.poll(), "Cancel injection must target a live real FFmpeg process")
            cancel.set()
            return child
        # The subprocess is real. Only cancellation timing is injected at the
        # process-created boundary, to exercise cleanup without a huge source.
        with patch.object(worker.subprocess, "Popen", side_effect=launch_then_cancel):
            with self.assertRaises(worker.WorkerError) as failed:
                analysis.extract(value["asset"]["probe"]["audio"][0])
        self.assertEqual(failed.exception.code, "E_CANCELLED")
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].poll(), "Cancelled child must be terminated/reaped")
        self.assertFalse((analysis.root / ".work/manifests/extract-a0.json").exists())
        self.assertEqual(list(analysis.root.rglob("*.tmp-*.wav")), [])
        self.assertEqual(worker.file_hash(OPTIONS.source, threading.Event()), value["source_sha256"])

    def test_07_cross_job_language_model_changes_reuse_only_verified_extraction(self):
        initial = worker.Analysis(OPTIONS.work_root, request("shared-first"), threading.Event(), lambda event: None)
        initial.extract(initial.audio[0])
        value = request("shared-second")
        value["project_id"] = "different-project"
        value["revision"] = 8
        value["parameters"]["language"] = "es"
        value["model_digest"] = "1" * 64
        events = []
        other = worker.Analysis(OPTIONS.work_root, value, threading.Event(), events.append)
        self.assertEqual(other.stage_key("extract-a0"), initial.stage_key("extract-a0"))
        relative = other.extract(other.audio[0])
        self.assertTrue(any(event.get("cached") is True and event.get("cache_scope") == "work_root" for event in events))
        self.assertEqual(worker.file_hash(other.root / relative, threading.Event()), worker.file_hash(initial.root / relative, threading.Event()))
        self.assertNotEqual(other.stage_key("transcribe-a0"), initial.stage_key("transcribe-a0"))
        self.assertFalse((other.root / "editorial/analysis.editorial.master.json").exists(), "Audio reuse must not rebind previous editorial observations")


def build_params(source, model, ffmpeg, ffprobe):
    probe = json.loads(subprocess.check_output([str(ffprobe), "-v", "error", "-show_format", "-show_streams", "-of", "json", str(source)],
                                             creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)))
    duration = float(probe["format"]["duration"])
    t0 = float(probe["format"].get("start_time", 0))
    audios = []
    for stream in probe["streams"]:
        if stream["codec_type"] == "audio":
            audios.append({"stream_index": stream["index"], "audio_index": len(audios), "codec": stream["codec_name"],
                           "channels": stream["channels"], "channel_layout": stream.get("channel_layout", ""), "sample_rate": int(stream["sample_rate"]),
                           "start_time": round(float(stream.get("start_time", 0)) * worker.FLICKS), "title": stream.get("tags", {}).get("title", "")})
    size = source.stat().st_size
    sha = hashlib.sha256(str(size).encode("ascii"))
    with source.open("rb") as input_file:
        for offset in (0, max(0, size // 2 - 4 * 1024 * 1024), max(0, size - 8 * 1024 * 1024)):
            input_file.seek(offset)
            sha.update(input_file.read(min(size, 8 * 1024 * 1024)))
    inventory = {"duracion": duration, "t0": t0, "video": None,
                 "pistas": [{"idx": audio["audio_index"], "codec": audio["codec"], "canales": audio["channels"], "sample_rate": audio["sample_rate"],
                             "start_time": audio["start_time"] / worker.FLICKS, "duracion": duration} for audio in audios]}
    fingerprint = {"size": size, "mtime_ns": source.stat().st_mtime_ns, "hash_muestreado": sha.hexdigest(),
                   "inventario_sha256": hashlib.sha256(json.dumps(inventory, sort_keys=True).encode("utf-8")).hexdigest()}
    asset = {"id": "synthetic-audio", "kind": "audio", "name": "Invented two-speaker speech", "path": str(source), "missing": False,
             "probe": {"container": "matroska", "duration": round(duration * worker.FLICKS), "start_time": round(t0 * worker.FLICKS), "audio": audios, "size": size},
             "fingerprint": fingerprint}
    return {"project_id": "synthetic-project", "revision": 0, "project_digest": worker.digest({"test": "synthetic-project"}), "asset": asset,
            "source_path": str(source), "source_sha256": worker.file_hash(source, threading.Event()), "model_path": str(model),
            "model_digest": worker.model_hash(model, threading.Event()), "parameters": {"language": "en", "device": "cpu", "threads": 2, "beam_size": 5, "steps": ["extract", "transcribe"]},
            "ffmpeg_path": str(ffmpeg)}


def main():
    global OPTIONS, PARAMS
    parser = argparse.ArgumentParser()
    for name in ("source", "model", "ffmpeg", "ffprobe", "work-root", "evidence"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--suite", choices=("all", "independent"), default="all")
    OPTIONS = parser.parse_args()
    for name in ("source", "model", "ffmpeg", "ffprobe", "work_root", "evidence"):
        setattr(OPTIONS, name, getattr(OPTIONS, name).resolve())
    if not OPTIONS.source.is_relative_to(REPO) or not OPTIONS.work_root.is_relative_to(REPO) or not OPTIONS.evidence.is_relative_to(REPO):
        parser.error("Fixtures/work/evidence must remain inside V2")
    if OPTIONS.work_root.exists():
        parser.error("Use a new work-root for reproducible first-run tests")
    OPTIONS.work_root.mkdir(parents=True)
    PARAMS = build_params(OPTIONS.source, OPTIONS.model, OPTIONS.ffmpeg, OPTIONS.ffprobe)
    started = time.monotonic()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ExtractionTests)
    if OPTIONS.suite == "all":
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(ContractTests))
    else:
        for name in ("test_01_hello_runtime_and_closed_capabilities", "test_02_run_requires_hello_and_invalid_request_rejected", "test_03_hash_preconditions_before_ffmpeg", "test_07_eof_cancels_and_leaves_no_success_master", "test_08_paths_reject_traversal_and_existing_escape"):
            suite.addTest(ContractTests(name))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    evidence = {"protocol": worker.PROTOCOL, "python": sys.version, "runtime": worker.runtime_versions(), "parameters": PARAMS,
                "worker_sha256": worker.file_hash(REPO / "workers/python/worker.py", threading.Event()),
                "suite": OPTIONS.suite, "elapsed_seconds": time.monotonic()-started, "tests_run": result.testsRun, "success": result.wasSuccessful(),
                "failures": [(str(test), trace) for test, trace in result.failures + result.errors], "events": EVENT_LOG,
                "source_unchanged": worker.file_hash(OPTIONS.source, threading.Event()) == PARAMS["source_sha256"]}
    OPTIONS.evidence.parent.mkdir(parents=True, exist_ok=True)
    OPTIONS.evidence.write_bytes(worker.canonical(evidence))
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
