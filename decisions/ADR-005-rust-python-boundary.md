# ADR-005 — límites Rust/Python

- Estado: provisional | Fecha: 2026-09-07 | Confianza: alta
- Evidencia: V1 `core.py`, `align.py`, `prosodia.py`, `laughter.py`, `vision.py`; `editorial_pipeline.py`; Gausian workspace/crates.

Rust recibe dominio, UI, media orchestration, persistence, scheduling and supervision; Python conserva model inference. No se atribuye speedup a Rust en inferencia. Reconsiderar solo si un modelo equivalente Rust pasa accuracy/performance parity.

Riesgo: dos runtimes y packaging. Prueba: worker fake/real con crash, OOM, resume y secret redaction.
