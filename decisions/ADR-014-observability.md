# ADR-014 — tracing y crash reporting

- Estado: provisional | Fecha: 2026-09-07

Adoptar `tracing` para spans/events estructurados, correlacionados por project/job/worker/command; errores con códigos estables y redacción antes de logs/crash. Telemetría opt-in. Prueba: panic, stderr FFmpeg, secrets, disk full y diagnóstico exportable.
