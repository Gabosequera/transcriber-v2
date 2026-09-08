# Deep dive de referencias

## Gausian

La separación `timeline/project/media-io/renderer/native-decoder/exporters/jobs` es un buen mapa de boundaries. `timeline::commands` devuelve comandos inversos; `project` contiene migraciones y tablas de assets/proxies/jobs/transcripts; `renderer` tiene WGSL y CPU fallback; `native-decoder` selecciona plugins por OS y mantiene ring/preroll. Advertencias: README promete capacidades que el CLI aún etiqueta demo y el license file contradice el README.

## OpenCut

Es un prototipo GPUI con `timeline_document`, `timeline_clip`, `timeline_interactions`, `preview_timeline`, `clip_render_plan` y `export_gstreamer`. El README confirma JSON por carpeta, media in-place, multitrack, snapping, waveform multiresolution y undo de operaciones múltiples. Declara explícitamente “experimental/prototype”; no usar su código sin licencia confirmada.

## Cutlass

El split `cutlass-models`, `cutlass-commands`, `cutlass-engine`, `cutlass-render`, `cutlass-compositor` y `cutlass-ai` encaja con V2. README documenta dry-run por defecto, comandos compartidos UI/AI, undo de un paso, keyframes y GPU preview. README también declara Linux sin backend media implementado; no usarlo como prueba de soporte Linux.

## Kerf

`kerf-core` modela timeline, revisiones, staged agent edits, tasks y diff; los tests prueban propuesta stale cuando el usuario edita por debajo y apply/discard. Es evidencia excelente para staging, pero `frontend` es Svelte/Tauri/WebView y `LICENSE.md` es PolyForm Noncommercial. Solo port conceptual.

## GStreamer/GES

El monorepo/bindings contiene el crate GES y ejemplos. La documentación oficial confirma Timeline/Tracks/Layers/Clips, `GESPipeline` preview/render, commit explícito y API no thread-safe. GES owner thread es obligatorio.

## MLT/Kdenlive/Shotcut

MLT aporta producer/tractor/filter/consumer; Kdenlive `timeline2/model` aporta clip/track/groups/snap/preview/render server; Shotcut `TimelineDock` y `timelinecommands` exponen acciones de selección, ripple, split, marker, zoom y undo. Son excelentes especificaciones de comportamiento, pero sus repositorios son GPL.
