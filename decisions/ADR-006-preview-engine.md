# ADR-006 — motor de preview

- Estado: provisional | Fecha: 2026-09-07

GStreamer-RS directo + clock/decoder service es la base; GES es opcional como adaptador de timeline. FFmpeg preview solo fallback/diagnóstico. Motivo: bus, seek, clocks, plugins y preview vivo; riesgo de plugins/packaging y frame copies. Prototipos P2/P3/P14 deciden backend por codec.
