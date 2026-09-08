# Continuación 3 — 2026-09-08

## Alcance y estado inicial

Leídos los tres prompts, STATUS, matriz, decisiones, investigación y ADRs pertinentes. Se continuó el último anexo de Astra, conservando el trabajo previo de Fable. E0/E1 ya cerradas; E2 abierta. V2 no tiene Git; no se encontraron AGENTS.md aplicables. V1 HEAD c3677ba568cfc7d5947ec4695c42fffef6d79e03 y `git status --short` vacío al inicio y al control final. No se modificó V1 ni references.

## Cambios

- F-008 cerrado: builder resuelve origen relativo contra el padre de .transcriptor y persiste `Demo.transcriptor/media/<id>.mp4`.
- F-009 cerrado para medios ausentes/inaccesibles, decoder fallido y fotogramas incompletos: validación al ejecutar el snapshot, solo assets utilizados por rango/formato; error accionable sin publicación. Audio distingue EOF correcto de exit code fallido. Las pruebas cubren desaparición posterior al snapshot, corrupción de video/audio, audio-only sin requerir video y rango que excluye ausentes.
- `ResolvedTimeline::extract_ranges`: selección de clips y tramos/items/bloques proyectados a todas sus ocurrencias, unión de solapes y concatenación temporal; composición/mezcla intactas. Integrado con diálogo, cola y guion. Importación de todos los contratos chunks V1 sigue pendiente E3.
- Prueba real de mute/solo/gain: dos pistas sumadas con factores 0,5/0,25, casos mezcla/mute/solo/silencio; comparación independiente con audio fuente y WAV exportado. 12000 frames por caso, error máximo 0,000015258789 (media unidad PCM16), silencio exacto.
- Smoke exige PSNR fuente/visor y hueco negro además de caches/log/hash; restaura variables de entorno. Capturas esperan a que terminen animaciones y toasts.

## Evidencia y comandos

Desde `G:/TODO/transcriptor-v2`, cargo `C:/Users/gabri/.cargo/bin/cargo.exe`:

| Verificación | Comando / artefacto | Resultado |
|---|---|---|
| Tests workspace | `cargo test --workspace --locked`; workspace-selection-final.log | 85 pasan: desktop15/application6/domain25/media24/v1compat11+4goldens |
| Fmt / Clippy | `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings` | fmt-selection.log, clippy-selection.log limpios |
| Release | `cargo build --release --locked -p transcriptor` | build-selection.log correcto; los cambios posteriores Rust son solo un test |
| Negativos export | media-continuacion-03.log | desaparición/corrupción rechazadas sin salida/staging |
| Mezcla | `cargo test -p tv2-media mute_solo --locked -- --nocapture` | mix-equivalence.log |
| Regresión E1/E2 | prepare_regression.py; --script; verify_export.py | regression-20260908-160410/: ambos fallos=false, verificadores OK |
| Selección + cachés | prepare_selection.py; --script selection.json/caches.json | selection-20260908-160410/: fallos=false, PSNR42,7–43,0dB |
| Paquete | build_windows.py --skip-build; package_smoke.ps1 | package-smoke-20260908-120555/: 499 hashes antes/después, fallos=false, verify-export.txt OK |

E1:12s/360frames, PSNR42,4–42,9dB. E2:720p/1080p, visor/export34,9–37,9dB y PiP correcto. Selección clips24s/720frames, tramos repetidos2s/60frames. Paquete14s/420frames, fuente42,6–42,8dB, visor30,6dB, hueco negro media0. Paquete anterior de este incremento: package-smoke-20260908-115558/, visor49,9dB al comparar el frame exacto157/30; inicialmente se comparó erróneamente5,25 con el seek cuantizado5,233. No confundir tiempos solicitados con frames presentados.

## Tres revisiones

- Funcional: selección exporta ambas ocurrencias sin duplicar audio enlazado; regresiones import/editar/guardar/reabrir/export intactas, preset/rango/audio presentes. F-008/F-009 reproducidos por código y corregidos mediante pruebas reales.
- Técnica: dominio puro, snapshot no mutado, rango semiabierto, unión antes de concatenar, validación en worker. Decoder de audio ya no equipara proceso fallido a EOF normal. No se certifican aún cancelación de IO bloqueado, cierre con trabajos activos ni detección universal de corrupción recuperable por FFmpeg.
- Experiencia: inspeccionadas capturas reales portable.png y export-selection.png; diálogo muestra semántica y opciones activas. Primera captura tenía animación/toasts y se repitió. `caches.json` reejecutado con wait5500. El cambio zoom/viewport interactúa con el tamaño persistido de eframe: las dimensiones reales constan en logs (incluido2560×1440), no certifica DPI OS ni todas las resoluciones solicitadas.

## Paquete provisional probado en host

`G:/TODO/transcriptor-v2/dist/Transcriptor V2 prueba ñ 20260908-160510.zip`

SHA256 `af2503b08f867bdf0a3fbb6470e49280be857028e16425e40a33446223d8a02f`. ZIP85.541.598bytes; exe21.303.808bytes; expandido225.633.502bytes. Extraído a TEMP Unicode, PATH solo System32/Windows; editor sin Rust/Python/FFmpeg globales. Python solo ejecuta el verificador de pruebas. Windows limpio, auditoría final de licencias y E6 pendientes.

## Continuación

E2 sigue abierta: VFR/rotación/fronteras, presupuestos globales de cache/RAM y benchmark UI densa, cierre/worker cancelación, sincronía física A/V F-001, foco/IME/layouts/DPI OS F-002. Implementar primero cierre con trabajos activos (on_exit hoy solo guarda UI; MediaCaches::Drop solo señala cancelación, no espera). No saltar a E3 aceptada por cantidad de tests. Traspaso completo de seis bloques al final del prompt00; STATUS y matriz actualizados.
