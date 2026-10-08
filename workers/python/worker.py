"""Offline transcription worker. Optional Python never edits a V2 project.

stdout is bounded NDJSON; diagnostics go to stderr. The supervising Rust host
owns the process tree. Only explicit run requests may start FFmpeg/inference.
"""
from __future__ import annotations

import argparse
import copy
from difflib import SequenceMatcher
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import sys
import threading
import time
import unicodedata
import uuid
import wave

PROTOCOL = "tv2-worker/1"
FLICKS = 705_600_000
MAX_LINE = 1024 * 1024
MODEL_FILES = ("config.json", "model.bin", "tokenizer.json", "vocabulary.txt")
DEPENDENCIES = ("faster-whisper", "ctranslate2", "numpy", "av", "tokenizers")
DERIVATION_VERSION = "v1-intensity-pauses-ava-conversation/1"


class WorkerError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def check_cancel(cancel):
    if cancel.is_set():
        raise WorkerError("E_CANCELLED", "Analysis cancelled")


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
            if error in (109, 232):
                if buffered:
                    yield bytes(buffered)
                return
            raise OSError(error, "Cannot inspect worker stdin pipe")
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
            check_cancel(cancel)
            block = source.read(1024 * 1024)
            if not block:
                break
            sha.update(block)
    return sha.hexdigest()


def model_hash(path, cancel):
    return digest({name: file_hash(Path(path) / name, cancel) for name in MODEL_FILES})


def mean_std(values, empty_std=1.0):
    if not values:
        return 0.0, empty_std
    mean = math.fsum(values) / len(values)
    std = math.sqrt(math.fsum((value - mean) ** 2 for value in values) / len(values)) or 1.0
    return mean, std


def word_values(word):
    start = float(word.get("start", word.get("t_ini", 0.0)))
    end = float(word.get("end", word.get("t_fin", start)))
    if not math.isfinite(start) or not math.isfinite(end):
        raise WorkerError("E_ARGUMENT", "Non-finite word timestamp")
    text = word.get("word", word.get("text", ""))
    if not isinstance(text, str):
        raise WorkerError("E_ARGUMENT", "Word text must be a string")
    probability = word.get("prob", word.get("asr_prob", word.get("probability")))
    if probability is not None and (type(probability) not in (float, int) or not math.isfinite(probability) or not 0 <= probability <= 1):
        raise WorkerError("E_ARGUMENT", "Invalid ASR probability")
    return {"start": start, "end": max(start, end), "word": text, "prob": probability}


def extract_word_intensity(audio, words, *, local_context_seconds=0.35, cancel=None, progress_cb=None):
    """V1 prosodia intensity formulas, bounded PCM windows and pure stdlib.

    Normalized PCM is already on the canonical source clock: do not add T0 or
    the track offset a second time. No arousal/emotion inference occurs here.
    """
    cancel = cancel or threading.Event()
    check_cancel(cancel)
    if not math.isfinite(local_context_seconds) or local_context_seconds < 0:
        raise WorkerError("E_ARGUMENT", "Invalid local intensity context")
    events = []
    with wave.open(str(audio), "rb") as waveform:
        if (waveform.getframerate(), waveform.getnchannels(), waveform.getsampwidth()) != (16000, 1, 2):
            raise WorkerError("E_UNSUPPORTED", "Intensity requires normalized PCM16 mono 16 kHz")
        length = waveform.getnframes()
        context = round(local_context_seconds * 16000)
        def measure(start, end):
            if end <= start:
                return -120.0, 0.0
            waveform.setpos(start)
            remaining, total, peak = end - start, 0.0, 0.0
            while remaining:
                check_cancel(cancel)
                count = min(remaining, 32768)
                samples = waveform.readframes(count)
                if len(samples) != count * 2:
                    raise WorkerError("E_RESULT", "Truncated normalized PCM")
                normalized = [value[0] / 32768.0 for value in struct.iter_unpack("<h", samples)]
                total += math.fsum(value * value for value in normalized)
                peak = max(peak, max(abs(value) for value in normalized))
                remaining -= count
            return 10 * math.log10(max(total / (end - start), 1e-12)), peak
        for index, word in enumerate(words):
            check_cancel(cancel)
            value = word_values(word)
            start_time = max(0.0, value["start"])
            end_time = max(start_time, value["end"])
            start = min(length, max(0, round(start_time * 16000)))
            end = min(length, max(start + 1, round(end_time * 16000)))
            before, after = max(0, start-context), min(length, end+context)
            floor = []
            if start > before:
                floor.append(measure(before, start)[0])
            if after > end:
                floor.append(measure(end, after)[0])
            rms, peak = measure(start, end)
            local_floor = math.fsum(floor) / len(floor) if floor else -120.0
            events.append({"word_index": index, "t_ini": round(start_time, 3), "t_fin": round(end_time, 3),
                           "rms_dbfs": round(rms, 3), "peak_dbfs": round(20 * math.log10(max(peak, 1e-9)), 3),
                           "local_floor_dbfs": round(local_floor, 3), "local_contrast_db": round(rms-local_floor, 3),
                           "duration": round(max(0.0, end_time-start_time), 3)})
            if progress_cb and (index % 500 == 0 or index + 1 == len(words)):
                progress_cb((index + 1) / max(1, len(words)))
    mean, std = mean_std([event["rms_dbfs"] for event in events])
    duration_mean, duration_std = mean_std([event["duration"] for event in events])
    for event in events:
        event["intensity_z"] = round((event["rms_dbfs"]-mean)/std, 3)
        duration_z = (event["duration"]-duration_mean)/duration_std
        linear = 0.72*event["intensity_z"] + 0.18*duration_z + 0.10*min(3.0, max(-3.0, event["local_contrast_db"]/6))
        # Equivalent sigmoid without overflow for unusually large populations.
        event["emphasis_score"] = round(1/(1+math.exp(-linear)) if linear >= 0 else math.exp(linear)/(1+math.exp(linear)), 3)
    return {"schema": "editorial-intensity/1", "sample_rate": 16000, "local_context_seconds": local_context_seconds,
            "baseline": {"mean": round(mean, 6), "std": round(std, 6)},
            "duration_baseline": {"mean": round(duration_mean, 6), "std": round(duration_std, 6)}, "events": events}


