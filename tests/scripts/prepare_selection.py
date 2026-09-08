"""Genera evidencia nueva de selección, repeticiones y cachés sin pisar la anterior."""
import json
from datetime import datetime, timezone
from pathlib import Path
from prepare_script import expand_template

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'implementation/evidence/e2' / ('selection-' + datetime.now(timezone.utc).strftime('%Y%m%d-%H%M%S-%f'))
OUT.mkdir()
def write(name, data):
    path = OUT / name
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding='utf-8')
    return str(path)

source = str(ROOT / 'tests/fixtures/media/fixture-a.mp4')
steps = [
    {'op':'viewport','width':1280,'height':720,'zoom':1},
    {'op':'import','path':source}, {'op':'insert','asset':0,'at':0},
    {'op':'insert','asset':0,'at':15}, {'op':'assert','clips':4},
    {'op':'select_clips','indices':[0,1,2,3]},
    {'op':'export','selection':'clips','preset':'h264-720p','dest':str(OUT/'clips.mp4')},
    {'op':'wait_export'}, {'op':'view','mode':'source'},
    {'op':'new_layer','name':'Tramo repetido'}, {'op':'set_in','t':1}, {'op':'set_out','t':2},
    {'op':'add_range'}, {'op':'assert','layers':1,'items':1},
    {'op':'view','mode':'sequence'}, {'op':'select_item','layer':0,'index':0},
    {'op':'export','selection':'items','preset':'h264-720p','dest':str(OUT/'items.mp4')},
    {'op':'wait_export'}, {'op':'action','id':'montage.export'},
    {'op':'wait','ms':5500},
    {'op':'screenshot','path':str(OUT/'export-selection.png')},
    {'op':'save','path':str(OUT/'selection-demo.transcriptor')}, {'op':'quit'}]
write('selection.json', steps)
write('clips-expect.json', {'source':source,'duration':24,'checks':[
    {'t':0.5,'source_t':0.5,'wrong_t':6.5}, {'t':11.9,'source_t':11.9,'wrong_t':3.9},
    {'t':12.5,'source_t':0.5,'wrong_t':6.5}, {'t':23.9,'source_t':11.9,'wrong_t':3.9}]})
write('items-expect.json', {'source':source,'duration':2,'checks':[
    {'t':0,'source_t':1,'wrong_t':6}, {'t':0.9,'source_t':1.9,'wrong_t':6.9},
    {'t':1,'source_t':1,'wrong_t':6}, {'t':1.9,'source_t':1.9,'wrong_t':6.9}]})
caches = (ROOT/'tests/scripts/e2-caches.json').read_text(encoding='utf-8')
write('caches.json', expand_template(json.loads(caches), OUT))
(ROOT/'implementation/evidence/e2/latest-selection.txt').write_text(str(OUT), encoding='utf-8')
print(OUT)
