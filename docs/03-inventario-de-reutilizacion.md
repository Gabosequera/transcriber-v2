# 03 — Inventario exacto de reutilización

| Proyecto | Archivo/crate | Función | Qué resuelve | Tipo | Cambios necesarios | Licencia | Riesgo | Decisión |
|---|---|---|---|---|---|---|---|---|
| Gausian | `crates/timeline/src/graph.rs`, `commands.rs` | graph, tracks, frame ranges, automation y comandos inversos | base de timeline tipado | adaptar concepto | reemplazar graph por modelo editorial/clip; usar rational timebase; añadir revisión/digest | conflicto Apache-2.0 vs README MPL-2.0 | crítico | prototipo de modelo; no copiar |
| Gausian | `crates/project/migrations/V0001..V0006` | assets, jobs, metadata, timeline, transcripts, proxies | tablas y migraciones | inspiración/posible dependencia parcial | separar estado interno de JSON externo; migrations auditadas | conflicto no resuelto | crítico | usar como comparación; no copiar |
| Gausian | `crates/renderer/src/shaders/{yuv_to_rgb,preview_nv12,preview_p010,blend}.wgsl` | YUV→RGB, preview y blend | camino GPU y formatos | adaptar solo con procedencia | validar colorimetría, stride, HDR y fallback CPU | conflicto no resuelto | crítico | no copiar; reimplementar tras revisión |
| Gausian | `crates/native-decoder/src/gstreamer_backend.rs` | selección por plataforma, ring de frames, seek/preroll, fallback software | decoder preview | portar conceptualmente | aislar owner thread, PTS/DTS/VFR, zero-copy por backend | conflicto no resuelto | crítico | inspiración y tests; no copy-paste |
| OpenCut | `rust/src/editor/timeline_document.rs`, `timeline_clip.rs`, `timeline_interactions.rs` | documento, clips, drag, snapping y selección | UX timeline multitrack | portar conceptualmente | integrar comandos/digests y tracks multimedia vs capas semánticas | licencia no confirmada | alto | no copiar; estudiar |
| OpenCut | `rust/src/editor/editing.rs` 689–725 | snapshots undo/redo y operaciones GES | undo práctico | inspiración | sustituir snapshot ciego por inverse command + revision precondition | bloqueada | alto | estudiar |
| OpenCut | `rust/src/editor/export_gstreamer.rs` | mapear timeline a GES y export | preview/export | adaptar concepto | separar graph normalizado y declarar plugin requirements | bloqueada | alto | estudiar |
| Cutlass | `crates/cutlass-commands/src/{lib.rs,command.rs}` | command protocol | command bus tipado | adaptar | añadir actor/project/base revision/idempotency/digest | MIT/Apache | medio | candidato fuerte |
| Cutlass | `crates/cutlass-engine/src/action`, tests `inverse_undo.rs`, `compound_undo.rs` | acciones y undo transaccional | operaciones revisables | adaptar | incluir dry-run, diff, stale y auditoría | MIT/Apache | medio | candidato fuerte |
| Cutlass | `crates/cutlass-render/src/{render,scene,resolve,media_cache}.rs`, `cutlass-compositor` | scene graph, cache y render GPU | compositor | portar conceptualmente | timeline editorial, cache budget, WGPU adapter y CPU fallback | MIT/Apache | alto | spike Fase 5 |
| Cutlass | `crates/cutlass-ai/src/validate/command.rs`, `src/wire/dtos/timeline.rs` | validación AI/DTOs | frontera agente | adaptar | prohibir path/SQL, capabilities, approval tiers | MIT/Apache | medio | candidato |
| Kerf | `crates/kerf-core/src/{project.rs,...}` y queue/MCP | EDL, proposals, diff, tareas persistentes | AI/humano review | portar conceptualmente | eliminar frontend web, PolyForm; usar schemas propios | PolyForm NC | crítico | no copiar |
| GStreamer-RS | `gstreamer-editing-services`, examples `ges.rs` | bindings GES | pipeline edición | dependencia | pin versión y ejecutar todo GES en owner thread | MIT/Apache + LGPL runtime | alto | usar dependencia |
| MLT | `src/framework/mlt_{tractor,producer}.*` | tractor/producer/filter | modelo NLE maduro | inspiración | no introducir GPL; comparar semantics | GPL | crítico | referencia |
| Kdenlive | `src/timeline2/model/{timelinemodel,trackmodel,clipmodel,snapmodel}.*` | tracks, groups, snapping | UX y invariantes | inspiración | reimplementar clean-room si procede | GPL | crítico | no copiar |
| Shotcut | `src/commands/timelinecommands.*`, `src/docks/timelinedock.cpp` | command undo, ripple, markers, docks | interacción | inspiración | trasladar solo comportamiento observable | GPL | crítico | no copiar |

## Regla de procedencia

Cada futura línea copiada debe entrar con SPDX, URL, commit, archivo, licencia y revisión legal. Mientras exista duda, se porta el concepto y se escribe una prueba propia.
