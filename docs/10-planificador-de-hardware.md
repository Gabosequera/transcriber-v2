# 10 — Scheduler de hardware

## Recursos

Inventario al arranque (CPU física/lógica, RAM total/disponible, GPU/driver/API, VRAM libre/total) sin importar modelos. Reservas por tarea: decode, render, AI-GPU, AI-CPU, network, export e indexado. Colas bounded con prioridad: interacción/playback > export foreground > AI > indexado.

No se bloquea el hilo de UI; playback tiene budget de frame y preemption cooperativa de AI. Modelos se cargan por LRU con `unload` confirmado; si no es posible, se reduce calidad o se pausa AI. Battery/thermal mode baja threads, fps y calidad.

## Métricas y objetivos iniciales

Registrar TTFW, first-frame, seek P50/P95/P99, scrub latency, dropped frames, UI frame time, A/V drift, RAM/VRAM idle/peak, import time, waveform rate, export factor realtime, model load/unload y recovery after crash. Gates iniciales: UI ≤16.7 ms P95 a 60Hz; drift ≤1 frame en 10 min; seek P95 definido por corpus y codec; zero crash/worker zombie en 100 cancelaciones.

## Degradación

GPU faltante: compositor CPU y decode software; VRAM insuficiente: proxy/CPU/model tier; plugin faltante: capability false con error accionable; OOM: liberar LRU, retry bounded y diagnóstico. Nunca asumir que `gpu=true` significa más rápido.
