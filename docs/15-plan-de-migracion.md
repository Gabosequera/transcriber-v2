# 15 — Plan de migración por fases

| Fase | Alcance | Puerta de aceptación |
|---|---|---|
| 0 | congelar schemas, golden fixtures, corpus, benchmarks V1, compatibilidad | 100% fixtures importan/exportan; digest/revision reproducibles; baseline medido |
| 1 | ventana nativa, importar video, frame, audio sync, una pista, playhead, zoom/scrub/marker, abrir master | Windows 10/11 + Linux; primer frame y sync dentro del gate; sin modelo en UI |
| 2 | tracks/clips/imagen/waveform/layers/topics/trims/undo/persistencia | round-trip JSON; 1000 clips/items navegables; stale y undo probados |
| 3 | supervisor Python, Whisper/MMS/risa/arousal/progreso/cancel/resume | crash/cancel/oom no pierde checkpoints; worker zombie cero |
| 4 | AI command bus, proposals, diff, preview, apply/reject, auditoría, MCP adapter | respuestas stale rechazadas; idempotencia; risk tier y aprobación humana |
| 5 | compositor GPU, proxies, HW decode, export, FCPXML/EDL y optimización | preview/export comparables; codec corpus; fallback software |
| 6 | import V1, updater, instalador, retiro gradual CustomTkinter | corpus real compatible, rollback, diagnósticos, instalación limpia en Win/Linux |

No hay big bang: cada fase puede convivir con V1 y publicar artefactos portables. El primer vertical slice debe usar una sola fuente real corta y un master existente, no un demo sintético solamente.
