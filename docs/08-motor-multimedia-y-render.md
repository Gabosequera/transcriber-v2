# 08 — Motor multimedia y render

## Comparación

| Opción | Preview/seek | Export | Licencia/packaging | Decisión |
|---|---|---|---|---|
| FFmpeg externo | robusto y aislado; seek por keyframe/accurate seek según flags | excelente, scripts y codecs amplios | binarios y codecs deben auditarse | obligatorio para probe/export baseline |
| libav bindings | menos spawn/copia | control fino | ABI/licencia y crash en proceso | no para primera entrega |
| GStreamer | pipeline vivo, bus, clocks, HW plugins | válido con encodebin | plugins/registry complejos | preview/decode principal |
| GES | timeline/tracks/layers/clips, edit modes, preview/render | cómodo | LGPL, API no thread-safe | adaptador detrás de owner thread |
| MLT | NLE maduro, tractor/producer/filter | maduro | GPL | referencia, no dependencia |
| compositor WGPU propio | control y potencial zero-copy | no decodifica por sí mismo | alta complejidad | spike después del slice |
| híbrido | GStreamer decode + WGPU preview + FFmpeg export | WYSIWYG requiere grafo común | complejidad media-alta | objetivo V2 condicionado a benchmarks |

## Grafo común

`ResolvedTimeline` se compila tanto a `PreviewGraph` como a `ExportGraph`; cada nodo conserva source/timeline range, PTS policy, transform, blend, gain y timebase. Se comparan frames de preview y export con tolerancia definida. GStreamer GES exige commit antes de que los cambios afecten salida y tiene reglas de solape; esas restricciones se validan antes de llamar a GES.

## Codecs y hardware

Detectar H.264/HEVC/AV1/VP9/ProRes, audio multipista, VFR/B-frames y `start_time`. Selección inicial: D3D11VA/NVDEC/QSV/VAAPI según plugin disponible, luego software. FFmpeg documenta que hwaccel puede no superar software y que las copias GPU→CPU pueden empeorar rendimiento; por eso preview y export registran backend, copias, dropped frames y drift.

J/K/L y scrub requieren playback clock monotónico, preroll y keyframe seek separado de accurate seek. Reverse y time-stretch quedan como capabilities: no declarar soporte hasta medir audio pitch-preserving y A/V sync.
