"""Verifica una exportación contra fotogramas fuente conocidos.

Uso: python verify_export.py <export.mp4> <expect.json>

expect.json: {"source": "...fixture-a.mp4", "fps": 30, "checks": [{"t": 2.5, "source_t": 1.5, "wrong_t": 6.5}, ...],
              "duration": 12.0, "tolerance_ms": 80}

Para cada check decodifica el fotograma del export en t y los del source en
source_t (esperado) y wrong_t (control); el PSNR con el esperado debe ser el
mayor y superar 28 dB. No exige hashes idénticos (codec con pérdida).
"""
import json, math, os, subprocess, sys

from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from support import media_tool

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
FF = media_tool("ffmpeg")
FP = media_tool("ffprobe")
W, H = 320, 180


def frame(path, t):
    out = subprocess.run([FF, "-v", "error", "-ss", f"{t:.6f}", "-i", path, "-frames:v", "1",
                          "-vf", f"scale={W}:{H}", "-f", "rawvideo", "-pix_fmt", "gray", "-"],
                         capture_output=True, check=True).stdout
    assert len(out) == W * H, f"frame incompleto en {path}@{t}: {len(out)}"
    return out


def psnr(a, b):
    mse = sum((x - y) * (x - y) for x, y in zip(a, b)) / len(a)
    return 99.0 if mse == 0 else 10 * math.log10(255 * 255 / mse)


def probe(path):
    out = subprocess.run([FP, "-v", "error", "-print_format", "json", "-show_format", "-show_streams", path],
                         capture_output=True, check=True, text=True).stdout
    return json.loads(out)


def audio_peak(path):
    out = subprocess.run([FF, "-v", "info", "-i", path, "-af", "astats=measure_overall=Peak_level:measure_perchannel=none",
                          "-f", "null", "-"], capture_output=True, text=True).stderr
    for line in out.splitlines():
        if "Peak level dB" in line:
            return float(line.split(":")[-1])
    return None


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    export, expect_path = sys.argv[1], sys.argv[2]
    expect = json.load(open(expect_path, encoding="utf-8"))
    ok = True
    info = probe(export)
    dur = float(info["format"]["duration"])
    kinds = [s["codec_type"] for s in info["streams"]]
    print(f"export: duración {dur:.3f} s (esperada {expect['duration']}), streams {kinds}")
    if abs(dur - expect["duration"]) * 1000 > expect.get("tolerance_ms", 80):
        print("  FALLO duración")
        ok = False
    if kinds.count("video") != 1 or kinds.count("audio") != 1:
        print("  FALLO streams")
        ok = False
    v = next(s for s in info["streams"] if s["codec_type"] == "video")
    print(f"  video {v['codec_name']} {v['width']}x{v['height']} {v.get('r_frame_rate')} nb_frames={v.get('nb_frames')}")
    peak = audio_peak(export)
    print(f"  audio pico {peak} dBFS")
    if peak is None or peak < -60:
        print("  FALLO audio silencioso")
        ok = False
    for c in expect["checks"]:
        got = frame(export, c["t"])
        exp = frame(expect["source"], c["source_t"])
        wrong = frame(expect["source"], c["wrong_t"])
        p_ok, p_bad = psnr(got, exp), psnr(got, wrong)
        verdict = "OK" if (p_ok >= 28 and p_ok > p_bad + 3) else "FALLO"
        if verdict == "FALLO":
            ok = False
        print(f"  t={c['t']}s → fuente {c['source_t']}s: PSNR {p_ok:.1f} dB (control {c['wrong_t']}s: {p_bad:.1f} dB) {verdict}")
    for c in expect.get("viewer_checks", []):
        # el visor guardó su fotograma compuesto (RGBA); se compara en gris a 320x180 con el export
        vp = subprocess.run([FF, "-v", "error", "-i", c["viewer"], "-frames:v", "1", "-vf", f"scale={W}:{H}",
                             "-f", "rawvideo", "-pix_fmt", "gray", "-"], capture_output=True, check=True).stdout
        got = frame(export, c["t"])
        p = psnr(got, vp)
        verdict = "OK" if p >= c.get("min_psnr", 26) else "FALLO"
        if verdict == "FALLO":
            ok = False
        print(f"  visor vs export t={c['t']}s: PSNR {p:.1f} dB (mín {c.get('min_psnr', 26)}) {verdict}")
    for c in expect.get("black_checks", []):
        got = frame(export, c["t"])
        mean = sum(got) / len(got)
        verdict = "OK" if mean <= c.get("max_mean", 2) else "FALLO"
        ok = ok and verdict == "OK"
        print(f"  hueco negro t={c['t']}s: media {mean:.3f} {verdict}")
    for c in expect.get("gap_checks", []):
        x0, y0, x1, y1 = [int(v * (W if i % 2 == 0 else H)) for i, v in enumerate(c["pip_region"])]
        got = frame(export, c["t"])
        base = frame(expect["source"], c["t"])
        def region(buf):
            return bytes(buf[y * W + x] for y in range(y0, y1) for x in range(x0, x1))
        p = psnr(region(got), region(base))
        pip_present = p < 22
        verdict = "OK" if pip_present == c["expect_pip"] else "FALLO"
        if verdict == "FALLO":
            ok = False
        print(f"  región PiP t={c['t']}s: PSNR vs fuente base {p:.1f} dB → PiP {'presente' if pip_present else 'ausente'} (esperado {'presente' if c['expect_pip'] else 'ausente'}) {verdict}")
    for c in expect.get("overlay_checks", []):
        rgb = subprocess.run([FF, "-v", "error", "-ss", f"{c['t']:.6f}", "-i", export, "-frames:v", "1",
                              "-vf", f"scale={W}:{H}", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                             capture_output=True, check=True).stdout
        x, y = int(c["x"] * W), int(c["y"] * H)
        i = (y * W + x) * 3
        r, g, b = rgb[i], rgb[i + 1], rgb[i + 2]
        base = frame(expect["source"], c["source_t"])
        p_src = psnr(frame(export, c["t"]), base)
        verdict = "OK" if (r >= c.get("min_r", 100) and p_src < 25) else "FALLO"
        if verdict == "FALLO":
            ok = False
        print(f"  overlay t={c['t']}s px({x},{y})=({r},{g},{b}) PSNR vs fuente sin overlay {p_src:.1f} dB {verdict}")
    print("RESULTADO:", "OK" if ok else "FALLO")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
