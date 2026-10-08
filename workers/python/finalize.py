"""Immutable, stdlib-only editorial assembly. No protocol or ML execution.

The host verifies job/project/revision receipts and file hashes. This layer
checks track/word identity, canonical audio/lineage and ranges again before
rederiving editorial projections. Imported extensions remain in the ASR copy.
"""
from __future__ import annotations
import copy
import hashlib
import json
import math
from pathlib import Path
import re
import threading
import types
import wave

ALGORITHM = "tv2-immutable-post-stage-editorial-finalization/1"
FLICKS = 705600000
if "_worker" not in globals():
    # The protocol host injects its exact verified derivation module. Direct
    # pure calls use adjacent source bytes, never an independently cached .pyc.
    _derivation_path = Path(__file__).with_name("worker.py")
    _worker = types.ModuleType("tv2_finalize_trusted_derivation")
    _worker.__file__ = str(_derivation_path)
    exec(compile(_derivation_path.read_bytes(), str(_derivation_path), "exec"), _worker.__dict__)
INTENSITY_KEYS = {"rms_dbfs", "peak_dbfs", "local_floor_dbfs", "local_contrast_db", "intensity_z", "emphasis_score", "asr_prob"}
CONVERSATION_KEYS = {"overlap_group", "duplicate_group", "duplicate_secondary"}


def preserve_extensions(original, derived):
    """Overlay derived object fields while retaining unknown object extensions.

    Arrays are observations, not positional identities: newly derived arrays
    replace old arrays instead of attaching an old comment to a different event.
    The complete old observations remain available in asr_original.
    """
    if not isinstance(original, dict) or not isinstance(derived, dict):
        return copy.deepcopy(derived)
    result = copy.deepcopy(original)
    for key, value in derived.items():
        result[key] = preserve_extensions(original.get(key), value)
    return result


class FinalizeError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def check(cancel):
    if cancel.is_set():
        raise FinalizeError("E_CANCELLED", "Editorial finalization cancelled")


def canonical(value):
    try:
        return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")
    except (TypeError, ValueError) as error:
        raise FinalizeError("E_ARGUMENT", "Finite JSON required") from error


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def file_hash(path, cancel):
    result = hashlib.sha256()
    with Path(path).open("rb") as source:
        while True:
            check(cancel)
            data = source.read(1024 * 1024)
            if not data:
                return result.hexdigest()
            result.update(data)


def fail(condition, message, code="E_ARGUMENT"):
    if not condition:
        raise FinalizeError(code, message)


def number(value, name):
    fail(type(value) in (int, float) and math.isfinite(value), name + " must be finite numeric")
    return float(value)


def sha(value, name):
    fail(isinstance(value, str) and re.fullmatch(r"[a-f0-9]{64}", value), name + " must be lowercase SHA256")
    return value


def interval(value, duration, name):
    fail(isinstance(value, dict), name + " must be object")
    start, end = number(value.get("t_ini"), name + " start"), number(value.get("t_fin"), name + " end")
    fail(0 <= start < end <= duration, name + " outside canonical source range")
    return start, end


def validate_track(track_id, track, duration, cancel):
    fail(isinstance(track, dict) and track.get("track_id") == track_id, "Track identity mismatch")
    fail(type(track.get("audio_index")) is int and track["audio_index"] >= 0, "Track audio index missing")
    fail(isinstance(track.get("words"), list) and isinstance(track.get("utterances"), list), "Words/utterances required")
    words, utterances = {}, set()
    for word in track["words"]:
        check(cancel)
        fail(isinstance(word, dict) and isinstance(word.get("word_id"), str) and word["word_id"] and word["word_id"] not in words, "Missing/duplicate word ID")
        fail(word.get("track_id") == track_id and isinstance(word.get("text"), str), "Cross-track word or missing text")
        interval(word, duration, "word")
        for key in ("probability", "prob", "asr_prob"):
            if word.get(key) is not None:
                fail(0 <= number(word[key], key) <= 1, "Invalid ASR probability")
        words[word["word_id"]] = word
    for utterance in track["utterances"]:
        check(cancel)
        fail(isinstance(utterance, dict) and isinstance(utterance.get("utterance_id"), str) and utterance["utterance_id"] and utterance["utterance_id"] not in utterances, "Missing/duplicate utterance ID")
        fail(utterance.get("track_id") == track_id and isinstance(utterance.get("text"), str), "Cross-track utterance or missing text")
        interval(utterance, duration, "utterance")
        ids = utterance.get("word_ids")
        fail(isinstance(ids, list) and all(isinstance(item, str) for item in ids) and len(set(ids)) == len(ids) and all(item in words for item in ids), "Missing/duplicate/cross-track utterance word reference")
        fail(isinstance(utterance.get("signals", {}), dict), "Utterance signals must be object")
        utterances.add(utterance["utterance_id"])
    return words, utterances


