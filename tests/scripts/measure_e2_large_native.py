"""Run a bounded synthetic V2 desktop workload and sample only its own process.

Uses a read-only asset descriptor from an existing synthetic V2 project. No
production source, models, original media, or other desktop process is changed.
"""
import argparse
import copy
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
FLICKS = 705600000


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


class ProcessMemory(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in [
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage",
            "PagefileUsage", "PeakPagefileUsage", "PrivateUsage",
        ]
    ]


def prepare(source_project, run, pairs, items, reuse_fixture, extended):
    original = json.loads(source_project.read_text(encoding="utf-8-sig"))
    asset = copy.deepcopy(next(a for a in original["assets"] if a["kind"] == "video"))
    source = Path(asset["path"])
    if not source.is_absolute():
        source = (source_project.parent / source).resolve()
    fixture_root = (ROOT / "tests/fixtures/media").resolve()
    if not source.resolve().is_relative_to(fixture_root):
        raise ValueError("Only synthetic media inside tests/fixtures/media are permitted")
    asset["path"] = source.as_posix()
    sequence = copy.deepcopy(original["sequences"][0])
    sequence.update(id="seq-perf-synthetic", name="Synthetic repetition", width=1280, height=720, markers=[], clips=[])
    video = copy.deepcopy(next(t for t in sequence["tracks"] if t["kind"] == "video"))
    audio = copy.deepcopy(next(t for t in sequence["tracks"] if t["kind"] == "audio"))
    video.update(id="track-perf-video", name="V1")
    audio.update(id="track-perf-audio", name="A1", gain_db=0.0)
    sequence["tracks"] = [video, audio]
    template = copy.deepcopy(next(c for c in original["sequences"][0]["clips"] if c["asset_id"] == asset["id"] and "audio_stream" not in c))
    for index in range(pairs):
        for track in [video, audio]:
            clip = copy.deepcopy(template)
            clip.update(id=f"clip-perf-{track['kind']}-{index:06d}", track_id=track["id"],
                        position=index * FLICKS, source={"start": index % 12 * FLICKS, "end": (index % 12 + 1) * FLICKS},
                        link_group=f"link-perf-{index:06d}", name=f"Synthetic {index}", gain_db=0.0)
            clip.pop("provenance", None)
            if track["kind"] == "audio":
                clip["audio_stream"] = 0
            sequence["clips"].append(clip)
    layers = []
    for n in range(2):
        rows = []
        for index in range(n * (items // 2), (n + 1) * (items // 2)):
            start = index * (12 * FLICKS - FLICKS // 5) // items
            rows.append({"item_id": f"item-perf-{index:06d}", "label": f"Invented item {index}",
                         "comment": "Synthetic benchmark; no transcription or personal material",
                         "ranges": [{"start": start, "end": start + FLICKS // 5}],
                         "state": "proposed", "edited": True, "origin": "user"})
        layers.append({"layer_id": f"layer-perf-{n}", "kind": "user", "name": f"Invented layer {n}",
                       "asset_id": asset["id"], "items": rows, "visible": True, "locked": False, "color": "#d09947"})
    # /1 is a documented V2 snapshot input with no prior audit/history. This
    # intentionally excludes durable-history costs from this UI benchmark.
    project = {"schema": "transcriptor-project/1", "project_id": f"proj-perf-{uuid.uuid4().hex[:12]}",
               "name": "Synthetic native performance", "revision": 0, "assets": [asset],
               "sequences": [sequence], "active_sequence": sequence["id"], "layers": layers,
               "layer_order": [layer["layer_id"] for layer in layers], "settings": {"snapping": True, "skip_trims_on_play": False}}
    target = run / "large.transcriptor/project.json"
    target.parent.mkdir()
    target.write_text(json.dumps(project, separators=(",", ":")), encoding="utf-8")
    if reuse_fixture:
        reused = json.loads(reuse_fixture.read_text(encoding="utf-8"))
        if len(reused["sequences"][0]["clips"]) != pairs * 2 or sum(len(layer["items"]) for layer in reused["layers"]) != items or reused["assets"] != [asset]:
            raise ValueError("Reuse fixture must match the requested workload and synthetic source")
        target.write_bytes(reuse_fixture.read_bytes())
    steps = [{"op": "log", "text": "PERF_OPEN_BEGIN"}, {"op": "open", "path": target.as_posix()},
             {"op": "assert", "clips": pairs * 2, "tracks": 2, "layers": 2, "items": items},
             {"op": "action", "id": "view.fit"}, {"op": "seek", "t": 1.0},
             {"op": "wait_frame", "ms": 15000}, {"op": "log", "text": "PERF_OPEN_FRAME_READY"},
             {"op": "set_in", "t": 0.0}, {"op": "set_out", "t": 4.0},
             {"op": "export", "preset": "h264-720p", "dest": (run / "under-load.mp4").as_posix(), "range": True},
             {"op": "log", "text": "PERF_EXPORT_ENQUEUED"}, {"op": "seek", "t": 4.0},
             {"op": "wait_frame", "ms": 15000}, {"op": "log", "text": "PERF_SEEK_EXPORT_READY"},
             {"op": "step_frames", "frames": 1}, {"op": "log", "text": "PERF_STEP_BEGIN"}]
    for index in range(20):
        steps.extend([{"op": "wait", "ms": 50}, {"op": "dump", "path": (run / f"step-{index:02d}.json").as_posix()}])
    steps.append({"op": "assert", "position": [4.0333, 4.0334], "playing": False})
    if extended:
        steps.extend([{"op": "wait_export", "ms": 120000}, {"op": "set_out", "t": 12.0},
                      {"op": "export", "preset": "h264-720p", "dest": (run / "playback-load.mp4").as_posix(), "range": True},
                      {"op": "log", "text": "PERF_PLAY_EXPORT_ENQUEUED"}])
    steps.extend([{"op": "play"}, {"op": "log", "text": "PERF_PLAY_BEGIN"}])
    for index in range(30):
        steps.extend([{"op": "wait", "ms": 100}, {"op": "dump", "path": (run / f"play-{index:02d}.json").as_posix()}])
    steps.extend([{"op": "pause"}, {"op": "wait", "ms": 500}, {"op": "log", "text": "PERF_PLAY_END"},
                  {"op": "dump", "path": (run / "final-state.json").as_posix()},
                  {"op": "wait_export", "ms": 120000}])
    if extended:
        steps.extend([{"op": "select_item", "layer": 0, "index": items // 2 - 1},
                      {"op": "view", "mode": "source"}, {"op": "action", "id": "view.fit"},
                      {"op": "log", "text": "PERF_CACHE_BEGIN"}, {"op": "seek", "t": 1.0}, {"op": "wait_frame", "ms": 15000},
                      {"op": "log", "text": "PERF_CACHE_SOURCE_FRAME_READY"}, {"op": "play"}])
        for index in range(10):
            steps.extend([{"op": "wait", "ms": 100}, {"op": "dump", "path": (run / f"cache-{index:02d}.json").as_posix()}])
        steps.extend([{"op": "pause"}, {"op": "wait", "ms": 500},
                      {"op": "assert_caches", "wave_columns_min": 1, "thumb_tiles_min": 1},
                      {"op": "screenshot", "path": (run / "large-source.png").as_posix()},
                      {"op": "view", "mode": "sequence"}, {"op": "action", "id": "view.fit"}, {"op": "wait", "ms": 500},
                      {"op": "assert", "clips": pairs * 2, "items": items, "revision": 0}])
    steps.extend([{"op": "screenshot", "path": (run / "large-native.png").as_posix()}, {"op": "quit"}])
    script = run / "perf-script.json"
    script.write_text(json.dumps(steps, indent=2), encoding="utf-8")
    return script, {"clips": pairs * 2, "items": items, "layers": 2, "tracks": 2,
                    "sequence_seconds": pairs, "source_seconds": 12, "source_bytes": source.stat().st_size,
                    "source_sha256": digest(source), "source": source.as_posix(), "project_bytes": target.stat().st_size,
                    "project_sha256": digest(target), "schema": project["schema"], "history": "none (synthetic input)"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_project", type=Path)
    parser.add_argument("--expected-exe-sha", required=True)
    parser.add_argument("--pairs", type=int, default=500)
    parser.add_argument("--items", type=int, default=20000)
    parser.add_argument("--exe", type=Path, default=ROOT / "target/release/Transcriptor.exe")
    parser.add_argument("--reuse-fixture", type=Path, help="Copy an existing synthetic benchmark input byte-for-byte")
    parser.add_argument("--extended", action="store_true", help="Add playback/export and source cache/selection acceptance after original reproduction")
    parser.add_argument("--external-processes", required=True, help="Recorded context of other apps/processes; not claimed isolated")
    args = parser.parse_args()
    if os.name != "nt" or not (1 <= args.pairs <= 1000 and 2 <= args.items <= 50000 and args.items % 2 == 0):
        raise ValueError("Requires Windows, <=2000 clips and <=50000 even items")
    run = ROOT / "implementation/evidence/e2/continuacion-14" / ("perf-" + datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S"))
    run.mkdir(parents=True, exist_ok=False)
    script, fixture = prepare(args.source_project.resolve(), run, args.pairs, args.items, args.reuse_fixture, args.extended)
    exe = args.exe.resolve()
    exe_sha = digest(exe)
    if exe_sha.lower() != args.expected_exe_sha.lower():
        raise ValueError("Executable identity changed; do not benchmark a different build")
    env = os.environ.copy()
    for variable, name in [("TRANSCRIPTOR_CONFIG_DIR", "config"), ("TRANSCRIPTOR_CACHE_DIR", "cache"), ("TRANSCRIPTOR_LOGS_DIR", "logs")]:
        env[variable] = str(run / name)
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    started = time.perf_counter()
    with (run / "stdout.log").open("w") as stdout, (run / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(exe), "--script", str(script)], cwd=run, env=env, startupinfo=startup, stdout=stdout, stderr=stderr)
        if process.pid == 26948:
            raise RuntimeError("Protected human process must never be measured")
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.K32GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessMemory), wintypes.DWORD]
        kernel.K32GetProcessMemoryInfo.restype = wintypes.BOOL
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = kernel.OpenProcess(0x1000 | 0x0010, False, process.pid)
        if not handle:
            process.terminate()
            raise ctypes.WinError(ctypes.get_last_error())
        events, samples, dumps = [], [], {}
        seen_lines = 0
        log = script.with_suffix(".log")
        try:
            while process.poll() is None:
                elapsed = time.perf_counter() - started
                if elapsed > 180:
                    process.terminate()  # Only the Popen-owned benchmark process.
                    raise TimeoutError("Own native benchmark exceeded 180 seconds")
                memory = ProcessMemory()
                memory.cb = ctypes.sizeof(memory)
                if kernel.K32GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb):
                    samples.append({"seconds": elapsed, "working_set_bytes": memory.WorkingSetSize,
                                    "peak_working_set_bytes": memory.PeakWorkingSetSize, "private_bytes": memory.PrivateUsage})
                if log.exists():
                    lines = log.read_text(encoding="utf-8", errors="replace").splitlines()
                    for line in lines[seen_lines:]:
                        if line.startswith(("paso ", "PERF_", "fotograma presentado", "ERROR", "ASSERT", "caches:", "  toast: Info: Exportado")):
                            events.append({"seconds": elapsed, "line": line})
                        if line.startswith("estado volcado en "):
                            path = Path(line.removeprefix("estado volcado en "))
                            if path.is_file() and path.name not in dumps:
                                value = json.loads(path.read_text(encoding="utf-8"))
                                dumps[path.name] = {"seconds": elapsed, "player": value["player"], "last_frame": value["last_frame"]}
                    seen_lines = len(lines)
                time.sleep(0.05)
        finally:
            kernel.CloseHandle(handle)
        process.wait()
    log_text = log.read_text(encoding="utf-8") if log.exists() else ""
    first = lambda token: next((event["seconds"] for event in events if event["line"] == token), None)
    open_begin, open_ready = first("PERF_OPEN_BEGIN"), first("PERF_OPEN_FRAME_READY")
    seek_begin = next((event["seconds"] for event in events if "Seek { t: 4.0 }" in event["line"]), None)
    seek_ready = first("PERF_SEEK_EXPORT_READY")
    step_begin = next((event["seconds"] for event in events if "AdvanceFrames { frames: 1 }" in event["line"]), None)
    stepped = next((value["seconds"] for name, value in dumps.items() if name.startswith("step-") and value["player"]["position_ms"] == 4033), None)
    playback = [value for name, value in dumps.items() if name.startswith("play-")]
    cache_samples = [value for name, value in dumps.items() if name.startswith("cache-")]
    size = sum(path.stat().st_size for path in run.rglob("*") if path.is_file())
    report = {"result": "PASS" if "guion terminado; fallos=false" in log_text and process.returncode == 0 else "FAIL",
              "exe": str(exe), "exe_sha256": exe_sha, "pid": process.pid, "protected_human_pid": 26948,
              "os": platform.platform(), "cpu_count": os.cpu_count(), "external_process_context": args.external_processes,
              "fixture": fixture, "elapsed_seconds": time.perf_counter() - started, "exit_code": process.returncode,
              "peak_working_set_bytes": max((s["peak_working_set_bytes"] for s in samples), default=0),
              "peak_private_bytes": max((s["private_bytes"] for s in samples), default=0),
              "open_to_requested_frame_ms": None if open_begin is None or open_ready is None else (open_ready - open_begin) * 1000,
              "seek_during_export_to_frame_ms": None if seek_begin is None or seek_ready is None else (seek_ready - seek_begin) * 1000,
              "step_to_first_observed_position_ms": None if stepped is None or step_begin is None else (stepped - step_begin) * 1000,
              "playback_samples": playback, "cache_playback_samples": cache_samples,
              "cache_open_to_requested_frame_ms": None if first("PERF_CACHE_BEGIN") is None or first("PERF_CACHE_SOURCE_FRAME_READY") is None else (first("PERF_CACHE_SOURCE_FRAME_READY") - first("PERF_CACHE_BEGIN")) * 1000,
              "sampler_max_interval_ms": max(((b["seconds"] - a["seconds"]) * 1000 for a, b in zip(samples, samples[1:])), default=0),
              "reuse_fixture": str(args.reuse_fixture) if args.reuse_fixture else None, "extended": args.extended,
              "stderr": (run / "stderr.log").read_text(encoding="utf-8", errors="replace"),
              "run_bytes": size, "source_unchanged": digest(Path(fixture["source"])) == fixture["source_sha256"],
              "measurement_limits": ["50 ms external sampling; includes GUI scheduling and script I/O", "No isolated OS/DPI/IME/input-latency claim", "Dump serializes 20000 items per sample; no claim of pure frame time", "Peak working set is only own desktop process, excludes FFmpeg child working sets", "Synthetic V2 /1 input excludes durable history, master and model memory"]}
    if size > 1024 ** 3:
        report["result"] = "FAIL_OVERSIZE"
    (run / "measurements.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    (run / "memory-samples.json").write_text(json.dumps(samples), encoding="utf-8")
    (run / "observed-events.json").write_text(json.dumps(events, indent=2), encoding="utf-8")
    print(json.dumps({"run": str(run), "measurements": report}, indent=2))
    raise SystemExit(0 if report["result"] == "PASS" and report["source_unchanged"] else 1)


if __name__ == "__main__":
    main()