def extract_pauses(words, *, min_dur=0.3, cancel=None):
    cancel = cancel or threading.Event()
    if not math.isfinite(min_dur) or min_dur <= 0:
        raise WorkerError("E_ARGUMENT", "Invalid pause threshold")
    values = sorted((word_values(word) for word in words), key=lambda value: value["start"])
    events = []
    if values and values[0]["start"] >= min_dur:
        events.append({"t_ini": 0.0, "t_fin": round(values[0]["start"], 3), "dur": round(values[0]["start"], 3),
                       "tipo": "pausa", "palabra_previa": None, "palabra_sig": values[0]["word"], "fin_frase_previa": False})
    cursor, previous = (values[0]["end"], values[0]) if values else (0.0, None)
    for value in values[1:]:
        check_cancel(cancel)
        gap = value["start"]-cursor
        if gap >= min_dur:
            text = previous["word"].rstrip('"\')»”').rstrip()
            events.append({"t_ini": round(cursor, 3), "t_fin": round(value["start"], 3), "dur": round(gap, 3),
                           "tipo": "pausa", "palabra_previa": previous["word"], "palabra_sig": value["word"],
                           "fin_frase_previa": text.endswith((".", "!", "?", "…"))})
        if value["end"] > cursor:
            cursor, previous = value["end"], value
    mean, std = mean_std([event["dur"] for event in events], empty_std=0.0)
    for event in events:
        event["dur_z"] = round((event["dur"]-mean)/std, 2)
    check_cancel(cancel)
    return {"events": events, "baseline": {"dur": {"mean": round(mean, 3), "std": round(std, 3), "min_dur": min_dur}}, "n": len(events)}


def extract_instructions(words, *, fin_gap=1.2, max_dur=20.0, cancel=None):
    cancel = cancel or threading.Event()
    if not all(math.isfinite(value) and value > 0 for value in (fin_gap, max_dur)):
        raise WorkerError("E_ARGUMENT", "Invalid instruction span threshold")
    variants = {"ava", "eva", "aba", "eba"}
    def normalize(text):
        return "".join(char for char in unicodedata.normalize("NFD", text.lower()) if char.isalpha() and not unicodedata.combining(char))
    values = sorted((word_values(word) for word in words), key=lambda value: value["start"])
    events, previous_end = [], -1.0
    for index, value in enumerate(values):
        check_cancel(cancel)
        if normalize(value["word"]) not in variants or value["start"] < previous_end:
            continue
        end, cause, captured, cursor = value["end"], "fin_audio", [], value["end"]
        for following in values[index+1:]:
            check_cancel(cancel)
            if following["start"]-cursor >= fin_gap:
                cause = "silencio"
                break
            if normalize(following["word"]) in variants and following["start"]-value["start"] > 2.0:
                end, cause = min(end, following["start"]), "nueva_invocacion"
                break
            if following["end"]-value["start"] > max_dur:
                cause = "tope"
                break
            captured.append(following["word"])
            cursor = max(cursor, following["end"])
            end = cursor
        if end-value["start"] > max_dur:
            end, cause = value["start"]+max_dur, "tope"
        previous = values[index-1] if index else None
        pause = value["start"]-previous["end"] if previous else value["start"]
        events.append({"t_ini": round(value["start"], 3), "t_fin": round(end, 3), "dur": round(end-value["start"], 3),
                       "tipo": "instruccion", "palabra": value["word"].strip(),
                       "prob": round(value["prob"], 3) if value["prob"] is not None else None,
                       "pausa_previa": max(round(pause, 3), 0.0), "palabra_previa": previous["word"] if previous else None,
                       "contexto_previo": " ".join(item["word"] for item in values[max(0, index-5):index]).strip() or None,
                       "texto": " ".join(captured).strip(), "fin_causa": cause})
        previous_end = end
    check_cancel(cancel)
    return {"events": events, "n": len(events), "params": {"variantes": sorted(variants), "fin_gap": fin_gap, "max_dur": max_dur, "pausa_previa_min": 0.4}}


