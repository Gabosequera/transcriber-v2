# 24 — Backlog de prototipos técnicos

| ID | Pregunta/hipótesis | Mínimo/dataset | Métricas y éxito |
|---|---|---|---|
| P1 | egui mantiene 60/120Hz con 10k–100k items | canvas virtualizado + clips sintéticos | frame P95 ≤16.7/8.3ms, keyboard latency; descartar si no hay LOD viable |
| P2 | preview GStreamer puede alimentar superficie WGPU | H.264/HEVC en Windows/Linux | first frame, copies, drops, GPU/CPU; éxito sin bloqueo UI |
| P3 | seek VFR/B-frames es aceptable | 3h VOD + GOP variable | P50/P95/P99, drift; éxito por codec gate |
| P4 | preview/export coinciden | mismos clips, FFmpeg/GStreamer | frame diff, color, A/V drift; éxito dentro tolerancia |
| P5 | GStreamer bundle es distribuible en Windows | clean VM + plugin matrix | tamaño, startup, registry, missing plugin; éxito instalación limpia |
| P6 | IPC soporta cancel/progreso/recovery | worker fake + payload grande por archivo | latency, backpressure, zombie, resume; NDJSON solo control |
| P7 | importer preserva V1 | fixtures V1 | round-trip IDs/digests/ranges; cero pérdida |
| P8 | reconcile SQLite/JSON es comprensible | cambios concurrentes/crash | conflictos detectados, diff determinista |
| P9 | commands serializables dan undo seguro | multi-doc edits | inverse, stale, idempotencia; cero overwrite |
| P10 | AI dry-run/diff/apply es seguro | respuestas válidas/viejas/maliciosas | rechazo correcto y aprobación por riesgo |
| P11 | scheduler responde a presión RAM/VRAM | GPU 8GB, CPU-only, memoria limitada | OOM recovery, playback priority, peak RAM/VRAM |
| P12 | crash recovery conserva checkpoints | kill en cada etapa | tiempo de recovery y no repetición de Whisper |
| P13 | egui cubre IME/accessibility/DPI | texto CJK, screen reader, 200% DPI | árbol AccessKit, edición, focus, resize |
| P14 | decode→GPU evita copias útiles | NV12/P010/HDR por GPU | copies/frame, latency, power; no prometer zero-copy si falla |

Prioridad: P7, P6, P2, P4, P5, P13 antes de comprometer stack; P1/P3/P9/P10 en el vertical slice; P11/P14 antes de optimización de producción.
