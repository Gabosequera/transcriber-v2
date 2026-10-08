"""Pure editorial worker boundaries; these tests do not claim LLM inference."""
import hashlib
import io
import os
import struct
from pathlib import Path
import tempfile
import threading
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / "workers/python/editorial_worker.py"
worker = types.ModuleType("editorial_worker_contract_test")
worker.__file__ = str(PATH)
exec(compile(PATH.read_bytes(), str(PATH), "exec"), worker.__dict__)


class Boundaries(unittest.TestCase):
    def setUp(self):
        self.cancel = threading.Event()
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.request = {"schema": "editorial-topics-request/1", "request_id": "request-1",
                        "source_master_digest": "a" * 64, "source_layers_digest": "b" * 64,
                        "pass_required": 1, "scope": {"t_ini": 1, "t_fin": 3}}

    def tearDown(self):
        self.temp.cleanup()

    def test_duplicate_json_and_nonfinite_rejected(self):
        for text in ('{"x":1,"x":2}', '{"nested":{"x":1,"x":2}}', '{"x":NaN}', '{"x":Infinity}'):
            with self.assertRaises((worker.EditorialError, ValueError)):
                worker.parse_json(text)

    def test_document_checks_bytes_before_parse(self):
        path = self.root / "input.json"
        path.write_text('{"x":1}', encoding="utf-8")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        self.assertEqual(worker.read_json(path, digest, self.cancel), {"x": 1})
        path.write_text('{"x":2}', encoding="utf-8")
        with self.assertRaises(worker.EditorialError):
            worker.read_json(path, digest, self.cancel)

    def test_document_size_limit(self):
        path = self.root / "input.json"
        path.write_bytes(b" " * 10 + b"{}")
        with self.assertRaises(worker.EditorialError):
            worker.read_json(path, hashlib.sha256(path.read_bytes()).hexdigest(), self.cancel, limit=8)

    def test_owned_path_rejects_escape_and_root(self):
        for path in (self.root, self.root / ".." / "outside.json"):
            with self.assertRaises(worker.EditorialError):
                worker.inside(self.root, path)

    @unittest.skipUnless(os.name == "nt", "Windows extended path alias")
    def test_windows_extended_prefix_preserves_containment(self):
        child = self.root / "input.json"
        child.write_bytes(b"{}")
        alias = Path("\\\\?\\" + str(child))
        self.assertTrue(child.samefile(alias))
        self.assertEqual(worker.inside(self.root, alias).read_bytes(), b"{}")
        outside = Path("\\\\?\\" + str(self.root.parent / "outside.json"))
        with self.assertRaises(worker.EditorialError):
            worker.inside(self.root, outside)

    def test_cancel_does_not_publish(self):
        self.cancel.set()
        with self.assertRaises(worker.EditorialError):
            worker.write_owned(self.root, "output.json", b"{}", self.cancel)
        self.assertFalse((self.root / "output.json").exists())
        self.assertEqual(list(self.root.glob("*.partial-*")), [])

    def test_atomic_output_bytes(self):
        result = worker.write_owned(self.root, "editorial/output.json", b'{"x":1}', self.cancel)
        self.assertEqual(result.read_bytes(), b'{"x":1}')
        self.assertEqual(list(result.parent.glob("*.partial-*")), [])

    def test_binding_is_transport_owned(self):
        result = worker.bind_proposal({"items": [], "complete": True}, self.request, {"trim_mode": "content"})
        self.assertEqual(result["request_id"], "request-1")
        self.assertEqual(result["source_master_digest"], "a" * 64)
        with self.assertRaises(worker.EditorialError):
            worker.bind_proposal({"items": [], "request_id": "other"}, self.request, {"trim_mode": "content"})

    def test_human_claims_are_not_silently_repaired(self):
        body = {"items": [{"state": "accepted", "edited": True}], "complete": True}
        result = worker.bind_proposal(body, self.request, {"trim_mode": "content"})
        self.assertEqual(result["items"], body["items"])  # Rust must reject, not normalize this away.

    def test_topics_second_pass_preserves_validated_previous_digest(self):
        self.request.update(pass_required=2, previous_pass_digest="c" * 64)
        result = worker.bind_proposal({"items": [], "complete": True}, self.request, {"trim_mode": "content"})
        self.assertEqual(result["pass"], 2)
        self.assertEqual(result["previous_pass_digest"], "c" * 64)

    def test_context_keeps_scope_words_and_human_layers(self):
        words = [{"word_id": str(i), "text": str(i), "t_ini": i, "t_fin": i + 1} for i in range(4)]
        layers = {"layers": [{"items": [{"edited": True, "comment": "human note"}]}]}
        docs = {"views/topics-request.json": self.request, "project.editorial.master.json": {"tracks": {"a0": {"words": words}},
                "asr_original": {"large_duplicate": "omitted from prompt"}}, "views/layers.json": layers}
        context = worker.prompt_context(docs, "topics")
        self.assertEqual([word["word_id"] for word in context["tracks"]["a0"]["words"]], ["1", "2"])
        self.assertEqual(context["existing_layers"], layers)
        self.assertNotIn("asr_original", context)

    def test_large_context_rejected_before_tokenizer_or_model(self):
        docs = {"views/topics-request.json": self.request, "project.editorial.master.json": {"tracks": {}},
                "views/layers.json": {"note": "x" * (worker.MAX_CONTEXT_BYTES + 1)}}
        with self.assertRaisesRegex(worker.EditorialError, "before tokenization"):
            worker.messages_for(docs, "topics", {"trim_mode": "content"})

    def test_topic_template_uses_the_requested_scope_not_example_seconds(self):
        for scope in ({"t_ini": 0.25, "t_fin": 9.531}, {"t_ini": 8.5, "t_fin": 20.75}):
            docs = {"views/topics-request.json": dict(self.request, scope=scope),
                    "project.editorial.master.json": {"tracks": {}}}
            system = worker.messages_for(docs, "topics", {"trim_mode": "content"})[0]["content"]
            template, _ = worker.json.JSONDecoder().raw_decode(system.split("Return ", 1)[1])
            self.assertEqual(template["items"][0]["ranges"], [scope])

    def test_second_topics_template_preserves_previous_ids_and_disjoint_ranges(self):
        previous = [{"item_id": "source-a", "ranges": [{"t_ini": 1.0, "t_fin": 1.5, "boundary_diagnostics": {}},
                                                       {"t_ini": 2.0, "t_fin": 3.0}]},
                    {"item_id": "source-b", "parent_id": "source-a",
                     "ranges": [{"t_ini": 2.0, "t_fin": 2.5}, {"t_ini": 2.75, "t_fin": 3.0}]}]
        docs = {"views/topics-request.json": dict(self.request, pass_required=2),
                "project.editorial.master.json": {"tracks": {}}, "views/topics-pass1.json": {"items": previous}}
        system = worker.messages_for(docs, "topics", {"trim_mode": "content"})[0]["content"]
        template, _ = worker.json.JSONDecoder().raw_decode(system.split("Return ", 1)[1])
        self.assertEqual([item["source_item_ids"] for item in template["items"]], [["source-a"], ["source-b"]])
        self.assertEqual(template["items"][0]["ranges"], [{"t_ini": 1.0, "t_fin": 1.5}, {"t_ini": 2.0, "t_fin": 3.0}])
        self.assertEqual(template["items"][1]["ranges"], previous[1]["ranges"])
        self.assertEqual(template["items"][1]["parent_id"], template["items"][0]["item_id"])
        self.assertIn("PASS 2 ONLY", system)
        self.assertNotIn("PASS 1:", system)
        docs["views/topics-pass1.json"]["items"] = []
        with self.assertRaisesRegex(worker.EditorialError, "requires previous"):
            worker.messages_for(docs, "topics", {"trim_mode": "content"})

    def test_previous_topics_projection_preserves_decisions_without_boundary_diagnostics(self):
        original = {"complete": True, "request_id": "previous-request", "items": [
            {"item_id": "child", "parent_id": "parent", "label": "Subject", "comment": "Retain this",
             "state": "proposed", "edited": False, "ranges": [{"t_ini": 1.25, "t_fin": 2.75,
             "boundary_diagnostics": {"algorithm": "host-evidence"}}]}]}
        docs = {"views/topics-request.json": dict(self.request, pass_required=2),
                "project.editorial.master.json": {"tracks": {}}, "views/topics-pass1.json": original}
        projected = worker.prompt_context(docs, "topics")["previous_topics"]
        self.assertEqual(projected["items"][0], {**original["items"][0], "ranges": [{"t_ini": 1.25, "t_fin": 2.75}]})
        self.assertTrue(projected["complete"])
        self.assertNotIn("request_id", projected)
        self.assertIn("boundary_diagnostics", original["items"][0]["ranges"][0])

    def test_montage_context_uses_current_document(self):
        guidance = {"target_seconds": 30, "tolerance": 0.1, "min_clip_seconds": 2,
                    "max_clip_seconds": 10, "allow_reorder": False, "media_duration": 90}
        request = dict(self.request, schema="editorial-montage-request/1", **guidance)
        docs = {"views/montage-request.json": request, "project.editorial.master.json": {"tracks": {}},
                "views/montage-current.json": {"clips": [{"clip_id": "retained-clip"}]}}
        self.assertEqual(worker.prompt_context(docs, "montage")["current_montage"], docs["views/montage-current.json"])
        self.assertEqual(worker.prompt_context(docs, "montage")["guidance"], guidance)

    def test_layer_destination_is_portable_and_unique_per_request(self):
        destinations = []
        for request_id in ("a" * 80, "b" * 80):
            docs = {"views/layers-request.json": dict(self.request, request_id=request_id),
                    "project.editorial.master.json": {"tracks": {}}}
            system = worker.messages_for(docs, "layers", {"trim_mode": "content"})[0]["content"]
            layer_id = "highlights-" + hashlib.sha256(request_id.encode()).hexdigest()
            self.assertIn(layer_id, system)
            self.assertLessEqual(len(layer_id), 80)
            self.assertRegex(layer_id, r"^[a-z0-9-]+$")
            destinations.append(layer_id)
        self.assertNotEqual(*destinations)

    def checkpoint(self, entries, payload=4):
        header = worker.canonical({"__metadata__": {"format": "pt"}, **entries})
        return struct.pack("<Q", len(header)) + header + bytes(payload)

    def test_checkpoint_header_accepts_complete_contiguous_inventory(self):
        entries = {"a": {"dtype": "BF16", "shape": [1], "data_offsets": [0, 2]},
                   "b": {"dtype": "BF16", "shape": [1], "data_offsets": [2, 4]}}
        data = self.checkpoint(entries)
        header, start, order = worker.checkpoint_header(io.BytesIO(data), len(data))
        self.assertEqual(header, entries)
        self.assertEqual(data[start:], bytes(4))
        self.assertEqual(order, ["a", "b"])

    def test_checkpoint_rejects_shape_dtype_and_offset_corruption(self):
        entries = [
            {"dtype": "F32", "shape": [2], "data_offsets": [0, 4]},
            {"dtype": "BF16", "shape": [True], "data_offsets": [0, 4]},
            {"dtype": "BF16", "shape": [-2], "data_offsets": [0, 4]},
            {"dtype": "BF16", "shape": [1, 1, 2], "data_offsets": [0, 4]},
            {"dtype": "BF16", "shape": [2], "data_offsets": [0, True]},
            {"dtype": "BF16", "shape": [1], "data_offsets": [2, 4]},
            {"dtype": "BF16", "shape": [3], "data_offsets": [0, 6]},
            {"dtype": "BF16", "shape": [1], "data_offsets": [0, 2]},
        ]
        for entry in entries:
            with self.subTest(entry=entry):
                data = self.checkpoint({"a": entry})
                with self.assertRaises(worker.EditorialError):
                    worker.checkpoint_header(io.BytesIO(data), len(data))
        overlap = self.checkpoint({name: {"dtype": "BF16", "shape": [1], "data_offsets": [0, 2]} for name in ("a", "b")})
        with self.assertRaises(worker.EditorialError):
            worker.checkpoint_header(io.BytesIO(overlap), len(overlap))

    def test_checkpoint_rejects_truncated_oversized_and_duplicate_headers(self):
        duplicate = b'{"__metadata__":{"format":"pt"},"a":{},"a":{}}'
        for data, size in [(b"tiny", 4), (struct.pack("<Q", worker.MAX_LINE + 1), worker.MAX_LINE + 20),
                           (struct.pack("<Q", 20) + b"{}", 40),
                           (struct.pack("<Q", len(duplicate)) + duplicate + b"00", len(duplicate) + 10)]:
            with self.assertRaises((worker.EditorialError, ValueError)):
                worker.checkpoint_header(io.BytesIO(data), size)

    def test_checkpoint_rejects_allocation_above_profile_without_reading_payload(self):
        entry = {"a": {"dtype": "BF16", "shape": [151936, 1537], "data_offsets": [0, 151936 * 1537 * 2]}}
        header = worker.canonical({"__metadata__": {"format": "pt"}, **entry})
        source = io.BytesIO(struct.pack("<Q", len(header)) + header)
        with self.assertRaisesRegex(worker.EditorialError, "Tensor exceeds"):
            worker.checkpoint_header(source, 8 + len(header) + 151936 * 1537 * 2)

    def job_fixture(self):
        job = self.root / "job-0123456789ab"
        input_dir = job / "inputs/views"
        input_dir.mkdir(parents=True)
        request_path = input_dir / "topics-request.json"
        request_path.write_bytes(worker.canonical(self.request))
        params = {"job_id": job.name, "project_id": "proj-test", "revision": 1, "project_digest": "a" * 64,
                  "asset_id": "asset-test", "input_digest": "b" * 64, "kind": "topics", "request_id": "request-1",
                  "request_digest": "c" * 64, "pass": 1, "documents": {"views/topics-request.json": {
                      "path": str(request_path), "sha256": hashlib.sha256(request_path.read_bytes()).hexdigest()}},
                  "backend": {"fixture": True}, "model": "unused", "model_manifest": "unused-manifest",
                  "parameters": {"temperature": 0.0, "seed": 0, "max_tokens": 32, "timeout_seconds": 10, "trim_mode": "content"},
                  "output": worker.OUTPUT}
        hello = {"model": "unused", "model_manifest": "unused-manifest"}
        return job, request_path, params, hello

    def test_timeout_retains_only_exact_utf8_diagnostic_and_uses_total_run_deadline(self):
        job, _, params, hello = self.job_fixture()
        raw = '{"tema":"reunión sin terminar'
        error = worker.EditorialError("E_TIMEOUT", "deadline", partial_raw=raw)
        with patch.object(worker, "identity", return_value=params["backend"]), \
                patch.object(worker.time, "monotonic", return_value=100.0), patch.object(worker, "infer", side_effect=error) as inference:
            with self.assertRaises(worker.EditorialError) as raised:
                worker.run_job(self.root, params, hello, params["backend"], self.cancel, lambda event: None)
            self.assertEqual(raised.exception.code, "E_TIMEOUT")
            self.assertEqual(inference.call_args.kwargs["deadline"], 109.0)
        self.assertEqual((job / ".work/model-output.txt").read_bytes(), raw.encode())
        self.assertFalse((job / worker.OUTPUT).exists())
        self.assertFalse((job / ".work/editorial-worker.json").exists())

    def test_loading_timeout_without_partial_does_not_create_output(self):
        job, _, params, hello = self.job_fixture()
        with patch.object(worker, "identity", return_value=params["backend"]), \
                patch.object(worker, "infer", side_effect=worker.EditorialError("E_TIMEOUT", "loading")):
            with self.assertRaises(worker.EditorialError) as raised:
                worker.run_job(self.root, params, hello, params["backend"], self.cancel, lambda event: None)
            self.assertEqual(raised.exception.code, "E_TIMEOUT")
        self.assertFalse((job / ".work").exists())
        self.assertFalse((job / worker.OUTPUT).exists())

    def test_cancel_has_priority_over_timeout_partial(self):
        job, _, params, hello = self.job_fixture()
        def stop(*args, **kwargs):
            self.cancel.set()
            raise worker.EditorialError("E_TIMEOUT", "deadline", partial_raw="partial")
        with patch.object(worker, "identity", return_value=params["backend"]), patch.object(worker, "infer", side_effect=stop):
            with self.assertRaises(worker.EditorialError) as raised:
                worker.run_job(self.root, params, hello, params["backend"], self.cancel, lambda event: None)
            self.assertEqual(raised.exception.code, "E_CANCELLED")
        self.assertFalse((job / ".work").exists())
        self.assertFalse((job / worker.OUTPUT).exists())

    def test_timeout_diagnostic_io_failure_preserves_failure_without_receipt(self):
        job, _, params, hello = self.job_fixture()
        with patch.object(worker, "identity", return_value=params["backend"]), \
                patch.object(worker, "infer", side_effect=worker.EditorialError("E_TIMEOUT", "deadline", partial_raw="partial")), \
                patch.object(worker, "write_owned", side_effect=OSError("disk unavailable")):
            with self.assertRaises(worker.EditorialError) as raised:
                worker.run_job(self.root, params, hello, params["backend"], self.cancel, lambda event: None)
            self.assertEqual(raised.exception.code, "E_TIMEOUT")
            self.assertIn("could not be saved", str(raised.exception))
        self.assertFalse((job / worker.OUTPUT).exists())
        self.assertFalse((job / ".work/editorial-worker.json").exists())

    def test_cached_receipt_rechecks_context_before_response(self):
        # Stub inference only exercises cache/publication mechanics, never claims
        # a model was exercised by this unit test.
        job, request_path, params, hello = self.job_fixture()
        with patch.object(worker, "identity", return_value=params["backend"]), patch.object(worker, "infer", return_value=('{"items":[],"complete":true}', True)):
            worker.run_job(self.root, params, hello, params["backend"], self.cancel, lambda event: None)
        original = (job / worker.OUTPUT).read_bytes()
        calls = 0
        def identity_after_change(*args):
            nonlocal calls
            calls += 1
            if calls == 2:
                request_path.write_bytes(b'{"changed":true}')
            return params["backend"]
        with patch.object(worker, "identity", side_effect=identity_after_change), patch.object(worker, "infer", side_effect=AssertionError("Cache must not rerun inference")):
            with self.assertRaisesRegex(worker.EditorialError, "Context changed before result"):
                worker.run_job(self.root, params, hello, params["backend"], self.cancel, lambda event: None)
        self.assertEqual((job / worker.OUTPUT).read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
