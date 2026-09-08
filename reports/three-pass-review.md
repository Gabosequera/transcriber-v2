# Tres pasadas de revisión crítica

## Pasada A — reconstrucción

Se revisaron entrypoints, módulos de UI, pipeline editorial/legacy, media/subprocess, manifests, schemas y tests; se verificaron símbolos concretos en `editorial_io`, `editorial_master`, `editorial_layers`, `editorial_trims`, `editorial_topics`, `editorial_montaje`, `editorial_projects`, `editorial_pipeline`, `medios`, `hardware` y `editorial_history`. Resultado: V1 sí posee contratos y reanudación más ricos que una app Python monolítica.

## Pasada B — contradicción

Se intentó refutar la propuesta: Rust no acelera los modelos Python por sí solo; egui no está probado para 100k items/IME; GStreamer/GES complica runtime y no es thread-safe; FFmpeg/GStreamer pueden divergir; SQLite puede crear doble autoridad; NDJSON no sirve para frames; licencias bloquean copia; y V1 tiene cambios históricamente frecuentes. Resultado: siete decisiones quedan provisionales y cuatro claims anteriores se corrigen.

## Pasada C — síntesis

Se conserva la semántica y los contratos V1, se migran las responsabilidades puras y de coordinación a Rust, se mantienen modelos en Python aislados, y se usan bibliotecas externas solo detrás de adaptadores. La implementación queda condicionada a P1–P14, especialmente importer V1, IPC, preview WGPU, comparación preview/export, packaging Windows y accesibilidad.

## Conclusión

La complejidad adicional se justifica únicamente por los límites de seguridad/compatibilidad; no se justifica portar modelos ni adoptar GES/SQLite como autoridades duplicadas.
