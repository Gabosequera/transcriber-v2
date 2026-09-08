# ADR-003 — SQLite interno + JSON externo

- Estado: provisional
- Fecha: 2026-09-07
- Nivel de confianza: alto

SQLite/event log sirve para consultas, jobs y revisiones; JSON sigue siendo portable, humano y consumible por AI. Import/export es explícito, con digest y diff. WAL no se expone como contrato de sincronización y no se asume que un archivo SQLite sea portable entre procesos sin política.

La condición de reconsideración es que el corpus real demuestre un coste de snapshot inaceptable; aun así no se eliminarán los JSON contractuales sin una migración de herramientas y una revisión de usabilidad.
