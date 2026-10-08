"""Verify own synthetic PERF exports and summarize sampled playback limits."""
import argparse
import json
import math
from pathlib import Path
import statistics

from verify_export import frame, probe, psnr, audio_peak


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    measurements = json.loads((args.run / "measurements.json").read_text(encoding="utf-8"))
    events = json.loads((args.run / "observed-events.json").read_text(encoding="utf-8"))
    source = measurements["fixture"]["source"]
    exports = []
    for name, duration, times in [("under-load.mp4", 4, [0.5, 2.5, 3.5]), ("playback-load.mp4", 12, [0.5, 5.5, 11.5])]:
        path = args.run / name
        if not path.is_file():
            continue
        metadata = probe(str(path))
        video = next(s for s in metadata["streams"] if s["codec_type"] == "video")
        checks = []
        for at in times:
            output = frame(str(path), at)
            right, wrong = psnr(output, frame(source, at)), psnr(output, frame(source, (at + 6) % 12))
            checks.append({"time_seconds": at, "source_psnr_db": right, "wrong_source_psnr_db": wrong, "pass": right > 28 and right > wrong + 3})
        peak = audio_peak(str(path))
        exports.append({"path": str(path), "duration": float(metadata["format"]["duration"]), "video": {key: video.get(key) for key in ["codec_name", "width", "height", "r_frame_rate", "nb_frames"]},
                        "audio_peak_dbfs": peak, "checks": checks,
                        "pass": abs(float(metadata["format"]["duration"]) - duration) < 0.08 and video["codec_name"] == "h264" and video["width"] == 1280 and video["height"] == 720 and video["r_frame_rate"] == "30/1" and int(video["nb_frames"]) == duration * 30 and peak is not None and peak > -60 and all(check["pass"] for check in checks)})
    playback = []
    for key in ["playback_samples", "cache_playback_samples"]:
        rows = measurements.get(key, [])
        if not rows:
            continue
        elapsed = rows[-1]["seconds"] - rows[0]["seconds"]
        advance = (rows[-1]["player"]["position_ms"] - rows[0]["player"]["position_ms"]) / 1000
        lags = [row["player"]["position_ms"] - row["last_frame"]["position_ms"] for row in rows if row["last_frame"]]
        decodes = [row["player"]["decode_ms"] for row in rows]
        playback.append({"phase": key, "samples": len(rows), "elapsed_seconds": elapsed, "position_advance_seconds": advance,
                         "sampled_clock_ratio": advance / elapsed, "buffering_snapshots": sum(row["player"]["buffering"] for row in rows),
                         "decode_ms_median": statistics.median(decodes), "decode_ms_p95": percentile(decodes, 0.95), "decode_ms_max": max(decodes),
                         "snapshot_to_frame_lag_ms_median": statistics.median(lags), "snapshot_to_frame_lag_ms_max": max(lags)})
    play_start = next((e["seconds"] for e in events if e["line"] == "PERF_PLAY_BEGIN"), None)
    export_done = next((e["seconds"] for e in events if e["line"].startswith("  toast: Info: Exportado") and "playback-load.mp4" in e["line"]), None)
    analysis = {"export_result": "PASS" if exports and all(e["pass"] for e in exports) else "FAIL", "exports": exports, "playback": playback,
                "playback_export_overlap_observed_seconds": None if play_start is None or export_done is None else export_done - play_start,
                "limits": ["Snapshots are not frame presentation timestamps or hardware/audio measurements", "Each dump serializes all 20000 items; sampler can be delayed", "Clock ratio is measured between sparse external observations, not instantaneous FPS", "Decode metrics belong to the player, not UI frame time", "Export completion is observed when its toast is first logged"]}
    (args.run / "analysis.json").write_text(json.dumps(analysis, indent=2), encoding="utf-8")
    print(json.dumps(analysis, indent=2))
    raise SystemExit(0 if analysis["export_result"] == "PASS" else 1)


if __name__ == "__main__":
    main()
