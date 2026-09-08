# ADR-017 — migración incremental y compatibilidad V1

- Estado: provisional | Fecha: 2026-09-07

Migrar por responsabilidad con golden fixtures y coexistencia V1/V2. Los JSON V1 no se sustituyen por SQLite opaco; import/export/reconcile es explícito. Fase siguiente solo comienza cuando P7 y el gate de no pérdida de IDs/rangos pasan.