def derive_conversation(tracks, *, cancel=None):
    """V1 overlap/bleed observations; retains every original utterance."""
    cancel = cancel or threading.Event()
    check_cancel(cancel)
    utterances = sorted((utterance for track in tracks.values() for utterance in track["utterances"]), key=lambda value: (value["t_ini"], value["track_id"], value["t_fin"]))
    overlaps, component, component_end = [], [], -1.0
    def commit(items):
        track_ids = {item["track_id"] for item in items}
        if len(track_ids) < 2:
            return
        identifier, intersections = f"overlap-{len(overlaps)+1:05d}", []
        for index, left in enumerate(items):
            check_cancel(cancel)
            for right in items[index+1:]:
                if left["track_id"] == right["track_id"]:
                    continue
                start, end = max(left["t_ini"], right["t_ini"]), min(left["t_fin"], right["t_fin"])
                if end > start:
                    intersections.append({"t_ini": round(start, 3), "t_fin": round(end, 3), "utterance_ids": [left["utterance_id"], right["utterance_id"]]})
        overlaps.append({"overlap_group": identifier, "t_ini": min(item["t_ini"] for item in items), "t_fin": max(item["t_fin"] for item in items),
                         "utterance_ids": [item["utterance_id"] for item in items], "track_ids": sorted(track_ids), "intersections": intersections})
        for item in items:
            item["overlap_group"] = identifier
    for utterance in utterances:
        check_cancel(cancel)
        if component and utterance["t_ini"] >= component_end:
            commit(component)
            component, component_end = [], -1.0
        component.append(utterance)
        component_end = max(component_end, utterance["t_fin"])
    if component:
        commit(component)
    words_by_id = {word["word_id"]: word for track in tracks.values() for word in track["words"]}
    def normalized(text):
        value = unicodedata.normalize("NFKD", text.lower())
        return " ".join(re.findall(r"[a-z0-9]{2,}", "".join(char for char in value if not unicodedata.combining(char))))
    def quality(utterance):
        probabilities = [word_values(words_by_id[identifier])["prob"] for identifier in utterance["word_ids"]]
        probabilities = [value for value in probabilities if value is not None]
        return (math.fsum(probabilities)/len(probabilities) if probabilities else 0.0, len(utterance.get("text") or ""))
    duplicates, active, claimed = [], [], set()
    for utterance in utterances:
        check_cancel(cancel)
        active = [other for other in active if other["t_fin"] > utterance["t_ini"]-0.45]
        text = normalized(utterance["text"])
        if len(text) >= 8 and utterance["utterance_id"] not in claimed:
            for other in active:
                if other["track_id"] == utterance["track_id"] or other["utterance_id"] in claimed:
                    continue
                intersection = max(0.0, min(utterance["t_fin"], other["t_fin"])-max(utterance["t_ini"], other["t_ini"]))
                shorter = min(utterance["t_fin"]-utterance["t_ini"], other["t_fin"]-other["t_ini"])
                if shorter <= 0 or intersection/shorter < 0.80:
                    continue
                similarity = SequenceMatcher(None, text, normalized(other["text"]), autojunk=False).ratio()
                if similarity < 0.92:
                    continue
                primary, secondary = sorted((utterance, other), key=quality, reverse=True)
                identifier = f"duplicate-{len(duplicates)+1:05d}"
                duplicates.append({"duplicate_group": identifier, "similarity": round(similarity, 4), "primary_utterance_id": primary["utterance_id"],
                                   "observations": [primary["utterance_id"], secondary["utterance_id"]], "reason": "coincidencia temporal y textual conservadora"})
                primary["duplicate_group"] = secondary["duplicate_group"] = identifier
                secondary["duplicate_secondary"] = True
                claimed.update((primary["utterance_id"], secondary["utterance_id"]))
                break
        active.append(utterance)
    check_cancel(cancel)
    return {"utterances": utterances, "clean_utterance_ids": [item["utterance_id"] for item in utterances if not item.get("duplicate_secondary")],
            "overlap_groups": overlaps, "duplicate_groups": duplicates}


def derive_editorial_track(track, audio, *, cancel=None, progress_cb=None):
    cancel = cancel or threading.Event()
    derived = copy.deepcopy(track)
    intensity = extract_word_intensity(audio, derived["words"], cancel=cancel, progress_cb=progress_cb)
    keys = ("rms_dbfs", "peak_dbfs", "local_floor_dbfs", "local_contrast_db", "intensity_z", "emphasis_score")
    for word, event in zip(derived["words"], intensity["events"]):
        word.update({key: event[key] for key in keys})
        if "asr_prob" not in word:
            word["asr_prob"] = word_values(word)["prob"]
        if "text" in word:
            word["text"] = word["text"].strip()
    words_by_id = {word["word_id"]: word for word in derived["words"]}
    for utterance in derived["utterances"]:
        check_cancel(cancel)
        utterance["text"] = utterance["text"].strip()
        selected = [words_by_id[identifier] for identifier in utterance["word_ids"]]
        signals = dict(utterance.get("signals", {}))
        signals.update(intensity_z_mean=round(math.fsum(word["intensity_z"] for word in selected)/len(selected), 4) if selected else None,
                       emphasis_max=max((word["emphasis_score"] for word in selected), default=None))
        utterance["signals"] = signals
    pauses = extract_pauses(derived["words"], cancel=cancel)
    instructions = extract_instructions(derived["words"], cancel=cancel)
    derived["baselines"] = dict(derived.get("baselines", {}), intensity=intensity["baseline"])
    derived["intensity"] = intensity
    derived["heuristics"] = {"pauses": pauses, "instructions": instructions, "provenance": {"algorithm": DERIVATION_VERSION,
                                "timestamp_basis": "ASR timestamps; forced alignment not performed", "editorial_decisions": "none"}}
    return derived


