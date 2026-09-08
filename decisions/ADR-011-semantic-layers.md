# ADR-011 — capas semánticas independientes

- Estado: aceptado | Fecha: 2026-09-07

Capas de autor, topics, trims, señales y AI comparten navegación temporal pero tienen fuente, estados, permisos y persistencia propios. Se conservan `item_id`, `parent_id`, multi-ranges, `edited`, `accepted` y tombstones. Las proyecciones son regenerables.
