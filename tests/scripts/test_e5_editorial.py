"""Pure stdlib V1-formula tests: invented words and synthetic PCM, no inference.

V1 source is read-only reference, never imported/executed. Golden values below
are analytic PCM expectations and explicit V1 rules, not V2-generated fixtures.
"""
import argparse
import copy
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import shutil
import struct
import threading
import time
import unittest
import wave

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("tv2_worker", REPO / "workers/python/worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
ROOT = None


def pcm(name, samples, rate=16000):
    path = ROOT / name
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(rate)
        output.writeframes(struct.pack("<" + "h"*len(samples), *samples))
    return path


def word(track, index, start, end, text, probability=None):
    return {"word_id": f"{track}-w-{index:06d}", "track_id": track, "t_ini": start, "t_fin": end, "text": text, "probability": probability}


def track(name, start=1.0, end=2.0, text="Mirá detrás de ti", probability=0.9):
    words = [word(name, 1, start, end, text, probability)]
    return {"track_id": name, "label": f"Voz {name}", "offset": 0.25 if name == "B" else 0.0, "words": words,
            "utterances": [{"utterance_id": f"{name}-u-000001", "track_id": name, "t_ini": start, "t_fin": end, "text": text, "word_ids": [words[0]["word_id"]]}]}


class EditorialTests(unittest.TestCase):
    def test_01_intensity_analytic_v1_pcm_golden(self):
        samples = [0]*16000
        samples[1600:4800] = [8192]*3200  # exactly 0.25 FS
        samples[6400:11200] = [16384]*4800  # exactly 0.5 FS
        audio = pcm("two-levels.wav", samples)
        words = [word("A", 1, 0.1, 0.3, "hola"), word("A", 2, 0.4, 0.7, "mundo")]
        result = worker.extract_word_intensity(audio, words)
        first, second = result["events"]
        self.assertEqual((first["rms_dbfs"], second["rms_dbfs"]), (-12.041, -6.021))
        self.assertEqual((first["peak_dbfs"], second["peak_dbfs"]), (-12.041, -6.021))
        self.assertEqual((first["intensity_z"], second["intensity_z"]), (-1.0, 1.0))
        self.assertEqual((first["emphasis_score"], second["emphasis_score"]), (0.354, 0.769))
        self.assertEqual(result["baseline"], {"mean": -9.031, "std": 3.01})
        self.assertEqual(result["duration_baseline"], {"mean": 0.25, "std": 0.05})
        # Context uses the average of before/after dB levels, not energy pooling.
        self.assertEqual(first["local_floor_dbfs"], round((-120+10*math.log10(0.25*4000/5600))/2, 3))
        self.assertEqual(len(result["events"]), len(words))

    def test_02_silence_empty_and_population_variance(self):
        audio = pcm("silence.wav", [0]*16000)
        words = [word("A", 1, 0.0, 0.5, "uno"), word("A", 2, 0.5, 1.0, "dos")]
        result = worker.extract_word_intensity(audio, words)
        for event in result["events"]:
            self.assertEqual(event["rms_dbfs"], -120.0)
            self.assertEqual(event["peak_dbfs"], -180.0)
            self.assertEqual(event["local_contrast_db"], 0.0)
            self.assertEqual(event["emphasis_score"], 0.5)
        self.assertEqual(result["baseline"], {"mean": -120.0, "std": 1.0})
        self.assertEqual(worker.extract_word_intensity(audio, [])["baseline"], {"mean": 0.0, "std": 1.0})

    def test_03_canonical_offset_not_applied_twice(self):
        audio = pcm("offset.wav", [0]*4000+[16384]*4000+[0]*8000)
        record = track("B", 0.25, 0.5, "desplazada")
        before = copy.deepcopy(record)
        result = worker.derive_editorial_track(record, audio)
        self.assertEqual(record, before)
        self.assertEqual(result["offset"], 0.25)
        self.assertEqual(result["words"][0]["rms_dbfs"], -6.021)
        self.assertEqual(result["words"][0]["t_ini"], 0.25)
        self.assertEqual(result["words"][0]["word_id"], "B-w-000001")

    def test_04_boundary_windows_and_zero_width_measurements(self):
        audio = pcm("boundaries.wav", [8192]*16000)
        words = [word("A", 1, -0.1, 0.1, "inicio"), word("A", 2, 0.99, 2.0, "final"), word("A", 3, 3.0, 3.0, "fuera")]
        original = copy.deepcopy(words)
        result = worker.extract_word_intensity(audio, words)
        self.assertEqual(words, original)
        self.assertEqual(len(result["events"]), 3)
        self.assertEqual(result["events"][0]["t_ini"], 0.0)
        self.assertEqual(result["events"][1]["rms_dbfs"], -12.041)
        self.assertEqual(result["events"][2]["rms_dbfs"], -120.0)
        self.assertTrue(all(math.isfinite(event["emphasis_score"]) for event in result["events"]))

    def test_05_pauses_v1_longest_end_and_punctuation(self):
        words = [word("A", 1, 0.5, 2.0, 'final.”'), word("A", 2, 1.0, 1.2, "anidada"), word("A", 3, 3.0, 3.2, "siguiente")]
        result = worker.extract_pauses(words)
        self.assertEqual(result["n"], 2)
        self.assertEqual([(value["t_ini"], value["t_fin"], value["dur_z"]) for value in result["events"]], [(0.0, 0.5, -1.0), (2.0, 3.0, 1.0)])
        self.assertEqual(result["events"][1]["palabra_previa"], 'final.”')
        self.assertTrue(result["events"][1]["fin_frase_previa"])
        self.assertEqual(worker.extract_pauses([])["baseline"], {"dur": {"mean": 0.0, "std": 0.0, "min_dur": 0.3}})
        # No trailing silence fabricated without a duration source.
        self.assertEqual(result["events"][-1]["t_fin"], 3.0)

    def test_06_activation_variants_repeat_and_missing_confidence(self):
        words = [word("A", 1, 0.0, 0.3, "oye"), word("A", 2, 0.5, 0.8, "¡Áva!"),
                 word("A", 3, 0.9, 1.1, "Ava", 0.83), word("A", 4, 1.2, 1.5, "corta"), word("A", 5, 3.0, 3.2, "otro")]
        result = worker.extract_instructions(words)
        self.assertEqual(result["n"], 1)
        event = result["events"][0]
        self.assertEqual((event["t_ini"], event["t_fin"], event["fin_causa"]), (0.5, 1.5, "silencio"))
        self.assertIsNone(event["prob"])
        self.assertEqual(event["contexto_previo"], "oye")
        self.assertEqual(event["texto"], "Ava corta")
        self.assertNotIn("accepted", event)

    def test_07_new_activation_with_overlaps_and_hard_cap(self):
        words = [word("A", 1, 0.0, 0.2, "Eva", 0.6), word("A", 2, 0.3, 4.0, "larga"), word("A", 3, 3.0, 3.2, "Eba", 0.9)]
        result = worker.extract_instructions(words)
        self.assertEqual(result["n"], 2)
        self.assertEqual(result["events"][0]["t_fin"], 3.0)
        self.assertEqual(result["events"][0]["fin_causa"], "nueva_invocacion")
        self.assertEqual(worker.extract_instructions([word("A", 1, 0.0, 30.0, "Aba")])["events"][0]["t_fin"], 20.0)

    def test_08_bleed_quality_and_accent_normalization_preserve_observations(self):
        tracks = {"A": track("A", probability=0.95), "B": track("B", 1.02, 2.02, "mira detras de ti", 0.2)}
        result = worker.derive_conversation(tracks)
        self.assertEqual(len(result["utterances"]), 2)
        self.assertEqual(result["clean_utterance_ids"], ["A-u-000001"])
        duplicate = result["duplicate_groups"][0]
        self.assertEqual(duplicate["similarity"], 1.0)
        self.assertEqual(duplicate["primary_utterance_id"], "A-u-000001")
        self.assertEqual(result["overlap_groups"][0]["intersections"], [{"t_ini": 1.02, "t_fin": 2.0, "utterance_ids": ["A-u-000001", "B-u-000001"]}])
        self.assertTrue(tracks["B"]["utterances"][0]["duplicate_secondary"])
        self.assertEqual(tracks["B"]["label"], "Voz B")
        self.assertEqual(tracks["B"]["words"][0]["probability"], 0.2)

    def test_09_half_open_overlap_and_same_track_are_not_duplicates(self):
        result = worker.derive_conversation({"A": track("A"), "B": track("B", 2.0, 3.0)})
        self.assertEqual(result["overlap_groups"], [])
        self.assertEqual(result["duplicate_groups"], [])
        first, second = track("A"), track("A", 1.1, 2.1)
        second["utterances"][0]["utterance_id"] = "A-u-000002"
        first["utterances"].extend(second["utterances"])
        self.assertEqual(worker.derive_conversation({"A": first})["duplicate_groups"], [])

    def test_10_conservative_similarity_and_shorter_overlap_threshold(self):
        first, second = track("A"), track("B", 1.21, 2.21)
        self.assertEqual(worker.derive_conversation({"A": first, "B": second})["duplicate_groups"], [])  # .79 < .80
        first, second = track("A"), track("B", text="texto diferente del todo")
        self.assertEqual(worker.derive_conversation({"A": first, "B": second})["duplicate_groups"], [])

    def test_11_enrichment_labels_signals_and_no_human_decisions(self):
        audio = pcm("labels.wav", [16384]*16000)
        original = track("A", 0.0, 1.0, " hola ", None)
        before = copy.deepcopy(original)
        derived = worker.derive_editorial_track(original, audio)
        self.assertEqual(original, before)
        self.assertEqual(derived["words"][0]["text"], "hola")
        self.assertEqual(derived["utterances"][0]["text"], "hola")
        self.assertIsNone(derived["words"][0]["asr_prob"])
        self.assertEqual(derived["utterances"][0]["signals"]["intensity_z_mean"], 0.0)
        self.assertEqual(derived["utterances"][0]["signals"]["emphasis_max"], 0.574)
        for field in ("arousal", "laughter", "emotions", "state", "accepted", "edited"):
            self.assertNotIn(field, derived)
        self.assertEqual(derived["heuristics"]["provenance"]["editorial_decisions"], "none")

    def test_12_invalid_confidence_formats_and_cancel_are_explicit(self):
        for probability in (1.1, -0.1, float("nan"), True):
            with self.assertRaises(worker.WorkerError):
                worker.extract_instructions([word("A", 1, 0, 1, "Ava", probability)])
        audio = pcm("wrong-rate.wav", [0]*8000, rate=8000)
        with self.assertRaises(worker.WorkerError) as failed:
            worker.extract_word_intensity(audio, [])
        self.assertEqual(failed.exception.code, "E_UNSUPPORTED")
        cancel = threading.Event()
        cancel.set()
        for action in (lambda: worker.extract_pauses([], cancel=cancel), lambda: worker.extract_instructions([], cancel=cancel), lambda: worker.derive_conversation({}, cancel=cancel)):
            with self.assertRaises(worker.WorkerError) as failed:
                action()
            self.assertEqual(failed.exception.code, "E_CANCELLED")

    def analysis_fixture(self, job_id):
        audio = pcm(job_id + ".wav", [8192]*16000)
        cancelled = threading.Event()
        raw = track("a0", 0.0, 1.0, " Ava recorta ", 0.91)
        raw.update(stream_index=0, audio_index=0)
        size = audio.stat().st_size
        sampled = hashlib.sha256(str(size).encode("ascii"))
        with audio.open("rb") as source:
            for offset in (0, max(0, size//2-4*1024*1024), max(0, size-8*1024*1024)):
                source.seek(offset)
                sampled.update(source.read(min(size, 8*1024*1024)))
        inventory = {"duracion": 1.0, "t0": 0.0, "video": None,
                     "pistas": [{"idx": 0, "codec": "pcm_s16le", "canales": 1, "sample_rate": 16000, "start_time": 0.0, "duracion": 1.0}]}
        fingerprint = {"size": size, "mtime_ns": audio.stat().st_mtime_ns, "hash_muestreado": sampled.hexdigest(),
                       "inventario_sha256": hashlib.sha256(json.dumps(inventory, sort_keys=True).encode("utf-8")).hexdigest()}
        params = {"job_id": job_id, "project_id": "invented-project", "revision": 0,
                  "project_digest": worker.digest({"invented": True}), "source_path": str(audio),
                  "source_sha256": worker.file_hash(audio, cancelled), "model_path": str(REPO / ".local/models/whisper-tiny"),
                  "model_digest": worker.model_hash(REPO / ".local/models/whisper-tiny", cancelled),
                  "ffmpeg_path": str(Path(shutil.which("ffmpeg")).resolve()),
                  "parameters": {"language": "en", "device": "cpu", "threads": 2, "beam_size": 5, "steps": ["extract", "transcribe"]},
                  "asset": {"id": "invented-audio", "kind": "audio", "path": str(audio),
                            "name": "Synthetic PCM and invented words", "missing": False, "fingerprint": fingerprint,
                            "probe": {"container": "wav", "size": size, "duration": worker.FLICKS, "start_time": 0,
                                      "audio": [{"audio_index": 0, "stream_index": 0, "start_time": 0, "codec": "pcm_s16le",
                                                 "channels": 1, "sample_rate": 16000, "duration": worker.FLICKS, "title": "Invented voice"}]}}}
        events = []
        analysis = worker.Analysis(ROOT / "cache-work", params, cancelled, events.append)
        normalized = worker.inside(analysis.root, ".work/audio/a0.wav")
        normalized.parent.mkdir(parents=True)
        shutil.copyfile(audio, normalized)
        transcript = worker.inside(analysis.root, ".work/transcripts/a0.json")
        worker.atomic_json(transcript, raw)
        return analysis, raw, audio, normalized, transcript, events

    def test_13_editorial_checkpoint_resume_exact_hash_and_invalidation(self):
        analysis, raw, audio, normalized, transcript, events = self.analysis_fixture("stdlib-checkpoint")
        cancelled, params = analysis.cancel, analysis.params
        original = copy.deepcopy(raw)
        result = analysis.editorial_track(raw, ".work/audio/a0.wav")
        self.assertEqual(raw, original)
        self.assertEqual(result["heuristics"]["provenance"]["source_transcript_sha256"], worker.file_hash(transcript, cancelled))
        first_key = analysis.stage_key("editorial-a0")
        events.clear()
        self.assertEqual(analysis.editorial_track(raw, ".work/audio/a0.wav"), result)
        self.assertTrue(events[-1]["cached"])
        artifact = worker.inside(analysis.root, ".work/editorial/a0.json")
        artifact.write_text('{"corrupt":true}', encoding="utf-8")
        self.assertIsNone(analysis.cached("editorial-a0"))
        self.assertEqual(analysis.editorial_track(raw, ".work/audio/a0.wav"), result)
        manifest_path = worker.inside(analysis.root, ".work/manifests/editorial-a0.json")
        manifest = json.loads(manifest_path.read_bytes())
        manifest["artifacts"] = {".work/transcripts/a0.json": worker.file_hash(transcript, cancelled)}
        worker.atomic_json(manifest_path, manifest)
        self.assertIsNone(analysis.cached("editorial-a0"))
        changed = copy.deepcopy(raw)
        changed["words"][0]["probability"] = 0.4
        worker.atomic_json(transcript, changed)
        self.assertNotEqual(analysis.stage_key("editorial-a0"), first_key)
        self.assertEqual(analysis.editorial_track(changed, ".work/audio/a0.wav")["words"][0]["asr_prob"], 0.4)
        before_key = analysis.stage_key("editorial-a0")
        shutil.copyfile(pcm("changed-audio.wav", [16384]*16000), normalized)
        self.assertNotEqual(analysis.stage_key("editorial-a0"), before_key)
        self.assertEqual(analysis.editorial_track(changed, ".work/audio/a0.wav")["words"][0]["rms_dbfs"], -6.021)
        self.assertEqual(worker.file_hash(audio, cancelled), params["source_sha256"])

    def test_14_master_payload_from_invented_verified_stage_inputs(self):
        # This is an import-contract fixture, not ASR acceptance: raw words are
        # deliberately invented, seeded through real hashed stage manifests.
        analysis, raw, audio, normalized, transcript, events = self.analysis_fixture("invented-payload")
        analysis.checkpoint("extract-a0", ".work/audio/a0.wav", {"fixture": "synthetic PCM; not FFmpeg extraction acceptance"})
        analysis.checkpoint("transcribe-a0", ".work/transcripts/a0.json", {"fixture": "invented words; not native inference acceptance"})
        receipt = analysis.run()
        master = json.loads(worker.inside(analysis.root, receipt["master_path"]).read_bytes())
        self.assertEqual(master["schema"], "editorial-master/1")
        self.assertEqual(master["analysis"]["completed_steps"], ["extract", "transcribe", "word_intensity", "deterministic_heuristics"])
        self.assertEqual(master["analysis"]["unavailable_steps"], ["forced_alignment", "laughter", "arousal"])
        derived, conversation = master["tracks"]["a0"], master["conversation"]
        fingerprint = master["media"]["fingerprint"]
        self.assertEqual(type(fingerprint["size"]), int)
        self.assertGreaterEqual(fingerprint["size"], 0)
        self.assertLess(fingerprint["size"], 2**64)
        self.assertEqual(fingerprint["size"], audio.stat().st_size)
        self.assertEqual(fingerprint, analysis.params["asset"]["fingerprint"])
        for name in ("hash_muestreado", "inventario_sha256"):
            self.assertEqual(len(fingerprint[name]), 64)
            self.assertEqual(int(fingerprint[name], 16) >= 0, True)
        worker.atomic_json(ROOT / "invented.editorial.master.json", master)
        worker.atomic_json(ROOT / "invented.asset.json", analysis.params["asset"])
        worker.atomic_json(ROOT / "invented.analysis-result.json", receipt)
        self.assertEqual(conversation["clean_utterance_ids"], ["a0-u-000001"])
        self.assertEqual(derived["words"][0]["asr_prob"], 0.91)
        self.assertEqual(derived["words"][0]["word_id"], "a0-w-000001")
        self.assertEqual(derived["utterances"][0]["signals"]["intensity_z_mean"], 0.0)
        for relative, sha in receipt["artifacts"].items():
            self.assertEqual(worker.file_hash(worker.inside(analysis.root, relative), analysis.cancel), sha)
        events.clear()
        self.assertEqual(analysis.run(), receipt)
        self.assertTrue(all(event["cached"] for event in events if event["stage"] in ("extract-a0", "transcribe-a0", "editorial-a0", "complete")))
        self.assertEqual(worker.file_hash(audio, analysis.cancel), analysis.params["source_sha256"])


def main():
    global ROOT
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-root", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    ROOT, evidence = args.work_root.resolve(), args.evidence.resolve()
    if not ROOT.is_relative_to(REPO) or not evidence.is_relative_to(REPO) or ROOT.exists():
        parser.error("Use a new fixture directory and evidence inside V2")
    ROOT.mkdir(parents=True)
    before = time.monotonic()
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(EditorialTests))
    report = {"suite": "editorial-stdlib-v1-formulas", "inference_exercised": False, "v1_executed": False,
              "worker_sha256": worker.file_hash(REPO / "workers/python/worker.py", threading.Event()), "algorithm": worker.DERIVATION_VERSION,
              "tests_run": result.testsRun, "success": result.wasSuccessful(), "elapsed_seconds": time.monotonic()-before,
              "failures": [(str(test), trace) for test, trace in result.failures+result.errors],
              "fixtures": {path.name: {"bytes": path.stat().st_size, "sha256": worker.file_hash(path, threading.Event())} for path in [*ROOT.glob("*.wav"), *ROOT.glob("*.master.json")]}}
    evidence.parent.mkdir(parents=True, exist_ok=True)
    evidence.write_bytes(worker.canonical(report))
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
