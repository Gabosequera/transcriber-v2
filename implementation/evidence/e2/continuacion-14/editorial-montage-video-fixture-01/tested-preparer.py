"""Copy a previously imported synthetic V2 video project; no GUI/ML/probe.

Master, metadata, audit and history stay byte-identical. Prompt sizing invokes
only the real worker's stdlib projection; its conservative layer/sequence inputs
are diagnostics, never a forged request or published editorial contract.
"""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import types

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "implementation/evidence/e2/continuacion-14/save-as-20260930-171900"
DEST = ROOT / ".local/editorial-montage-video-fixture-01"
CAP = 64 * 1024 ** 2


def sha(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def manifest(root):
    return {path.relative_to(root).as_posix(): {"bytes": path.stat().st_size, "sha256": sha(path)}
            for path in sorted(root.rglob("*")) if path.is_file()}


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False), encoding="utf-8")


def main():
    if DEST.exists():
        raise ValueError("Fresh owned fixture only; preserve prior evidence")
    store = SOURCE / "withdrawn-origin.transcriptor"
    before = manifest(store)
    project = json.loads((store / "project.json").read_bytes())["project"]
    assert len(project["masters"]) == 1 and len(project["assets"]) == 1
    evidence = project["masters"][0]
    asset = next(asset for asset in project["assets"] if asset["id"] == evidence["asset_id"])
    assert asset["probe"]["video"] and asset["probe"]["video"]["width"] == 640
    medium = store.parent / asset["path"]  # Same ProjectStore::resolve_path portable base.
    assert medium.is_file() and medium.stat().st_size == asset["fingerprint"]["size"]
    media_sha = sha(medium)
    assert media_sha == sha(ROOT / "tests/fixtures/media/fixture-a.mp4")
    reference = evidence["document"]
    master_file = store / "evidence" / (reference["sha256"] + ".json")
    assert sha(master_file) == reference["sha256"]
    master = json.loads(master_file.read_bytes())
    assert master["transcription"]["model"] == "fixture"
    assert all(track["label"] == "Invented speaker" for track in master["tracks"].values())
    assert master["media"]["duration"] == asset["probe"]["duration"] / 705600000 == 12
    for key in ("size", "hash_muestreado", "inventario_sha256"):
        assert master["media"]["fingerprint"][key] == asset["fingerprint"][key]
    assert Path(master["media"]["path"]).is_file()
    assert sha(Path(master["media"]["path"])) == media_sha

    worker_path = ROOT / "workers/python/editorial_worker.py"
    worker_bytes = worker_path.read_bytes()
    worker = types.ModuleType("editorial_fixture_projection")
    worker.__file__ = str(worker_path)
    exec(compile(worker_bytes, str(worker_path), "exec"), worker.__dict__)
    assert not any(name.partition(".")[0] in {"torch", "transformers", "numpy", "tokenizers", "safetensors"} for name in sys.modules)
    # Include native metadata as an intentionally larger diagnostic view than
    # layer_to_v1. A real bound request must still be created by the Rust host.
    layers = [layer for layer in project["layers"] if layer["asset_id"] == asset["id"]
              and "master_projection" not in layer.get("extra", {}) and layer["kind"] in {"user", "topics", "ai"}]
    sequence = next(sequence for sequence in project["sequences"] if sequence["id"] == project["active_sequence"])
    documents = {"project.editorial.master.json": master,
                 "views/montage-request.json": {"scope": {"t_ini": 0, "t_fin": 12}, "pass_required": 1,
                    "target_seconds": 600, "tolerance": .15, "min_clip_seconds": 3,
                    "max_clip_seconds": 120, "allow_reorder": True, "media_duration": 12},
                 "views/layers.json": {"layers": layers}, "views/montage-current.json": sequence}
    messages = worker.messages_for(documents, "montage", {"trim_mode": "content"})
    projection_bytes = sum(len(message["content"].encode()) for message in messages)
    context = worker.prompt_context(documents, "montage")
    assert "synthetic_extension" not in context and "Invented immutable evidence." not in messages[1]["content"]
    assert sha(worker_path) == hashlib.sha256(worker_bytes).hexdigest()
    expected = sum(value["bytes"] for value in before.values()) + medium.stat().st_size
    assert expected < CAP
    DEST.mkdir()
    copied = DEST / "fixture.transcriptor"
    shutil.copytree(store, copied)
    (DEST / "media").mkdir()
    shutil.copy2(medium, DEST / asset["path"])
    assert manifest(copied) == before and manifest(store) == before
    assert sha(DEST / asset["path"]) == media_sha == sha(medium)
    report = {"prepared_only": True, "native_llm_inference": False, "gui_exercised": False,
              "asr_inference": False, "speech_invented_over_synthetic_video": True,
              "real_speech_alignment_accepted": False, "new_probe_fabricated": False,
              "original_common_import": str(SOURCE / "save-as.log"), "source_store": str(store),
              "fixture_store": str(copied), "source_unchanged": True, "project_files_byte_identical": True,
              "source_manifest": before, "media": {"portable_path": asset["path"], "source": str(medium),
                  "copy": str(DEST / asset["path"]), "sha256": media_sha, "bytes": medium.stat().st_size},
              "first_master_asset_id": evidence["asset_id"], "asset_id": asset["id"], "video_probe": asset["probe"]["video"],
              "fingerprint": asset["fingerprint"], "master_sha256": reference["sha256"], "master_bytes": master_file.stat().st_size,
              "master_source_digest": evidence["source_digest"], "revision": project["revision"],
              "worker_sha256": sha(worker_path), "script_sha256": sha(Path(__file__)),
              "conservative_prompt_projection_bytes": projection_bytes, "context_cap": worker.MAX_CONTEXT_BYTES,
              "root_extension_not_projected": True, "tokenization_exercised": False,
              "sizing_is_not_authoritative_bound_request": True, "copied_data_bytes": expected, "budget_bytes": CAP}
    write(DEST / "prepared.json", report)
    write(DEST / "conservative-prompt-context.json", context)
    (DEST / "README.txt").write_text(
        "PREPARED ONLY: technical LLM montage fixture, no ASR/real speech alignment or human quality acceptance.\n"
        "Copied exact previously imported V2 demo video/master/project/audit/history/bundles; all original files intact.\n"
        "asset.path resolves from fixture.transcriptor parent to its own media/fixture-a.mp4 copy.\n"
        "Master words/acoustic values are explicitly invented demo fixture values, not results transferred from other media.\n"
        "No new Rust importer is required: original import used normal native ImportV1 commands; current Rust host\n"
        "must load the copied ProjectStore, validate source bundles and prepare a fresh montage request normally.\n"
        "Conservative stdlib sizing is not a bound request; tokenizer4096 cap still requires the real host check.\n"
        "After coordinating the ML window, parent may run:\n"
        f".local/e5-editorial-venv/Scripts/python.exe tests/scripts/run_editorial_host_owned.py .local/e5-editorial-montage-host-01 {copied} --kind montage\n",
        encoding="utf-8")
    assert sum(path.stat().st_size for path in DEST.rglob("*") if path.is_file()) < CAP
    print(json.dumps({key: report[key] for key in ("prepared_only", "first_master_asset_id", "master_bytes", "conservative_prompt_projection_bytes", "copied_data_bytes", "source_unchanged")}))


if __name__ == "__main__":
    main()
