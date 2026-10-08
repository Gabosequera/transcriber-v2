"""Prepare/run isolated documentary export and real external-monitor acceptance.

Preparation launches nothing. Execution requires an explicitly pinned NEW build.
Only own synthetic V2 copies are written; no V1 app, ML, or physical audio.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

from measure_e2_rf64_native import own_job

ROOT = Path(__file__).resolve().parents[2]
OLD_GUI_SHA = "349df668c006a1565613ad6770edd918fb514243b5e2b32888469335f2837c36"
CAP = 2 * 1024 ** 3


def sha(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024*1024), b""):
            value.update(block)
    return value.hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def project(path):
    value = read(path)
    return value["project"] if value.get("schema") == "transcriptor-project-storage/1" else value


def manifest(root):
    return {path.relative_to(root).as_posix(): {"bytes": path.stat().st_size, "sha256": sha(path)}
            for path in sorted(root.rglob("*")) if path.is_file()}


def write_json(path, value):
    path.write_bytes(canonical(value))


def steps(run):
    saved = run / "watched.transcriptor"
    exported = run / "owned-v1-export"
    probe = [{"op": "import", "path": (run / "media/fixture-a.mp4").as_posix()},
             {"op": "save", "path": (run / "probe.transcriptor").as_posix()}, {"op": "quit"}]
    main = [{"op": "import_v1", "path": (run / "synthetic-editorial").as_posix(), "media": (run / "media/fixture-a.mp4").as_posix()},
            {"op": "assert", "revision": 1, "layers": 9, "items": 17},
            {"op": "save", "path": saved.as_posix()},
            {"op": "export_v1_folder", "dest": exported.as_posix(), "include_montage": True, "ms": 30000},
            {"op": "dump", "path": (run / "export-first.json").as_posix()},
            {"op": "await_signal", "path": (run / "first-export-verified.signal").as_posix(), "ms": 30000},
            {"op": "export_v1_folder", "dest": exported.as_posix(), "include_montage": True, "ms": 30000, "expect_error": "IO"},
            {"op": "assert", "revision": 1},
            {"op": "dump", "path": (run / "external-edit-ready.json").as_posix()},
            {"op": "await_signal", "path": (run / "external-edited.signal").as_posix(), "ms": 30000},
            {"op": "wait_external", "fields": 1, "conflicts": 0, "ms": 15000},
            {"op": "dump", "path": (run / "external-review.json").as_posix()},
            {"op": "screenshot", "path": (run / "external-review.png").as_posix()},
            {"op": "apply_external", "ms": 15000},
            {"op": "assert", "revision": 2, "layers": 9, "items": 17},
            {"op": "dump", "path": (run / "external-applied.json").as_posix()},
            {"op": "undo"}, {"op": "assert", "revision": 3},
            {"op": "dump", "path": (run / "external-undone.json").as_posix()},
            {"op": "redo"}, {"op": "assert", "revision": 4},
            {"op": "dump", "path": (run / "external-redone.json").as_posix()},
            {"op": "save", "path": saved.as_posix()}, {"op": "quit"}]
    reopen = [{"op": "open", "path": (saved / "project.json").as_posix()}, {"op": "assert", "revision": 4},
              {"op": "undo"}, {"op": "assert", "revision": 5}, {"op": "redo"}, {"op": "assert", "revision": 6},
              {"op": "save", "path": saved.as_posix()}, {"op": "new_project"},
              {"op": "import_v1", "path": exported.as_posix(), "media": (run / "media/fixture-a.mp4").as_posix()},
              {"op": "assert", "revision": 1, "layers": 9, "items": 17},
              {"op": "dump", "path": (run / "document-roundtrip.json").as_posix()}, {"op": "quit"}]
    for label, value in [("probe", probe), ("main", main), ("reopen", reopen)]:
        write_json(run / f"{label}-script.json", value)


def prepare(run):
    allowed = (ROOT / ".local", ROOT / "implementation/evidence/e2/continuacion-14")
    if run.exists() or not any(run.is_relative_to(path) and run != path for path in allowed):
        raise ValueError("Use a fresh own V2 evidence directory")
    run.mkdir(parents=True)
    fixture = ROOT / "tests/fixtures/v1/demo-a/editorial"
    shutil.copytree(fixture, run / "synthetic-editorial")
    (run / "media").mkdir()
    shutil.copy2(ROOT / "tests/fixtures/media/fixture-a.mp4", run / "media/fixture-a.mp4")
    (run / "synthetic-editorial/empty/nested").mkdir(parents=True)
    (run / "synthetic-editorial/support").mkdir()
    (run / "synthetic-editorial/support/own-synthetic.bin").write_bytes(b"Invented V2 documentary evidence.\n" * 32768)
    steps(run)
    write_json(run / "prepared.json", {"prepared_only": True, "native_executed": False,
        "media_sha256": sha(run / "media/fixture-a.mp4"), "original_fixture": manifest(fixture),
        "requires_new_build": True, "forbidden_old_gui_sha256": OLD_GUI_SHA, "budget_bytes": CAP})
    (run / "README.txt").write_text(
        "PREPARED ONLY: no native acceptance yet. Do not run old build349df or touch human PID6868.\n"
        "After root authorizes a compiled new build, execute:\n"
        f"python tests/scripts/accept_e3_document_external_native.py --run {run} --exe NEW_ABSOLUTE_EXE --sha256 NEW_HASH\n"
        "Runner probes its copied medium through native V2, rebases only copied synthetic documents to actual fingerprint,\n"
        "then performs new-folder export, rejects repeated destination without changing bytes, waits for real monitor,\n"
        "applies one synthetic project-name change (human_review defaults false), Undo/Redo, Save/reopen and reimport export.\n"
        "No forced watcher polling, direct session commit, V1 execution, ML, physical playback or human GUI control.\n"
        "Full field-conflict choices, stale-diff Apply, V1-linked document watcher and permission UX remain separate cases.\n", encoding="utf-8")


def rebase_own_fixture(run):
    folder = run / "synthetic-editorial"
    source = project(run / "probe.transcriptor/project.json")
    asset = next(asset for asset in source["assets"] if asset["name"] == "fixture-a.mp4")
    fingerprint = asset["fingerprint"]
    master_path = folder / "demo-a.editorial.master.json"
    master = read(master_path)
    old_digest = next(read(path)["source_master_digest"] for path in (folder / "layers").glob("*.json"))
    master["media"].update(path=(run / "media/fixture-a.mp4").as_posix(), fingerprint=copy.deepcopy(fingerprint))
    master["project"]["name"] = "Invented V2 document/external acceptance"
    for track in master["tracks"].values():
        track["label"] = "Invented speaker"
    master["own_synthetic_extension"] = {"invented": "Immutable auxiliary fixture. " * 4096}
    new_digest = hashlib.sha256(canonical({key: value for key, value in master.items() if key not in {"generated_at", "chunks"}})).hexdigest()
    write_json(master_path, master)
    def update(value):
        if isinstance(value, dict):
            if {"size", "hash_muestreado", "inventario_sha256"} <= value.keys():
                value.update({key: fingerprint[key] for key in {"size", "hash_muestreado", "inventario_sha256"}})
            return {key: new_digest if child == old_digest else update(child) for key, child in value.items()}
        if isinstance(value, list):
            return [update(child) for child in value]
        return value
    for path in folder.rglob("*.json"):
        if path != master_path:
            write_json(path, update(read(path)))
    write_json(run / "native-inputs.json", {"files": manifest(folder), "fingerprint": fingerprint, "master_digest": new_digest})


def execute(run, phase, exe, callback=None):
    environment = os.environ.copy()
    for variable, directory in [("TRANSCRIPTOR_CONFIG_DIR", "config"), ("TRANSCRIPTOR_CACHE_DIR", "cache"), ("TRANSCRIPTOR_LOGS_DIR", "logs")]:
        environment[variable] = str(run / phase / directory)
    started = time.monotonic()
    report = {"phase": phase, "accepted": False}
    try:
        with (run / f"{phase}-stdout.log").open("wb") as stdout, (run / f"{phase}-stderr.log").open("wb") as stderr:
            process = subprocess.Popen([str(exe), "--script", str(run / f"{phase}-script.json")], cwd=run, env=environment,
                                       stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
            assert process.pid != 6868
            report["pid"] = process.pid
            kernel, job = own_job(process)
            try:
                while process.poll() is None:
                    footprint = sum(path.stat().st_size for path in run.rglob("*") if path.is_file())
                    report["peak_footprint_bytes"] = max(footprint, report.get("peak_footprint_bytes", 0))
                    if time.monotonic()-started > 120 or footprint > CAP:
                        raise TimeoutError("Own native acceptance time/footprint limit")
                    if callback:
                        callback()
                    time.sleep(.05)
            finally:
                kernel.CloseHandle(job)  # Only this exact process tree.
                process.wait(timeout=10)
            report["exit_code"] = process.returncode
        log = (run / f"{phase}-script.log").read_text(encoding="utf-8", errors="replace")
        if process.returncode != 0 or "quit; fallos=false" not in log:
            raise AssertionError("Native script failed; retain original logs and stop")
        report["accepted"] = True
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        report["duration_seconds"] = time.monotonic()-started
        write_json(run / f"{phase}-process.json", report)


def run_native(run, exe, expected_sha):
    metadata = read(run / "prepared.json")
    if not metadata["prepared_only"] or (run / "acceptance.json").exists() or (run / "probe.transcriptor").exists():
        raise ValueError("Run only a fresh, unexecuted prepared fixture; preserve failures")
    if expected_sha.lower() == OLD_GUI_SHA or sha(exe) != expected_sha.lower():
        raise ValueError("Explicit matching NEW compiled editor SHA required")
    if manifest(ROOT / "tests/fixtures/v1/demo-a/editorial") != metadata["original_fixture"]:
        raise ValueError("Read-only synthetic source changed since preparation")
    write_json(run / "binary.json", {"path": str(exe), "sha256": sha(exe), "bytes": exe.stat().st_size})
    execute(run, "probe", exe)
    rebase_own_fixture(run)
    state = {}
    def callback():
        if not state.get("first") and (run / "export-first.json").exists():
            state["first"] = manifest(run / "owned-v1-export")
            if not state["first"] or ".transcriptor-export.json" not in state["first"]:
                raise AssertionError("Missing completed documentary export")
            write_json(run / "first-export-manifest.json", state["first"])
            (run / "first-export-verified.signal").write_bytes(b"")
        if not state.get("external") and (run / "external-edit-ready.json").exists():
            if manifest(run / "owned-v1-export") != state["first"]:
                raise AssertionError("Repeated destination changed first export")
            path = run / "watched.transcriptor/project.json"
            wire = read(path)
            data = wire["project"] if wire.get("schema") == "transcriptor-project-storage/1" else wire
            state["original_name"] = data["name"]
            data["name"] = "Invented external edit owned by E3 fixture"
            temporary = path.with_name("own-external.partial")
            temporary.write_bytes(canonical(wire))
            os.replace(temporary, path)
            state["external"] = True
            write_json(run / "external-write.json", {"before_name": state["original_name"], "after_name": data["name"], "revision": data["revision"], "file_sha256": sha(path)})
            (run / "external-edited.signal").write_bytes(b"")
    execute(run, "main", exe, callback)
    review = read(run / "external-review.json")["external"]["change"]
    assert review["base_revision"] == 1 and len(review["fields"]) == 1 and not review["conflicts"] and review["human_review"] is False
    assert review["fields"][0]["path"] == "/name"
    expected_name = "Invented external edit owned by E3 fixture"
    for label, revision, name in [("external-applied", 2, expected_name), ("external-undone", 3, state["original_name"]), ("external-redone", 4, expected_name)]:
        snapshot = read(run / f"{label}.json")
        assert snapshot["revision"] == revision and snapshot["name"] == name
    execute(run, "reopen", exe)
    saved = project(run / "watched.transcriptor/project.json")
    assert saved["revision"] == 6 and saved["name"] == expected_name
    inputs = read(run / "native-inputs.json")["files"]
    assert manifest(run / "synthetic-editorial") == inputs
    export = run / "owned-v1-export"
    for name in ["demo-a.editorial.master.json", "support/own-synthetic.bin"]:
        assert sha(export / name) == inputs[name]["sha256"]
    assert (export / "empty/nested").is_dir()
    assert manifest(ROOT / "tests/fixtures/v1/demo-a/editorial") == metadata["original_fixture"]
    assert sha(run / "media/fixture-a.mp4") == metadata["media_sha256"]
    write_json(run / "acceptance.json", {"accepted": True, "binary_sha256": expected_sha.lower(), "external_change": review,
        "native_saved_revision": 6, "source_bundle_bytes_preserved": True, "repeated_destination_unchanged": True,
        "budget_bytes": CAP, "gui_exercised": True, "physical_audio": False, "inference": False,
        "limits": ["One nonconflicting project-name change, not V1-linked document watcher", "Human conflict/consent UX not accepted",
                   "No independent stale-diff/racing writer case", "Prepared fixture alone is not native acceptance"]})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepare", type=Path)
    parser.add_argument("--run", type=Path)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--sha256")
    args = parser.parse_args()
    if args.prepare and not args.run:
        prepare(args.prepare.resolve())
    elif args.run and args.exe and args.sha256 and not args.prepare:
        run = args.run.resolve()
        if not any(run.is_relative_to(path) and run != path for path in [ROOT / ".local", ROOT / "implementation/evidence/e2/continuacion-14"]):
            parser.error("Run must remain inside own V2 evidence scope")
        run_native(run, args.exe.resolve(), args.sha256)
    else:
        parser.error("Choose --prepare OWN_NEW_DIR or --run PREPARED_DIR --exe NEW_EXE --sha256 NEW_HASH")


if __name__ == "__main__":
    main()
