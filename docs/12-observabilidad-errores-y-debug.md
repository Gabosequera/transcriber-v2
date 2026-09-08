# 12 — Observabilidad, errores y debug

Usar `tracing` con spans por `project_id`, `job_id`, `worker_id`, `command_id`, `actor` y `correlation_id`. Eventos tipados: `CommandParsed`, `CommandRejected`, `RevisionConflict`, `WorkerHeartbeat`, `FrameDropped`, `SeekCompleted`, `ExportStarted`, `ExportFinished`, `PluginMissing`, `ModelOom`.

Errores estables (`TRN-IO-*`, `TRN-MEDIA-*`, `TRN-TIMELINE-*`, `TRN-AI-*`, `TRN-WORKER-*`, `TRN-LICENSE-*`) separan mensaje localizable, detalle técnico, causa, retryability y remediation. FFmpeg stderr se parsea pero se conserva truncado/redactado; panic hook genera crash report sin secretos. Logs rotativos y JSONL opcional; diagnóstico exportable incluye versions, capabilities, plugins y métricas, no media ni tokens.

La consola de usuario consume eventos de progreso, no strings hardcodeados de cada UI. Telemetría es opt-in y no se envía por defecto.
