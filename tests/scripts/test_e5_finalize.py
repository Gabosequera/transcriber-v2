"""Pure synthetic regression tests; no ASR, model imports or inference."""
from pathlib import Path
import copy
import hashlib
import importlib.util
import math
import struct
import sys
import tempfile
import threading
import types
import unittest
import wave

ROOT = Path(__file__).resolve().parents[2]
before_modules = set(sys.modules)
spec = importlib.util.spec_from_file_location("test_finalizer", ROOT / "workers/python/finalize.py")
finalize = importlib.util.module_from_spec(spec)
spec.loader.exec_module(finalize)
MODEL_MODULES_IMPORTED = {name.split(".")[0] for name in set(sys.modules) - before_modules} & {"numpy", "torch", "torchaudio", "transformers", "av", "faster_whisper"}


class FinalizationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tv2-finalization-synthetic-")
        self.addCleanup(self.temp.cleanup)
        self.audio = Path(self.temp.name) / "invented.wav"
        with wave.open(str(self.audio), "wb") as wav:
            wav.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            wav.writeframes(b"".join(struct.pack("<h", int((800 if i < 8000 else 8000) * math.sin(i * 0.07))) for i in range(32000)))
        self.pcm_sha = hashlib.sha256(self.audio.read_bytes()).hexdigest()
        self.source_sha = "a" * 64
        self.raw_sha = "b" * 64
        words = [{"word_id": "a0-w1", "track_id": "a0", "text": " hola ", "t_ini": 0.1, "t_fin": 0.25,
                  "probability": 0.9, "asr_time": {"start": 0.1, "end": 0.25}, "extension": {"human": "keep"}},
                 {"word_id": "a0-w2", "track_id": "a0", "text": " mundo", "t_ini": 1.2, "t_fin": 1.4,
                  "probability": 0.8, "asr_time": {"start": 1.2, "end": 1.4}}]
        self.track = {"track_id": "a0", "audio_index": 0, "stream_index": 1, "offset": 0, "label": "Synthetic",
                      "words": words, "utterances": [{"utterance_id": "a0-u1", "track_id": "a0", "text": " hola mundo ",
                      "t_ini": 0.1, "t_fin": 1.4, "word_ids": [w["word_id"] for w in words], "signals": {"human": 7},
                      "duplicate_secondary": True, "overlap_group": "stale", "extension": [1, 2]}],
                      "segments": [{"human": True}], "asr": {"engine": "invented-test-only"},
                      "heuristics": {"provenance": {"source_transcript_sha256": self.raw_sha}}}
        self.base = {"schema": "editorial-master/1", "media": {"duration": 2, "fingerprint": {"inventario_sha256": self.source_sha}},
                     "tracks": {"a0": copy.deepcopy(self.track)}, "chunks": [], "extension": {"preserve": True},
                     "analysis": {"lineage": {"source_sha256": self.source_sha}, "completed_steps": ["extract", "transcribe"], "unavailable_steps": ["forced_alignment", "arousal", "laughter"]}}
        self.inputs = {"a0": {"audio": self.audio, "alignment": None, "arousal": None, "laughter": None}}

    def stage_metadata(self, schema):
        return {"schema": schema, "algorithm": "invented-test-only/1", "execution_state": "completed",
                "normalized_audio_sha256": self.pcm_sha, "source_sha256": self.source_sha,
                "source_duration_ticks": 2 * finalize.FLICKS, "timebase": "flicks/705600000"}

    def alignment(self):
        aligned = copy.deepcopy(self.track)
        aligned["alignment"] = self.stage_metadata("tv2-aligned-words/1") | {"source_transcript_sha256": self.raw_sha}
        for word, times in zip(aligned["words"], [(0.6, 0.8), (1.6, 1.8)]):
            word.update(t_ini=times[0], t_fin=times[1], alignment_source="mms")
        return aligned

    def full_inputs(self):
        aligned = self.alignment()
        arousal = copy.deepcopy(aligned)
        arousal["arousal_analysis"] = self.stage_metadata("tv2-arousal-words/1") | {
            "transcript_sha256": finalize.digest(aligned), "native_inference_executed": True, "timebase": "canonical-normalized-pcm/1"}
        arousal["arousal"] = {"schema": "editorial-arousal/1", "baseline": {"mean": 0.7, "std": 0.2},
                              "events": [{"t_ini": 0.5, "t_fin": 1.9, "arousal": 0.7, "arousal_z": 1.0, "dominance": 0.6, "valence": 0.4, "rms_dbfs": -20}]}
        for word in arousal["words"]:
            word.update(arousal=0.7, arousal_z=1.0)
        laughter = {"schema": "tv2-laughter-events/1", "track_id": "a0", "audio_index": 0, "audio_offset_ticks": 0,
                    "laughter": self.stage_metadata("tv2-laughter-events/1") | {"windows": 1},
                    "events": [{"event_id": "a0-laugh-1", "track_id": "a0", "tipo": "laughter", "t_ini": 1.0, "t_fin": 1.2,
                                "conf": 0.8, "max_conf": 0.9, "mean_conf": 0.8, "dur": 0.2}]}
        return {"a0": self.inputs["a0"] | {"alignment": aligned, "arousal": arousal, "laughter": laughter}}

    def assert_rejected(self, base=None, inputs=None, code=None):
        with self.assertRaises(finalize.FinalizeError) as caught:
            finalize.assemble_master(self.base if base is None else base, self.inputs if inputs is None else inputs)
        if code:
            self.assertEqual(caught.exception.code, code)

    def test_import_is_stdlib_only(self):
        self.assertEqual(MODEL_MODULES_IMPORTED, set())

    def test_preserves_unknown_human_extensions_inside_rederived_objects(self):
        self.base["conversation"] = {"reviewer_comment": "keep", "extension": {"human": True}}
        track = self.base["tracks"]["a0"]
        track["heuristics"].update(review_note="keep", pauses={"review_note": "keep pause note"})
        track["heuristics"]["provenance"]["reviewer"] = {"comment": "keep nested"}
        track["intensity"] = {"human_extension": 7, "baseline": {"human_extension": 8}}
        track["baselines"] = {"intensity": {"human_extension": 9}}
        self.base["analysis"]["finalization"] = {"human_extension": 10}
        original = copy.deepcopy(self.base)
        final = finalize.assemble_master(self.base, self.inputs)
        self.assertEqual(self.base, original)
        self.assertEqual(final["asr_original"], original)
        self.assertEqual(final["conversation"]["reviewer_comment"], "keep")
        current = final["tracks"]["a0"]
        self.assertEqual(current["heuristics"]["review_note"], "keep")
        self.assertEqual(current["heuristics"]["pauses"]["review_note"], "keep pause note")
        self.assertEqual(current["heuristics"]["provenance"]["reviewer"], {"comment": "keep nested"})
        self.assertEqual(current["intensity"]["baseline"]["human_extension"], 8)
        self.assertEqual(current["baselines"]["intensity"]["human_extension"], 9)
        self.assertEqual(final["analysis"]["finalization"]["human_extension"], 10)
        self.assertEqual(current["intensity"]["schema"], "editorial-intensity/1")
        self.assertIn("utterances", final["conversation"])

    def test_preserved_finalization_extensions_do_not_resurrect_unselected_stage_claims(self):
        self.base["analysis"]["finalization"] = {"tracks": {"a0": {"human_extension": "keep", "stages": {
            "arousal": {"native_inference_executed": True}, "laughter": {"execution_state": "completed"}, "reviewer_note": "keep"}}}}
        final = finalize.assemble_master(self.base, self.inputs)
        provenance = final["analysis"]["finalization"]["tracks"]["a0"]
        self.assertEqual(provenance["human_extension"], "keep")
        self.assertEqual(provenance["stages"], {"reviewer_note": "keep"})

    def test_host_injected_verified_derivation_is_reused(self):
        sentinel = types.SimpleNamespace(DERIVATION_VERSION="verified-test")
        namespace = {"__file__": str(ROOT / "workers/python/finalize.py"), "_worker": sentinel, "__name__": "injected_test"}
        exec(compile((ROOT / "workers/python/finalize.py").read_bytes(), namespace["__file__"], "exec"), namespace)
        self.assertIs(namespace["_worker"], sentinel)

    def test_inventory_hash_is_not_full_source_identity(self):
        self.base["media"]["fingerprint"]["inventario_sha256"] = "c" * 64
        final = finalize.assemble_master(self.base, self.full_inputs())
        self.assertEqual(final["analysis"]["finalization"]["source_sha256"], self.source_sha)
        del self.base["analysis"]["lineage"]["source_sha256"]
        final = finalize.assemble_master(self.base, self.full_inputs())
        self.assertEqual(final["analysis"]["finalization"]["source_sha256"], self.source_sha)

    def test_canonical_audio_may_end_before_video_source(self):
        self.base["media"]["duration"] = 10.222
        with wave.open(str(self.audio), "wb") as wav:
            wav.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            wav.writeframes(b"\0\0" * round(9.531 * 16000))
        final = finalize.assemble_master(self.base, self.inputs)
        self.assertEqual(final["media"]["duration"], 10.222)
        self.assertEqual(final["tracks"]["a0"]["utterances"][0]["t_fin"], 1.4)
        self.base["tracks"]["a0"]["words"][1]["t_fin"] = 9.8
        self.base["tracks"]["a0"]["utterances"][0]["t_fin"] = 9.8
        self.assert_rejected(code="E_PRECONDITION")

    def test_rejects_audio_longer_than_source_clock(self):
        self.base["media"]["duration"] = 1.9
        self.assert_rejected(code="E_PRECONDITION")

    def test_full_assembly_is_immutable_and_preserves_asr_extensions(self):
        original = copy.deepcopy(self.base)
        inputs = self.full_inputs()
        input_copy = copy.deepcopy(inputs)
        final = finalize.assemble_master(self.base, inputs)
        self.assertEqual(self.base, original)
        self.assertEqual(inputs, input_copy)
        self.assertEqual(final["asr_original"], original)
        track = final["tracks"]["a0"]
        for old, word in zip(self.track["words"], track["words"]):
            for field in ("word_id", "track_id", "text", "probability", "asr_time", "extension"):
                if field in old:
                    self.assertEqual(word[field], old[field])
        self.assertEqual(track["utterances"][0]["text"], self.track["utterances"][0]["text"])
        self.assertEqual(track["utterances"][0]["signals"]["human"], 7)
        self.assertEqual(track["segments"], self.track["segments"])
        self.assertEqual(track["utterances"][0]["extension"], [1, 2])
        self.assertEqual(final["extension"], self.base["extension"])
        self.assertEqual((track["utterances"][0]["t_ini"], track["utterances"][0]["t_fin"]), (0.6, 1.8))
        self.assertNotIn("duplicate_secondary", track["utterances"][0])
        self.assertNotIn("overlap_group", track["utterances"][0])
        self.assertEqual(track["arousal"][0]["track_id"], "a0")
        self.assertEqual(track["laughter"], inputs["a0"]["laughter"]["events"])
        self.assertTrue({"forced_alignment", "arousal", "laughter", "editorial_finalization"} <= set(final["analysis"]["completed_steps"]))
        self.assertEqual(final["analysis"]["unavailable_steps"], [])
        self.assertFalse(final["analysis"]["finalization"]["inference_executed_by_finalizer"])

    def test_alignment_recalculates_intensity_pauses_and_conversation(self):
        baseline = finalize.assemble_master(self.base, self.inputs)
        aligned = finalize.assemble_master(self.base, {"a0": self.inputs["a0"] | {"alignment": self.alignment()}})
        self.assertGreater(aligned["tracks"]["a0"]["words"][0]["rms_dbfs"], baseline["tracks"]["a0"]["words"][0]["rms_dbfs"] + 15)
        self.assertNotEqual(aligned["tracks"]["a0"]["heuristics"]["pauses"], baseline["tracks"]["a0"]["heuristics"]["pauses"])
        self.assertEqual(aligned["conversation"]["utterances"][0]["t_ini"], 0.6)

    def test_final_timing_recalculates_cross_track_overlap(self):
        second = copy.deepcopy(self.track)
        second.update(track_id="a1", audio_index=1)
        for index, word in enumerate(second["words"]):
            word.update(word_id=f"a1-w{index+1}", track_id="a1", t_ini=1.5 + index * 0.2, t_fin=1.65 + index * 0.2)
        second["utterances"][0].update(utterance_id="a1-u1", track_id="a1", text=" different phrase ", t_ini=1.5, t_fin=1.85,
                                      word_ids=[w["word_id"] for w in second["words"]])
        self.base["tracks"]["a1"] = second
        inputs = self.inputs | {"a1": self.inputs["a0"]}
        before = finalize.assemble_master(self.base, inputs)
        self.assertEqual(before["conversation"]["overlap_groups"], [])
        after = finalize.assemble_master(self.base, inputs | {"a0": self.inputs["a0"] | {"alignment": self.alignment()}})
        self.assertEqual(len(after["conversation"]["overlap_groups"]), 1)
        self.assertEqual(after["tracks"]["a0"]["utterances"][0]["overlap_group"], after["tracks"]["a1"]["utterances"][0]["overlap_group"])

    def test_base_only_does_not_claim_optional_inference_and_preserves_chunks(self):
        self.base["chunks"] = [{"extension": "human", "start": 0, "end": 2}]
        final = finalize.assemble_master(self.base, self.inputs)
        self.assertEqual(final["chunks"], self.base["chunks"])
        self.assertEqual(final["analysis"]["unavailable_steps"], self.base["analysis"]["unavailable_steps"])
        self.assertFalse({"forced_alignment", "arousal", "laughter"} & set(final["analysis"]["completed_steps"]))
        self.assertEqual(final["analysis"]["finalization"]["tracks"]["a0"]["stages"], {})

    def test_fallback_alignment_does_not_claim_mms(self):
        aligned = self.alignment()
        for word in aligned["words"]:
            word["alignment_source"] = "whisper_fallback"
        aligned["alignment"]["execution_state"] = "completed_with_fallbacks"
        final = finalize.assemble_master(self.base, {"a0": self.inputs["a0"] | {"alignment": aligned}})
        self.assertNotIn("forced_alignment", final["analysis"]["completed_steps"])

    def test_rejects_preexisting_chunks_with_alignment(self):
        self.base["chunks"] = [{"start": 0, "end": 2}]
        self.assert_rejected(inputs={"a0": self.inputs["a0"] | {"alignment": self.alignment()}}, code="E_UNSUPPORTED")

    def test_rejects_mutated_stage_identity_text_probability_and_original_times(self):
        for mutate in [lambda a: a["words"].pop(), lambda a: a["words"].reverse(),
                       lambda a: a["words"][0].update(track_id="a1"), lambda a: a["words"][0].update(text="changed"),
                       lambda a: a["words"][0].update(probability=0.1), lambda a: a["words"][0].update(asr_time={"start": 0.0, "end": 0.1}),
                       lambda a: a["utterances"][0].update(word_ids=["missing"]), lambda a: a["words"][1].update(word_id="a0-w1")]:
            with self.subTest(mutation=mutate):
                stage = self.alignment(); mutate(stage)
                self.assert_rejected(inputs={"a0": self.inputs["a0"] | {"alignment": stage}})

    def test_rejects_stale_stage_lineage_times_audio_and_source(self):
        mutations = [lambda i: i["alignment"]["alignment"].update(normalized_audio_sha256="c" * 64),
                     lambda i: i["alignment"]["alignment"].update(source_sha256="c" * 64),
                     lambda i: i["alignment"]["alignment"].update(source_transcript_sha256="c" * 64),
                     lambda i: i["arousal"]["arousal_analysis"].update(transcript_sha256="c" * 64),
                     lambda i: i["arousal"]["words"][0].update(t_ini=0.1),
                     lambda i: i["arousal"]["arousal"]["events"][0].update(track_id="a1"),
                     lambda i: i["laughter"].update(audio_offset_ticks=1),
                     lambda i: i["laughter"]["events"][0].update(track_id="a1")]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                inputs = self.full_inputs(); mutate(inputs["a0"])
                self.assert_rejected(inputs=inputs, code="E_PRECONDITION")

    def test_rejects_invalid_closed_structure_ranges_and_nonfinite(self):
        for mutate in [lambda b: b["tracks"]["a0"]["words"][0].update(t_fin=3),
                       lambda b: b["tracks"]["a0"]["words"][0].update(t_ini=True),
                       lambda b: b["tracks"]["a0"]["words"][0].update(probability=float("nan")),
                       lambda b: b["media"].update(fingerprint=[]), lambda b: b["tracks"]["a0"].update(heuristics=[]),
                       lambda b: b["tracks"]["a0"]["utterances"][0].update(word_ids=["missing"])]:
            with self.subTest(mutation=mutate):
                base = copy.deepcopy(self.base); mutate(base)
                self.assert_rejected(base=base)
        self.assert_rejected(inputs={})
        self.assert_rejected(inputs={"a0": self.inputs["a0"] | {"unknown": True}})

    def test_rejects_duplicate_ids_across_tracks(self):
        second = copy.deepcopy(self.track)
        second.update(track_id="a1", audio_index=1)
        for word in second["words"]:
            word["track_id"] = "a1"
        second["utterances"][0]["track_id"] = "a1"
        self.base["tracks"]["a1"] = second
        self.assert_rejected(inputs=self.inputs | {"a1": self.inputs["a0"]})

    def test_rejects_pcm_changed_during_derivation(self):
        original = finalize._worker.derive_editorial_track
        def changed(*args, **kwargs):
            derived = original(*args, **kwargs)
            with self.audio.open("ab") as stream:
                stream.write(b"changed")
            return derived
        finalize._worker.derive_editorial_track = changed
        self.addCleanup(setattr, finalize._worker, "derive_editorial_track", original)
        self.assert_rejected(code="E_PRECONDITION")

    def test_cancel_before_and_during_assembly_preserves_base(self):
        original = copy.deepcopy(self.base)
        event = threading.Event(); event.set()
        with self.assertRaises(finalize.FinalizeError) as caught:
            finalize.assemble_master(self.base, self.inputs, event)
        self.assertEqual(caught.exception.code, "E_CANCELLED")
        class MidCancel:
            count = 0
            def is_set(self):
                self.count += 1
                return self.count >= 12
        with self.assertRaises(finalize.FinalizeError) as caught:
            finalize.assemble_master(self.base, self.full_inputs(), MidCancel())
        self.assertEqual(caught.exception.code, "E_CANCELLED")
        self.assertEqual(self.base, original)


if __name__ == "__main__":
    unittest.main(verbosity=2)
