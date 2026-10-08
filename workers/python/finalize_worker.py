"""Offline, stdlib-only publication of a new editorial generation.

Upstream jobs stay immutable. Rust validates their receipts before and after this
worker; this boundary independently binds every byte it reads to the request.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import re
import sys
import threading
import types
import uuid

PROTOCOL = "tv2-finalize/1"
OUTPUT = "editorial/generation.editorial.master.json"
MAX_LINE = 1024 * 1024
MAX_JSON = 128 * 1024 * 1024


class FinalizeError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def check(cancel):
    if cancel.is_set():
        raise FinalizeError("E_CANCELLED", "Generation cancelled")


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")


def file_hash(path, cancel):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            check(cancel)
            digest.update(block)
    check(cancel)
    return digest.hexdigest()


def verified(ref, cancel, *, parse=False):
    if not isinstance(ref, dict) or set(ref) != {"path", "sha256"} or not re.fullmatch(r"[0-9a-f]{64}", ref["sha256"]):
        raise FinalizeError("E_ARGUMENT", "Invalid artifact reference")
    path = Path(ref["path"])
    if not path.is_absolute() or not path.is_file():
        raise FinalizeError("E_PATH", "An absolute regular input file is required")
    digest, blocks, count = hashlib.sha256(), [], 0
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            check(cancel)
            digest.update(block)
            if parse:
                count += len(block)
                if count > MAX_JSON:
                    raise FinalizeError("E_LIMIT", "Generation JSON exceeds 128 MiB")
                blocks.append(block)
    if digest.hexdigest() != ref["sha256"]:
        raise FinalizeError("E_PRECONDITION", "Artifact changed: "+str(path))
    return json.loads(b"".join(blocks), parse_constant=lambda _: (_ for _ in ()).throw(ValueError("Non-finite JSON"))) if parse else path


def validate(params):
    fields = {"job_id", "project_id", "revision", "project_digest", "asset_id", "source_sha256", "input_digest",
              "base_master", "tracks", "modules", "lineage"}
    if not isinstance(params, dict) or set(params) != fields:
        raise FinalizeError("E_ARGUMENT", "Unknown or missing generation fields")
    if not isinstance(params["job_id"], str) or not re.fullmatch(r"job-[0-9a-f]{12}", params["job_id"]):
        raise FinalizeError("E_ARGUMENT", "Invalid job ID")
    if type(params["revision"]) is not int or params["revision"] < 0:
        raise FinalizeError("E_ARGUMENT", "Invalid revision")
    for field in ("project_digest", "source_sha256", "input_digest"):
        if not isinstance(params[field], str) or not re.fullmatch(r"[0-9a-f]{64}", params[field]):
            raise FinalizeError("E_ARGUMENT", "Invalid digest")
    if not isinstance(params["modules"], dict) or set(params["modules"]) != {"finalize", "derivation"}:
        raise FinalizeError("E_ARGUMENT", "Generation modules are required")
    if not isinstance(params["tracks"], dict) or not params["tracks"]:
        raise FinalizeError("E_ARGUMENT", "Generation requires audio tracks")
    for name, track in params["tracks"].items():
        if not re.fullmatch(r"a[0-9]+", name) or not isinstance(track, dict) or set(track) != {"audio", "alignment", "arousal", "laughter"}:
            raise FinalizeError("E_ARGUMENT", "Invalid track generation inputs")
        if track["arousal"] is not None and track["alignment"] is None:
            raise FinalizeError("E_ARGUMENT", "Arousal requires its aligned word input")


def envelope(line):
    if len(line) > MAX_LINE:
        raise FinalizeError("E_LIMIT", "Request exceeds protocol limit")
    if not line.endswith(b"\n"):
        raise FinalizeError("E_PROTOCOL", "Incomplete NDJSON envelope at EOF")
    request = json.loads(line)
    if (not isinstance(request, dict) or set(request) != {"protocol", "id", "method", "params"}
            or request["protocol"] != PROTOCOL or not isinstance(request["id"], str)
            or not 1 <= len(request["id"]) <= 128 or not isinstance(request["method"], str)
            or not isinstance(request["params"], dict)):
        raise FinalizeError("E_PROTOCOL", "Unknown or malformed envelope")
    return request


def own_path(root, relative):
    candidate = root / relative
    if not candidate.resolve().is_relative_to(root.resolve()):
        raise FinalizeError("E_PATH", "Generation output escapes job")
    candidate.parent.mkdir(parents=True, exist_ok=True)
    if not candidate.resolve().is_relative_to(root.resolve()):
        raise FinalizeError("E_PATH", "Generation output changed during creation")
    return candidate


def atomic(path, value):
    tmp = path.with_name(path.name+"."+uuid.uuid4().hex+".partial")
    try:
        with tmp.open("xb") as stream:
            stream.write(canonical(value))
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp, path)
    finally:
        tmp.unlink(missing_ok=True)


def load_module(name, ref, cancel, injected=None):
    path = verified(ref, cancel)
    source = path.read_bytes()
    if hashlib.sha256(source).hexdigest() != ref["sha256"]:
        raise FinalizeError("E_PRECONDITION", "Module changed during verified source load")
    check(cancel)
    module = types.ModuleType(name)
    module.__file__ = str(path)
    if injected:
        module.__dict__.update(injected)
    # Never let SourceFileLoader substitute an unverified, timestamp-valid pyc.
    exec(compile(source, str(path), "exec"), module.__dict__)
    return module


def input_refs(params):
    validate(params)
    refs = [params["base_master"], *params["modules"].values()]
    for inputs in params["tracks"].values():
        refs.extend(ref for ref in inputs.values() if ref is not None)
    return refs


def assemble(params, cancel):
    derivation = load_module("tv2_finalize_trusted_derivation", params["modules"]["derivation"], cancel)
    assembler = load_module("tv2_finalize", params["modules"]["finalize"], cancel, {"_worker": derivation})
    base = verified(params["base_master"], cancel, parse=True)
    tracks = {name: {kind: (None if ref is None else verified(ref, cancel, parse=kind != "audio"))
                     for kind, ref in inputs.items()} for name, inputs in params["tracks"].items()}
    master = assembler.assemble_master(base, tracks, cancel=cancel)
    analysis = master.setdefault("analysis", {})
    analysis["generation"] = assembler.preserve_extensions(analysis.get("generation"), {
        "protocol": PROTOCOL, "job_id": params["job_id"], "input_digest": params["input_digest"],
        "lineage": params["lineage"], "modules": params["modules"], "editorial_decisions": "none"})
    return master


def check_modules(params):
    module_path = Path(params["modules"]["finalize"]["path"]).resolve()
    derivation_path = Path(params["modules"]["derivation"]["path"]).resolve()
    if module_path.parent != derivation_path.parent or derivation_path.name != "worker.py":
        raise FinalizeError("E_ARGUMENT", "Finalizer and verified derivation must be siblings")


def validate_verify(params):
    if not isinstance(params, dict) or set(params) != {"request", "master"}:
        raise FinalizeError("E_ARGUMENT", "Verify requires only request and master reference")
    validate(params["request"])
    ref = params["master"]
    if (not isinstance(ref, dict) or set(ref) != {"path", "sha256"}
            or not isinstance(ref["path"], str) or not isinstance(ref["sha256"], str)
            or not re.fullmatch(r"[0-9a-f]{64}", ref["sha256"])):
        raise FinalizeError("E_ARGUMENT", "Invalid verification master reference")


def verify(params, cancel):
    """Fresh deterministic derivation, independent of all output checkpoints.

    This path never creates directories, publishes output or updates caches.
    Both the compared document and all consumed source bytes are hash-bound.
    """
    validate_verify(params)
    request = params["request"]
    refs = input_refs(request) + [params["master"]]
    for ref in refs:
        verified(ref, cancel)
    check_modules(request)
    actual = verified(params["master"], cancel, parse=True)
    expected = assemble(request, cancel)
    check(cancel)
    if canonical(actual) != canonical(expected):
        raise FinalizeError("E_PRECONDITION", "Generation differs from verified deterministic derivation")
    for ref in refs:
        verified(ref, cancel)
    check(cancel)
    return {"verified": True, "sha256": params["master"]["sha256"]}


def run(root, params, cancel, emit):
    refs = input_refs(params)
    for ref in refs:
        verified(ref, cancel)
    check_modules(params)
    job_root = root / params["job_id"]
    if not job_root.resolve().is_relative_to(root.resolve()):
        raise FinalizeError("E_PATH", "Job root escapes work root")
    target = own_path(job_root, OUTPUT)
    checkpoint = own_path(job_root, ".work/generation.json")
    # Resume is tied to every input, including upstream receipts and source code.
    key = hashlib.sha256(canonical(params)).hexdigest()
    cached = False
    if checkpoint.is_file() and checkpoint.stat().st_size <= MAX_LINE and target.is_file():
        try:
            saved = json.loads(checkpoint.read_bytes())
            cached = (isinstance(saved, dict) and set(saved) == {"schema", "input_key", "sha256"} and saved.get("schema") == "tv2-generation-stage/1"
                      and saved.get("input_key") == key and saved.get("sha256") == file_hash(target, cancel))
        except (ValueError, OSError):
            pass
    if not cached:
        emit({"event": "progress", "job_id": params["job_id"], "stage": "finalize", "fraction": 0})
        master = assemble(params, cancel)
        for ref in refs:
            verified(ref, cancel)
        check(cancel)
        atomic(target, master)
        atomic(checkpoint, {"schema": "tv2-generation-stage/1", "input_key": key, "sha256": file_hash(target, cancel)})
    for ref in refs:
        verified(ref, cancel)
    check(cancel)
    emit({"event": "progress", "job_id": params["job_id"], "stage": "finalize", "fraction": 1, "cached": cached})
    return {field: params[field] for field in ("job_id", "project_id", "revision", "project_digest", "asset_id", "source_sha256", "input_digest")} | {
        "master_path": OUTPUT, "artifacts": {OUTPUT: file_hash(target, cancel)}}


class Worker:
    def __init__(self, root):
        self.root, self.lock = root.resolve(), threading.Lock()
        self.active, self.cancel, self.job_id, self.initialized = None, threading.Event(), None, False

    def emit(self, value):
        data = canonical({"protocol": PROTOCOL, **value})
        if len(data) > MAX_LINE:
            raise FinalizeError("E_LIMIT", "Response exceeds protocol limit")
        with self.lock:
            sys.stdout.buffer.write(data+b"\n")
            sys.stdout.buffer.flush()

    def analyze(self, request_id, params, verifying=False):
        try:
            result = verify(params, self.cancel) if verifying else run(self.root, params, self.cancel, self.emit)
            check(self.cancel)
            self.emit({"id": request_id, "result": result})
        except Exception as error:
            self.emit({"id": request_id, "error": {"code": getattr(error, "code", "E_FINALIZE"), "message": str(error)[:4096]}})

    def stop(self):
        self.cancel.set()
        if self.active:
            self.active.join(timeout=1)

    def serve(self):
        try:
            while line := sys.stdin.buffer.readline(MAX_LINE+1):
                request_id = None
                try:
                    request = envelope(line)
                    request_id, method, params = request["id"], request["method"], request["params"]
                    if method == "hello":
                        if params or (self.active and self.active.is_alive()):
                            raise FinalizeError("E_ARGUMENT", "Hello requires empty params and an idle worker")
                        self.initialized = True
                        self.emit({"id": request_id, "result": {"protocol": PROTOCOL, "stdlib_only": True, "python": platform.python_version()}})
                    elif method in ("run", "verify"):
                        if not self.initialized or (self.active and self.active.is_alive()):
                            raise FinalizeError("E_PROTOCOL", "Hello required and one active job allowed")
                        verifying = method == "verify"
                        validate_verify(params) if verifying else validate(params)
                        self.cancel, self.job_id = threading.Event(), (params["request"] if verifying else params)["job_id"]
                        self.active = threading.Thread(target=self.analyze, args=(request_id, params, verifying), daemon=True)
                        self.active.start()
                    elif method == "cancel":
                        if self.job_id is None or params != {"job_id": self.job_id}:
                            raise FinalizeError("E_ARGUMENT", "Cancel requires the active job ID")
                        self.cancel.set()
                        self.emit({"id": request_id, "result": {"cancel_requested": True}})
                    elif method == "shutdown":
                        if params:
                            raise FinalizeError("E_ARGUMENT", "Shutdown requires empty params")
                        self.stop()
                        return
                    else:
                        raise FinalizeError("E_PROTOCOL", "Unknown method")
                except Exception as error:
                    self.emit({"id": request_id, "error": {"code": getattr(error, "code", "E_PROTOCOL"), "message": str(error)[:4096]}})
                    if len(line) > MAX_LINE:
                        return
        finally:
            self.stop()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-root", type=Path, required=True)
    Worker(parser.parse_args().work_root).serve()
