# 16 — Plan de validación y benchmarks

## Corpus

Fixtures sintéticos: CFR/VFR, H.264/HEVC/AV1/VP9/ProRes, B-frames, mono/stereo/multipista, OBS offsets, gaps, audio-only, imágenes y 3h VOD. Corpus real anonimizados con master/layers/trims y golden exports.

## Pruebas

- Propiedad: rangos finitos, mapping source↔timeline, no solape inválido por track, parent containment, digest determinista.
- Golden: JSON V1 round-trip, migration diff, EDL/FCPXML, frame hashes y A/V drift.
- Fault injection: FFmpeg kill, GStreamer plugin missing, worker crash/OOM/timeout, JSON externo editado, stale AI, disk full, GPU unavailable.
- UI: AccessKit tree, keyboard/IME, DPI 100/150/200%, multimonitor, resize, 60/120Hz, 1000+ elementos.
- Load: múltiples jobs, bounded queues, cancelación repetida, startup sin GPU.

Cada medición registra commit de V2, commit de repositorio de referencia si aplica, hardware/driver, OS, codec, settings, warm/cold cache, p50/p95/p99 y artefactos. No prometer “GPU faster” sin comparar copia/seek/startup.
