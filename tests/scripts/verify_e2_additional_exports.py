"""Check the vertical, PCM, and IN/OUT outputs of e2-escenario1."""
import argparse
import json
from pathlib import Path

from verify_export import audio_peak, frame, probe, psnr


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    args = parser.parse_args()
    reports = []
    for name, duration, size, frames in [
        ("escenario1-vertical.mp4", 12.0, (1080, 1920), 360),
        ("escenario1-audio.wav", 12.0, None, None),
        ("escenario1-inout-720p.mp4", 2.0, (1280, 720), 60),
    ]:
        path = args.run / name
        info = probe(str(path))
        audio = [s for s in info["streams"] if s["codec_type"] == "audio"]
        video = [s for s in info["streams"] if s["codec_type"] == "video"]
        observed_duration = float(info["format"]["duration"])
        assert abs(observed_duration - duration) <= 0.080, (name, observed_duration)
        assert len(audio) == 1 and len(video) == int(size is not None), name
        assert audio[0]["channels"] == 2 and int(audio[0]["sample_rate"]) == 48000, name
        assert audio[0]["codec_name"] == ("pcm_s16le" if size is None else "aac"), name
        if size is not None:
            assert (video[0]["width"], video[0]["height"]) == size, name
            assert video[0]["codec_name"] == "h264" and video[0]["r_frame_rate"] == "30/1", name
            assert int(video[0]["nb_frames"]) == frames, name
        peak = audio_peak(str(path))
        assert peak is not None and peak > -60, (name, peak)
        reports.append({"file": name, "duration": observed_duration, "audio_peak_dbfs": peak, "video_size": size, "frames": frames})

    full = args.run / "escenario1-720p.mp4"
    selected = args.run / "escenario1-inout-720p.mp4"
    for output_time in [0.5, 1.5]:
        observed = frame(str(selected), output_time)
        correct = psnr(observed, frame(str(full), output_time + 2))
        control = psnr(observed, frame(str(full), 10.5))
        assert correct >= 28 and correct > control + 3, (output_time, correct, control)
        reports.append({"inout_time": output_time, "sequence_time": output_time + 2, "psnr_db": correct, "wrong_time_psnr_db": control})
    print(json.dumps({"result": "OK", "checks": reports}, indent=2))


if __name__ == "__main__":
    main()
