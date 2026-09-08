# 00 — Resumen ejecutivo

## Recomendación

1. Mantener T0, fingerprints, manifests, digests, revisiones, tombstones, JSON legible y edición no destructiva como contratos de producto; Rust los convierte en invariantes tipadas.
2. Usar `egui` + `eframe`/`egui-wgpu` para una GUI nativa Windows/Linux y un widget de timeline custom; adoptar AccessKit desde el primer vertical slice. Slint queda como alternativa secundaria si el prototipo de accesibilidad/docking/IME falla, sujeto a decisión de licencia.
3. Separar dominio de media: GStreamer-RS para preview y decode; GES solo como adaptador de edición/render bajo un owner thread; FFmpeg/FFprobe externo para probe, compatibilidad y export reproducible. El preview y el export comparten un grafo normalizado y fixtures.
4. Implementar un command bus único para humano y AI con `base_revision`, digest, dry-run, diff, validación, apply/reject, undo y auditoría. La AI nunca escribe SQLite ni filesystem directamente.
5. Usar workers Python por proceso, con capacidades/versiones, heartbeats, cancelación y checkpoints; NDJSON/stdio local primero, sockets/named pipes solo para superficies que necesitan separación o integración externa; MCP detrás de una frontera autenticada.

## Stack propuesto

Rust: `egui`, `eframe`, `egui-wgpu`, `winit`, `serde`, `serde_json`, `rusqlite`/SQLite, `tracing`, `thiserror`, `tokio` solo donde el beneficio sea demostrable, GStreamer-RS, FFmpeg/FFprobe como binarios supervisados. Python: workers existentes y sus entornos aislados.

## Nivel de confianza

Medio-alto para separación de responsabilidades y persistencia; medio para egui (IME, docking y accesibilidad avanzada siguen siendo riesgos de prototipo); medio para GStreamer/GES en Windows por packaging de plugins y diferencias de hardware.

## Estado de auditoría

La segunda auditoría clasifica esta recomendación como provisional: la separación Rust/Python, los contratos V1 y la restricción de GES están verificados; egui, WYSIWYG preview/export, SQLite como segunda capa y el scheduler requieren prototipos. El dato histórico de 129 tests no se considera verificado.

## No decidido por documentación

El decoder exacto por codec/GPU, el compositor zero-copy, la versión mínima de GStreamer, la distribución de FFmpeg, el protocolo remoto de agentes y el formato de migración SQLite deben validarse con benchmarks y revisión legal.
