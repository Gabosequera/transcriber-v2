# 18 — Matriz conservar/refactorizar/migrar

| Componente | Responsabilidad actual | Calidad | Acoplamiento | Rendimiento | Acción recomendada | Destino |
|---|---|---:|---:|---:|---|---|
| `app.py` | entrypoint, Manual, settings, UI | media | alto | UI/imports | mantener temporalmente; extraer servicios | Rust GUI + legacy temporal |
| `automatico_ui.py` | workspace pipeline/editorial | media | muy alto | UI | refactorizar por view-model; no portar widgets | Rust GUI |
| `editor_medios.py` | playback, timeline, gestures, FFplay | media | muy alto | interacción/seek | encapsular clock/media; reescribir superficie | Rust GUI/media |
| `editorial_layers.py` | schema, validation, store, adapters, merge | alta | medio | bajo | conservar contrato; reimplementar validator tipado | Rust core/persistencia |
| `editorial_layers_ui.py` | gestos/inspector/admin capas | media | alto | UI | migrar comportamiento, no estructura | Rust GUI |
| `editorial_trims.py` | heurística, proposal, merge, filter plan | alta | medio | CPU/IO | conservar algoritmo/schema; separar planner de export | Rust core + FFmpeg process |
| `editorial_pipeline.py` | plan, manifests, workers, resume | alta pero grande | alto | orchestration | conservar semántica; supervisor Rust | Rust supervisor + Python worker |
| `pipeline.py` | multimodal legacy | variable | alto | inference | encapsular y retirar gradualmente | Python compatibility worker |
| `podcast_export.py` | filter_complex, formats, export records | alta | medio | FFmpeg-bound | conservar contrato; adapter process | FFmpeg process |
| `core.py/align.py/prosodia/laughter` | inference/acoustic signals | funcional | bajo-medio | modelo nativo | no portar ahora | Python inference worker |
| `vision.py/cara/describir/escena_audio` | multimodal/VLM | variable/online | medio | GPU/network | encapsular capacidades/secrets | Python inference worker |
| `editorial_master/chunks/topics/montaje/projects` | dominio editorial y derivación | alta | medio | JSON/CPU | portar invariantes y golden fixtures | Rust core |
| `editorial_history.py` | undo/redo revision-aware | alta | bajo | bajo | reimplementar serializable | Rust core |
| `medios.py/playback_clock.py` | probe/fingerprint/clock/subprocess | media-alta | alto | IO/seek | conservar contratos; nueva media service | Rust media + FFmpeg/GStreamer |
| `hardware.py` | detección/config CPU/GPU | media | medio | bajo | rediseñar scheduler; conservar heurísticas | Rust scheduler |
| `updater.py/launcher.py/app_paths.py` | instalación/rutas/release | alta | medio | startup | mantener temporalmente, luego portar | packaging/update |
| `tests/` | unit/integration/smoke | alta | medio | n/a | conservar como golden/spec; añadir cross-runtime | herramienta de validación |
| `requirements*.txt`, runtimes/models | distribución ML | necesaria | alta | carga/VRAM | no portar; aislar y versionar | Python runtime |

No se recomienda reescribir un archivo grande como unidad. La unidad de migración es responsabilidad + contrato + fixture.
