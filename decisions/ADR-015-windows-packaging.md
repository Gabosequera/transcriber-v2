# ADR-015 — packaging Windows

- Estado: provisional | Fecha: 2026-09-07

Distribuir exe Rust y runtimes declarados; GStreamer/FFmpeg/plugins se prueban en clean VM; no depender de PATH del usuario. El launcher/updater V1 se conserva durante migración. P5 define tamaño, registry, DLL search, firma y rollback.
