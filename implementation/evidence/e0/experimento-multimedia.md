# E0 · Experimento temprano: ventana nativa, video embebido y dependencias Windows

Fecha: 2026-09-07. Build `cargo build` (dev, `opt-level=1`) y release. Host: ver `initial-state.md`.

## Pregunta, métrica, decisión

- **Pregunta**: ¿puede una ventana nativa egui/eframe (wgpu) presentar video decodificado por un proceso FFmpeg externo sin bloquear la UI, y encuentra el paquete sus dependencias sin instalación global?
- **Métrica**: primer fotograma tras seek y coste de composición por fotograma en el hilo del reproductor (`decode_ms` del snapshot), sin instalar GStreamer ni FFmpeg globales.
- **Decisión**: D-0003 (FFmpeg por proceso + compositor propio); GStreamer queda como alternativa detrás de las mismas interfaces.

## Resultado

- La ventana abre con backend wgpu (Vulkan sobre RTX 4070 según el log; los avisos `VK_LAYER_OW_*` proceden de capas de overlay de terceros instaladas en el host, no del producto).
- FFmpeg se resuelve desde `packaging/third-party/ffmpeg` (origen «workspace de desarrollo»); en el paquete se resolverá junto al ejecutable.
- Decodificación 640×360@30 → visor 872×490: `decode_ms` 1–3 ms por fotograma compuesto en lectura secuencial (medido en `evidence/e1/*-estado*.json`); seek con reapertura de proceso: primer fotograma en < 400 ms (el guion espera el fotograma exacto del playhead y lo obtuvo en todos los casos).
- Tests con medios reales: `tv2-media` 8/8 (`-ss` exacto fuera de keyframe con GOP 2 s: fotograma 45 a 1,5 s; audio `atempo` 2× reduce la duración a la mitad; overlay PNG con alfa compuesto sobre video).
- Riesgo multimedia reproducido y resuelto en E1: **comandos del reproductor perdidos** durante la espera entre fotogramas (`recv_timeout` consumía y descartaba `Pause`/`Seek`), detectado con el guion (split en 6,9 s en vez de 5,0 s); corregido con `carry` del comando al ciclo siguiente y verificado por el guion (`fotograma presentado: 00:00:05.000`).

## Límites conocidos

- Sin zero-copy: cada fotograma se copia proceso→RAM→textura. Aceptable para 1080p en este host; 4K se medirá en PERF-01.
- Cada seek abre un proceso FFmpeg (~30–80 ms); un scrub rápido coalesce a la última posición. Caché de decodificadores calientes pendiente (E2).
- Las capturas del guion son capturas reales de la ventana (eframe `ViewportCommand::Screenshot`), no maquetas.
