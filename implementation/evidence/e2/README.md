# Evidencia E2 (editor multimedia dentro del alcance)

**Actualización 2026-09-08:** estado y evidencia vigentes en `continuacion-02.md`, regresiones nuevas en `latest-regression.json`. Smoke del ZIP fallida (`package-smoke-20260908-065502/`), F-008/F-009 abiertos; E2 sigue abierta. La tabla siguiente conserva la evidencia de la primera sesión.

Build: `cargo build --release -p transcriptor` (2026-09-08), ejecutado desde `%TEMP%` (fuera del workspace). Guiones en `tests/scripts/`, verificador `tests/scripts/verify_export.py`.

| Artefacto | Qué demuestra |
|---|---|
| `e2-escenario1.log`, `01-composicion-4000ms.png`, `02-tras-corte-7000ms.png`, `03-hueco-5200ms.png`, `04-exportado.png` | Escenario integrado 1 «Editor sin ML»: dos videos (PiP con transformación), PNG con alfa, dos audios (fixture + WAV con ganancia −12 dB en A3), corte en 5,0 y 5,5 s (fuera de keyframe), borrado sin ripple con hueco, undo/redo, guardar/reabrir, export 720p/1080p/vertical/WAV e IN/OUT. |
| `viewer-4000ms.png`, `viewer-7000ms.png`, `viewer-5200ms.png` | Fotogramas compuestos por el visor (mismo compositor que el export). |
| `verify-720p.txt`, `verify-1080p.txt` | Comparación visor↔export (PSNR 30–31 dB, umbral 26), cortes contra fuente (PSNR ≥ 42 dB), región PiP presente en 4,0 s y ausente en el hueco 5,2 s, duración exacta 12,000 s. |
| `escenario1-*.mp4/.wav` | Salidas reales (no se sobrescriben; sha256 en el log). |
| `e2-transporte.log`, `transporte-*.json/png` | Velocidades 1×/3×/4× con posición esperada, skim 8× (keyframes, sin audio), pausa, frame step exacto (+1 = 2,0333 s; −10 = 1,700 s), loop 2–3 s (posición se mantiene en el rango y avanza), parada al final. |
| `e2-import-v1.log`, `import-v1-01-secuencia.png`, `import-v1-02-fuente-capas.png`, `import-v1-03-exportado.png`, `import-v1-*-estado.json`, `import-v1-montaje-720p.mp4` | Importación de `tests/fixtures/v1/demo-a` (generada por los módulos de V1): 2 capas (temas con multirrango y jerarquía, tombstone `item-borrado`), 2 carriles de recortes (aceptado/propuesto/desactivado), montaje aplanado en 5 tramos (repetición incluida) exportado a 7,000 s. |

Tests automatizados asociados: `apps/desktop/src/gesture_tests.rs` (egui_kittest: arrastre en una transacción, Escape, trim, scrub, teclas bloqueadas al escribir), `crates/v1compat/tests/golden_v1.rs` (fingerprint V2 == V1 sobre el mismo archivo, digest de master, unión de recortes, aplanado y mapping fuente→secuencia iguales a los calculados por V1).
