"""Bounded native Save As / bundle relocation / durable undo acceptance.

Copies only V2 synthetic fixtures; never executes V1, test binaries or ML.
All filesystem mutations target this newly created own evidence directory.
"""
import copy
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

from measure_e2_rf64_native import own_job

ROOT = Path(__file__).resolve().parents[2]
EXE_SHA = "349df668c006a1565613ad6770edd918fb514243b5e2b32888469335f2837c36"
CAP = 2 * 1024 ** 3


def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def read_project(path):
    wire = json.loads(path.read_text(encoding="utf-8"))
    return wire["project"] if wire.get("schema") == "transcriptor-project-storage/1" else wire


def checked_move(run, source, target):
    for path in [source, target]:
        assert path.resolve().is_relative_to(run.resolve()) and path.resolve() != run.resolve()
    assert source.is_dir() and not target.exists()
    source.rename(target)


def tree_hashes(root):
    return {p.relative_to(root).as_posix(): {"bytes": p.stat().st_size, "sha256": sha(p)} for p in sorted(root.rglob("*")) if p.is_file()}


def prepare(run):
    fixture = ROOT / "tests/fixtures/v1/demo-a/editorial"
    folder = run / "synthetic-editorial"
    shutil.copytree(fixture, folder)
    media = run / "media/fixture-a.mp4"; media.parent.mkdir()
    shutil.copy2(ROOT / "tests/fixtures/media/fixture-a.mp4", media)
    saved = json.loads((ROOT / "implementation/evidence/local/prepared-20260930-133827-995706/escenario1.transcriptor/project.json").read_text(encoding="utf-8"))
    asset = next(a for a in saved["assets"] if a["name"] == "fixture-a.mp4")
    fp = asset["fingerprint"]
    master_path = folder / "demo-a.editorial.master.json"
    master = json.loads(master_path.read_text(encoding="utf-8"))
    old_digest = next(json.loads(p.read_text(encoding="utf-8"))["source_master_digest"] for p in (folder / "layers").glob("*.json"))
    master["media"].update(path=media.as_posix(), fingerprint=copy.deepcopy(fp))
    master["project"]["name"] = "Invented V2 portability fixture"
    for track in master["tracks"].values():
        track["label"] = "Invented speaker"
    master["synthetic_extension"] = {"text": "Invented immutable evidence. " * 4096}
    new_digest = hashlib.sha256(canonical({k: v for k, v in master.items() if k not in {"generated_at", "chunks"}})).hexdigest()
    master_path.write_bytes(canonical(master))
    def update(value):
        if isinstance(value, dict):
            if {"size", "hash_muestreado", "inventario_sha256"} <= value.keys():
                value.update({k: fp[k] for k in {"size", "hash_muestreado", "inventario_sha256"}})
            for key, child in value.items():
                value[key] = new_digest if child == old_digest else update(child)
        elif isinstance(value, list):
            value = [update(child) for child in value]
        return value
    for path in folder.rglob("*.json"):
        if path != master_path:
            path.write_bytes(canonical(update(json.loads(path.read_text(encoding="utf-8")))))
    auxiliary = folder / "support/payload.bin"; auxiliary.parent.mkdir()
    block = (b"V2 synthetic immutable auxiliary; no personal data.\n" * 25000)[:1024 * 1024]
    block = block.ljust(1024 * 1024, b" ")
    with auxiliary.open("xb") as target:
        for _ in range(34):
            target.write(block)
    (folder / "empty/nested").mkdir(parents=True)
    manifest = tree_hashes(folder)
    assert master_path.stat().st_size > 65536 and auxiliary.stat().st_size > 32 * 1024 ** 2
    (run / "inputs.json").write_text(json.dumps({"media_sha256": sha(media), "files": manifest, "directories": ["empty", "empty/nested"]}, indent=2))
    return folder, media, manifest


