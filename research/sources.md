# Fuentes consultadas

Fecha de consulta: 2026-09-07. Las rutas locales de `references/` son la evidencia de código fijada en `repositories-lock.json`; las siguientes son documentación oficial o texto legal original.

| Fuente | URL | Versión/fecha | Afirmación respaldada | Tipo |
|---|---|---|---|---|
| egui README y accesibilidad | https://github.com/emilk/egui ; https://github.com/emilk/egui/blob/main/docs/accessibility.md | rama main consultada 2026-09-07 | eframe soporta Windows/Linux; backend wgpu; AccessKit y tests de árbol accesible; API en desarrollo | código/docs |
| Slint desktop/backends | https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/ ; https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/ | docs actuales consultadas 2026-09-07 | Windows 10/11 y Linux; winit/Qt/software/Skia/FemtoVG; selección de backend | documentación |
| Slint licencia | https://slint.dev/faqs ; https://slint.dev/agreements/slint-software-license.pdf | licencia consultada 2026-09-07 | GPLv3, licencia royalty-free y licencia comercial; no asumir coste cero para producto futuro | legal/docs |
| GTK4 Rust | https://gtk-rs.org/gtk4-rs/stable/latest/book/ | estable consultada 2026-09-07 | bindings Rust, event loop y toolkit nativo | documentación |
| GPUI oficial | https://gpui.rs/ | sitio consultado 2026-09-07 | framework Rust de Zed; documentación y ejemplos; no prueba de soporte Windows/Linux de editor V2 | documentación |
| GTK4 oficial | https://docs.gtk.org/gtk4/section-accessibility.html ; https://docs.gtk.org/gtk4/windows.html | GTK 4.23.x docs consultadas 2026-09-07 | AT-SPI/Windows accessibility y backend Win32; bundling requerido | documentación |
| GES | https://gstreamer.freedesktop.org/documentation/gst-editing-services/index.html ; https://gstreamer.freedesktop.org/documentation/rust/stable/latest/docs/gstreamer_editing_services/ | GStreamer/GES actuales consultados 2026-09-07 | Timeline/Tracks/Layers/Clips/Pipeline; Windows; LGPL; bindings no thread-safe | docs/API |
| GES timeline | https://gstreamer.freedesktop.org/documentation/gst-editing-services/gestimeline.html ; https://gstreamer.freedesktop.org/documentation/gst-editing-services/gestimelineelement.html | actuales consultadas 2026-09-07 | commit explícito, reglas de solape, edición move/start-trim/end-trim y snapping | docs/API |
| GStreamer threads | https://gstreamer.freedesktop.org/documentation/application-development/advanced/threads.html ; https://gstreamer.freedesktop.org/documentation/additional/design/MT-refcounting.html | actuales consultadas 2026-09-07 | pipeline multihilo, bus/mensajes y reglas de ownership | documentación |
| GStreamer 1.28 | https://gstreamer.freedesktop.org/releases/1.28/ | release notes consultadas 2026-09-07 | task pool en GES, cambios de VA plugin y mejoras de errores/MT-safety | release notes |
| FFmpeg general | https://ffmpeg.org/ffmpeg.html ; https://ffmpeg.org/general.html | docs generadas/actuales consultadas 2026-09-07 | seek exacto vs keyframe, hwaccel, coste de copias GPU↔CPU y dispositivos DXVA/QSV/VAAPI | documentación |
| FFmpeg legal | https://ffmpeg.org/legal.html ; https://ffmpeg.org/doxygen/trunk/md_LICENSE.html | actual consultada 2026-09-07 | LGPL por defecto, partes GPL, --enable-gpl/nonfree, linking y obligaciones de distribución | legal |
| MCP transports | https://modelcontextprotocol.io/specification/draft/basic/transports | draft consultado 2026-09-07 | MCP usa JSON-RPC; stdio y Streamable HTTP; localhost, Origin y autenticación | especificación |
| JSON-RPC | https://www.jsonrpc.org/specification | 2.0 consultada 2026-09-07 | request/response/error e idempotencia a nivel de aplicación | especificación |
| SQLite WAL | https://sqlite.org/wal.html ; https://sqlite.org/atomiccommit.html | docs consultadas 2026-09-07 | concurrencia y límites de WAL/commit atómico | documentación |
| Rust tracing | https://docs.rs/tracing/latest/tracing/ | versión docs consultada 2026-09-07 | spans, eventos estructurados y contexto de diagnóstico | API |

Las URLs de repositorios y paths de evidencia están en los documentos por decisión. No se usaron resultados de buscador como fuente final. Las afirmaciones de rendimiento y soporte final permanecen pendientes de benchmark propio.
