# 02 — Matriz de proyectos open source

| Proyecto/commit | Arquitectura y evidencia | Madurez/actividad en el corte | Licencia y uso | Decisión |
|---|---|---|---|---|
| Gausian `2e173a0` | workspace Rust; `crates/timeline`, `project` con migraciones SQLite, `renderer` WGSL YUV/NV12/P010, `media-io`, `native-decoder`; README declara egui+wgpu, GStreamer, FCPXML/EDL/JSON y proxies | activo pero pequeño; probar packaging y cobertura real | conflicto: `LICENSE` es Apache-2.0, README declara MPL-2.0 core y pro comerciales | no copiar hasta resolver la discrepancia con el titular; excelente mapa de separación |
| OpenCut `fee06cc` | GPUI; `rust/src/editor/timeline_document.rs`, `editing.rs`, `preview_timeline.rs`, `export_gstreamer.rs`; JSON autosave y stacks undo/redo en README/código | muy activo en fecha de corte; Windows real no probado aquí | no se usó como fuente de derechos; ausencia de licencia explícita válida bloquea copia | referencia conceptual y tests de interacción; no copiar |
| Cutlass `22437e2` | crates de models/commands/engine/render/compositor/AI; Slint desktop; `cutlass-ai/src/validate`; tests de inverse/compound undo y render | activo, arquitectura rica; APIs en evolución | MIT/Apache-2.0, conservar avisos y revisar terceros | candidato a adaptar/usar patrones, no copiar sin inventario de terceros |
| Kerf `5fc1db8` | `kerf-core` + frontend Svelte; project/EDL, SQLite, queue, MCP, diff/proposals, FFmpeg | activo, pero frontend y contrato en evolución | PolyForm Noncommercial; copyright del autor | inspiración de dominio/AI; no copiar ni depender para producto comercial |
| GStreamer-RS `b13e8e0` | bindings por crate; incluye `gstreamer-editing-services`; ejemplos GES; upstream principal | upstream maduro/activo | MIT/Apache en bindings; GStreamer/GES upstream LGPL y plugins separados | usar como dependencia respetando runtime/plugin/licencias |
| MLT `0f8244a` | C/C++; producer/tractor/playlist/filter/consumer, modules y profiles | muy maduro, amplio ecosistema | GPL/COPYING | referencia arquitectónica; no enlazar/copiar sin decisión GPL |
| Kdenlive `d108f8` | Qt/QML + C++; `timeline2/model` (clip/track/snap/groups), render server, preview, OTIO | muy maduro; gran coste de integración | GPL + REUSE | solo referencia de interacción/arquitectura |
| Shotcut `ca58e5` | Qt/QML + MLT; `TimelineDock`, `timelinecommands`, preview/export/GPU info | maduro; fuerte dependencia MLT/Qt | GPL/COPYING | solo referencia de interacción/packaging |

## Conclusión

Ningún repositorio resuelve simultáneamente GUI nativa, timeline editorial y AI control seguro. La composición recomendada toma ideas pequeñas y trazables: Gausian para crate boundaries/render; Cutlass para commands/undo/AI validation; OpenCut/Kdenlive/Shotcut para UX; Kerf para propuestas/diffs; GStreamer/FFmpeg como dependencias, no como modelo de verdad.