def stage_track(base, candidate, duration, cancel, *, same_times):
    fail(isinstance(candidate, dict) and candidate.get("track_id") == base["track_id"] and candidate.get("audio_index") == base["audio_index"], "Stage track/audio index mismatch", "E_PRECONDITION")
    validate_track(base["track_id"], candidate, duration, cancel)
    fail([w["word_id"] for w in candidate["words"]] == [w["word_id"] for w in base["words"]], "Stage word IDs/order/cardinality changed", "E_PRECONDITION")
    for old, new in zip(base["words"], candidate["words"]):
        check(cancel)
        fail(new["text"].strip() == old["text"].strip(), "Stage ASR text changed", "E_PRECONDITION")
        for key in ("probability", "prob", "asr_time"):
            fail((key in old) == (key in new) and old.get(key) == new.get(key), "Stage ASR probability/original time changed", "E_PRECONDITION")
        if same_times:
            fail((old["t_ini"], old["t_fin"]) == (new["t_ini"], new["t_fin"]), "Arousal word times are stale after alignment", "E_PRECONDITION")
    fail([u["utterance_id"] for u in candidate["utterances"]] == [u["utterance_id"] for u in base["utterances"]], "Stage utterance IDs/order changed", "E_PRECONDITION")
    for old, new in zip(base["utterances"], candidate["utterances"]):
        fail(old["word_ids"] == new["word_ids"] and old["text"].strip() == new["text"].strip(), "Stage utterance membership/text changed", "E_PRECONDITION")


def metadata(value, schema, pcm_sha, duration, source_sha, expected_transcript=None):
    fail(isinstance(value, dict) and value.get("schema") == schema, "Unknown stage metadata schema")
    fail(isinstance(value.get("algorithm"), str) and value["algorithm"] and isinstance(value.get("execution_state"), str), "Stage algorithm/execution state required")
    states = {"tv2-aligned-words/1": {"completed", "completed_with_fallbacks", "no_alignable_words"},
              "tv2-arousal-words/1": {"completed", "no_speech_windows"}, "tv2-laughter-events/1": {"completed"}}
    fail(value["execution_state"] in states[schema], "Unknown stage execution state")
    fail(sha(value.get("normalized_audio_sha256"), "normalized audio") == pcm_sha, "Stage audio is stale", "E_PRECONDITION")
    stage_source = sha(value.get("source_sha256"), "source")
    if source_sha:
        fail(stage_source == source_sha, "Stage source lineage mismatch", "E_PRECONDITION")
    ticks = value.get("source_duration_ticks")
    fail(type(ticks) is int and ticks > 0 and abs(ticks / FLICKS - duration) <= 0.001, "Stage source duration mismatch", "E_PRECONDITION")
    allowed = {"flicks/705600000", "canonical-normalized-pcm/1"} if schema == "tv2-arousal-words/1" else {"flicks/705600000"}
    fail(value.get("timebase") in allowed, "Stage timebase mismatch", "E_PRECONDITION")
    if expected_transcript:
        key = "source_transcript_sha256" if schema == "tv2-aligned-words/1" else "transcript_sha256"
        fail(sha(value.get(key), key) == expected_transcript, "Stage transcript lineage is stale", "E_PRECONDITION")
    return stage_source


