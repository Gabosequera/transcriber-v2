"""Offline frame laughter inference, no ASR, training code or project mutation.

Head naming/forward adapted by reading Taisei Omine's MIT-licensed model:
omine-me/LaughterSegmentation@a525292d26f744e14624e3a2f1fb5e3c7858d7b3.
See LAUGHTER-UPSTREAM-LICENSE.txt. Trained weights are research-only.
Never imports train.model or code that terminates other processes.
"""
from __future__ import annotations
import argparse
from array import array
from bisect import bisect_right
from collections import deque
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import platform
import struct
import sys
import threading
import time
import uuid
import wave

PROTOCOL = "tv2-laughter/1"
ALGORITHM = "omine-v1-global-maxpool-streamed-amplitude/1"
FLICKS = 705_600_000
MAX_LINE = 1024*1024
OUTPUT = "laughter/events.json"
MODEL_SHA = "449b14f73c70db26da9b4a59ee77d9a9b29fbcaceb083dd7ea27cdfaa68442a0"
CONFIG_SHA = "ffcc5c417fe11433447975d5053b2279fbeafd6bca03dd2753082e72ad2d36b7"


class LaughterError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def check(cancel):
    if cancel.is_set():
        raise LaughterError("E_CANCELLED", "Laughter detection cancelled")


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
            raise OSError(error, "Cannot inspect laughter stdin pipe")
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
    sha = hashlib.sha256()
    with Path(path).open("rb") as source:
        while True:
            check(cancel)
            block = source.read(1024*1024)
            if not block:
                return sha.hexdigest()
            sha.update(block)


def atomic_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name+".tmp-"+uuid.uuid4().hex)
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
        raise LaughterError("E_PATH", "Unsafe laughter artifact path")
    target = root.joinpath(*relative.split("/"))
    if not target.resolve().is_relative_to(root.resolve()):
        raise LaughterError("E_PATH", "Laughter artifact escapes work root")
    return target


