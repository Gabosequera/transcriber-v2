# 17 — Riesgos, preguntas y bloqueos

## Críticos

1. Licencias: OpenCut sin licencia permisiva confirmada, Kerf PolyForm NC y GPL de MLT/Kdenlive/Shotcut; requiere legal.
2. Preview/export: dos graphs pueden divergir con VFR, B-frames, offsets y color; requiere golden frame tests.
3. GES: API no thread-safe, commit y reglas de solape; requiere owner thread y prueba de 3h.
4. Windows packaging: GStreamer registry/plugins, FFmpeg build, drivers y named pipes; requiere instalador real.
5. GPU: zero-copy puede no existir fuera de un backend; las copias pueden empeorar; requiere matriz hardware.
6. egui: accesibilidad/IME/docking y API en evolución; requiere spike con AccessKit y texto internacional.

## Preguntas abiertas

¿Qué distribución comercial de FFmpeg/codecs se desea? ¿Cuál es la versión mínima de Windows y Linux? ¿Se acepta instalar GStreamer o debe ser bundled? ¿Qué esquema de timebase soporta VFR? ¿Qué agente externo y autenticación se soportan? ¿Qué cambios de schema V1 requieren compatibilidad indefinida?

## Bloqueos actuales

No se ejecutaron benchmarks con medios ni builds de referencia: la misión prohíbe escribir artefactos en V1 y esta fase fue documental. La ausencia de licencia explícita de OpenCut bloquea reutilización de código. La validación de hardware/packaging y la revisión legal quedan como gates, no como supuestos.
