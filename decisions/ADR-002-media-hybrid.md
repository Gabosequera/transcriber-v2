# ADR-002 — media híbrida: GStreamer preview + FFmpeg export

- Estado: provisional
- Fecha: 2026-09-07
- Nivel de confianza: medio-alto

## Decisión

Usar GStreamer-RS para pipeline de preview/decode y FFmpeg/FFprobe externo como baseline de probe/export. GES se encapsula en un `GesOwner` de un solo hilo/event loop y no es fuente de verdad. WGPU compositor propio se incorpora solo después de medir.

## Motivo

GStreamer ofrece bus, clock, plugins y caminos HW; GES aporta Timeline/Tracks/Layers/Clips y render, pero sus bindings advierten que la API no es thread-safe y su timeline tiene reglas/commit. FFmpeg documenta accurate seek, hardware APIs y el coste de copias GPU↔CPU, lo que favorece aislamiento y reproducibilidad de export.

## Failure modes

Plugin missing, color mismatch, VFR drift, B-frame seek, GES overlap rejection, orphan process, encoder license change. Todos deben producir capabilities/error codes y fallback software.
