"""Genera las fixtures sintéticas que faltan en esta carpeta (no regenera las existentes:
`fixture-a.mp4` fija la identidad usada por los goldens V1 en `tests/fixtures/v1`).

Uso: python tests/fixtures/media/make_fixtures.py [--force nombre ...]

- fixture-sync.mp4: 12 s testsrc2 640x360 30 fps con contador de fotograma; el audio (48 kHz) es
  silencio con una ráfaga de 1 kHz de 60 ms al inicio de cada segundo entero y el video muestra un
  recuadro blanco en esos mismos 60 ms. Sirve para medir la sincronía A/V (export y `atempo`).
- fixture-rot90.mp4: 4 s con metadata de rotación 90° (display matrix) para probar `display_size`
  y la rotación automática al decodificar.
- fixture-vfr.mp4: 6 s con fotogramas irregulares (select + fps_mode vfr): `r_frame_rate` y
  `avg_frame_rate` difieren, así que el probe debe marcar VFR.
- fixture-espacios/ñ medios prueba/fixture-ñ.mp4: copia de fixture-b en una ruta con espacios y
  caracteres no ASCII (MED-01).
"""
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
FF = os.path.join(ROOT, "packaging", "third-party", "ffmpeg", "ffmpeg.exe")
FONT = "C\\:/Windows/Fonts/arial.ttf"


def run(args):
    print("  ffmpeg", " ".join(a if " " not in a else repr(a) for a in args))
    subprocess.run([FF, "-hide_banner", "-loglevel", "error", "-y", *args], check=True)


def make_sync(path):
    vf = (
        f"drawtext=fontfile='{FONT}':text='S %{{n}}':x=20:y=20:fontsize=48:fontcolor=white:box=1:boxcolor=black@0.7,"
        f"drawtext=fontfile='{FONT}':text='%{{pts\\:hms}}':x=20:y=300:fontsize=36:fontcolor=yellow:box=1:boxcolor=black@0.7,"
        "drawbox=x=400:y=200:w=200:h=120:color=white:t=fill:enable='lt(mod(t\\,1)\\,0.06)'"
    )
    run([
        "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30:duration=12",
        "-f", "lavfi", "-i", "aevalsrc=if(lt(mod(t\\,1)\\,0.06)\\,0.8*sin(2*PI*1000*t)\\,0):s=48000:d=12",
        "-vf", vf, "-c:v", "libx264", "-preset", "fast", "-crf", "18", "-g", "60", "-pix_fmt", "yuv420p",
        "-c:a", "pcm_s16le", "-shortest", path,
    ])


def make_rot90(path):
    vf = f"drawtext=fontfile='{FONT}':text='R %{{n}}':x=20:y=20:fontsize=48:fontcolor=white:box=1:boxcolor=black@0.7"
    tmp = path + ".plain.mp4"
    run([
        "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30:duration=4",
        "-f", "lavfi", "-i", "sine=frequency=330:sample_rate=48000:duration=4",
        "-vf", vf, "-c:v", "libx264", "-preset", "fast", "-crf", "20", "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-b:a", "96k", "-shortest", tmp,
    ])
    # FFmpeg 8 ya no convierte la etiqueta `rotate` en display matrix al codificar; la forma
    # verificada es fijar la rotación en la entrada y copiar los streams (la side data se conserva).
    run(["-display_rotation", "90", "-i", tmp, "-c", "copy", "-movflags", "+faststart", path])
    os.remove(tmp)


def make_vfr(path):
    vf = (
        f"drawtext=fontfile='{FONT}':text='V %{{n}}':x=20:y=20:fontsize=48:fontcolor=white:box=1:boxcolor=black@0.7,"
        "select='not(mod(n\\,3))+not(mod(n\\,7))',setpts=N/(30*TB)*0+PTS"
    )
    run([
        "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30:duration=6",
        "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=6",
        "-vf", vf, "-fps_mode", "vfr", "-c:v", "libx264", "-preset", "fast", "-crf", "20", "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-b:a", "96k", "-shortest", path,
    ])


def make_start5(path):
    run(["-i", os.path.join(HERE, "fixture-sync.mp4"), "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
         "-output_ts_offset", "5", "-muxdelay", "0", path])


def make_audio_delay(path):
    source = os.path.join(HERE, "fixture-sync.mp4")
    run(["-i", source, "-itsoffset", "0.25", "-i", source, "-map", "0:v:0", "-map", "1:a:0", "-c", "copy", path])


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    force = set(sys.argv[2:]) if len(sys.argv) > 2 and sys.argv[1] == "--force" else set()
    targets = {
        "fixture-sync.mp4": make_sync,
        "fixture-rot90.mp4": make_rot90,
        "fixture-vfr.mp4": make_vfr,
        "fixture-start5.ts": make_start5,
        "fixture-audio-delay.mp4": make_audio_delay,
    }
    for name, fn in targets.items():
        path = os.path.join(HERE, name)
        if os.path.exists(path) and name not in force:
            print(f"{name}: ya existe")
            continue
        print(f"{name}: generando")
        fn(path)
    uni_dir = os.path.join(HERE, "fixture-espacios", "ñ medios prueba")
    uni = os.path.join(uni_dir, "fixture-ñ.mp4")
    if not os.path.exists(uni):
        os.makedirs(uni_dir, exist_ok=True)
        shutil.copyfile(os.path.join(HERE, "fixture-b.mp4"), uni)
        print(f"{uni}: copiado")
    else:
        print(f"{uni}: ya existe")


if __name__ == "__main__":
    main()