def runtime():
    lock = Path(__file__).with_name("requirements-laughter.lock")
    expected = dict(line.split("==", 1) for line in lock.read_text(encoding="utf-8").splitlines() if line and not line.startswith("#"))
    versions = {}
    for name in expected:
        try:
            versions[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            versions[name] = None
    if platform.python_version() != "3.11.15" or versions != expected:
        raise LaughterError("runtime_mismatch", "CPython3.11.15 and exact laughter lock required")
    return {"python": platform.python_version(), "dependencies": versions, "platform": platform.platform(),
            "worker_sha256": file_hash(Path(__file__), threading.Event()), "lock_sha256": file_hash(lock, threading.Event())}


def validate_params(params):
    required = {"job_id", "project_id", "revision", "project_digest", "asset_id", "track_id", "audio_index", "audio_offset_ticks", "source_sha256", "source_duration_ticks", "timebase",
                "normalized_audio_path", "normalized_audio_sha256", "model_path", "config_path", "manifest_path", "model_sha256", "config_sha256", "parameters"}
    if not isinstance(params, dict) or set(params) != required:
        raise LaughterError("E_ARGUMENT", "Closed laughter request required")
    if not isinstance(params["job_id"], str) or not re_id(params["job_id"]):
        raise LaughterError("E_PATH", "Invalid laughter job ID")
    for name in ("project_id", "asset_id", "track_id"):
        if not isinstance(params[name], str) or not re_id(params[name]):
            raise LaughterError("E_ARGUMENT", "Invalid binding ID")
    if type(params["revision"]) is not int or params["revision"] < 0 or type(params["audio_index"]) is not int or params["audio_index"] < 0:
        raise LaughterError("E_ARGUMENT", "Invalid revision/audio index")
    if params["timebase"] != "flicks/705600000" or type(params["source_duration_ticks"]) is not int or not 0 < params["source_duration_ticks"] <= FLICKS*24*3600:
        raise LaughterError("E_ARGUMENT", "Positive source duration in Flicks required")
    if type(params["audio_offset_ticks"]) is not int or not 0 <= params["audio_offset_ticks"] < params["source_duration_ticks"]:
        raise LaughterError("E_ARGUMENT", "Audio offset within source duration required")
    for name in ("project_digest", "source_sha256", "normalized_audio_sha256", "model_sha256", "config_sha256"):
        if not isinstance(params[name], str) or len(params[name]) != 64 or any(char not in "0123456789abcdef" for char in params[name]):
            raise LaughterError("E_ARGUMENT", "Invalid SHA256")
    for name in ("normalized_audio_path", "model_path", "config_path", "manifest_path"):
        if not isinstance(params[name], str) or not Path(params[name]).is_absolute():
            raise LaughterError("E_PATH", "Absolute local laughter paths required")
    options = params["parameters"]
    if not isinstance(options, dict) or set(options) != {"device", "threads", "threshold", "amplitude_boost", "min_dur", "merge_gap", "input_sec", "overlap_sec", "batch_size"}:
        raise LaughterError("E_ARGUMENT", "Closed laughter parameters required")
    if options["device"] != "cpu" or type(options["threads"]) is not int or options["threads"] != 2 or options["input_sec"] != 7 or options["overlap_sec"] != 2:
        raise LaughterError("E_UNSUPPORTED", "Laughter increment uses CPU2 and 7s/2s windows")
    if type(options["batch_size"]) is not int or not 1 <= options["batch_size"] <= 2 or type(options["amplitude_boost"]) is not bool:
        raise LaughterError("E_ARGUMENT", "batch_size1..2 and boolean boost required")
    for name, minimum, maximum in (("threshold", 0.01, 0.99), ("min_dur", 0.02, 10), ("merge_gap", 0, 2)):
        if type(options[name]) not in (int, float) or not math.isfinite(options[name]) or not minimum <= options[name] <= maximum:
            raise LaughterError("E_ARGUMENT", "Invalid laughter threshold/duration")


def re_id(value):
    return 0 < len(value) <= 128 and value[0].isascii() and value[0].isalnum() and all(char.isascii() and (char.isalnum() or char in "_-") for char in value)


def pcm_info(path):
    with wave.open(str(path), "rb") as audio:
        if (audio.getframerate(), audio.getnchannels(), audio.getsampwidth(), audio.getcomptype()) != (16000, 1, 2, "NONE"):
            raise LaughterError("E_UNSUPPORTED", "Normalized mono PCM16 16kHz required")
        if audio.getnframes() <= 0:
            raise LaughterError("E_ARGUMENT", "Empty normalized PCM")
        return audio.getnframes()


def pcm_blocks(path, cancel):
    with wave.open(str(path), "rb") as audio:
        while data := audio.readframes(16000):
            check(cancel)
            yield [value[0] for value in struct.iter_unpack("<h", data)]


def silence_ranges(path, frames, cancel):
    """270ms/-35dBFS/1ms RMS scan equivalent to V1 pydub rules.

    Keeps a ring of 270 energies instead of a whole waveform/prefix. PCM is
    quantized exactly as V1's float→int16 preprocessor for silence measurement.
    """
    total_ms = round(frames/16)
    ring, energy, count, millis, previous, beginning = deque(), 0, 0, 0, None, None
    ranges = []
    threshold = 32768*10**(-35/20)
    for samples in pcm_blocks(path, cancel):
        for offset in range(0, len(samples), 16):
            block = samples[offset:offset+16]
            if millis >= total_ms:
                break
            contribution = sum(math.trunc(sample*32767/32768)**2 for sample in block)
            ring.append((contribution, len(block)))
            energy += contribution
            count += len(block)
            if len(ring) > 270:
                old, size = ring.popleft()
                energy -= old
                count -= size
            millis += 1
            if len(ring) != 270 or int(math.sqrt(energy/count)) > threshold:
                continue
            start = millis-270
            if previous is not None and start > previous+270:
                ranges.append((beginning*16, min(frames, (previous+270)*16)))
                beginning = start
            if beginning is None:
                beginning = start
            previous = start
    if beginning is not None:
        ranges.append((beginning*16, min(frames, (previous+270)*16)))
    return ranges


def amplitude_gain(index, region, total):
    start, end = region
    fade = 2400
    if end-start <= fade*2:
        return 5.0
    if index < start+fade:
        return 1+4*(index-start)/(fade-1)
    if index >= end-fade and end < total:
        return 5-4*(index-(end-fade))/(fade-1)
    return 5.0


def transform_samples(samples, start, total, ranges, peak=1):
    starts = [region[0] for region in ranges]
    region_index = max(0, bisect_right(starts, start)-1)
    output = []
    for offset, sample in enumerate(samples):
        index = start+offset
        while region_index < len(ranges) and index >= ranges[region_index][1]:
            region_index += 1
        gain = amplitude_gain(index, ranges[region_index], total) if region_index < len(ranges) and ranges[region_index][0] <= index < ranges[region_index][1] else 1.0
        output.append(sample/32768*gain/peak)
    return output


def boost_plan(path, frames, cancel):
    ranges = silence_ranges(path, frames, cancel)
    peak, index = 0.0, 0
    for samples in pcm_blocks(path, cancel):
        processed = transform_samples(samples, index, frames, ranges)
        peak = max(peak, max((abs(value) for value in processed), default=0))
        index += len(samples)
    return ranges, peak or 1.0


def pool_window(timeline, probabilities, start, frame_duration):
    base = round(start/frame_duration)
    for index, value in enumerate(probabilities):
        if not math.isfinite(value) or not 0 <= value <= 1:
            raise LaughterError("E_RESULT", "Invalid laughter frame probability")
        if base+index < len(timeline):
            timeline[base+index] = max(timeline[base+index], value)


def extract_events(timeline, frame_duration, duration, options, track_id, cancel):
    candidates, index = [], 0
    while index < len(timeline):
        check(cancel)
        if timeline[index] < options["threshold"]:
            index += 1
            continue
        end = index
        while end < len(timeline) and timeline[end] >= options["threshold"]:
            end += 1
        start_time, end_time = round(index*frame_duration, 3), min(round(end*frame_duration, 3), duration)
        values = timeline[index:end]
        if 0 <= start_time < end_time <= duration:
            maximum = round(max(values), 3)
            event = {"t_ini": start_time, "t_fin": end_time, "tipo": "laughter", "conf": maximum,
                     "max_conf": maximum, "mean_conf": round(math.fsum(values)/len(values), 3)}
            if candidates and event["t_ini"]-candidates[-1]["t_fin"] < options["merge_gap"]:
                candidates[-1]["t_fin"] = event["t_fin"]
                candidates[-1]["conf"] = candidates[-1]["max_conf"] = max(candidates[-1]["max_conf"], event["max_conf"])
                candidates[-1]["mean_conf"] = round((candidates[-1]["mean_conf"]+event["mean_conf"])/2, 3)
            else:
                candidates.append(event)
        index = end
    events = []
    for event in candidates:
        if event["t_fin"]-event["t_ini"] >= options["min_dur"]:
            event.update(dur=round(event["t_fin"]-event["t_ini"], 3), track_id=track_id, event_id=f"{track_id}-laugh-{len(events)+1:05d}")
            events.append(event)
    return events


def absolute_events(events, offset_ticks, source_ticks):
    offset, source_duration = offset_ticks/FLICKS, source_ticks/FLICKS
    for event in events:
        event["t_ini"] += offset
        event["t_fin"] = min(event["t_fin"]+offset, source_duration)
    return events


class Analysis:
    def __init__(self, root, params, cancel, emit):
        validate_params(params)
        self.params, self.cancel, self.emit = params, cancel, emit
        self.root = inside(root.resolve(), params["job_id"])
        self.runtime = runtime()

    def progress(self, stage, fraction, **extra):
        check(self.cancel)
        self.emit({"event": "progress", "job_id": self.params["job_id"], "stage": stage, "fraction": min(1, max(0, fraction)), **extra})

    def verify(self):
        params = self.params
        if params["model_sha256"] != MODEL_SHA or params["config_sha256"] != CONFIG_SHA:
            raise LaughterError("E_PRECONDITION", "Only pinned official laughter weights/config supported")
        for path, sha in (("normalized_audio_path", "normalized_audio_sha256"), ("model_path", "model_sha256"), ("config_path", "config_sha256")):
            if file_hash(params[path], self.cancel) != params[sha]:
                raise LaughterError("E_PRECONDITION", "Laughter source/model/config SHA changed")
        manifest_path = Path(params["manifest_path"])
        if manifest_path.stat().st_size > MAX_LINE:
            raise LaughterError("E_LIMIT", "Laughter manifest too large")
        data = manifest_path.read_bytes()
        manifest = json.loads(data)
        if manifest.get("model") != "omine-me/LaughterSegmentation" or manifest.get("model_license") != "research-only" or manifest.get("model_revision") != "cb10e3920766372f06bbd9657724f24dc39fa3e4":
            raise LaughterError("E_PRECONDITION", "Laughter manifest/model revision mismatch")
        for name, path, sha, size in (("model.safetensors", "model_path", MODEL_SHA, 1261816628), ("config.json", "config_path", CONFIG_SHA, 1531)):
            expected = manifest.get("files", {}).get(name, {})
            if expected.get("sha256") != sha or expected.get("size") != size or Path(params[path]).stat().st_size != size:
                raise LaughterError("E_PRECONDITION", "Laughter manifest bytes mismatch")
        return hashlib.sha256(data).hexdigest()

    def load_model(self):
        self.progress("load-laughter", 0)
        try:
            import torch
            from transformers import Wav2Vec2Config, Wav2Vec2ForAudioFrameClassification
            from safetensors.torch import load_file
        except (ImportError, OSError) as error:
            raise LaughterError("E_DEPENDENCY_RUNTIME", "Installed laughter dependency cannot load: "+str(error)) from error
        check(self.cancel)
        torch.set_num_threads(2)
        config = Wav2Vec2Config.from_dict(json.loads(Path(self.params["config_path"]).read_bytes()))
        config.num_labels, config.problem_type = 1, "single_label_classification"
        class InferenceOnly(torch.nn.Module):
            def __init__(self):
                super().__init__()
                self.audio_model = Wav2Vec2ForAudioFrameClassification(config)
            def forward(self, input_values):
                return self.audio_model(input_values=input_values).logits.squeeze(-1)
        model = InferenceOnly()
        check(self.cancel)
        state = load_file(self.params["model_path"], device="cpu")
        check(self.cancel)
        model.load_state_dict(state, strict=True)
        del state
        model.eval()
        self.progress("load-laughter", 1)
        return torch, model

    def cached(self, key, target, checkpoint):
        try:
            if checkpoint.stat().st_size > MAX_LINE:
                return None
            cached = json.loads(checkpoint.read_bytes())
            if (cached.get("schema") != "tv2-laughter-stage/1" or cached.get("stage") != "laughter" or cached.get("input_digest") != key
                or not isinstance(cached.get("artifacts"), dict) or set(cached["artifacts"]) != {OUTPUT}
                or file_hash(target, self.cancel) != cached["artifacts"][OUTPUT]):
                return None
            return cached["artifacts"]
        except (OSError, ValueError, KeyError, TypeError, AttributeError):
            return None

    def run(self):
        check(self.cancel)
        self.progress("verify-inputs", 0)
        manifest_sha = self.verify()
        frames = pcm_info(self.params["normalized_audio_path"])
        audio_offset = self.params["audio_offset_ticks"]/FLICKS
        duration = min(frames/16000, self.params["source_duration_ticks"]/FLICKS-audio_offset)
        if frames/16000 > duration+1/16000:
            raise LaughterError("E_PRECONDITION", "Normalized PCM exceeds source duration")
        key = digest({"params": self.params, "runtime": self.runtime, "manifest_sha256": manifest_sha, "algorithm": ALGORITHM})
        target, checkpoint = inside(self.root, OUTPUT), inside(self.root, ".work/manifests/laughter.json")
        cached = self.cached(key, target, checkpoint)
        if cached:
            self.progress("laughter", 1, cached=True)
            return self.receipt(cached)
        self.progress("verify-inputs", 1)
        options = self.params["parameters"]
        self.progress("preprocess", 0)
        ranges, peak = boost_plan(self.params["normalized_audio_path"], frames, self.cancel) if options["amplitude_boost"] else ([], 1.0)
        self.progress("preprocess", 1)
        torch, model = self.load_model()
        win, hop, step = 112000, 80000, 80000*options["batch_size"]
        timeline, frame_duration, windows = None, None, 0
        for offset in range(0, frames, step):
            check(self.cancel)
            waveforms = []
            with wave.open(self.params["normalized_audio_path"], "rb") as audio:
                for batch in range(options["batch_size"]):
                    start = offset+batch*hop
                    if start >= frames:
                        break
                    audio.setpos(start)
                    data = audio.readframes(min(win, frames-start))
                    samples = [value[0] for value in struct.iter_unpack("<h", data)]
                    values = transform_samples(samples, start, frames, ranges, peak)
                    values.extend([0.0]*(win-len(values)))
                    waveforms.append(values)
                    if len(samples) < win:
                        break
            with torch.inference_mode():
                probabilities = torch.sigmoid(model(torch.tensor(waveforms, dtype=torch.float32))).cpu().tolist()
            check(self.cancel)
            if len(probabilities) != len(waveforms) or not probabilities or not probabilities[0]:
                raise LaughterError("E_RESULT", "Invalid laughter model output shape")
            if timeline is None:
                frame_duration = 7/len(probabilities[0])
                timeline = array("f", [0.0])*(int(duration/frame_duration)+4)
            for batch, probability in enumerate(probabilities):
                if len(probability) != round(7/frame_duration):
                    raise LaughterError("E_RESULT", "Laughter frame count changed between windows")
                pool_window(timeline, probability, (offset+batch*hop)/16000, frame_duration)
                windows += 1
            self.progress("laughter", min(1, (offset+step)/frames), windows=windows, cached=False)
            if len(waveforms[-1]) == win and offset+(len(waveforms)-1)*hop+win > frames:
                break
        events = extract_events(timeline or [], frame_duration or 0.02, duration, options, self.params["track_id"], self.cancel)
        absolute_events(events, self.params["audio_offset_ticks"], self.params["source_duration_ticks"])
        if self.verify() != manifest_sha:
            raise LaughterError("E_PRECONDITION", "Laughter manifest changed during inference")
        if runtime() != self.runtime:
            raise LaughterError("E_PRECONDITION", "Laughter worker/lock/runtime changed during inference")
        bindings = {name: self.params[name] for name in ("job_id", "project_id", "revision", "project_digest", "asset_id", "track_id", "audio_index", "audio_offset_ticks")}
        result = {"schema": "tv2-laughter-events/1", **bindings, "events": events,
                  "laughter": {"algorithm": ALGORITHM, "runtime": self.runtime, "model_sha256": MODEL_SHA, "config_sha256": CONFIG_SHA,
                               "model_manifest_sha256": manifest_sha, "license": "research-only", "execution_state": "completed",
                               "normalized_audio_sha256": self.params["normalized_audio_sha256"], "source_sha256": self.params["source_sha256"],
                               "source_duration_ticks": self.params["source_duration_ticks"], "timebase": self.params["timebase"],
                               "source_verification": "source SHA bound lineage from host; normalized PCM bytes revalidated",
                               "frame_duration_seconds": frame_duration, "windows": windows, "parameters": options,
                               "maximum_frame_probability": max(timeline) if timeline else None, "editorial_decisions": "none"}}
        atomic_json(target, result)
        artifacts = {OUTPUT: file_hash(target, self.cancel)}
        check(self.cancel)
        atomic_json(checkpoint, {"schema": "tv2-laughter-stage/1", "stage": "laughter", "input_digest": key, "artifacts": artifacts})
        self.progress("complete", 1, cached=False)
        return self.receipt(artifacts)

    def receipt(self, artifacts):
        return {name: self.params[name] for name in ("job_id", "project_id", "revision", "project_digest", "asset_id", "track_id", "audio_index", "audio_offset_ticks", "source_sha256", "source_duration_ticks", "timebase", "normalized_audio_sha256", "model_sha256", "config_sha256")} | {"laughter_path": OUTPUT, "artifacts": artifacts}


class Worker:
    def __init__(self, root):
        self.root, self.lock = root.resolve(), threading.Lock()
        self.active, self.cancel, self.job_id, self.initialized = None, None, None, False
    def emit(self, value):
        data = canonical({"protocol": PROTOCOL, **value})
        if len(data) > MAX_LINE:
            raise LaughterError("E_LIMIT", "Laughter message too large")
        with self.lock:
            sys.stdout.buffer.write(data+b"\n")
            sys.stdout.buffer.flush()
    def error(self, request_id, error):
        self.emit({"id": request_id, "error": {"code": getattr(error, "code", "E_LAUGHTER"), "message": str(error)[:4096]}})
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
            identifier = None
            try:
                if len(line) > MAX_LINE or not line.endswith(b"\n"):
                    raise LaughterError("E_LIMIT", "Oversized or incomplete laughter request")
                request = json.loads(line)
                if not isinstance(request, dict):
                    raise LaughterError("E_PROTOCOL", "Laughter request must be object")
                identifier = request.get("id")
                if set(request) != {"protocol", "id", "method", "params"} or request["protocol"] != PROTOCOL or type(identifier) not in (str, int):
                    raise LaughterError("E_PROTOCOL", "Invalid laughter envelope")
                method, params = request["method"], request["params"]
                if method == "hello":
                    self.emit({"id": identifier, "result": {"protocol": PROTOCOL, "runtime": runtime(), "capabilities": {
                        "laughter": {"implemented": True, "runtime_state": "not_loaded", "license": "research-only", "devices": ["cpu"], "threads": 2},
                        "output_schema": "tv2-laughter-events/1", "project_mutation": False, "gui_integration": False}}})
                    self.initialized = True
                elif method == "run":
                    if not self.initialized:
                        raise LaughterError("E_PROTOCOL", "Successful hello required before laughter detection")
                    if self.active and self.active.is_alive():
                        raise LaughterError("E_BUSY", "One laughter job per worker")
                    validate_params(params)
                    self.cancel, self.job_id = threading.Event(), params["job_id"]
                    self.active = threading.Thread(target=self.analyze, args=(identifier, params), daemon=True)
                    self.active.start()
                elif method == "cancel":
                    if not isinstance(params, dict) or params.get("job_id") != self.job_id:
                        raise LaughterError("E_ARGUMENT", "Cancel requires active laughter job ID")
                    active = self.active is not None and self.active.is_alive()
                    if active:
                        self.cancel.set()
                    self.emit({"id": identifier, "result": {"job_id": self.job_id, "cancel_requested": active}})
                elif method == "shutdown":
                    self.stop()
                    self.emit({"id": identifier, "result": {"shutdown": True}})
                    return
                else:
                    raise LaughterError("E_METHOD", "Unknown laughter method")
            except Exception as error:
                self.error(identifier, error)
                if len(line) > MAX_LINE:
                    break
        self.stop()



def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-root", required=True, type=Path)
    options = parser.parse_args()
    for name in ("HF_TOKEN", "HUGGING_FACE_HUB_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY"):
        os.environ.pop(name, None)
    os.environ.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", HF_HUB_DISABLE_IMPLICIT_TOKEN="1")
    Worker(options.work_root).serve()


if __name__ == "__main__":
    main()
