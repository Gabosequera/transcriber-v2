"""Genera una fixture V1 auténtica ejecutando los módulos puros de V1 en una copia aislada.

- Copia los .py de V1 a una carpeta temporal (nunca escribe en el checkout V1);
  PYTHONDONTWRITEBYTECODE=1; config/estado quedan en la copia.
- Usa `medios.fingerprint` (ffprobe del FFmpeg vendorizado en V2) sobre
  tests/fixtures/media/fixture-a.mp4, y construye master, capa, recortes y montaje
  con `editorial_layers`, `editorial_trims` y `editorial_montaje` de V1.
- Salida: tests/fixtures/v1/demo-a/editorial/ y fingerprint-v1.json (para
  comparar con el fingerprint calculado por V2).

Uso: python make_v1_fixture.py [ruta-V1]
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from support import media_tool

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
V1 = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(os.environ.get("TRANSCRIPTOR_V1_DIR", ROOT.parent / "transcriber"))
MEDIA = ROOT / "tests" / "fixtures" / "media" / "fixture-a.mp4"
OUT = HERE / "demo-a"
FFDIR = Path(media_tool("ffmpeg")).parent

GENERATOR = r'''
import json, os, sys
import medios, editorial_layers, editorial_trims, editorial_montaje, editorial_io, editorial_chunks

media = Path(sys.argv[1]); out = Path(sys.argv[2])
info = medios.inspeccionar(media)
fp = medios.fingerprint(media, info)
(out / "editorial" / "layers").mkdir(parents=True, exist_ok=True)
(out / "editorial" / "views").mkdir(parents=True, exist_ok=True)

def word(track, start, end, text):
    return {"word_id": f"{track}-w-{int(start * 100):06d}", "track_id": track, "t_ini": start, "t_fin": end, "text": text}

words_a = [word("A", 1.0, 1.4, "hola"), word("A", 1.5, 1.9, "mundo"), word("A", 5.0, 5.4, "bien"), word("A", 10.0, 10.5, "fin")]
words_b = [word("B", 3.0, 3.3, "sí")]
utterances = [
    {"utterance_id": "A-u-000001", "track_id": "A", "t_ini": 1.0, "t_fin": 1.9, "text": "hola mundo", "word_ids": [w["word_id"] for w in words_a[:2]], "signals": {}},
    {"utterance_id": "B-u-000001", "track_id": "B", "t_ini": 3.0, "t_fin": 3.3, "text": "sí", "word_ids": [words_b[0]["word_id"]], "signals": {}},
    {"utterance_id": "A-u-000002", "track_id": "A", "t_ini": 5.0, "t_fin": 5.4, "text": "bien", "word_ids": [words_a[2]["word_id"]], "signals": {}},
    {"utterance_id": "A-u-000003", "track_id": "A", "t_ini": 10.0, "t_fin": 10.5, "text": "fin", "word_ids": [words_a[3]["word_id"]], "signals": {}},
]
master = {
    "schema": editorial_io.SCHEMA_MASTER, "generated_at": "2026-09-07T00:00:00+00:00",
    "project": {"name": "demo-a", "profile": "editorial_voz"},
    "media": {"path": str(media), "duration": info["duracion"], "t0": info["t0"], "fingerprint": fp},
    "transcription": {"model": "fixture"},
    "tracks": {
        "A": {"track_id": "A", "label": "Gabriel", "words": words_a, "utterances": [u for u in utterances if u["track_id"] == "A"], "laughter": [], "arousal": []},
        "B": {"track_id": "B", "label": "Amigo", "words": words_b, "utterances": [u for u in utterances if u["track_id"] == "B"],
              "laughter": [{"event_id": "B-laugh-00001", "track_id": "B", "t_ini": 6.0, "t_fin": 7.0, "conf": 0.9, "max_conf": 0.9}], "arousal": []},
    },
    "conversation": {"utterances": utterances, "clean_utterance_ids": [u["utterance_id"] for u in utterances], "overlap_groups": [], "duplicate_groups": []},
    "chunks": [],
}
editorial_io.atomic_write_json(out / "editorial" / "demo-a.editorial.master.json", master)

store = editorial_layers.LayerStore(out / "editorial", master)
layer = editorial_layers.new_layer(master, "Temas", kind="topics", layer_id="layer-demo-topics")
topic = editorial_layers.new_item(0.5, 4.0, "Saludo", "Apertura del episodio"); topic["item_id"] = "item-tema-1"; topic["state"] = "accepted"
topic["ranges"].append({"t_ini": 9.0, "t_fin": 11.0})
sub = editorial_layers.new_item(1.0, 2.0, "Presentación"); sub["item_id"] = "item-sub-1"; sub["parent_id"] = topic["item_id"]; sub["edited"] = False
gone = editorial_layers.new_item(6.0, 6.5, "Borrado"); gone["item_id"] = "item-borrado"
layer["items"] = [topic, sub]
layer["deleted_item_ids"] = [gone["item_id"]]
store.save(layer)
manual = editorial_layers.new_layer(master, "Notas manuales", layer_id="layer-demo-notas")
punto = editorial_layers.new_item(7.25, 7.5, "Breve", "tramo corto"); punto["item_id"] = "item-breve"
apagado = editorial_layers.new_item(8.0, 9.0, "Descartado"); apagado["item_id"] = "item-off"; apagado["state"] = "disabled"
manual["items"] = [punto, apagado]
store.save(manual)

trims = editorial_trims.new_document(fp, info["duracion"])
editorial_trims.add_cut(trims, 2.0, 2.5, origin="user", reason="muletilla", accepted=True)
editorial_trims.add_cut(trims, 4.0, 4.8, origin="ai", reason="tangente")
c3 = editorial_trims.add_cut(trims, 8.5, 9.0, origin="silence", reason="silencio")
c3["enabled"] = False
editorial_trims.save_document(out / "editorial" / "views" / "trims.json", trims)

doc = editorial_montaje.new_document(fp, info["duracion"])
doc, c1 = editorial_montaje.add_clip(doc, 1.0, 5.0, label="Saludo")
doc, c2 = editorial_montaje.add_clip(doc, 9.0, 11.0, label="Cierre")
doc, c3 = editorial_montaje.add_clip(doc, 6.0, 7.0, at=2.0, track="V2", label="Risa encima")
doc, c4 = editorial_montaje.add_clip(doc, 1.0, 2.0, label="Repetido")
doc, _ = editorial_montaje.set_state(doc, [c2["clip_id"]], "accepted")
editorial_montaje.save_document(out / "editorial" / "views" / "montaje.json", doc)

flat = editorial_montaje.flatten(doc)
json.dump({"fingerprint": fp, "info": info, "source_master_digest": editorial_chunks.source_master_digest(master),
           "trims_enabled_intervals": editorial_trims.enabled_intervals(trims), "montaje_flatten": flat,
           "montaje_total_seconds": editorial_montaje.total_seconds(doc),
           "source_to_seq_1_5": editorial_montaje.source_to_seq(doc, 1.5)},
          open(out / "expected-v1.json", "w", encoding="utf-8"), indent=1, ensure_ascii=False)
print("ok", len(flat), "tramos")
'''


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if not (V1 / "editorial_layers.py").is_file():
        sys.exit(f"V1 no encontrado en {V1}")
    with tempfile.TemporaryDirectory(prefix="v1iso-") as tmp:
        iso = Path(tmp)
        for py in V1.glob("*.py"):
            shutil.copy(py, iso / py.name)
        (iso / "gen.py").write_text(GENERATOR, encoding="utf-8")
        if OUT.exists():
            shutil.rmtree(OUT)
        OUT.mkdir(parents=True)
        env = dict(os.environ)
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        env["PATH"] = str(FFDIR) + os.pathsep + env.get("PATH", "")
        env["XDG_CACHE_HOME"] = str(iso / "cache")
        r = subprocess.run([sys.executable, "gen.py", str(MEDIA), str(OUT)], cwd=iso, env=env, capture_output=True, text=True, encoding="utf-8", errors="replace")
        print(r.stdout)
        if r.returncode != 0:
            print(r.stderr)
            sys.exit(r.returncode)
        # eliminar rutas absolutas de la máquina en el master (portabilidad de la fixture)
        mp = OUT / "editorial" / "demo-a.editorial.master.json"
        m = json.loads(mp.read_text(encoding="utf-8"))
        m["media"]["path"] = "../../media/fixture-a.mp4"
        mp.write_text(json.dumps(m, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for p in sorted(OUT.rglob("*")):
        if p.is_file():
            print(p.relative_to(OUT), p.stat().st_size)


if __name__ == "__main__":
    main()
