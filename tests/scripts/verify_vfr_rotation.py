"""PTS/frame oracle without input seeking or fps filter; 28 dB fixed threshold."""
import json
import subprocess
import sys
from pathlib import Path
from verify_export import FF, FP, psnr, frame, probe

ROOT = Path(__file__).resolve().parents[2]

def decode(path, vf, input_args=()):
    return subprocess.check_output([FF, '-v', 'error', *input_args, '-i', str(path), '-an', '-vf', vf,
                                    '-fps_mode', 'passthrough', '-pix_fmt', 'gray', '-f', 'rawvideo', '-'])

def main():
    run = Path(sys.argv[1])
    for name in ['vfr', 'rot90']:
        source = ROOT / f'tests/fixtures/media/fixture-{name}.mp4'
        timestamps = json.loads(subprocess.check_output([FP, '-v', 'error', '-select_streams', 'v:0', '-show_frames',
                    '-show_entries', 'frame=best_effort_timestamp_time', '-of', 'json', str(source)]))['frames']
        pts = [float(f['best_effort_timestamp_time']) for f in timestamps]
        vf = 'scale=320:180'
        args = []
        if name == 'rot90':
            args = ['-noautorotate']
            vf = 'transpose=cclock,scale=101:180,pad=320:180:(ow-iw)/2:(oh-ih)/2'
        raw = decode(source, vf, args)
        size = 320 * 180
        native = [raw[i:i+size] for i in range(0, len(raw), size)]
        assert len(native) == len(pts)
        output = run / f'{name}.mp4'
        raw = decode(output, 'scale=320:180')
        exported = [raw[i:i+size] for i in range(0, len(raw), size)]
        dur = float(probe(str(source))['format']['duration'])
        assert abs(len(exported)/30-dur) <= 1/30 + 1e-6
        scores = []
        # Every output frame, including first/last and irregular native boundaries.
        for i, pixels in enumerate(exported):
            t = i / 30
            expected = max(n for n,p in enumerate(pts) if p <= t + 1e-6)
            score = psnr(pixels, native[expected])
            scores.append(score)
            assert score >= 28, (name, i, t, pts[expected], score)
        print(f'{name}: {len(exported)} frames; native PTS oracle min PSNR={min(scores):.3f} dB')
        for t in [1.1, 2.2]:
            viewer = decode(run/f'{name}-viewer-{t}.png', 'scale=320:180')
            score = psnr(frame(str(output), t), viewer)
            assert score >= 28, (name, 'viewer', t, score)
            print(f'{name}: viewer t={t} PSNR={score:.3f} dB')
    print('RESULTADO: OK')

if __name__ == '__main__':
    main()
