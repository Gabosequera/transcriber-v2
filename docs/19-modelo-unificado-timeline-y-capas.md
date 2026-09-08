# 19 — Modelo unificado timeline/capas

## Modelo

`TemporalRef` conecta todos los objetos con `{asset_id, source_range, timeline_range, timebase, t0}`. `MediaClip` vive en una `MediaTrack` y afecta composición; `SemanticItem` vive en `SemanticLayer` y aporta evidencia, etiqueta o propuesta. Ambos exponen `TemporalRangeRef`, pero solo el primero entra automáticamente al render.

```text
Asset ──(source_range)──> MediaClip ──(timeline_range)──> MediaTrack ──> ResolvedTimeline
   │                                      │
   └── evidence/source_ids <── SemanticItem <── SemanticLayer
```

## Relaciones V1

`word_id`, `utterance_id`, `track_id`, `chunk_id`, `cut_id`, `item_id` y fingerprints ya conectan master, señales, capas, trims y proyectos hijos. Falta un contrato único para vincular un clip multimedia con evidencia semántica y distinguir source-time de project-time en todos los exportadores.

## Fuente y derivación

Fuente: media fingerprint/master, transcripción alineada, decisión humana y clip editado. Derivado: waveform, thumbnails, `views/*.json`, signals markdown, layer adapters y caches. Inmutable: media original, evidencia con digest y audit log. Editable humano: tracks/clips, markers, layers, trims. AI: solo propuestas y comandos permitidos. Un cambio de master/fingerprint invalida derivados dependientes; un cambio de timeline remapea o invalida items según política explícita.

El modelo no importa egui, GES ni FFmpeg; esos sistemas reciben snapshots compilados.
