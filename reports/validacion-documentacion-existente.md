# Validación de documentación existente

| ID | Documento | Afirmación | Evidencia actual | Estado | Corrección necesaria |
|---|---|---|---|---|---|
| D-01 | 00, 04 | V2 debe separar UI, dominio, media y workers | módulos V1 y layouts de crates Gausian/Cutlass | Verificado | mantener como arquitectura, no como rendimiento demostrado |
| D-02 | 00, 01 | V1 conserva T0, fingerprints, atomic writes, digests y resume | `medios.py`, `editorial_io.py`, `editorial_pipeline.py`, `editorial_history.py` | Verificado | añadir símbolos y fixtures en docs 22/23 |
| D-03 | 01 | V1 tiene 28,6k líneas de producto | conteo previo de `.py` con criterio no congelado; 81 Python Git ahora | Parcialmente verificado | fijar script/alcance en Fase 0; no tratar como métrica de calidad |
| D-04 | 01 | V1 tiene 129 tests pasando | no se ejecutaron tests por regla de no escritura; docs/historial muestran conteos variables | Sin evidencia | eliminar lenguaje factual; reproducir en entorno aislado |
| D-05 | 00, ADR-001 | egui es GUI primaria adecuada | docs egui: native/wgpu/AccessKit; no benchmark V2 | Requiere benchmark | aceptar con condiciones, no definitivo |
| D-06 | 00, ADR-001 | Slint es alternativa | docs Slint desktop/backends/licencia | Verificado | añadir coste de licencia y spike de video |
| D-07 | 00, 04 | GES no es thread-safe | docs Rust GES oficiales y crate local | Verificado | owner thread obligatorio |
| D-08 | 08 | FFmpeg externo es baseline export | `podcast_export.py`, `medios.py`, docs FFmpeg seek/hwaccel | Parcialmente verificado | probar empaquetado, codecs y WYSIWYG |
| D-09 | 08 | preview/export compartirán grafo | propuesta V2, no existe implementación | Propuesta | convertir en requisito de compatibilidad |
| D-10 | 07 | NDJSON/stdio sirve para workers | diseño; sin medición de payload/cancel | Hipótesis a validar | limitar a control/metadatos, nunca frames/tensores |
| D-11 | 07 | MCP será adaptador externo | MCP oficial define transports; integración V2 no existe | Propuesta | capability/authz y threat model antes de implementar |
| D-12 | 11 | SQLite + JSON híbrido evita doble autoridad | diseño, no implementación | Hipótesis a validar | definir reconcile y crash tests |
| D-13 | 14 | Gausian es MPL-2.0 | README dice MPL; `LICENSE` es Apache-2.0 | Contradicho | bloquear copia y pedir aclaración |
| D-14 | 14 | Cutlass es MIT/Apache | LICENSE-MIT/APACHE presentes | Verificado | auditar assets/deps transitivas |
| D-15 | 14 | MLT/Kdenlive/Shotcut son GPL | COPYING/REUSE y código | Verificado | solo referencia salvo decisión legal |
| D-16 | 02/03 | OpenCut puede reutilizarse | README muestra prototipo; licencia no confirmada | Requiere revisión legal | solo clean-room/conducta observable |
| D-17 | 05/06 | tracks y capas semánticas deben distinguirse | V1 usa layers/adapters y tracks en master/export | Verificado | formalizar IDs en doc 19 |
| D-18 | 15 | migración incremental es preferible | V1 tiene contratos y pipeline reutilizables; no comparación de coste total | Parcialmente verificado | gate por fase y kill criteria |
| D-19 | 10 | scheduler LRU/preemption reducirá presión | propuesta sin mediciones | Hipótesis a validar | prototipo de presión RAM/VRAM |
| D-20 | 12 | tracing cubre observabilidad propuesta | docs oficiales describen spans/events; V2 no lo integra | Propuesta | crear error taxonomy y fixture de redacción |
