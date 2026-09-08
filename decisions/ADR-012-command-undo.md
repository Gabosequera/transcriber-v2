# ADR-012 — commands serializables e undo

- Estado: provisional | Fecha: 2026-09-07

UI y AI llaman el mismo command bus. Cada command produce inverse/diff y valida revision/digest. Se adopta la idea de Cutlass y la protección stale de V1; no se adopta snapshot ciego como única seguridad. P9 valida multi-document transaction y ramas de redo.