def assemble_master(baseMaster, tracksInputs, cancel=None):
    cancel = cancel if cancel is not None else threading.Event()
    check(cancel)
    original_digest = digest(baseMaster)
    fail(isinstance(baseMaster, dict) and baseMaster.get("schema") == "editorial-master/1", "ASR editorial-master/1 required")
    fail(isinstance(baseMaster.get("media"), dict) and isinstance(baseMaster.get("tracks"), dict) and baseMaster["tracks"], "Media and nonempty ASR tracks required")
    duration = number(baseMaster["media"].get("duration"), "source duration")
    fail(0 < duration <= 24 * 3600, "Invalid source duration")
    fail(isinstance(tracksInputs, dict) and set(tracksInputs) == set(baseMaster["tracks"]), "Inputs must cover every ASR track exactly")
    fail(isinstance(baseMaster.get("analysis", {}), dict) and isinstance(baseMaster.get("chunks", []), list), "Invalid analysis/chunks structure")
    fingerprint = baseMaster["media"].get("fingerprint", {})
    fail(isinstance(fingerprint, dict), "Invalid source fingerprint")
    lineage_context = baseMaster.get("analysis", {}).get("lineage", {})
    fail(isinstance(lineage_context, dict), "Invalid base lineage")
    # Inventory hashes describe a probe/inventory record, not the media bytes.
    # Only a verified full-source lineage hash may bind stage source hashes.
    source_sha = lineage_context.get("source_sha256")
    if source_sha is not None:
        sha(source_sha, "base source")
    result, audio_hashes, track_provenance = copy.deepcopy(baseMaster), {}, {}
    global_words, global_utterances, audio_indices = set(), set(), set()
    stages_completed = set()
    for track_id, base in baseMaster["tracks"].items():
        check(cancel)
        fail(isinstance(track_id, str) and track_id, "Track map key required")
        words, utterances = validate_track(track_id, base, duration, cancel)
        fail(not global_words.intersection(words) and not global_utterances.intersection(utterances) and base["audio_index"] not in audio_indices, "Cross-track duplicate IDs/audio index")
        global_words.update(words); global_utterances.update(utterances); audio_indices.add(base["audio_index"])
        inputs = tracksInputs[track_id]
        fail(isinstance(inputs, dict) and set(inputs) == {"audio", "alignment", "arousal", "laughter"}, "Closed track inputs required")
        fail(isinstance(inputs["audio"], (str, Path)), "Normalized audio path required")
        audio = Path(inputs["audio"])
        with wave.open(str(audio), "rb") as pcm:
            fail((pcm.getframerate(), pcm.getnchannels(), pcm.getsampwidth(), pcm.getcomptype()) == (16000, 1, 2, "NONE"), "Canonical PCM16 mono 16k required", "E_UNSUPPORTED")
            pcm_duration = pcm.getnframes() / 16000
            # Canonical audio starts at source T0; an audio stream may end
            # before its containing video. Never treat missing PCM as silence.
            fail(0 < pcm_duration <= duration + 1 / 16000 + 1e-9, "Canonical PCM exceeds the source clock", "E_PRECONDITION")
        pcm_sha = audio_hashes[track_id] = file_hash(audio, cancel)
        current = copy.deepcopy(base)
        provenance = {"normalized_audio_sha256": pcm_sha, "stages": {}}
        heuristics = base.get("heuristics", {})
        fail(isinstance(heuristics, dict) and isinstance(heuristics.get("provenance", {}), dict) and isinstance(base.get("baselines", {}), dict), "Invalid track provenance/baselines")
        expected_transcript = heuristics.get("provenance", {}).get("source_transcript_sha256")
        if expected_transcript:
            sha(expected_transcript, "ASR transcript")
        alignment = inputs["alignment"]
        if alignment is not None:
            fail(not baseMaster.get("chunks"), "Alignment with preexisting chunks requires explicit rechunk implementation", "E_UNSUPPORTED")
            stage_track(base, alignment, duration, cancel, same_times=False)
            lineage = metadata(alignment.get("alignment"), "tv2-aligned-words/1", pcm_sha, duration, source_sha, expected_transcript)
            source_sha = source_sha or lineage
            for word, aligned in zip(current["words"], alignment["words"]):
                fail(aligned.get("alignment_source") in {"mms", "mms_interpolated", "whisper_unalignable", "whisper_fallback"}, "Alignment source required")
                word.update(t_ini=aligned["t_ini"], t_fin=aligned["t_fin"], alignment_source=aligned["alignment_source"])
            current["alignment"] = copy.deepcopy(alignment["alignment"])
            provenance["stages"]["alignment"] = {"input_digest": digest(alignment), "execution_state": alignment["alignment"]["execution_state"]}
            if any(word["alignment_source"] == "mms" for word in current["words"]):
                stages_completed.add("forced_alignment")
            expected_transcript = digest(alignment)
        arousal = inputs["arousal"]
        if arousal is not None:
            stage_track(current, arousal, duration, cancel, same_times=True)
            lineage = metadata(arousal.get("arousal_analysis"), "tv2-arousal-words/1", pcm_sha, duration, source_sha, expected_transcript)
            source_sha = source_sha or lineage
            windows = arousal.get("arousal")
            fail(isinstance(windows, dict) and windows.get("schema") == "editorial-arousal/1" and isinstance(windows.get("events"), list) and isinstance(windows.get("baseline"), dict), "Invalid arousal windows")
            events = []
            for index, event in enumerate(windows["events"]):
                check(cancel)
                interval(event, duration, "arousal event")
                fail(event.get("track_id", track_id) == track_id, "Cross-track arousal event", "E_PRECONDITION")
                for key in ("arousal", "arousal_z", "dominance", "valence", "rms_dbfs"):
                    number(event.get(key), key)
                events.append(copy.deepcopy(event) | {"event_id": f"{track_id}-arousal-{index+1:06d}", "track_id": track_id})
            for word, enriched in zip(current["words"], arousal["words"]):
                for key in ("arousal", "arousal_z"):
                    fail(key in enriched, "Arousal word signal missing")
                    if enriched[key] is not None:
                        number(enriched[key], key)
                    word[key] = enriched[key]
            current.update(arousal=events, arousal_windows=copy.deepcopy(windows), arousal_analysis=copy.deepcopy(arousal["arousal_analysis"]))
            current["baselines"] = dict(current.get("baselines", {}), arousal=copy.deepcopy(windows["baseline"]))
            provenance["stages"]["arousal"] = {"input_digest": digest(arousal), "execution_state": arousal["arousal_analysis"]["execution_state"], "native_inference_executed": arousal["arousal_analysis"].get("native_inference_executed") is True}
            if arousal["arousal_analysis"].get("native_inference_executed") is True:
                stages_completed.add("arousal")
        laughter = inputs["laughter"]
        if laughter is not None:
            fail(isinstance(laughter, dict) and laughter.get("schema") == "tv2-laughter-events/1" and laughter.get("track_id") == track_id and type(laughter.get("audio_index")) is int and laughter.get("audio_index") == base["audio_index"], "Laughter track/schema mismatch", "E_PRECONDITION")
            fail(type(laughter.get("audio_offset_ticks")) is int and laughter["audio_offset_ticks"] == 0, "Canonical padded audio must not have an offset added twice", "E_PRECONDITION")
            lineage_context = baseMaster.get("analysis", {}).get("lineage", {})
            fail(isinstance(lineage_context, dict), "Invalid base lineage")
            for key, expected in lineage_context.items():
                if key in {"project_id", "revision", "project_digest", "asset_id"}:
                    fail(laughter.get(key) == expected, "Laughter job lineage mismatch", "E_PRECONDITION")
            fail(isinstance(laughter.get("laughter"), dict), "Laughter metadata required")
            laugh_meta = copy.deepcopy(laughter["laughter"])
            # Production laughter metadata has no nested schema; validate using
            # its outer document schema without modifying preserved evidence.
            fail("schema" not in laugh_meta or laugh_meta["schema"] == "tv2-laughter-events/1", "Unknown nested laughter schema")
            lineage = metadata(dict(laugh_meta, schema="tv2-laughter-events/1"), "tv2-laughter-events/1", pcm_sha, duration, source_sha)
            fail(type(laugh_meta.get("windows")) is int and laugh_meta["windows"] >= 0, "Laughter window count required")
            source_sha = source_sha or lineage
            fail(isinstance(laughter.get("events"), list), "Laughter events required")
            ids = set()
            for event in laughter["events"]:
                check(cancel)
                interval(event, duration, "laughter event")
                fail(event.get("track_id") == track_id and isinstance(event.get("event_id"), str) and event["event_id"] and event["event_id"] not in ids, "Laughter missing/duplicate/cross-track ID", "E_PRECONDITION")
                ids.add(event["event_id"])
                for key in ("conf", "max_conf", "mean_conf"):
                    fail(0 <= number(event.get(key), key) <= 1, "Invalid laughter confidence")
            current.update(laughter=copy.deepcopy(laughter["events"]), laughter_analysis=laugh_meta)
            provenance["stages"]["laughter"] = {"input_digest": digest(laughter), "execution_state": laugh_meta["execution_state"]}
            if type(laugh_meta.get("windows")) is int and laugh_meta["windows"] > 0:
                stages_completed.add("laughter")
        # Utterance membership is immutable; timings follow the final words.
        final_words = {word["word_id"]: word for word in current["words"]}
        fail(all(word["t_fin"] <= pcm_duration + 1 / 16000 + 1e-9 for word in current["words"]), "Final word range exceeds available canonical PCM", "E_PRECONDITION")
        for utterance in current["utterances"]:
            check(cancel)
            if utterance["word_ids"]:
                selected = [final_words[identifier] for identifier in utterance["word_ids"]]
                utterance.update(t_ini=min(word["t_ini"] for word in selected), t_fin=max(word["t_fin"] for word in selected))
            for key in CONVERSATION_KEYS:
                utterance.pop(key, None)
        # Derivation's legacy aliases/text normalization must never change
        # original ASR fields; use a transient projection then restore them.
        transient = copy.deepcopy(current)
        for word in transient["words"]:
            for key in ("start", "end", "word", "prob"):
                word.pop(key, None)
        try:
            derived = _worker.derive_editorial_track(transient, audio, cancel=cancel)
        except _worker.WorkerError as error:
            raise FinalizeError(error.code, str(error)) from error
        for field in ("baselines", "intensity", "heuristics"):
            derived[field] = preserve_extensions(current.get(field), derived[field])
        for word, authoritative in zip(derived["words"], current["words"]):
            signals = {key: word[key] for key in INTENSITY_KEYS if key in word}
            word.clear(); word.update(copy.deepcopy(authoritative)); word.update(signals)
        for utterance, authoritative in zip(derived["utterances"], current["utterances"]):
            utterance["text"] = authoritative["text"]
        derived["heuristics"]["provenance"].update(timestamp_basis="final stage word timestamps; optional alignment lineage retained", normalized_audio_sha256=pcm_sha,
            finalization_algorithm=ALGORITHM, base_master_digest=original_digest)
        if expected_transcript:
            derived["heuristics"]["provenance"]["source_transcript_sha256"] = expected_transcript
        result["tracks"][track_id] = derived
        track_provenance[track_id] = provenance
    try:
        result["conversation"] = preserve_extensions(baseMaster.get("conversation"), _worker.derive_conversation(result["tracks"], cancel=cancel))
    except _worker.WorkerError as error:
        raise FinalizeError(error.code, str(error)) from error
    check(cancel)
    for track_id, inputs in tracksInputs.items():
        fail(file_hash(inputs["audio"], cancel) == audio_hashes[track_id], "Canonical PCM changed during finalization", "E_PRECONDITION")
    analysis = result.setdefault("analysis", {})
    completed = analysis.get("completed_steps", [])
    fail(isinstance(completed, list) and all(isinstance(step, str) for step in completed), "Invalid completed steps")
    analysis["completed_steps"] = list(dict.fromkeys(completed + sorted(stages_completed) + ["word_intensity", "deterministic_heuristics", "editorial_finalization"]))
    unavailable = analysis.get("unavailable_steps", [])
    fail(isinstance(unavailable, list) and all(isinstance(step, str) for step in unavailable), "Invalid unavailable steps")
    analysis["unavailable_steps"] = [step for step in unavailable if step not in stages_completed]
    previous_finalization = copy.deepcopy(analysis.get("finalization"))
    if isinstance(previous_finalization, dict) and isinstance(previous_finalization.get("tracks"), dict):
        # The current receipt selects executed stages. Preserve annotations, but
        # never resurrect an old stage claim merely because this plan omitted it.
        for track in previous_finalization["tracks"].values():
            if isinstance(track, dict) and isinstance(track.get("stages"), dict):
                for stage in ("alignment", "arousal", "laughter"):
                    track["stages"].pop(stage, None)
    analysis["finalization"] = preserve_extensions(previous_finalization, {
        "algorithm": ALGORITHM, "base_master_digest": original_digest, "tracks": track_provenance,
        "derivation_algorithm": _worker.DERIVATION_VERSION, "source_sha256": source_sha, "editorial_decisions": "none", "inference_executed_by_finalizer": False})
    result["asr_original"] = copy.deepcopy(baseMaster)
    check(cancel)
    canonical(result)
    fail(digest(baseMaster) == original_digest, "Caller mutated ASR base during assembly", "E_PRECONDITION")
    return result