def execute(run, phase, steps, checkpoints=None):
    script = run / f"{phase}.json"; script.write_text(json.dumps(steps, indent=2))
    env = os.environ.copy()
    for variable, name in [("TRANSCRIPTOR_CONFIG_DIR", "config"), ("TRANSCRIPTOR_CACHE_DIR", "cache"), ("TRANSCRIPTOR_LOGS_DIR", "logs")]:
        env[variable] = str(run / phase / name)
    start = time.perf_counter(); observations = []; responded = set()
    with (run / f"{phase}-stdout.log").open("w") as stdout, (run / f"{phase}-stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(ROOT / "target/release/Transcriptor.exe"), "--script", str(script)], cwd=run, env=env,
                                   stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
        assert process.pid != 6868
        kernel, job = own_job(process)
        try:
            while process.poll() is None:
                footprint = sum(p.stat().st_size for p in run.rglob("*") if p.is_file())
                if footprint > CAP or time.perf_counter() - start > 120:
                    raise RuntimeError("Own acceptance budget/deadline exceeded")
                for label, root in (checkpoints or {}).items():
                    if label in responded:
                        continue
                    p = root / "project.json"
                    try:
                        saved = read_project(p) if p.is_file() else None
                    except (OSError, ValueError):
                        saved = None
                    if saved and saved["revision"] == 3 and (root / "history.json").is_file() and (root / "audit/index.json").is_file() and not (root / ".pending-commit.json").exists():
                        observations.append({"checkpoint": label, "seconds": time.perf_counter() - start, "revision": saved["revision"], "tree": tree_hashes(root)})
                        (run / f"{label}.signal").write_bytes(b"")
                        responded.add(label)
                time.sleep(0.1)
        finally:
            kernel.CloseHandle(job)
        process.wait(timeout=10)
    log = script.with_suffix(".log").read_text(encoding="utf-8", errors="replace")
    report = {"phase": phase, "pid": process.pid, "exit_code": process.returncode, "elapsed_seconds": time.perf_counter() - start,
              "result": "PASS" if process.returncode == 0 and "guion terminado; fallos=false" in log else "FAIL", "checkpoints": observations}
    (run / f"{phase}-report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps({k: v for k, v in report.items() if k != "checkpoints"}), flush=True)
    assert report["result"] == "PASS", "Native failure preserved; stop"
    return report


def bundles(root, inputs):
    project = read_project(root / "project.json")
    master = project["masters"][0]
    bundle = master["source_bundle"]
    assert {"empty", "empty/nested"} <= set(bundle["directories"])
    checked = {}
    for name, original in inputs.items():
        if name in bundle["files"]:
            file = bundle["files"][name]
            assert file["sha256"] == original["sha256"] and file["size"] == original["bytes"]
            assert file["source"].startswith("@project/source-bundles/")
            target = root / file["source"].removeprefix("@project/")
            assert sha(target) == original["sha256"] and target.stat().st_size == original["bytes"]
            checked[name] = file
        else:
            inline = bundle["documents"][name].encode("utf-8")
            assert len(inline) == original["bytes"] and hashlib.sha256(inline).hexdigest() == original["sha256"]
            checked[name] = {"inline": True, "bytes": len(inline), "sha256": original["sha256"]}
    return {"revision": project["revision"], "project_id": project["project_id"], "master_source_digest": master["source_digest"], "files": checked, "directories": bundle["directories"]}


def verify_identities(run):
    projects = [read_project(run / root / "project.json") for root in ["withdrawn-origin.transcriptor", "relocated.transcriptor", "copy.transcriptor"]]
    assert len({p["project_id"] for p in projects}) == 1
    for project in projects[1:]:
        for key in ("layers", "sequences", "masters"):
            assert project[key] == projects[0][key], "Saved model identity/content changed"
    layer = next(l for l in projects[0]["layers"] if l["name"] == "Human portability edit")
    item = layer["items"][0]
    # IN/OUT are in sequence view: seq2 maps to source6, seq4 maps to
    # source9 in this known montage. This checks preserved fractional ticks,
    # without claiming a Source-view edit or changing range semantics.
    expected = {"start": round(6.000001 * 705600000), "end": round(9.000002 * 705600000)}
    assert item["ranges"] == [expected]
    states = [json.loads((run / name).read_text(encoding="utf-8")) for name in ["reopened-state.json", "undo-state.json", "redo-state.json", "copy-state.json"]]
    ids = lambda state: {i["id"] for layer in state["layers"] for i in layer["items"]}
    assert ids(states[0]) - ids(states[1]) == {item["item_id"]}
    assert ids(states[0]) == ids(states[2]) == ids(states[3])
    return {"result": "PASS", "project_id": projects[0]["project_id"], "revisions": [p["revision"] for p in projects],
            "layers_sequences_masters_exactly_unchanged": True, "durable_submillisecond_ticks": expected,
            "new_item_id": item["item_id"], "undo_removes_only_new_item": True, "redo_and_copy_identity_sets_equal": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resume-run", type=Path, help="Continue an already completed own Save As phase after verifier correction")
    args = parser.parse_args()
    exe = ROOT / "target/release/Transcriptor.exe"
    assert sha(exe) == EXE_SHA
    evidence = ROOT / "implementation/evidence/e2/continuacion-14"
    run = args.resume_run.resolve() if args.resume_run else evidence / datetime.now(timezone.utc).strftime("save-as-%Y%m%d-%H%M%S")
    assert run.resolve().parent == evidence.resolve() and run.name.startswith("save-as-")
    if args.resume_run:
        manifest = json.loads((run / "inputs.json").read_text(encoding="utf-8"))["files"]
        folder, media = run / "synthetic-editorial", run / "media/fixture-a.mp4"
        assert tree_hashes(folder) == manifest
    else:
        run.mkdir(); (run / ".gitignore").write_text("**/payload.bin\n**/source-bundles/*\n**/objects/*\n**/.write.lock\n")
        folder, media, manifest = prepare(run)
    a, b, moved, clone = [run / name for name in ["origin.transcriptor", "save-as.transcriptor", "relocated.transcriptor", "copy.transcriptor"]]
    first = [{"op": "import_v1", "path": folder.as_posix(), "media": media.as_posix()}, {"op": "wait", "ms": 1500},
             # Four editable layers/seven items plus five read-only master
             # projections: A words4/utterances3, B word1/utterance1/laughter1.
             {"op": "assert", "revision": 1, "layers": 9, "items": 17}, {"op": "dump", "path": (run / "imported-state.json").as_posix()},
             {"op": "new_layer", "name": "Human portability edit"}, {"op": "set_in", "t": 2.000001}, {"op": "set_out", "t": 4.000002},
             {"op": "add_range"}, {"op": "assert", "revision": 3, "layers": 10, "items": 18},
             {"op": "save", "path": a.as_posix()}, {"op": "await_signal", "path": (run / "saved-a.signal").as_posix(), "ms": 30000},
             {"op": "save", "path": b.as_posix()}, {"op": "await_signal", "path": (run / "saved-b.signal").as_posix(), "ms": 30000},
             {"op": "screenshot", "path": (run / "save-as-native.png").as_posix()}, {"op": "quit"}]
    print(json.dumps({"run": str(run), "exe_sha256": EXE_SHA, "budget_bytes": CAP}), flush=True)
    if args.resume_run:
        phase1 = json.loads((run / "save-as-report.json").read_text(encoding="utf-8"))
        assert phase1["result"] == "PASS" and "guion terminado; fallos=false" in (run / "save-as.log").read_text(encoding="utf-8")
    else:
        phase1 = execute(run, "save-as", first, {"saved-a": a, "saved-b": b})
    a_bundle, b_bundle = bundles(a, manifest), bundles(b, manifest)
    assert a_bundle == b_bundle
    assert tree_hashes(a / "audit") == tree_hashes(b / "audit"), "Save As dropped/changed audit"
    checked_move(run, folder, run / "withdrawn-editorial")
    checked_move(run, a, run / "withdrawn-origin.transcriptor")
    checked_move(run, b, moved)
    assert not folder.exists() and not a.exists() and not b.exists()
    second = [{"op": "open", "path": (moved / "project.json").as_posix()}, {"op": "assert", "revision": 3, "layers": 10, "items": 18},
              {"op": "dump", "path": (run / "reopened-state.json").as_posix()}, {"op": "undo"}, {"op": "assert", "revision": 4, "layers": 10, "items": 17},
              {"op": "dump", "path": (run / "undo-state.json").as_posix()}, {"op": "redo"}, {"op": "assert", "revision": 5, "items": 18},
              {"op": "save", "path": moved.as_posix()}, {"op": "wait", "ms": 500}, {"op": "seek", "t": 1.5}, {"op": "wait_frame", "ms": 10000},
              {"op": "set_in", "t": 0}, {"op": "set_out", "t": 2}, {"op": "export", "preset": "h264-720p", "dest": (run / "relocated-export.mp4").as_posix(), "range": True},
              {"op": "wait_export", "ms": 30000}, {"op": "dump", "path": (run / "redo-state.json").as_posix()},
              {"op": "screenshot", "path": (run / "relocated-native.png").as_posix()}, {"op": "quit"}]
    phase2 = execute(run, "relocated", second)
    moved_bundle = bundles(moved, manifest)
    shutil.copytree(moved, clone)
    third = [{"op": "open", "path": (clone / "project.json").as_posix()}, {"op": "assert", "revision": 5, "layers": 10, "items": 18},
             {"op": "undo"}, {"op": "assert", "revision": 6, "items": 17}, {"op": "redo"}, {"op": "assert", "revision": 7, "items": 18},
             {"op": "dump", "path": (run / "copy-state.json").as_posix()}, {"op": "save", "path": clone.as_posix()}, {"op": "wait", "ms": 500}, {"op": "quit"}]
    phase3 = execute(run, "copied", third)
    clone_bundle = bundles(clone, manifest)
    from verify_export import frame, psnr, probe, audio_peak
    export = run / "relocated-export.mp4"
    info = probe(str(export))
    assert float(info["format"]["duration"]) == 2 and int(next(s for s in info["streams"] if s["codec_type"] == "video")["nb_frames"]) == 60
    expected = psnr(frame(str(export), 1.5), frame(str(media), 2.5)); wrong = psnr(frame(str(export), 1.5), frame(str(media), 6.5))
    assert expected > 28 and expected > wrong
    peak = audio_peak(str(export)); assert peak > -60
    final = {"result": "PASS", "exe_sha256": EXE_SHA, "phases": [phase1, phase2, phase3], "origin": a_bundle, "save_as": b_bundle,
             "moved": moved_bundle, "copied": clone_bundle, "export": {"sha256": sha(export), "duration": 2, "frames": 60, "expected_psnr": expected, "wrong_psnr": wrong, "audio_peak_dbfs": peak},
             "run_bytes": sum(p.stat().st_size for p in run.rglob("*") if p.is_file()), "source_originals_unchanged": tree_hashes(run / "withdrawn-editorial") == manifest,
             "limitations": ["Own synthetic editorial interchange fixture, no V1 app execution", "Source media remains external and available in own run/media; Save As does not copy media", "No documentary folder-export dialog or watcher Apply hook", "No physical audio/focus acceptance"]}
    final["identity_verification"] = verify_identities(run)
    assert final["run_bytes"] < CAP and final["source_originals_unchanged"]
    (run / "acceptance.json").write_text(json.dumps(final, indent=2))
    print(json.dumps({"result": final["result"], "run": str(run), "run_bytes": final["run_bytes"], "export": final["export"]}), flush=True)


if __name__ == "__main__":
    main()
