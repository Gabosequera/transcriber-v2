"""Prepare an isolated native script for a human drag; never launch any GUI."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FLICKS = 705600000


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("synthetic_project", type=Path)
    args = parser.parse_args()
    project = json.loads(args.synthetic_project.read_text(encoding="utf-8"))
    if len(project["sequences"][0]["clips"]) != 1000 or len(project["layers"]) != 2:
        raise ValueError("Requires the original synthetic 1000-clip PERF input")
    for asset in project["assets"]:
        if not Path(asset["path"]).resolve().is_relative_to((ROOT / "tests/fixtures/media").resolve()):
            raise ValueError("Synthetic fixture sources only")
    # Deliberately broad equal ranges make the middle a body hit rather than
    # the trim handle of the latest overlapping short item. IDs stay distinct.
    for layer_index, layer in enumerate(project["layers"]):
        for item in layer["items"]:
            item["ranges"] = [{"start": layer_index * 6 * FLICKS, "end": (layer_index + 1) * 6 * FLICKS}]
    project["project_id"] += "-preview"
    project["name"] = "Invented massive preview acceptance"
    run = ROOT / "implementation/evidence/e2/continuacion-14" / ("preview-prepared-" + datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S"))
    run.mkdir(parents=True, exist_ok=False)
    target = run / "preview.transcriptor/project.json"
    target.parent.mkdir()
    target.write_text(json.dumps(project, separators=(",", ":")), encoding="utf-8")
    config = run / "config"
    config.mkdir()
    (config / "ui-state.json").write_text(json.dumps({"library_width": 200.0, "inspector_width": 260.0, "timeline_height": 500.0,
        "console_open": False, "console_height": 160.0, "last_project": None, "last_media_dir": None, "last_export_dir": None,
        "snapping": False, "follow_playhead": True, "track_heights": {}, "zoom_px_per_s": 40.0}), encoding="utf-8")
    p = lambda name: (run / name).as_posix()
    steps = [{"op": "viewport", "width": 1400.0, "height": 1000.0, "zoom": 0.85},
             {"op": "open", "path": target.as_posix()}, {"op": "assert", "clips": 1000, "items": 20000, "revision": 0},
             {"op": "select_item", "layer": 0, "index": 0}, {"op": "view", "mode": "source"},
             {"op": "action", "id": "view.fit"}, {"op": "seek", "t": 3.0}, {"op": "wait_frame", "ms": 15000},
             {"op": "action", "id": "tools.select_all"}, {"op": "dump", "path": p("before.json")},
             {"op": "screenshot", "path": p("ready.png")}, {"op": "log", "text": "PHASE1_READY: human drag layer0 middle right slightly; hold mouse until screenshot"},
             {"op": "await_signal", "path": p("capture-commit.signal"), "ms": 300000},
             {"op": "screenshot", "path": p("preview-commit.png")},
             {"op": "await_signal", "path": p("released.signal"), "ms": 300000}, {"op": "wait", "ms": 1000},
             {"op": "dump", "path": p("after-commit.json")}, {"op": "assert", "clips": 1000, "items": 20000, "revision": 1},
             {"op": "undo"}, {"op": "assert", "revision": 2, "items": 20000}, {"op": "redo"}, {"op": "assert", "revision": 3, "items": 20000},
             {"op": "dump", "path": p("after-redo.json")}, {"op": "action", "id": "tools.select_all"},
             {"op": "log", "text": "PHASE2_READY: human drag same layer; hold for screenshot; then Escape to cancel"},
             {"op": "await_signal", "path": p("capture-cancel.signal"), "ms": 300000},
             {"op": "screenshot", "path": p("preview-cancel.png")},
             {"op": "await_signal", "path": p("cancelled.signal"), "ms": 300000}, {"op": "wait", "ms": 1000},
             {"op": "dump", "path": p("after-cancel.json")}, {"op": "assert", "revision": 3, "items": 20000},
             {"op": "screenshot", "path": p("after-cancel.png")}, {"op": "quit"}]
    (run / "preview-script.json").write_text(json.dumps(steps, indent=2), encoding="utf-8")
    instructions = f"""Prepared only; not executed or accepted. Use a future release containing previewLOD.
Never run against or close the human instance already open. Launch a new instance only after coordination, with
TRANSCRIPTOR_CONFIG_DIR={config}
TRANSCRIPTOR_CACHE_DIR={run / 'cache'}
TRANSCRIPTOR_LOGS_DIR={run / 'logs'}
Transcriptor.exe --script {run / 'preview-script.json'}

This invented input has1000clips/20000items, with10000layer0 ranges[0;6) so a middle body hit can drag all selected items.
At PHASE1_READY, drag the middle of layer0 slightly right (e.g.0.25s), avoiding handles, and KEEP the mouse pressed.
Coordinator creates capture-commit.signal as an empty file here. Hold until preview-commit.png is written.
Then release the mouse, wait for one committed change, and coordinator creates released.signal.
Script asserts revision1, runs undo/redo to revisions2/3, and enters PHASE2_READY.
Drag again and hold; coordinator creates capture-cancel.signal and waits for preview-cancel.png.
Press Escape, release, and coordinator creates cancelled.signal. Revision must stay3.
Compare full items/ranges in after-redo.json vs after-cancel.json; no cancellation mutation is permitted.
Every signal is local/non-secret. Timeouts fail; a screenshot file alone is not acceptance.
No script hook injects pointer events or calls preview: real human input is required. View/layout visibility remains unverified.
"""
    (run / "README.txt").write_text(instructions, encoding="utf-8")
    print(json.dumps({"prepared_only": True, "run": str(run), "project_sha256": hashlib.sha256(target.read_bytes()).hexdigest(), "steps": len(steps)}))


if __name__ == "__main__":
    main()
