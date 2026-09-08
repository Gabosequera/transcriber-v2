# Trazabilidad de afirmaciones críticas

| Claim | Clasificación | Evidencia exacta | Resultado/método |
|---|---|---|---|
| T0 y offset son autoridad temporal V1 | HECHO VERIFICADO | V1 `medios.py` módulo/docstring y extracción normalizada; `playback_clock.py` | lectura estática; requiere golden VFR |
| master tiene fingerprint, tracks y conversation | HECHO VERIFICADO | `editorial_master.build_master`; `editorial_io.SCHEMA_MASTER` | función devuelve `editorial-master/1` y estructura observada |
| layers protegen edited/deleted y revisiones | HECHO VERIFICADO | `editorial_layers.LayerStore.save/delete/merge_responses` | validación estática de revision/digest/tombstone |
| trims/tópicos/montaje rechazan stale | HECHO VERIFICADO | `editorial_trims.validate_proposal`, `editorial_topics`, `editorial_montaje` | comparan digest/request/pass; no se ejecutó |
| undo V1 no se persiste | HECHO VERIFICADO | `editorial_history.HistoryStack` docstring y slots | memoria, profundidad 50 |
| GES requiere owner thread | HECHO VERIFICADO | docs oficiales GES-Rust: API not thread safe; `gstreamer-editing-services/src/lib.rs` | documentación actual + código del commit |
| FFmpeg hwaccel puede perder contra CPU | HECHO VERIFICADO | docs oficiales `ffmpeg.html`, sección `-hwaccel` | incluye coste de copias GPU→CPU |
| egui soporta AccessKit | HECHO VERIFICADO | egui `docs/accessibility.md` y README oficial | soporte de widgets/custom info; no prueba timeline custom |
| egui cumple editor completo | HIPÓTESIS A VALIDAR | no hay benchmark V2 ni timeline 100k | spike requerido |
| GStreamer preview y FFmpeg export son WYSIWYG | HIPÓTESIS A VALIDAR | solo propuesta ADR-002 | frame comparison requerido |
| Rust acelera inferencia | CONTRADICCIÓN | V1 inference vive en faster-whisper/Torch/etc.; Rust solo orquesta | no atribuir speedup de modelos |
| V1 tiene 129 tests actuales | SIN EVIDENCIA | historial/docs con conteos variables, tests no ejecutados | reproducir fuera de V1 |
| Gausian tiene licencia única clara | CONTRADICCIÓN | `README.md` vs `LICENSE` del commit `2e173a0` | bloqueo legal |