def versions():
    result = {}
    for name in DEPENDENCIES:
        try:
            result[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            result[name] = None
    return result


def runtime_versions():
    expected = {}
    for line in Path(__file__).with_name("requirements-transcription.lock").read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            name, version = line.strip().split("==", 1)
            expected[name] = version
    actual = {}
    for name in expected:
        try:
            actual[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            actual[name] = None
    if platform.python_version() != "3.12.13" or actual != expected:
        differences = {name: {"expected": expected[name], "actual": actual[name]} for name in expected if expected[name] != actual[name]}
        raise WorkerError("runtime_mismatch", "Python 3.12.13 and exact transcription lock required: " + json.dumps({"python": platform.python_version(), "packages": differences}))
    return actual


def inside(root, relative):
    # This same boundary applies to requests, cached manifests and outputs.
    if not isinstance(relative, str) or "\\" in relative:
        raise WorkerError("E_PATH", "Unsafe artifact path")
    parts = relative.split("/")
    if any(part in ("", ".", "..") for part in parts) or ":" in relative:
        raise WorkerError("E_PATH", "Unsafe artifact path")
    target = root.joinpath(*parts)
    if not target.resolve().is_relative_to(root.resolve()):
        raise WorkerError("E_PATH", "Artifact escapes job directory")
    return target


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


def validate_request(params):
    required = {"job_id", "project_id", "revision", "project_digest", "asset", "source_path", "source_sha256", "model_path", "model_digest", "parameters", "ffmpeg_path"}
    if not isinstance(params, dict) or set(params) != required:
        raise WorkerError("E_ARGUMENT", "Run requires the closed analysis request")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,127}", params["job_id"]):
        raise WorkerError("E_PATH", "Invalid job ID")
    if not isinstance(params["revision"], int) or isinstance(params["revision"], bool) or params["revision"] < 0:
        raise WorkerError("E_ARGUMENT", "Invalid project revision")
    for name in ("source_sha256", "model_digest", "project_digest"):
        if not isinstance(params[name], str) or not re.fullmatch(r"[0-9a-f]{64}", params[name]):
            raise WorkerError("E_ARGUMENT", f"Invalid {name}")
    options = params["parameters"]
    if not isinstance(options, dict) or set(options) != {"language", "device", "threads", "beam_size", "steps"}:
        raise WorkerError("E_ARGUMENT", "Invalid transcription parameters")
    if options["device"] != "cpu" or options["steps"] != ["extract", "transcribe"]:
        raise WorkerError("E_UNSUPPORTED", "Only CPU extraction/transcription is implemented")
    for name, maximum in (("threads", 4), ("beam_size", 10)):
        if type(options[name]) is not int or not 1 <= options[name] <= maximum:
            raise WorkerError("E_ARGUMENT", f"Invalid {name}")
    if not isinstance(options["language"], str) or not re.fullmatch(r"[a-z]{2,3}", options["language"]):
        raise WorkerError("E_ARGUMENT", "Set an explicit supported language code")
    for name in ("source_path", "model_path", "ffmpeg_path"):
        if not isinstance(params[name], str) or not Path(params[name]).is_absolute():
            raise WorkerError("E_PATH", f"{name} must be an absolute local path")
    asset = params["asset"]
    try:
        probe = asset["probe"]
        audio = probe["audio"]
        duration = probe["duration"] / FLICKS
        if not math.isfinite(duration) or duration <= 0 or not audio or asset.get("missing", False):
            raise ValueError("No available audio")
        indexes = [stream["audio_index"] for stream in audio]
        if len(set(indexes)) != len(indexes) or any(type(index) is not int or index < 0 for index in indexes):
            raise ValueError("Invalid audio stream indexes")
        if not asset["fingerprint"]["hash_muestreado"] or not asset["fingerprint"]["inventario_sha256"]:
            raise ValueError("Missing media identity")
    except (KeyError, TypeError, ValueError, OverflowError) as error:
        raise WorkerError("E_ARGUMENT", f"Invalid asset: {error}") from error


class Analysis:
    def __init__(self, root, params, cancel, emit):
        validate_request(params)
        self.params, self.cancel, self.emit = params, cancel, emit
        self.work_root = root.resolve()
        self.root = inside(root, params["job_id"])
        self.root.mkdir(parents=True, exist_ok=True)
        self.audio = params["asset"]["probe"]["audio"]
        self.duration = params["asset"]["probe"]["duration"] / FLICKS
        self.last_progress = 0.0
        self.runtime = {"python": platform.python_version(), "platform": platform.platform(), "packages": runtime_versions(),
                        "worker_sha256": file_hash(Path(__file__), cancel), "ffmpeg_sha256": file_hash(params["ffmpeg_path"], cancel),
                        "lock_sha256": file_hash(Path(__file__).with_name("requirements-transcription.lock"), cancel)}
        self.key = digest({"request": params, "runtime": self.runtime})

    def stage_key(self, stage):
        if stage.startswith("extract-a"):
            index = int(stage.removeprefix("extract-a"))
            stream = next(value for value in self.audio if value["audio_index"] == index)
            return digest({"source_sha256": self.params["source_sha256"], "audio_index": index,
                           "source_start_time": stream["start_time"], "canonical_t0": self.params["asset"]["probe"]["start_time"],
                           "duration_ticks": self.params["asset"]["probe"]["duration"],
                           "normalization": "v1-delta-ms;mono;16000;pcm_s16le;duration-cap/1",
                           "ffmpeg_sha256": self.runtime["ffmpeg_sha256"], "worker_sha256": self.runtime["worker_sha256"]})
        if stage.startswith("transcribe-a"):
            index = int(stage.removeprefix("transcribe-a"))
            stream = next(value for value in self.audio if value["audio_index"] == index)
            return digest({"normalized_audio_sha256": file_hash(inside(self.root, f".work/audio/a{index}.wav"), self.cancel),
                           "stream": stream, "duration": self.duration, "canonical_t0": self.params["asset"]["probe"]["start_time"],
                           "model_digest": self.params["model_digest"], "parameters": self.params["parameters"],
                           "runtime": {key: value for key, value in self.runtime.items() if key != "ffmpeg_sha256"}})
        if stage.startswith("editorial-a"):
            index = int(stage.removeprefix("editorial-a"))
            return digest({"algorithm": DERIVATION_VERSION, "worker_sha256": self.runtime["worker_sha256"], "python": self.runtime["python"],
                           "normalized_audio_sha256": file_hash(inside(self.root, f".work/audio/a{index}.wav"), self.cancel),
                           "asr_transcript_sha256": file_hash(inside(self.root, f".work/transcripts/a{index}.json"), self.cancel)})
        if stage == "master":
            return digest({"binding": self.params, "runtime": self.runtime,
                           "transcripts": {f"a{stream['audio_index']}": file_hash(inside(self.root, f".work/transcripts/a{stream['audio_index']}.json"), self.cancel) for stream in self.audio},
                           "editorial": {f"a{stream['audio_index']}": file_hash(inside(self.root, f".work/editorial/a{stream['audio_index']}.json"), self.cancel) for stream in self.audio}})
        raise WorkerError("E_STAGE", "Unknown analysis stage")

    def copy_atomic(self, source, target):
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_name(target.name + ".tmp-" + uuid.uuid4().hex)
        try:
            with source.open("rb") as input_file, temporary.open("xb") as output:
                while block := input_file.read(1024 * 1024):
                    check_cancel(self.cancel)
                    output.write(block)
                output.flush()
                os.fsync(output.fileno())
            check_cancel(self.cancel)
            os.replace(temporary, target)
        finally:
            temporary.unlink(missing_ok=True)

    def shared_extract(self, stage, target):
        key = self.stage_key(stage)
        cached_audio = inside(self.work_root, f".cache/extract/{key}.wav")
        manifest = inside(self.work_root, f".cache/extract/{key}.json")
        try:
            if manifest.stat().st_size > MAX_LINE:
                return False
            value = json.loads(manifest.read_text(encoding="utf-8"))
            if value.get("schema") != "tv2-extraction-cache/1" or value.get("input_digest") != key:
                return False
            if file_hash(cached_audio, self.cancel) != value["audio_sha256"]:
                return False
            self.copy_atomic(cached_audio, target)
            # A concurrent cache writer cannot slip different bytes between
            # verification and copy into a successful job checkpoint.
            if file_hash(target, self.cancel) != value["audio_sha256"]:
                return False
            return True
        except (OSError, ValueError, KeyError, TypeError) as error:
            return False

    def publish_extract_cache(self, stage, target):
        key = self.stage_key(stage)
        cached_audio = inside(self.work_root, f".cache/extract/{key}.wav")
        self.copy_atomic(target, cached_audio)
        atomic_json(inside(self.work_root, f".cache/extract/{key}.json"),
                    {"schema": "tv2-extraction-cache/1", "input_digest": key, "audio_sha256": file_hash(cached_audio, self.cancel)})

    def progress(self, stage, fraction, **detail):
        check_cancel(self.cancel)
        now = time.monotonic()
        if now - self.last_progress >= 0.1 or fraction in (0, 1) or detail.get("cached"):
            self.last_progress = now
            self.emit({"event": "progress", "job_id": self.params["job_id"], "stage": stage, "fraction": min(1.0, max(0.0, fraction)), **detail})

    def cached(self, stage):
        manifest = inside(self.root, f".work/manifests/{stage}.json")
        try:
            if manifest.stat().st_size > MAX_LINE:
                return None
            value = json.loads(manifest.read_text(encoding="utf-8"))
            if value.get("schema") != "tv2-analysis-stage/1" or value.get("stage") != stage or value.get("input_digest") != self.stage_key(stage):
                return None
            artifacts = value["artifacts"]
            if stage == "master":
                expected_name = "editorial/analysis.editorial.master.json"
            elif re.fullmatch(r"extract-a[0-9]+", stage):
                expected_name = f".work/audio/{stage.removeprefix('extract-')}.wav"
            elif re.fullmatch(r"transcribe-a[0-9]+", stage):
                expected_name = f".work/transcripts/{stage.removeprefix('transcribe-')}.json"
            elif re.fullmatch(r"editorial-a[0-9]+", stage):
                expected_name = f".work/editorial/{stage.removeprefix('editorial-')}.json"
            else:
                return None
            # Verify the exact file subsequently read, not an attacker-selected
            # replacement artifact with a valid unrelated hash.
            if not isinstance(artifacts, dict) or set(artifacts) != {expected_name}:
                return None
            for relative, expected in artifacts.items():
                if file_hash(inside(self.root, relative), self.cancel) != expected:
                    return None
            check_cancel(self.cancel)
            return value
        except (OSError, ValueError, KeyError, TypeError, WorkerError) as error:
            if isinstance(error, WorkerError) and error.code == "E_CANCELLED":
                raise
            return None

    def checkpoint(self, stage, relative, metadata=None):
        check_cancel(self.cancel)
        artifacts = {relative: file_hash(inside(self.root, relative), self.cancel)}
        value = {"schema": "tv2-analysis-stage/1", "stage": stage, "input_digest": self.stage_key(stage),
                 "artifacts": artifacts, "runtime": self.runtime, "metadata": metadata or {}}
        atomic_json(inside(self.root, f".work/manifests/{stage}.json"), value)
        return value

    def extract(self, stream):
        index = stream["audio_index"]
        stage = f"extract-a{index}"
        relative = f".work/audio/a{index}.wav"
        cached = self.cached(stage)
        if cached:
            self.progress(stage, 1, cached=True)
            return relative
        self.progress(stage, 0, cached=False)
        target = inside(self.root, relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        if self.shared_extract(stage, target):
            self.checkpoint(stage, relative, {"shared_cache": True})
            self.progress(stage, 1, cached=True, cache_scope="work_root")
            return relative
        temporary = target.with_name(f"a{index}.tmp-{uuid.uuid4().hex}.wav")
        t0 = self.params["asset"]["probe"]["start_time"]
        delta = (stream["start_time"] - t0) / FLICKS
        filters = []
        if delta > 0:
            filters.append(f"adelay={round(delta * 1000)}:all=1")
        elif delta < 0:
            filters.append(f"atrim=start={-delta:.6f},asetpts=PTS-STARTPTS")
        command = [self.params["ffmpeg_path"], "-hide_banner", "-loglevel", "error", "-nostdin", "-n", "-i", self.params["source_path"],
                   "-map", f"0:a:{index}", "-vn", "-ac", "1", "-ar", "16000", "-threads", str(self.params["parameters"]["threads"])]
        if filters:
            command += ["-af", ",".join(filters)]
        command += ["-t", format(self.duration, ".9f"), "-c:a", "pcm_s16le", str(temporary)]
        child = None
        reader = None
        stderr_tail = bytearray()
        try:
            check_cancel(self.cancel)
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                     creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            def drain():
                while block := child.stderr.read(4096):
                    stderr_tail.extend(block)
                    if len(stderr_tail) > 16384:
                        del stderr_tail[:-16384]
            reader = threading.Thread(target=drain, daemon=True)
            reader.start()
            while child.poll() is None:
                check_cancel(self.cancel)
                time.sleep(0.05)
            reader.join(timeout=2)
            if child.returncode:
                raise WorkerError("E_FFMPEG", "Extraction failed: " + stderr_tail.decode("utf-8", errors="replace"))
            check_cancel(self.cancel)
            with wave.open(str(temporary), "rb") as extracted:
                seconds = extracted.getnframes() / extracted.getframerate()
                if extracted.getframerate() != 16000 or extracted.getnchannels() != 1 or seconds <= 0 or seconds > self.duration + 1 / 16000:
                    raise WorkerError("E_FFMPEG", "Invalid normalized audio duration/format")
            os.replace(temporary, target)
            self.checkpoint(stage, relative, {"source_audio_index": index, "offset_seconds": delta, "duration_seconds": seconds})
            self.publish_extract_cache(stage, target)
            self.progress(stage, 1, cached=False)
            return relative
        finally:
            if child is not None:
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait()
                if child.stderr:
                    if reader:
                        reader.join(timeout=2)
                    child.stderr.close()
            temporary.unlink(missing_ok=True)

    def times(self, start, end):
        if not math.isfinite(start) or not math.isfinite(end):
            raise WorkerError("E_RESULT", "ASR returned non-finite timestamps")
        maximum = math.floor(self.duration * 1000) / 1000
        start, end = max(0.0, round(start, 3)), min(maximum, round(end, 3))
        return (start, end) if end > start else None

    def transcribe(self, stream, relative, model):
        index, track = stream["audio_index"], f"a{stream['audio_index']}"
        stage, artifact = f"transcribe-{track}", f".work/transcripts/{track}.json"
        cached = self.cached(stage)
        if cached:
            self.progress(stage, 1, cached=True)
            return json.loads(inside(self.root, artifact).read_text(encoding="utf-8"))
        self.progress(stage, 0, cached=False)
        options = self.params["parameters"]
        segments, info = model.transcribe(str(inside(self.root, relative)), language=options["language"], beam_size=options["beam_size"],
                                          word_timestamps=True, vad_filter=False, condition_on_previous_text=True)
        words, utterances, raw_segments = [], [], []
        discarded = 0
        for segment in segments:
            check_cancel(self.cancel)
            word_ids = []
            for word in segment.words or []:
                interval = self.times(float(word.start), float(word.end))
                if interval is None:
                    discarded += 1
                    continue
                word_id = f"{track}-w-{len(words)+1:06d}"
                words.append({"word_id": word_id, "track_id": track, "text": word.word, "t_ini": interval[0], "t_fin": interval[1],
                              "probability": word.probability, "asr_time": {"start": word.start, "end": word.end}})
                word_ids.append(word_id)
            interval = self.times(float(segment.start), float(segment.end))
            raw_segments.append({"segment_id": segment.id, "start": segment.start, "end": segment.end, "text": segment.text,
                                 "avg_logprob": segment.avg_logprob, "no_speech_prob": segment.no_speech_prob})
            if interval:
                utterances.append({"utterance_id": f"{track}-u-{len(utterances)+1:06d}", "track_id": track, "text": segment.text.strip(),
                                   "t_ini": interval[0], "t_fin": interval[1], "word_ids": word_ids})
            self.progress(stage, min(segment.end / max(info.duration, 0.001), 0.99), segments=len(raw_segments))
        result = {"track_id": track, "label": stream.get("title") or track, "stream_index": stream["stream_index"], "audio_index": index,
                  "offset": (stream["start_time"] - self.params["asset"]["probe"]["start_time"]) / FLICKS,
                  "words": words, "utterances": utterances, "segments": raw_segments,
                  "asr": {"language": info.language, "language_probability": info.language_probability,
                          "timestamp_method": "whisper-word-timestamps", "discarded_empty_ranges": discarded,
                          "alignment": "not_run", "model_digest": self.params["model_digest"]}}
        atomic_json(inside(self.root, artifact), result)
        self.checkpoint(stage, artifact)
        self.progress(stage, 1, cached=False)
        return result

    def run(self):
        params = self.params
        if any(version is None for version in self.runtime["packages"].values()):
            raise WorkerError("E_DEPENDENCY", "Install the isolated transcription lock first")
        if file_hash(params["source_path"], self.cancel) != params["source_sha256"]:
            raise WorkerError("E_PRECONDITION", "Source SHA256 changed")
        if model_hash(params["model_path"], self.cancel) != params["model_digest"]:
            raise WorkerError("E_PRECONDITION", "Local model digest changed")
        audio_paths = [(stream, self.extract(stream)) for stream in self.audio]
        tracks = {}
        model = None
        for stream, relative in audio_paths:
            if not self.cached(f"transcribe-a{stream['audio_index']}") and model is None:
                check_cancel(self.cancel)
                # Loading a verified directory with local_files_only forbids
                # network fetches and implicit Hugging Face credentials.
                try:
                    from faster_whisper import WhisperModel
                except ImportError as error:
                    blocked = "control de aplicaciones" in str(error).lower() or "application control" in str(error).lower()
                    code = "E_DEPENDENCY_BLOCKED" if blocked else "E_DEPENDENCY_RUNTIME"
                    raise WorkerError(code, f"Installed inference dependency cannot load: {error}") from error
                model = WhisperModel(params["model_path"], device="cpu", compute_type="int8", cpu_threads=params["parameters"]["threads"],
                                     num_workers=1, local_files_only=True)
            result = self.transcribe(stream, relative, model)
            tracks[result["track_id"]] = self.editorial_track(result, relative)
        if file_hash(params["source_path"], self.cancel) != params["source_sha256"] or model_hash(params["model_path"], self.cancel) != params["model_digest"]:
            raise WorkerError("E_PRECONDITION", "Source/model changed during analysis")
        self.progress("conversation-heuristics", 0)
        conversation = derive_conversation(tracks, cancel=self.cancel)
        self.progress("conversation-heuristics", 1)
        master = {"schema": "editorial-master/1", "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                  "project": {"name": params["asset"].get("name", "Local analysis")},
                  "media": {"path": params["source_path"], "duration": self.duration, "t0": params["asset"]["probe"]["start_time"] / FLICKS,
                            "fingerprint": params["asset"]["fingerprint"]},
                  "tracks": tracks, "conversation": conversation,
                  "analysis": {"protocol": PROTOCOL, "input_digest": self.key, "runtime": self.runtime,
                               "completed_steps": ["extract", "transcribe", "word_intensity", "deterministic_heuristics"],
                               "derivation": {"algorithm": DERIVATION_VERSION, "selection": "fixed post-ASR derivation; Parameters.steps select extract/transcribe only",
                                              "conversation_policy": "V1 overlap/bleed heuristic; original observations retained", "editorial_decisions": "none"},
                               "unavailable_steps": ["forced_alignment", "laughter", "arousal"]}, "chunks": []}
        master_relative = "editorial/analysis.editorial.master.json"
        transcript_hashes = {f"a{stream['audio_index']}": file_hash(inside(self.root, f".work/transcripts/a{stream['audio_index']}.json"), self.cancel) for stream, _ in audio_paths}
        final_cached = self.cached("master")
        if final_cached and final_cached.get("metadata", {}).get("transcripts") != transcript_hashes:
            final_cached = None
        if final_cached is None:
            atomic_json(inside(self.root, master_relative), master)
            self.checkpoint("master", master_relative, {"transcripts": transcript_hashes})
        artifacts = {master_relative: file_hash(inside(self.root, master_relative), self.cancel)}
        for stream, relative in audio_paths:
            for artifact in (relative, f".work/transcripts/a{stream['audio_index']}.json", f".work/editorial/a{stream['audio_index']}.json"):
                artifacts[artifact] = file_hash(inside(self.root, artifact), self.cancel)
        self.progress("complete", 1, cached=final_cached is not None)
        return {name: params[name] for name in ("job_id", "project_id", "revision", "project_digest", "source_sha256", "model_digest")} | {"master_path": master_relative, "artifacts": artifacts}

    def editorial_track(self, transcript, audio_relative):
        track = transcript["track_id"]
        stage, artifact = f"editorial-{track}", f".work/editorial/{track}.json"
        if self.cached(stage):
            self.progress(stage, 1, cached=True)
            return json.loads(inside(self.root, artifact).read_text(encoding="utf-8"))
        self.progress(stage, 0, cached=False)
        result = derive_editorial_track(transcript, inside(self.root, audio_relative), cancel=self.cancel,
                                       progress_cb=lambda fraction: self.progress(stage, fraction, cached=False))
        result["heuristics"]["provenance"].update(normalized_audio_sha256=file_hash(inside(self.root, audio_relative), self.cancel),
                                                 source_transcript_sha256=file_hash(inside(self.root, f".work/transcripts/{track}.json"), self.cancel),
                                                 worker_sha256=self.runtime["worker_sha256"])
        atomic_json(inside(self.root, artifact), result)
        self.checkpoint(stage, artifact)
        self.progress(stage, 1, cached=False)
        return result


class Worker:
    def __init__(self, root):
        self.root = root.resolve()
        self.output_lock = threading.Lock()
        self.active = None
        self.cancel = None
        self.job_id = None
        self.initialized = False

    def emit(self, value):
        data = canonical({"protocol": PROTOCOL, **value})
        if len(data) > MAX_LINE:
            raise WorkerError("E_LIMIT", "Worker message exceeds protocol limit")
        with self.output_lock:
            sys.stdout.buffer.write(data + b"\n")
            sys.stdout.buffer.flush()

    def error(self, request_id, error):
        self.emit({"id": request_id, "error": {"code": getattr(error, "code", "E_WORKER"), "message": str(error)[:4096]}})

    def analyze(self, request_id, params):
        try:
            result = Analysis(self.root, params, self.cancel, self.emit).run()
            check_cancel(self.cancel)
            self.emit({"id": request_id, "result": result})
        except Exception as error:
            self.error(request_id, error)

    def stop(self):
        if self.cancel:
            self.cancel.set()
        if self.active:
            self.active.join(timeout=5)
        # Native inference cannot be interrupted in the middle of one decoder
        # call. Rust's process-tree lease terminates it after its close deadline.

    def serve(self):
        for line in request_lines(sys.stdin.buffer):
            request_id = None
            try:
                if len(line) > MAX_LINE or not line.endswith(b"\n"):
                    raise WorkerError("E_LIMIT", "Oversized or incomplete NDJSON request")
                request = json.loads(line)
                if not isinstance(request, dict):
                    raise WorkerError("E_PROTOCOL", "Request must be an object")
                request_id = request.get("id")
                if request.get("protocol") != PROTOCOL or type(request_id) not in (str, int) or set(request) != {"protocol", "id", "method", "params"}:
                    raise WorkerError("E_PROTOCOL", "Invalid worker request envelope")
                method, params = request["method"], request["params"]
                if method == "hello":
                    packages = runtime_versions()
                    self.emit({"id": request_id, "result": {"protocol": PROTOCOL, "python": platform.python_version(), "dependencies": packages,
                                                           "runtime_verification": "versions_verified; native inference loads on explicit run",
                                                           "dependency_status": "installed_not_verified_for_native_execution",
                                                           "capabilities": {"extract": True, "transcribe": {"implemented": True, "installed": all(packages.values()), "runtime_state": "not_loaded"}, "devices": ["cpu"],
                                                                            "word_intensity": {"implemented": True, "runtime_state": "stdlib_ready", "input": "normalized WAV and ASR words"},
                                                                            "heuristics": {"implemented": True, "runtime_state": "stdlib_ready", "algorithm": DERIVATION_VERSION},
                                                                            "forced_alignment": False, "laughter": False, "arousal": False}}})
                    self.initialized = True
                elif method == "run":
                    if not self.initialized:
                        raise WorkerError("E_PROTOCOL", "Successful hello is required before analysis")
                    if self.active and self.active.is_alive():
                        raise WorkerError("E_BUSY", "One analysis at a time per supervised worker")
                    validate_request(params)
                    self.cancel, self.job_id = threading.Event(), params["job_id"]
                    self.active = threading.Thread(target=self.analyze, args=(request_id, params), daemon=True)
                    self.active.start()
                elif method == "cancel":
                    if not isinstance(params, dict) or params.get("job_id") != self.job_id:
                        raise WorkerError("E_ARGUMENT", "Cancel requires the active job ID")
                    active = self.active is not None and self.active.is_alive()
                    if active:
                        self.cancel.set()
                    self.emit({"id": request_id, "result": {"job_id": self.job_id, "cancel_requested": active}})
                elif method == "shutdown":
                    self.stop()
                    self.emit({"id": request_id, "result": {"shutdown": True}})
                    return
                else:
                    raise WorkerError("E_METHOD", "Unknown worker method")
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
