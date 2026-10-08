"""Offline audEERING arousal worker. V1 is never imported; only run loads ML.

The host owns cancellation deadlines/process-tree termination. A Torch native
forward cannot be interrupted cooperatively until it returns. No project writes.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import struct
import sys
import threading
import time
import uuid
import wave

PROTOCOL = "tv2-arousal/1"
ALGORITHM = "v1-audeering-speech-regions-windowed-cpu/1"
MODEL = "audeering/wav2vec2-large-robust-12-ft-emotion-msp-dim"
MODEL_REVISION = "6eba34a2485ea31cb03600241787c3a5edab8626"
WEIGHTS_SHA256 = "efa5ac1a13b2d2f42182738e44794b1eb4c0cdd221a8b4ae11304c3a5f5fae95"
MODEL_FILES = {"model.safetensors", "config.json", "preprocessor_config.json"}
PINNED_FILES = {"model.safetensors": (661375508, WEIGHTS_SHA256),
                "config.json": (2344, "c0962c3d1f065972bbebbba0bbffb8016ef4e9aae4a9b07e5fec22f770d2cddb"),
                "preprocessor_config.json": (214, "60ca5a31e13f69ee2fbf147504c8676db5f6398fd7a6b12294341dff838edfcf")}
OUTPUT = "arousal/arousal-words.json"
MAX_LINE = 1024 * 1024
SAMPLE_RATE = 16000
FLICKS = 705_600_000


class ArousalError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def check(cancel):
    if cancel.is_set():
        raise ArousalError("E_CANCELLED", "Arousal cancelled")


def request_lines(stream):
    """Bounded incremental NDJSON without holding Windows CRT stdin locks.

    A blocking Python stdin read on another thread can hold the CRT descriptor
    lock needed during NumPy DLL initialization. PeekNamedPipe reports bytes
    already available; the sole reader consumes at most that count via os.read.
    Partial envelopes remain bounded and EOF still yields an incomplete line.
    """
    if sys.platform != "win32":
        while line := stream.readline(MAX_LINE+1):
            yield line
        return
    import ctypes
    from ctypes import wintypes
    import msvcrt
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetFileType.argtypes = (wintypes.HANDLE,)
    kernel.GetFileType.restype = wintypes.DWORD
    kernel.PeekNamedPipe.argtypes = (wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p,
                                    ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p)
    kernel.PeekNamedPipe.restype = wintypes.BOOL
    descriptor = stream.fileno()
    handle = msvcrt.get_osfhandle(descriptor)
    if kernel.GetFileType(handle) != 3:
        # A regular file does not wait for more bytes while holding its CRT lock.
        while line := stream.readline(MAX_LINE+1):
            yield line
        return
    buffered = bytearray()
    while True:
        available = wintypes.DWORD()
        if not kernel.PeekNamedPipe(handle, None, 0, None, ctypes.byref(available), None):
            error = ctypes.get_last_error()
            if error in (109, 232):  # Broken/disconnected owned stdin pipe.
                if buffered:
                    yield bytes(buffered)
                return
            raise OSError(error, "Cannot inspect arousal stdin pipe")
        if not available.value:
            time.sleep(.02)
            continue
        chunk = os.read(descriptor, min(available.value, MAX_LINE+1-len(buffered)))
        if not chunk:
            if buffered:
                yield bytes(buffered)
            return
        buffered.extend(chunk)
        while (boundary := buffered.find(b"\n")) >= 0:
            yield bytes(buffered[:boundary+1])
            del buffered[:boundary+1]
        if len(buffered) > MAX_LINE:
            yield bytes(buffered)
            return


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def file_hash(path, cancel):
    check(cancel)
    sha = hashlib.sha256()
    with Path(path).open("rb") as source:
        while True:
            check(cancel)
            block = source.read(1024 * 1024)
            if not block:
                return sha.hexdigest()
            sha.update(block)


def atomic_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp-" + uuid.uuid4().hex)
    try:
        with temporary.open("xb") as output:
            output.write(canonical(value))
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def inside(root, relative):
    if not isinstance(relative, str) or "\\" in relative or ":" in relative or any(part in ("", ".", "..") for part in relative.split("/")):
        raise ArousalError("E_PATH", "Unsafe arousal artifact path")
    path = root.joinpath(*relative.split("/"))
    if not path.resolve().is_relative_to(root.resolve()):
        raise ArousalError("E_PATH", "Arousal artifact escapes work root")
    return path


def read_json(path, limit):
    with Path(path).open("rb") as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise ArousalError("E_LIMIT", "Arousal input document exceeds limit")
    return json.loads(data)


def runtime():
    lock = Path(__file__).with_name("requirements-arousal.lock")
    if not lock.is_file():
        raise ArousalError("runtime_mismatch", "Isolated arousal runtime lock is not prepared")
    expected = dict(line.split("==", 1) for line in lock.read_text(encoding="utf-8").splitlines() if line and not line.startswith("#"))
    actual = {}
    for name in expected:
        try:
            actual[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            actual[name] = None
    if platform.python_version() != "3.12.13" or actual != expected:
        raise ArousalError("runtime_mismatch", "Python 3.12.13 and exact isolated arousal lock required")
    cancel = threading.Event()
    return {"python": platform.python_version(), "packages": actual, "platform": platform.platform(),
            "worker_sha256": file_hash(Path(__file__), cancel), "lock_sha256": file_hash(lock, cancel)}


def finite(value, name):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise ArousalError("E_ARGUMENT", name + " must be finite numeric")
    return float(value)


def validate_params(params):
    required = {"job_id", "project_id", "revision", "project_digest", "asset_id", "normalized_audio_path", "normalized_audio_sha256",
                "transcript_path", "transcript_sha256", "model_path", "model_digest", "manifest_path", "manifest_sha256", "parameters",
                "source_sha256", "source_duration_ticks", "timebase"}
    if not isinstance(params, dict) or set(params) != required:
        raise ArousalError("E_ARGUMENT", "Closed arousal request required")
    if not isinstance(params["job_id"], str) or not re.fullmatch(r"job-[a-f0-9]{12}", params["job_id"]):
        raise ArousalError("E_ARGUMENT", "Invalid arousal job ID")
    for key in ("project_id", "asset_id"):
        if not isinstance(params[key], str) or not 0 < len(params[key]) <= 256:
            raise ArousalError("E_ARGUMENT", "Invalid arousal identity")
    if type(params["revision"]) is not int or not 0 <= params["revision"] < 2**64:
        raise ArousalError("E_ARGUMENT", "Invalid arousal revision")
    for key in ("project_digest", "normalized_audio_sha256", "transcript_sha256", "model_digest", "manifest_sha256", "source_sha256"):
        if not isinstance(params[key], str) or not re.fullmatch(r"[a-f0-9]{64}", params[key]):
            raise ArousalError("E_ARGUMENT", "Invalid arousal digest")
    for key in ("normalized_audio_path", "transcript_path", "model_path", "manifest_path"):
        if not isinstance(params[key], str) or not params[key] or len(params[key]) > 32768:
            raise ArousalError("E_ARGUMENT", "Invalid arousal input locator")
    if params["timebase"] != "canonical-normalized-pcm/1" or type(params["source_duration_ticks"]) is not int or not 0 < params["source_duration_ticks"] <= 24*3600*FLICKS:
        raise ArousalError("E_ARGUMENT", "Arousal requires canonical normalized PCM timebase/duration")
    options = params["parameters"]
    if not isinstance(options, dict) or set(options) != {"device", "threads", "window_seconds", "hop_seconds", "batch_size", "gap_seconds", "padding_seconds", "scope"}:
        raise ArousalError("E_ARGUMENT", "Closed arousal parameters required")
    if options["device"] != "cpu" or type(options["threads"]) is not int or not 1 <= options["threads"] <= 4 or type(options["batch_size"]) is not int or not 1 <= options["batch_size"] <= 8:
        raise ArousalError("E_ARGUMENT", "Arousal supports CPU, 1–4 threads and batches 1–8")
    window, hop = finite(options["window_seconds"], "window"), finite(options["hop_seconds"], "hop")
    gap, padding = finite(options["gap_seconds"], "gap"), finite(options["padding_seconds"], "padding")
    if not 0.1 <= hop <= window <= 30 or not 0 <= gap <= 30 or not 0 <= padding <= 10 or options["scope"] not in ("speech-regions", "full-audio"):
        raise ArousalError("E_ARGUMENT", "Invalid arousal windows or speech scope")


def word_times(word):
    start = finite(word.get("t_ini", word.get("start")), "word start")
    end = finite(word.get("t_fin", word.get("end")), "word end")
    return start, end


def validate_track(track, duration):
    if not isinstance(track, dict) or not isinstance(track.get("words"), list) or len(track["words"]) > 1_000_000:
        raise ArousalError("E_ARGUMENT", "Invalid arousal word track")
    ids = set()
    for word in track["words"]:
        if not isinstance(word, dict) or not isinstance(word.get("text"), str) or not isinstance(word.get("word_id"), str) or not word["word_id"] or word["word_id"] in ids:
            raise ArousalError("E_ARGUMENT", "Word identity/text missing or duplicated")
        start, end = word_times(word)
        if not 0 <= start < end <= duration:
            raise ArousalError("E_ARGUMENT", "Word outside normalized audio")
        ids.add(word["word_id"])


def speech_regions(words, *, gap_seconds=1.0, padding_seconds=0.5, duration):
    gap, padding = finite(gap_seconds, "gap"), finite(padding_seconds, "padding")
    if gap < 0 or padding < 0:
        raise ArousalError("E_ARGUMENT", "Invalid speech region gap/padding")
    merged = []
    for start, end in sorted(word_times(word) for word in words):
        if merged and start <= merged[-1][1] + gap:
            merged[-1][1] = max(merged[-1][1], end)
        else:
            merged.append([start, end])
    padded = []
    for start, end in merged:
        region = [max(0.0, start-padding), min(duration, end+padding)]
        if region[1] <= region[0]:
            continue
        if padded and region[0] <= padded[-1][1]:
            padded[-1][1] = max(padded[-1][1], region[1])
        else:
            padded.append(region)
    return [(round(start, 3), round(end, 3)) for start, end in padded]


def window_offsets(length, hop, regions):
    if type(length) is not int or length <= 0 or type(hop) is not int or hop <= 0:
        raise ArousalError("E_ARGUMENT", "Invalid arousal sample/hop count")
    if regions is None:
        return list(range(0, length, hop))
    offsets = set()
    for start_seconds, end_seconds in regions:
        start = max(0, math.floor(start_seconds*SAMPLE_RATE/hop)*hop)
        end = min(length, max(start, math.ceil(end_seconds*SAMPLE_RATE)))
        offsets.update(range(start, end, hop))
    return sorted(offsets)


def read_chunk(path, offset, count, cancel):
    check(cancel)
    with wave.open(str(path), "rb") as audio:
        if (audio.getframerate(), audio.getnchannels(), audio.getsampwidth(), audio.getcomptype()) != (SAMPLE_RATE, 1, 2, "NONE"):
            raise ArousalError("E_UNSUPPORTED", "Arousal requires normalized PCM16 mono 16 kHz")
        if not 0 <= offset <= audio.getnframes() or count < 0:
            raise ArousalError("E_ARGUMENT", "Invalid arousal sample window")
        valid = min(count, audio.getnframes()-offset)
        audio.setpos(offset)
        data = audio.readframes(valid)
        if len(data) != valid*2:
            raise ArousalError("E_RESULT", "Truncated normalized PCM")
    check(cancel)
    samples = [value[0]/32768 for value in struct.iter_unpack("<h", data)]
    return samples + [0.0]*(count-valid), valid


def event_from_values(offset, valid, samples, values):
    if len(values) != 3 or not valid:
        raise ArousalError("E_RESULT", "Invalid arousal model output cardinality")
    predictions = [finite(value, "model prediction") for value in values]
    rms = math.sqrt(math.fsum(sample*sample for sample in samples[:valid])/valid)
    return {"t_ini": round(offset/SAMPLE_RATE, 3), "t_fin": round((offset+valid)/SAMPLE_RATE, 3),
            "arousal": round(predictions[0], 6), "dominance": round(predictions[1], 6), "valence": round(predictions[2], 6),
            "rms_dbfs": round(20*math.log10(max(rms, 1e-9)), 3)}


def baseline(events):
    if not events:
        return {"mean": 0.0, "std": 1.0}
    values = [finite(event["arousal"], "arousal") for event in events]
    mean, std = statistics.fmean(values), statistics.pstdev(values) or 1.0
    for event in events:
        event["arousal_z"] = round((event["arousal"]-mean)/std, 3)
    return {"mean": round(mean, 6), "std": round(std, 6)}


def associate_arousal(words, events):
    # Process sorted words but return original order/IDs and deep copies. V1's
    # sorted-window overlap semantics use half-open intervals; no matching value
    # is null, not zero. Overlapping model windows each contribute their weight.
    enriched = [copy.deepcopy(word) for word in words]
    ordered_events = sorted(events, key=lambda event: event["t_ini"])
    first = 0
    for index in sorted(range(len(words)), key=lambda i: word_times(words[i])[0]):
        start, end = word_times(words[index])
        while first < len(ordered_events) and ordered_events[first]["t_fin"] <= start:
            first += 1
        weight = value = value_z = 0.0
        for position in range(first, len(ordered_events)):
            event = ordered_events[position]
            if event["t_ini"] >= end:
                break
            overlap = max(0.0, min(end, event["t_fin"])-max(start, event["t_ini"]))
            if overlap:
                weight += overlap
                value += overlap*event["arousal"]
                value_z += overlap*event["arousal_z"]
        enriched[index]["arousal"] = round(value/weight, 6) if weight else None
        enriched[index]["arousal_z"] = round(value_z/weight, 3) if weight else None
    return enriched


def checkpoint(root, key, cancel):
    path = inside(root, ".work/manifests/arousal.json")
    try:
        value = read_json(path, MAX_LINE)
        if value.get("schema") != "tv2-arousal-stage/1" or value.get("stage") != "arousal" or value.get("input_digest") != key or not isinstance(value.get("artifacts"), dict) or set(value["artifacts"]) != {OUTPUT}:
            return None
        if file_hash(inside(root, OUTPUT), cancel) != value["artifacts"][OUTPUT]:
            return None
        return value["artifacts"]
    except (OSError, ValueError, KeyError, TypeError):
        return None
    except ArousalError as error:
        if error.code == "E_CANCELLED":
            raise
        return None


class Analysis:
    def __init__(self, root, params, cancel, emit):
        validate_params(params)
        self.params, self.cancel, self.emit = params, cancel, emit
        self.root = inside(root.resolve(), params["job_id"])
        self.runtime = runtime()

    def progress(self, stage, fraction, **extra):
        check(self.cancel)
        self.emit({"event": "progress", "job_id": self.params["job_id"], "stage": stage, "fraction": fraction, **extra})

    def preconditions(self):
        for path, sha in (("normalized_audio_path", "normalized_audio_sha256"), ("transcript_path", "transcript_sha256"), ("manifest_path", "manifest_sha256")):
            if file_hash(self.params[path], self.cancel) != self.params[sha]:
                raise ArousalError("E_PRECONDITION", path + " SHA changed")
        manifest = read_json(self.params["manifest_path"], MAX_LINE)
        if manifest.get("schema") != "tv2-arousal-model/1" or manifest.get("model") != MODEL or manifest.get("revision") != MODEL_REVISION or manifest.get("license") != "cc-by-nc-sa-4.0" or set(manifest.get("files", {})) != MODEL_FILES:
            raise ArousalError("E_PRECONDITION", "Arousal model manifest mismatch")
        model = Path(self.params["model_path"])
        hashes = {}
        for name in sorted(MODEL_FILES):
            path = inside(model.resolve(), name)
            hashes[name] = file_hash(path, self.cancel)
            entry = manifest["files"][name]
            if not isinstance(entry, dict) or hashes[name] != entry.get("sha256") or type(entry.get("size")) is not int or path.stat().st_size != entry["size"]:
                raise ArousalError("E_PRECONDITION", "Arousal model file changed")
            if (entry["size"], hashes[name]) != PINNED_FILES[name]:
                raise ArousalError("E_PRECONDITION", "Arousal file differs from pinned revision")
        if hashes["model.safetensors"] != WEIGHTS_SHA256 or manifest["files"]["model.safetensors"]["size"] != 661375508 or digest(hashes) != self.params["model_digest"]:
            raise ArousalError("E_PRECONDITION", "Arousal weights differ from pinned public source")
        config = read_json(model/"config.json", MAX_LINE)
        processor = read_json(model/"preprocessor_config.json", MAX_LINE)
        if config.get("model_type") != "wav2vec2" or config.get("hidden_size") != 1024 or config.get("num_hidden_layers") != 12 or config.get("id2label") != {"0": "arousal", "1": "dominance", "2": "valence"} or processor.get("sampling_rate") != SAMPLE_RATE or processor.get("do_normalize") is not True:
            raise ArousalError("E_PRECONDITION", "Arousal architecture/processor mismatch")

    def load_model(self):
        check(self.cancel)
        self.progress("load-arousal", 0)
        try:
            self.progress("load-arousal", 0, component="torch-import")
            import torch
            self.progress("load-arousal", 0, component="safetensors-import")
            from safetensors.torch import load_file
            self.progress("load-arousal", 0, component="transformers-import")
            from transformers import Wav2Vec2Config, Wav2Vec2FeatureExtractor
            from transformers.models.wav2vec2.modeling_wav2vec2 import Wav2Vec2Model, Wav2Vec2PreTrainedModel
        except (ImportError, OSError) as error:
            raise ArousalError("E_DEPENDENCY_RUNTIME", "Arousal native runtime cannot load: " + str(error)) from error
        check(self.cancel)
        torch.set_num_threads(self.params["parameters"]["threads"])

        class RegressionHead(torch.nn.Module):
            def __init__(self, config):
                super().__init__()
                self.dense = torch.nn.Linear(config.hidden_size, config.hidden_size)
                self.dropout = torch.nn.Dropout(config.final_dropout)
                self.out_proj = torch.nn.Linear(config.hidden_size, config.num_labels)

            def forward(self, values):
                return self.out_proj(self.dropout(torch.tanh(self.dense(self.dropout(values)))))

        class EmotionModel(Wav2Vec2PreTrainedModel):
            def __init__(self, config):
                super().__init__(config)
                self.wav2vec2, self.classifier = Wav2Vec2Model(config), RegressionHead(config)
                self.init_weights()

            def forward(self, values):
                return self.classifier(self.wav2vec2(values)[0].mean(dim=1))

        self.progress("load-arousal", .1, component="local-config")
        config = Wav2Vec2Config.from_pretrained(self.params["model_path"], local_files_only=True)
        processor = Wav2Vec2FeatureExtractor.from_pretrained(self.params["model_path"], local_files_only=True)
        model = EmotionModel(config)
        check(self.cancel)
        self.progress("load-arousal", .5, component="safetensors-load")
        state = load_file(str(Path(self.params["model_path"])/"model.safetensors"), device="cpu")
        model.load_state_dict(state, strict=True)
        del state
        check(self.cancel)
        model.eval()
        self.progress("load-arousal", 1)
        return torch, processor, model

    def run(self):
        check(self.cancel)
        self.progress("verify-inputs", 0)
        self.preconditions()
        with wave.open(self.params["normalized_audio_path"], "rb") as audio:
            if (audio.getframerate(), audio.getnchannels(), audio.getsampwidth(), audio.getcomptype()) != (SAMPLE_RATE, 1, 2, "NONE"):
                raise ArousalError("E_UNSUPPORTED", "Arousal requires normalized PCM16 mono 16 kHz")
            length = audio.getnframes()
        duration = self.params["source_duration_ticks"]/FLICKS
        if not 0 < length/SAMPLE_RATE <= duration + 1/SAMPLE_RATE:
            raise ArousalError("E_PRECONDITION", "Normalized PCM exceeds source duration")
        track = read_json(self.params["transcript_path"], 64*1024*1024)
        validate_track(track, min(duration, length/SAMPLE_RATE))
        options = self.params["parameters"]
        regions = speech_regions(track["words"], gap_seconds=options["gap_seconds"], padding_seconds=options["padding_seconds"], duration=min(duration, length/SAMPLE_RATE)) if options["scope"] == "speech-regions" else None
        window, hop = max(1, round(options["window_seconds"]*SAMPLE_RATE)), max(1, round(options["hop_seconds"]*SAMPLE_RATE))
        offsets = window_offsets(length, hop, regions)
        key = digest({"request": self.params, "runtime": self.runtime, "algorithm": ALGORITHM})
        cached = checkpoint(self.root, key, self.cancel)
        if cached:
            check(self.cancel)
            self.progress("complete", 1, cached=True)
            return self.receipt(cached)
        self.progress("verify-inputs", 1)
        loaded, events = None, []
        for first in range(0, len(offsets), options["batch_size"]):
            check(self.cancel)
            if loaded is None:
                loaded = self.load_model()
            torch, processor, model = loaded
            positions = offsets[first:first+options["batch_size"]]
            chunks = [read_chunk(self.params["normalized_audio_path"], offset, window, self.cancel) for offset in positions]
            inputs = processor([samples for samples, _ in chunks], sampling_rate=SAMPLE_RATE, padding=True, return_tensors="pt")["input_values"]
            check(self.cancel)
            with torch.inference_mode():
                predictions = model(inputs).detach().cpu().tolist()
            check(self.cancel)
            if len(predictions) != len(positions):
                raise ArousalError("E_RESULT", "Arousal batch cardinality mismatch")
            for offset, (samples, valid), values in zip(positions, chunks, predictions):
                event = event_from_values(offset, valid, samples, values)
                if event["t_ini"] < event["t_fin"]:
                    events.append(event)
            self.progress("arousal", min(1.0, (first+len(positions))/len(offsets)), cached=False)
        check(self.cancel)
        self.preconditions()
        arousal = {"schema": "editorial-arousal/1", "model": MODEL, "window_seconds": options["window_seconds"], "hop_seconds": options["hop_seconds"],
                   "scope": options["scope"], "analyzed_regions": [list(region) for region in regions] if regions is not None else [[0.0, round(min(duration, length/SAMPLE_RATE), 3)]],
                   "baseline": baseline(events), "events": events}
        result = copy.deepcopy(track)
        result["words"] = associate_arousal(track["words"], events)
        result["arousal"] = arousal
        result["arousal_analysis"] = {"schema": "tv2-arousal-words/1", "algorithm": ALGORITHM, "runtime": self.runtime,
                                      "model_digest": self.params["model_digest"], "manifest_sha256": self.params["manifest_sha256"],
                                      "normalized_audio_sha256": self.params["normalized_audio_sha256"], "transcript_sha256": self.params["transcript_sha256"],
                                      "source_sha256": self.params["source_sha256"], "source_duration_ticks": self.params["source_duration_ticks"], "timebase": self.params["timebase"],
                                      "execution_state": "completed" if loaded else "no_speech_windows", "native_inference_executed": loaded is not None,
                                      "source_verification": "source SHA is host-bound lineage; worker verifies normalized PCM and transcript bytes",
                                      "editorial_decisions": "none", "language_validation": "trained on English MSP-Podcast; Spanish accuracy not accepted"}
        check(self.cancel)
        target = inside(self.root, OUTPUT)
        atomic_json(target, result)
        artifacts = {OUTPUT: file_hash(target, self.cancel)}
        check(self.cancel)
        atomic_json(inside(self.root, ".work/manifests/arousal.json"), {"schema": "tv2-arousal-stage/1", "stage": "arousal", "input_digest": key, "artifacts": artifacts})
        self.progress("complete", 1, cached=False)
        return self.receipt(artifacts)

    def receipt(self, artifacts):
        return {name: self.params[name] for name in ("job_id", "project_id", "revision", "project_digest", "asset_id", "normalized_audio_sha256", "transcript_sha256", "model_digest", "manifest_sha256", "source_sha256", "source_duration_ticks", "timebase")} | {"arousal_path": OUTPUT, "artifacts": artifacts}


class Worker:
    def __init__(self, root):
        self.root, self.lock = root.resolve(), threading.Lock()
        self.active, self.cancel, self.job_id, self.initialized = None, None, None, False

    def emit(self, value):
        data = canonical({"protocol": PROTOCOL, **value})
        if len(data) >= MAX_LINE:
            raise ArousalError("E_LIMIT", "Arousal response exceeds NDJSON limit")
        with self.lock:
            sys.stdout.buffer.write(data+b"\n")
            sys.stdout.buffer.flush()

    def error(self, request_id, error):
        code = getattr(error, "code", "E_AROUSAL")
        if isinstance(error, MemoryError):
            code = "E_OOM"
        if "Control de aplicaciones" in str(error) or "application control" in str(error).lower():
            code = "E_DEPENDENCY_BLOCKED"
        self.emit({"id": request_id, "error": {"code": code, "message": str(error)[:4096]}})

    def analyze(self, request_id, params):
        try:
            result = Analysis(self.root, params, self.cancel, self.emit).run()
            check(self.cancel)
            self.emit({"id": request_id, "result": result})
        except Exception as error:
            self.error(request_id, error)

    def stop(self):
        if self.cancel:
            self.cancel.set()
        if self.active:
            self.active.join(timeout=5)

    def serve(self):
        for line in request_lines(sys.stdin.buffer):
            request_id = None
            try:
                if len(line) > MAX_LINE or not line.endswith(b"\n"):
                    raise ArousalError("E_LIMIT", "Oversized or incomplete arousal request")
                request = json.loads(line)
                if not isinstance(request, dict):
                    raise ArousalError("E_PROTOCOL", "Arousal request must be object")
                request_id = request.get("id")
                if set(request) != {"protocol", "id", "method", "params"} or request["protocol"] != PROTOCOL or type(request_id) not in (str, int):
                    raise ArousalError("E_PROTOCOL", "Invalid arousal envelope")
                method, params = request["method"], request["params"]
                if method == "hello":
                    value = runtime()
                    self.emit({"id": request_id, "result": {"protocol": PROTOCOL, "runtime": value,
                              "capabilities": {"arousal": {"implemented": True, "runtime_state": "not_loaded", "model": MODEL, "devices": ["cpu"]},
                                               "output_schema": "tv2-arousal-words/1", "asr": False, "project_mutation": False, "gui_integration": False}}})
                    self.initialized = True
                elif method == "run":
                    if not self.initialized:
                        raise ArousalError("E_PROTOCOL", "Successful hello required before arousal")
                    if self.active and self.active.is_alive():
                        raise ArousalError("E_BUSY", "One arousal job per worker")
                    validate_params(params)
                    self.cancel, self.job_id = threading.Event(), params["job_id"]
                    self.active = threading.Thread(target=self.analyze, args=(request_id, params), daemon=True)
                    self.active.start()
                elif method == "cancel":
                    if not isinstance(params, dict) or set(params) != {"job_id"} or params["job_id"] != self.job_id:
                        raise ArousalError("E_ARGUMENT", "Cancel requires active arousal job ID")
                    active = self.active is not None and self.active.is_alive()
                    if active:
                        self.cancel.set()
                    self.emit({"id": request_id, "result": {"job_id": self.job_id, "cancel_requested": active}})
                elif method == "shutdown":
                    self.stop()
                    self.emit({"id": request_id, "result": {"shutdown": True}})
                    return
                else:
                    raise ArousalError("E_METHOD", "Unknown arousal method")
            except Exception as error:
                self.error(request_id, error)
                if len(line) > MAX_LINE:
                    break
        self.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--diagnostic-trace-after", type=int, choices=range(10, 121))
    args = parser.parse_args()
    for name in ("HF_TOKEN", "HUGGING_FACE_HUB_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY"):
        os.environ.pop(name, None)
    os.environ.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", HF_HUB_DISABLE_IMPLICIT_TOKEN="1")
    if args.diagnostic_trace_after:
        import faulthandler
        faulthandler.dump_traceback_later(args.diagnostic_trace_after, repeat=False)
    try:
        Worker(args.work_root).serve()
    finally:
        if args.diagnostic_trace_after:
            faulthandler.cancel_dump_traceback_later()


if __name__ == "__main__":
    main()
