# 11 — Persistencia y compatibilidad JSON

## Arquitectura híbrida

SQLite guarda índice de proyecto, event log, revisiones, jobs, caches y relaciones para consultas eficientes. JSON versionado sigue siendo el contrato externo: master, layers, trims, chunks, topics, montage, manifests, vistas de agente y export records. No hay sync bidireccional silenciosa.

Flujo: import JSON → validar schema/digest → transaction SQLite → snapshot determinista; mutación command → SQLite/event → export JSON atómico; JSON externo editado → detectar digest/base revision → ofrecer reconcile explícito (import as new revision, diff, accept/reject). Nunca sobrescribir automáticamente.

## Contratos V1 inventariados

`editorial-master/1`, `editorial-trims/1`, `editorial-trims-proposal/1`, `editorial-layer/1`, `editorial-layers-proposal/1`, `editorial-layers-view/1`, `editorial-chunks/1`, `editorial-topics-request/1`, `editorial-topics-proposal/1`, `editorial-montage-request/1`, `editorial-montage-proposal/1`, `editorial-catalog/1`, `keymap/1`, `editorial-derivation/1`, `editorial-derived-audio/1`, y manifests de pasos con digests/estado. Schema, campos y productores están en `reports/v1-contract-inventory.md`.

## Migración

Conservar nombres y semántica V1; añadir `schema` nuevo solo cuando cambie una invariante. Migradores puros y versionados producen backup, diff y digest antes/después. Un JSON legible debe poder inspeccionarse sin SQLite; vistas pueden regenerarse y llevan `generated_from`.
