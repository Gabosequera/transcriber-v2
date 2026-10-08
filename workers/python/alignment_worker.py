"""Offline MMS alignment transport. Separate from ASR; never edits a project.

Only run loads Torch. The host must supervise this process tree and terminate
after its cooperative deadline: one native forward call cannot be interrupted.
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
import struct
import sys
import threading
import time
import unicodedata
import uuid
import wave
import zipfile

PROTOCOL = "tv2-alignment/1"
ALGORITHM = "v1-mms-fa-cpu-bounded/1"
MAX_LINE = 1024*1024
OUTPUT = "alignment/aligned-words.json"
FLICKS = 705_600_000


class AlignmentError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def check(cancel):
    if cancel.is_set():
        raise AlignmentError("E_CANCELLED", "Alignment cancelled")


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
            raise OSError(error, "Cannot inspect alignment stdin pipe")
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
    output = hashlib.sha256()
    with Path(path).open("rb") as source:
        while True:
            check(cancel)
            block = source.read(1024*1024)
            if not block:
                return output.hexdigest()
            output.update(block)


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
    if not isinstance(relative, str) or "\\" in relative or ":" in relative or any(value in ("", ".", "..") for value in relative.split("/")):
        raise AlignmentError("E_PATH", "Unsafe alignment artifact path")
    path = root.joinpath(*relative.split("/"))
    if not path.resolve().is_relative_to(root.resolve()):
        raise AlignmentError("E_PATH", "Alignment artifact escapes work root")
    return path


def runtime():
    lock = Path(__file__).with_name("requirements-alignment.lock")
    expected = dict(line.split("==", 1) for line in lock.read_text(encoding="utf-8").splitlines() if line and not line.startswith("#"))
    actual = {}
    for name in expected:
        try:
            actual[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            actual[name] = None
    if platform.python_version() != "3.12.13" or actual != expected:
        raise AlignmentError("runtime_mismatch", "Python 3.12.13 and exact alignment lock required")
    cancel = threading.Event()
    return {"python": platform.python_version(), "dependencies": actual, "worker_sha256": file_hash(Path(__file__), cancel),
            "lock_sha256": file_hash(lock, cancel), "platform": platform.platform()}


def normalize(text):
    value = unicodedata.normalize("NFKD", text)
    return "".join(char for char in value.lower() if not unicodedata.combining(char) and ("a" <= char <= "z" or char == "'"))


def validate_params(params):
    required = {"job_id", "project_id", "revision", "project_digest", "asset_id", "normalized_audio_path", "normalized_audio_sha256",
                "transcript_path", "transcript_sha256", "model_path", "manifest_path", "model_sha256", "parameters",
                "source_sha256", "source_duration_ticks", "timebase"}
    if not isinstance(params, dict) or set(params) != required:
        raise AlignmentError("E_ARGUMENT", "Closed alignment request required")
    if not isinstance(params["job_id"], str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,127}", params["job_id"]):
        raise AlignmentError("E_PATH", "Invalid alignment job ID")
    if type(params["revision"]) is not int or params["revision"] < 0:
        raise AlignmentError("E_ARGUMENT", "Invalid revision")
    for name in ("project_id", "asset_id"):
        if not isinstance(params[name], str) or not params[name] or len(params[name]) > 128:
            raise AlignmentError("E_ARGUMENT", "Invalid binding ID")
    if params["timebase"] != "flicks/705600000" or type(params["source_duration_ticks"]) is not int or not 0 < params["source_duration_ticks"] <= FLICKS*24*3600:
        raise AlignmentError("E_ARGUMENT", "Positive source duration in Flicks required")
    for name in ("project_digest", "normalized_audio_sha256", "transcript_sha256", "model_sha256", "source_sha256"):
        if not isinstance(params[name], str) or not re.fullmatch(r"[0-9a-f]{64}", params[name]):
            raise AlignmentError("E_ARGUMENT", "Invalid SHA256")
    for name in ("normalized_audio_path", "transcript_path", "model_path", "manifest_path"):
        if not isinstance(params[name], str) or not Path(params[name]).is_absolute():
            raise AlignmentError("E_PATH", "Absolute local alignment paths required")
    options = params["parameters"]
    if not isinstance(options, dict) or set(options) != {"device", "threads", "batch_words", "margin", "max_batch_seconds"}:
        raise AlignmentError("E_ARGUMENT", "Closed alignment parameters required")
    if options["device"] != "cpu" or options["threads"] != 2 or type(options["threads"]) is not int:
        raise AlignmentError("E_UNSUPPORTED", "Alignment increment supports CPU with two threads")
    if type(options["batch_words"]) is not int or not 1 <= options["batch_words"] <= 120:
        raise AlignmentError("E_ARGUMENT", "batch_words must be 1..120")
    if type(options["margin"]) not in (int, float) or not math.isfinite(options["margin"]) or not 0 <= options["margin"] <= 1:
        raise AlignmentError("E_ARGUMENT", "margin must be 0..1 seconds")
    if type(options["max_batch_seconds"]) not in (int, float) or not math.isfinite(options["max_batch_seconds"]) or not 0 < options["max_batch_seconds"] <= 30:
        raise AlignmentError("E_ARGUMENT", "batch duration must be at most 30 seconds")


def validate_track(track, duration):
    if not isinstance(track, dict) or not isinstance(track.get("track_id"), str) or not isinstance(track.get("words"), list):
        raise AlignmentError("E_ARGUMENT", "Transcript requires track_id and words array")
    ids, previous = set(), 0.0
    for word in track["words"]:
        if not isinstance(word, dict) or not isinstance(word.get("word_id"), str) or not word["word_id"] or word["word_id"] in ids:
            raise AlignmentError("E_ARGUMENT", "Unique word IDs required")
        if word.get("track_id") != track["track_id"] or not isinstance(word.get("text"), str):
            raise AlignmentError("E_ARGUMENT", "Word track/text mismatch")
        start, end = word.get("t_ini"), word.get("t_fin")
        if type(start) not in (int, float) or type(end) not in (int, float) or not math.isfinite(start) or not math.isfinite(end) or start < previous or end <= start or end > duration:
            raise AlignmentError("E_ARGUMENT", "Ordered finite positive word ranges within PCM required")
        ids.add(word["word_id"])
        previous = start


def batches(words, options, duration, cancel):
    index = 0
    while index < len(words):
        check(cancel)
        group = words[index:index+options["batch_words"]]
        # Bound the maximum end, not just the last word: overlapping ASR words
        # must not hide a pathological earlier interval.
        while len(group) > 1 and max(word["t_fin"] for word in group)-group[0]["t_ini"] > options["max_batch_seconds"]:
            group = group[:-1]
        span = max(word["t_fin"] for word in group)-group[0]["t_ini"]
        if span > options["max_batch_seconds"]:
            raise AlignmentError("E_ARGUMENT", "One ASR word exceeds bounded alignment window")
        start = max(0.0, group[0]["t_ini"]-options["margin"])
        end = min(duration, max(word["t_fin"] for word in group)+options["margin"])
        yield index, group, start, end
        index += len(group)


def apply_times(group, newtimes, failure=None, duration=math.inf):
    """V1 fallback/interpolation policy; original ASR records remain explicit."""
    output = []
    for index, original in enumerate(group):
        word = copy.deepcopy(original)
        word["asr_original"] = copy.deepcopy(original)
        if index in newtimes:
            word["t_ini"], word["t_fin"] = newtimes[index]
            word["alignment_source"] = "mms"
        elif newtimes:
            previous = max((value for value in newtimes if value < index), default=None)
            following = min((value for value in newtimes if value > index), default=None)
            if previous is not None:
                start = newtimes[previous][1]
                end = min(duration, round((start+(newtimes[following][0] if following is not None else start+0.1))/2, 3))
                if end > start:
                    word["t_ini"], word["t_fin"], word["alignment_source"] = start, end, "mms_interpolated"
                else:
                    word["alignment_source"] = "whisper_unalignable"
            else:
                word["alignment_source"] = "whisper_unalignable"
        else:
            word["alignment_source"] = "whisper_fallback" if failure else "whisper_unalignable"
        output.append(word)
    return output


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
        params = self.params
        for path, sha in (("normalized_audio_path", "normalized_audio_sha256"), ("transcript_path", "transcript_sha256"), ("model_path", "model_sha256")):
            if file_hash(params[path], self.cancel) != params[sha]:
                raise AlignmentError("E_PRECONDITION", path + " SHA256 changed")
        manifest_path = Path(params["manifest_path"])
        if manifest_path.stat().st_size > MAX_LINE:
            raise AlignmentError("E_LIMIT", "Model manifest too large")
        manifest_bytes = manifest_path.read_bytes()
        manifest = json.loads(manifest_bytes)
        if manifest.get("model") != "torchaudio.pipelines.MMS_FA" or manifest.get("torchaudio_version") != "2.8.0+cpu" or manifest.get("sha256") != params["model_sha256"] or type(manifest.get("size")) is not int or manifest["size"] != Path(params["model_path"]).stat().st_size:
            raise AlignmentError("E_PRECONDITION", "MMS model manifest mismatch")
        return hashlib.sha256(manifest_bytes).hexdigest()

    def load_model(self):
        check(self.cancel)
        self.progress("load-mms", 0)
        try:
            import torch
            import torchaudio
            from torchaudio.pipelines._wav2vec2 import utils
        except (ImportError, OSError) as error:
            raise AlignmentError("E_DEPENDENCY_RUNTIME", "Installed alignment dependency cannot load: " + str(error)) from error
        check(self.cancel)
        torch.set_num_threads(2)
        bundle = torchaudio.pipelines.MMS_FA
        # get_model/get_state_dict would download automatically. Use the pinned
        # wheel's architecture and transformations with a verified local file.
        model = utils._get_model(bundle._model_type, bundle._params)
        check(self.cancel)
        state = torch.load(self.params["model_path"], map_location="cpu", weights_only=True, mmap=zipfile.is_zipfile(self.params["model_path"]))
        check(self.cancel)
        utils._remove_aux_axes(state, bundle._remove_aux_axis)
        model.load_state_dict(state, strict=True)
        del state
        model = utils._extend_model(model, normalize_waveform=bundle._normalize_waveform, apply_log_softmax=True, append_star=True).eval()
        check(self.cancel)
        self.progress("load-mms", 1)
        return torch, model, bundle.get_tokenizer(), bundle.get_aligner()

    def run(self):
        check(self.cancel)
        self.progress("verify-inputs", 0)
        manifest_sha = self.preconditions()
        with wave.open(self.params["normalized_audio_path"], "rb") as audio:
            if (audio.getframerate(), audio.getnchannels(), audio.getsampwidth(), audio.getcomptype()) != (16000, 1, 2, "NONE"):
                raise AlignmentError("E_UNSUPPORTED", "Alignment requires normalized PCM16 mono 16 kHz")
            duration = audio.getnframes()/16000
            if not 0 < duration <= 24*3600:
                raise AlignmentError("E_ARGUMENT", "Invalid normalized PCM duration")
            source_duration = self.params["source_duration_ticks"]/FLICKS
            if duration > source_duration+1/16000:
                raise AlignmentError("E_PRECONDITION", "Normalized PCM exceeds bound source duration")
            duration = min(duration, source_duration)
        transcript_path = Path(self.params["transcript_path"])
        if transcript_path.stat().st_size > 64*1024*1024:
            raise AlignmentError("E_LIMIT", "Alignment transcript too large")
        track = json.loads(transcript_path.read_bytes())
        validate_track(track, duration)
        key = digest({"request": self.params, "runtime": self.runtime, "manifest_sha256": manifest_sha, "algorithm": ALGORITHM})
        target, checkpoint = inside(self.root, OUTPUT), inside(self.root, ".work/manifests/alignment.json")
        try:
            if checkpoint.stat().st_size > MAX_LINE:
                raise ValueError("oversized checkpoint")
            cached = json.loads(checkpoint.read_bytes())
            valid = cached.get("schema") == "tv2-alignment-stage/1" and cached.get("stage") == "alignment" and cached.get("input_digest") == key and isinstance(cached.get("artifacts"), dict) and set(cached["artifacts"]) == {OUTPUT}
            if valid and cached["artifacts"][OUTPUT] == file_hash(target, self.cancel):
                self.progress("alignment", 1, cached=True)
                return self.receipt(cached["artifacts"])
        except (OSError, ValueError, KeyError, TypeError):
            pass
        groups = list(batches(track["words"], self.params["parameters"], duration, self.cancel))
        self.progress("verify-inputs", 1)
        loaded, aligned, failures = None, [], []
        for index, group, start, end in groups:
            check(self.cancel)
            normalized = [normalize(word["text"]) for word in group]
            alignable = [position for position, text in enumerate(normalized) if text]
            newtimes, failure = {}, None
            if alignable:
                if loaded is None:
                    loaded = self.load_model()
                torch, model, tokenizer, aligner = loaded
                with wave.open(self.params["normalized_audio_path"], "rb") as audio:
                    start_sample, end_sample = int(start*16000), int(end*16000)
                    audio.setpos(start_sample)
                    data = audio.readframes(end_sample-start_sample)
                    if len(data) != (end_sample-start_sample)*2:
                        raise AlignmentError("E_RESULT", "Truncated normalized PCM")
                samples = [value[0]/32768 for value in struct.iter_unpack("<h", data)]
                check(self.cancel)
                try:
                    waveform = torch.tensor(samples, dtype=torch.float32).unsqueeze(0)
                    with torch.inference_mode():
                        emission, _ = model(waveform)
                    check(self.cancel)
                    spans = aligner(emission[0].cpu(), tokenizer([normalized[position] for position in alignable]))
                    ratio = waveform.size(1)/emission.size(1)/16000
                    for position, tokens in zip(alignable, spans):
                        word_start = round(tokens[0].start*ratio+start_sample/16000, 3)
                        word_end = round(tokens[-1].end*ratio+start_sample/16000, 3)
                        if not 0 <= word_start < word_end <= duration:
                            raise ValueError("MMS produced invalid word range")
                        newtimes[position] = (word_start, word_end)
                    if len(newtimes) != len(alignable):
                        raise ValueError("MMS word/span cardinality mismatch")
                except AlignmentError:
                    raise
                except Exception as error:
                    newtimes = {}
                    failure = str(error)[:4096]
                    failures.append({"first_word_index": index, "word_count": len(group), "window": {"start": start, "end": end}, "error": failure})
            aligned.extend(apply_times(group, newtimes, failure, duration))
            self.progress("alignment", (index+len(group))/max(1, len(track["words"])), cached=False)
        check(self.cancel)
        if self.preconditions() != manifest_sha:
            raise AlignmentError("E_PRECONDITION", "Model manifest changed during alignment")
        result = copy.deepcopy(track)
        result["words"] = aligned
        result["alignment"] = {"schema": "tv2-aligned-words/1", "algorithm": ALGORITHM, "runtime": self.runtime,
                               "model_sha256": self.params["model_sha256"], "model_manifest_sha256": manifest_sha,
                               "normalized_audio_sha256": self.params["normalized_audio_sha256"], "source_transcript_sha256": self.params["transcript_sha256"],
                               "source_sha256": self.params["source_sha256"], "source_duration_ticks": self.params["source_duration_ticks"], "timebase": self.params["timebase"],
                               "source_verification": "source SHA is bound lineage from host; worker revalidates normalized PCM and transcript bytes",
                               "failed_batches": failures, "aligned_words": sum(word["alignment_source"] == "mms" for word in aligned),
                               "interpolated_words": sum(word["alignment_source"] == "mms_interpolated" for word in aligned),
                               "execution_state": "completed_with_fallbacks" if failures else "completed" if loaded else "no_alignable_words",
                               "utterances": "original ASR ranges retained; host must rederive master projections", "editorial_decisions": "none"}
        atomic_json(target, result)
        artifacts = {OUTPUT: file_hash(target, self.cancel)}
        check(self.cancel)
        atomic_json(checkpoint, {"schema": "tv2-alignment-stage/1", "stage": "alignment", "input_digest": key, "artifacts": artifacts})
        self.progress("complete", 1, cached=False)
        return self.receipt(artifacts)

    def receipt(self, artifacts):
        return {name: self.params[name] for name in ("job_id", "project_id", "revision", "project_digest", "asset_id", "normalized_audio_sha256", "transcript_sha256", "model_sha256", "source_sha256", "source_duration_ticks", "timebase")} | {"alignment_path": OUTPUT, "artifacts": artifacts}


class Worker:
    def __init__(self, root):
        self.root, self.lock = root.resolve(), threading.Lock()
        self.active, self.cancel, self.job_id, self.initialized = None, None, None, False

    def emit(self, value):
        data = canonical({"protocol": PROTOCOL, **value})
        if len(data) > MAX_LINE:
            raise AlignmentError("E_LIMIT", "Alignment message too large")
        with self.lock:
            sys.stdout.buffer.write(data+b"\n")
            sys.stdout.buffer.flush()

    def error(self, request_id, error):
        self.emit({"id": request_id, "error": {"code": getattr(error, "code", "E_ALIGNMENT"), "message": str(error)[:4096]}})

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
                    raise AlignmentError("E_LIMIT", "Oversized or incomplete alignment request")
                request = json.loads(line)
                if not isinstance(request, dict):
                    raise AlignmentError("E_PROTOCOL", "Alignment request must be object")
                request_id = request.get("id")
                if set(request) != {"protocol", "id", "method", "params"} or request["protocol"] != PROTOCOL or type(request_id) not in (str, int):
                    raise AlignmentError("E_PROTOCOL", "Invalid alignment envelope")
                method, params = request["method"], request["params"]
                if method == "hello":
                    value = runtime()
                    self.emit({"id": request_id, "result": {"protocol": PROTOCOL, "runtime": value,
                              "capabilities": {"forced_alignment": {"implemented": True, "runtime_state": "not_loaded", "model": "MMS_FA", "devices": ["cpu"], "threads": 2},
                                               "output_schema": "tv2-aligned-words/1", "asr": False, "project_mutation": False, "gui_integration": False}}})
                    self.initialized = True
                elif method == "run":
                    if not self.initialized:
                        raise AlignmentError("E_PROTOCOL", "Successful hello required before alignment")
                    if self.active and self.active.is_alive():
                        raise AlignmentError("E_BUSY", "One alignment job per worker")
                    validate_params(params)
                    self.cancel, self.job_id = threading.Event(), params["job_id"]
                    self.active = threading.Thread(target=self.analyze, args=(request_id, params), daemon=True)
                    self.active.start()
                elif method == "cancel":
                    if not isinstance(params, dict) or params.get("job_id") != self.job_id:
                        raise AlignmentError("E_ARGUMENT", "Cancel requires active alignment job ID")
                    active = self.active is not None and self.active.is_alive()
                    if active:
                        self.cancel.set()
                    self.emit({"id": request_id, "result": {"job_id": self.job_id, "cancel_requested": active}})
                elif method == "shutdown":
                    self.stop()
                    self.emit({"id": request_id, "result": {"shutdown": True}})
                    return
                else:
                    raise AlignmentError("E_METHOD", "Unknown alignment method")
            except Exception as error:
                self.error(request_id, error)
                if len(line) > MAX_LINE:
                    break
        self.stop()



def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-root", required=True, type=Path)
    args = parser.parse_args()
    for name in ("HF_TOKEN", "HUGGING_FACE_HUB_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY"):
        os.environ.pop(name, None)
    os.environ.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", HF_HUB_DISABLE_IMPLICIT_TOKEN="1")
    Worker(args.work_root).serve()


if __name__ == "__main__":
    main()
