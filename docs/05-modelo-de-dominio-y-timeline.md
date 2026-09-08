# 05 — Modelo de dominio y timeline

## Coordenadas

La autoridad temporal es `T0`, expresada internamente en `Time = rational(frame, timebase)` cuando existe una tasa de frames estable; los límites de audio/eventos conservan nanosegundos o microsegundos. No usar `f64` como identidad. Cada source tiene `source_range`; cada clip tiene `timeline_range`; el mapping es explícito y soporta offsets OBS, VFR y gaps.

```text
Project
 ├─ MediaAsset(fingerprint, streams, t0, duration)
 ├─ MediaTrack(video/audio, order, mute/solo/lock)
 │   └─ Clip(source_range, timeline_range, transform, gain, opacity, keyframes)
 ├─ SemanticLayer(kind, revision, items[])
 │   └─ Item(parent_id, state, origin, ranges[], evidence, source_ids)
 └─ RevisionGraph / Commands / ExportRecords
```

## Media tracks vs capas editoriales

Comparten `TemporalRange`, selección, snapping, cursor y renderer de overlays. No comparten persistencia ni reglas:

| Dimensión | Track multimedia | Capa editorial/semántica |
|---|---|---|
| autoridad | `ResolvedTimeline` | evidencia/interpretación del master |
| solape | resuelto por composición y prioridad | permitido; multi-rango y jerarquía |
| mutación | comandos de clip/track | comandos de item/layer/propuesta |
| estado | visible/mute/solo/lock | proposed/accepted/disabled/deleted |
| identidad | clip/source/track UUID | layer/item UUID estable, source IDs |
| export | afecta audio/video | solo afecta si un comando lo materializa |

## Operaciones

`InsertClip`, `MoveClip`, `TrimStart`, `TrimEnd`, `SplitClip`, `SetTransform`, `SetGain`, `SetTrackState`, `AddMarker`, `AddSemanticItem`, `UpdateRanges`, `AcceptProposal`, `DisableItem`, `DeleteWithTombstone`. Cada operación produce inverse command, diff y nueva revisión.

Snapping se calcula contra edges de clips, markers, palabras, risas y playhead con tolerancia dependiente de zoom; nunca altera silenciosamente el rango pedido: registra `requested` y `resolved`.

## Evidencia V1

`editorial_io.py` valida tiempos finitos/rangos; `editorial_layers.py` valida jerarquía máxima 32 y descendencia dentro del padre; `editorial_projects.py` remapea rangos al hijo; `podcast_export.py` conserva offsets y registra desplazamientos de keyframe copy. Estas invariantes son golden fixtures de Fase 0.
