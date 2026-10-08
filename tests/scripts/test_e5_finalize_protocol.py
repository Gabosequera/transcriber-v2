"""Finalizer transport/file-boundary tests. No model inference is claimed."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
import py_compile
import os
import copy
import subprocess
import queue
from unittest import mock

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("tv2_finalize_worker", REPO / "workers/python/finalize_worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class Boundaries(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="finalize-protocol-", dir=REPO / ".local")
        self.root = Path(self.directory.name)
        self.cancel = threading.Event()

    def tearDown(self):
        self.directory.cleanup()

    def ref(self, name, data):
        path = self.root / name
        path.write_bytes(data)
        return {"path": str(path), "sha256": hashlib.sha256(data).hexdigest()}

    def generation_fixture(self):
        spec = importlib.util.spec_from_file_location("verify_synthetic_fixture", REPO / "tests/scripts/test_e5_finalize.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        fixture = module.FinalizationTests("test_import_is_stdlib_only")
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        fixture.base["conversation"] = {"human_extension": {"keep": True}}
        fixture.base["tracks"]["a0"]["heuristics"]["human_extension"] = "keep"
        fixture.base["analysis"]["generation"] = {"human_extension": "keep"}
        def source_ref(path):
            return {"path": str(path.resolve()), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        params = {"job_id": "job-012345abcdef", "project_id": "invented-project", "revision": 1,
                  "project_digest": "a"*64, "asset_id": "invented-asset", "source_sha256": fixture.source_sha,
                  "input_digest": "b"*64, "base_master": self.ref("base.json", worker.canonical(fixture.base)),
                  "tracks": {"a0": {"audio": source_ref(fixture.audio), "alignment": None, "arousal": None, "laughter": None}},
                  "modules": {"finalize": source_ref(REPO/"workers/python/finalize.py"), "derivation": source_ref(REPO/"workers/python/worker.py")},
                  "lineage": {"kind": "invented-stdlib-fixture-no-ASR"}}
        receipt = worker.run(self.root, params, self.cancel, lambda _: None)
        path = self.root/params["job_id"]/receipt["master_path"]
        ref = {"path": str(path), "sha256": receipt["artifacts"][worker.OUTPUT]}
        return params, path, ref

    def snapshot(self):
        return {str(p.relative_to(self.root)): (p.read_bytes() if p.is_file() else None) for p in self.root.rglob("*")}

    def test_verify_fresh_derivation_is_read_only_and_preserves_extensions(self):
        params, path, ref = self.generation_fixture()
        before = self.snapshot()
        result = worker.verify({"request": params, "master": ref}, self.cancel)
        self.assertEqual(result, {"verified": True, "sha256": ref["sha256"]})
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(json.loads(path.read_bytes())["analysis"]["generation"]["human_extension"], "keep")

    def test_verify_rejects_rehashed_derivations_and_human_extension_forgery(self):
        params, path, ref = self.generation_fixture()
        original = json.loads(path.read_bytes())
        attacks = [("analysis", "lineage", "source_sha256"), ("analysis", "generation", "modules", "derivation", "sha256"),
                   ("conversation", "human_extension"), ("conversation", "clean_utterance_ids"),
                   ("tracks", "a0", "heuristics", "human_extension"), ("tracks", "a0", "heuristics", "pauses", "n"),
                   ("tracks", "a0", "intensity", "sample_rate"), ("tracks", "a0", "words", 0, "intensity_z"),
                   ("tracks", "a0", "words", 0, "extension")]
        for keys in attacks:
            with self.subTest(path=keys):
                changed = copy.deepcopy(original)
                target = changed
                for key in keys[:-1]:
                    target = target[key]
                target[keys[-1]] = "rehashed-forgery"
                data = worker.canonical(changed); path.write_bytes(data)
                forged = ref | {"sha256": hashlib.sha256(data).hexdigest()}
                before = self.snapshot()
                with self.assertRaisesRegex(worker.FinalizeError, "deterministic"):
                    worker.verify({"request": params, "master": forged}, self.cancel)
                self.assertEqual(self.snapshot(), before)

    def test_forged_checkpoint_cannot_authorize_derivation(self):
        params, path, ref = self.generation_fixture()
        changed = json.loads(path.read_bytes())
        changed["conversation"]["clean_utterance_ids"] = []
        data = worker.canonical(changed); path.write_bytes(data)
        forged_sha = hashlib.sha256(data).hexdigest()
        checkpoint = self.root/params["job_id"]/".work/generation.json"
        cache = json.loads(checkpoint.read_bytes()); cache["sha256"] = forged_sha
        checkpoint.write_bytes(worker.canonical(cache))
        cached = worker.run(self.root, params, self.cancel, lambda _: None)
        self.assertEqual(cached["artifacts"][worker.OUTPUT], forged_sha)
        with self.assertRaisesRegex(worker.FinalizeError, "deterministic"):
            worker.verify({"request": params, "master": ref | {"sha256": forged_sha}}, self.cancel)

    def test_verify_cancel_and_changed_input_leave_all_outputs_untouched(self):
        params, path, ref = self.generation_fixture()
        before = self.snapshot(); self.cancel.set()
        with self.assertRaisesRegex(worker.FinalizeError, "cancelled"):
            worker.verify({"request": params, "master": ref}, self.cancel)
        self.assertEqual(self.snapshot(), before); self.cancel.clear()
        original = worker.assemble
        def cancelled_during_assembly(request, cancel):
            expected = original(request, cancel)
            cancel.set()
            return expected
        with mock.patch.object(worker, "assemble", cancelled_during_assembly):
            with self.assertRaisesRegex(worker.FinalizeError, "cancelled"):
                worker.verify({"request": params, "master": ref}, self.cancel)
        self.assertEqual(self.snapshot(), before); self.cancel.clear()
        def changed_input(request, cancel):
            expected = original(request, cancel)
            Path(request["base_master"]["path"]).write_bytes(b"{}")
            return expected
        with mock.patch.object(worker, "assemble", changed_input):
            with self.assertRaisesRegex(worker.FinalizeError, "changed"):
                worker.verify({"request": params, "master": ref}, self.cancel)
        self.assertEqual(path.read_bytes(), before[str(path.relative_to(self.root))])

    def test_verify_closed_request_and_type_identity(self):
        params, path, ref = self.generation_fixture()
        with self.assertRaises(worker.FinalizeError):
            worker.verify({"request": params, "master": ref, "cache": True}, self.cancel)
        changed = json.loads(path.read_bytes()); changed["analysis"]["generation"]["lineage"]["kind"] = True
        data = worker.canonical(changed); path.write_bytes(data)
        with self.assertRaises(worker.FinalizeError):
            worker.verify({"request": params, "master": ref | {"sha256": hashlib.sha256(data).hexdigest()}}, self.cancel)

    def test_verify_real_protocol_child_is_read_only_and_requires_hello(self):
        import sys
        params, path, ref = self.generation_fixture()
        child = subprocess.Popen([sys.executable, "-I", str(REPO/"workers/python/finalize_worker.py"), "--work-root", str(self.root)],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        responses = queue.Queue()
        def read():
            for line in child.stdout:
                responses.put(json.loads(line))
        reader = threading.Thread(target=read, daemon=True); reader.start()
        def call(identifier, method, value):
            child.stdin.write(worker.canonical({"protocol": worker.PROTOCOL, "id": identifier, "method": method, "params": value})+b"\n")
            child.stdin.flush()
            return responses.get(timeout=10)
        try:
            self.assertIn("error", call("before-hello", "verify", {"request": params, "master": ref}))
            self.assertEqual(call("hello", "hello", {})["result"]["protocol"], worker.PROTOCOL)
            before = self.snapshot()
            verified = call("verify", "verify", {"request": params, "master": ref})
            self.assertEqual(verified["result"], {"verified": True, "sha256": ref["sha256"]})
            self.assertEqual(self.snapshot(), before)
        finally:
            if child.poll() is None:
                child.stdin.write(worker.canonical({"protocol": worker.PROTOCOL, "id": "shutdown", "method": "shutdown", "params": {}})+b"\n")
                child.stdin.flush(); child.stdin.close()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill(); child.wait(timeout=5)
            reader.join(timeout=2)
            self.assertEqual(child.returncode, 0)
            self.assertEqual(child.stderr.read(), b"")
            child.stdout.close(); child.stderr.close()

    def test_byte_identity_before_json(self):
        ref = self.ref("value.json", b'{"word":"invented"}')
        self.assertEqual(worker.verified(ref, self.cancel, parse=True), {"word": "invented"})
        Path(ref["path"]).write_bytes(b'{"word":"modified"}')
        with self.assertRaisesRegex(worker.FinalizeError, "changed"):
            worker.verified(ref, self.cancel, parse=True)

    def test_closed_reference(self):
        ref = self.ref("value.json", b"{}")
        ref["ignored_sibling"] = True
        with self.assertRaises(worker.FinalizeError):
            worker.verified(ref, self.cancel)

    def test_nonfinite_is_not_accepted(self):
        ref = self.ref("value.json", b'{"metric":NaN}')
        with self.assertRaises(ValueError):
            worker.verified(ref, self.cancel, parse=True)

    def test_json_bound(self):
        ref = self.ref("value.json", b'{"word":"invented"}')
        original = worker.MAX_JSON
        worker.MAX_JSON = 4
        try:
            with self.assertRaisesRegex(worker.FinalizeError, "128 MiB"):
                worker.verified(ref, self.cancel, parse=True)
        finally:
            worker.MAX_JSON = original

    def test_cancel_interrupts_input_hash(self):
        ref = self.ref("value.json", b"{}")
        self.cancel.set()
        with self.assertRaisesRegex(worker.FinalizeError, "cancelled"):
            worker.verified(ref, self.cancel)

    def test_output_traversal_rejected_before_directory_creation(self):
        owned = self.root / "job"
        with self.assertRaises(worker.FinalizeError):
            worker.own_path(owned, "../escape/file.json")
        self.assertFalse((self.root / "escape").exists())

    def test_atomic_publication_leaves_no_partial(self):
        target = worker.own_path(self.root / "job", worker.OUTPUT)
        worker.atomic(target, {"fixture": True})
        self.assertEqual(json.loads(target.read_bytes()), {"fixture": True})
        self.assertEqual(list(target.parent.glob("*.partial")), [])

    def test_canonical_disallows_nonfinite_metrics(self):
        with self.assertRaises(ValueError):
            worker.canonical({"metric": float("inf")})

    def test_complete_json_without_newline_is_incomplete_at_eof(self):
        raw = worker.canonical({"protocol": worker.PROTOCOL, "id": "hello", "method": "hello", "params": {}})
        with self.assertRaisesRegex(worker.FinalizeError, "Incomplete"):
            worker.envelope(raw)
        self.assertEqual(worker.envelope(raw+b"\n")["id"], "hello")

    def test_unknown_envelope_fields_and_nonstrings_are_rejected(self):
        request = {"protocol": worker.PROTOCOL, "id": "hello", "method": "hello", "params": {}}
        for changed in (request | {"extra": True}, request | {"id": 1}, request | {"params": []}):
            with self.assertRaises(worker.FinalizeError):
                worker.envelope(worker.canonical(changed)+b"\n")

    def test_verified_source_cannot_be_replaced_by_timestamp_valid_pyc(self):
        approved = b"VALUE = 'approved'\n"
        poisoned = b"VALUE = 'unhashed'\n"
        self.assertEqual(len(approved), len(poisoned))
        ref = self.ref("module.py", poisoned)
        path = Path(ref["path"])
        original_stat = path.stat()
        py_compile.compile(str(path), doraise=True)
        path.write_bytes(approved)
        os.utime(path, ns=(original_stat.st_atime_ns, original_stat.st_mtime_ns))
        ref["sha256"] = hashlib.sha256(approved).hexdigest()
        loaded = worker.load_module("owned_fixture", ref, self.cancel)
        self.assertEqual(loaded.VALUE, "approved")


if __name__ == "__main__":
    unittest.main(verbosity=2)
