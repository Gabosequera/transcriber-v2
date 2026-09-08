"""Experimento acotado: identificar por píxeles el PTS nativo elegido por -ss/fps."""
import json, subprocess
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from support import media_tool

ROOT=Path(__file__).resolve().parents[2]
FF=media_tool("ffmpeg")
FP=media_tool("ffprobe")
source=str(ROOT/'tests/fixtures/media/fixture-vfr.mp4')
frames=json.loads(subprocess.check_output([FP,'-v','error','-select_streams','v:0','-show_frames','-show_entries','frame=best_effort_timestamp_time','-of','json',source]))['frames']
pts=[float(f['best_effort_timestamp_time']) for f in frames]
raw=subprocess.check_output([FF,'-v','error','-i',source,'-an','-vf','scale=64:36,format=gray','-fps_mode','passthrough','-f','rawvideo','-'])
n=64*36
native=[raw[i:i+n] for i in range(0,len(raw),n)]
assert len(native)==len(pts)
for t in [.05,.15,1.05,1.11,2.233333,4.15]:
    expected=max(i for i,p in enumerate(pts) if p<=t+1e-6)
    for mode in [[],['-noaccurate_seek']]:
        got=subprocess.check_output([FF,'-v','error',*mode,'-ss',str(t),'-i',source,'-an','-vf','fps=fps=30/1:start_time=0:round=near,scale=64:36,format=gray','-frames:v','1','-f','rawvideo','-'])
        scores=[sum(abs(a-b) for a,b in zip(got,f)) for f in native]
        best=min(range(len(scores)),key=scores.__getitem__)
        print(json.dumps({'seek':t,'noaccurate':bool(mode),'expected_previous_pts':pts[expected],'actual_pts':pts[best],'error':scores[best]}))
