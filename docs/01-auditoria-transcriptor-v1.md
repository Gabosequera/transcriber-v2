# 01 — Auditoría de Transcriptor V1

## Estado observado

- Git al inicio: `c3677ba568cfc7d5947ec4695c42fffef6d79e03`; `git status --short` vacío; fecha `2026-09-07T15:21:28-04:00`.
- Segunda auditoría: misma revisión, rama `main`, `git status --short` vacío, fecha `2026-09-07T16:07:05-04:00`; 96.116 archivos físicos (~29,9 GB) frente a 112 archivos Git.
- El árbol Git contiene 112 archivos; el conteo de módulos Python de producto es aproximadamente 28,6k líneas. No se contaron caches/runtimes/modelos como producto.
- La GUI está en `app.py`, `automatico_ui.py`, `editor_medios.py` y `editorial_layers_ui.py`; usa Python 3.13/CustomTkinter según `README.md`, instalación y código de imports.
- El pipeline se divide entre `editorial_pipeline.py`, `pipeline.py`, `core.py`, `align.py`, `laughter.py`, `prosodia.py`, `vision.py`, `cara.py`, `describir.py`, `escena_audio.py`, `medios.py` y `podcast_export.py`.
- El repositorio separa `releases`, `runtimes`, `shared`, modelos/caches y configuración. `hardware.py` antepone herramientas y evita importar torch/CTranslate2 en el arranque.

## Contratos y garantías confirmados

| Área | Evidencia V1 | Garantía a conservar |
|---|---|---|
| identidad | `medios.py`: size + hash muestreado + inventario ffprobe | identidad por contenido, no path |
| tiempo | `medios.py`, `playback_clock.py`, `editorial_io.py` | reloj canónico T0, offsets explícitos, tiempos finitos |
| atomicidad | `editorial_io.atomic_write_text/json`, `editorial_projects.py` | temp + fsync + replace, cleanup en error |
| revisión | `editorial_layers.py`, `marcas.py`, `editorial_history.py` | revision monotónica, conflicto externo, stale explícito |
| AI | `editorial_trims.py`, `editorial_topics.py`, `editorial_montaje.py` | request id, master/layers digest, pass, validación y merge protegido |
| capas | `editorial_layers.py` | items multirrango, estados proposed/accepted/disabled, parent_id, tombstones |
| derivación | `editorial_projects.py` | padre/hijo, mapping source→child, baselines y no inferencia repetida |
| export | `podcast_export.py`, `editorial_montaje.py` | originales sin modificar, scripts por archivo, EDL/FCPXML, registro |
| reanudación | `editorial_pipeline.py`, manifests de pasos | idempotencia, checkpoint por etapa, `skipped`, adopción de manifests legacy |

## Diferencias respecto al contexto recibido

No se encontró evidencia en este corte de una suite de 129 pruebas ejecutada; sí hay una colección amplia en `tests/` y el historial/documentación registra conteos variables por fase. Por eso el número 129 queda como dato histórico pendiente de reproducir, no como hecho actual. El árbol está limpio antes y después; el conteo de líneas depende de si se incluyen UI/test y debe congelarse con una herramienta reproducible en Fase 0.

## Riesgos V1 relevantes para V2

Imports nativos dentro de la UI, conflicto de runtimes CUDA/cuDNN/OpenMP, subprocess sin aislamiento universal, múltiples formatos ad hoc y acoplamiento de UI a sidecars. V2 debe usar procesos separados para modelos y un núcleo de dominio sin imports de ML.
