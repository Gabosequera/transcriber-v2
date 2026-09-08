# Red-team de arquitectura

| Supuesto | Contraejemplo | Mitigación/gate |
|---|---|---|
| preview = export | filtros/GES y FFmpeg redondean distinto | `ResolvedTimeline`, frame hashes y drift golden |
| seek preciso siempre | VFR/B-frames/stream copy cae en keyframe | accurate/keyframe policy visible y medida |
| GPU acelera | copia frame GPU→CPU y startup empeoran | benchmark cold/warm por backend |
| GES thread-safe por bindings Rust | docs advierten no thread-safe | `GesOwner` único |
| AI respuesta válida | llega tarde o con JSON parcial | envelope, digest/pass, schema, idempotencia |
| JSON externo no cambia | editor externo modifica mientras UI vive | digest/revision y reconcile explícito |
| worker no cae | OOM/CUDA/OpenMP/plugin | supervisor, checkpoint, retry CPU/model tier |
| Windows igual que Linux | plugins/registry/paths/named pipe/DPI | CI + PC real y instalador limpio |
| egui cubre accesibilidad | custom canvas no tiene semántica | AccessKit tree, keyboard/IME tests |
| 3h VOD es solo más datos | memory/cache/seek/LOD explotan | bounded queues, LOD, corpus 3h |
