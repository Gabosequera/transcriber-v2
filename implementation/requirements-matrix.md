# Matriz de requisitos

**Actualización continuación 5:** implementación hasta E4 autorizada; testing real y revisión general aplazados por el usuario. E3 parcial/E4 pendiente. Se conservan las filas/evidencia histórica debajo. Nuevos alcances:

| ID | Incremento alpha.2 | Estado y evidencia |
|---|---|---|
| UI-03/UI-04 | Captura/edición/restauración de atajos; rechazo de conflictos; persistencia atómica | Implementado; keymap compilado, ejecución bloqueada por Windows 4551; interacción real pendiente |
| DAT-02 | Recuperación explícita con undo; guardado CAS+lock | Parcial; store/session unitarios; crash/multidocumento/journal/migraciones pendientes |
| DAT-03 | Detectar escritura externa al guardar y rechazar sin pisarla | Parcial; dos stores/JSON incompleto/lock en unitarios; watcher/diff/reconcile pendientes |
| DAT-04 | Historial multistep y rutas estables tras Save As | Parcial; session unitarios; flujo real pendiente |
| AI-01/02/03 | Validación protocolo/proyecto/digest, identidad de retry, diff ampliado, límites | Base interna implementada; permisos/MCP/propuestas/cliente E4 pendientes |
| OPS-01/PKG-01 | Wrapper MSVC portable y templates de guiones | Compilación/expansión controladas; release de fuentes, sin nuevo paquete probado |

Detalles y logs exactos: `evidence/continuacion-05.md`. No reclasificar las pruebas físicas históricamente pendientes como completadas.


Estados: `pendiente`, `implementado-sin-verificar`, `verificado`, `fallo`, `bloqueado`, `opcional-no-incluido`. «Verificado» exige comando/build/caso identificables y artefacto en `implementation/evidence/`.

Build de referencia: `cargo build --release --locked -p transcriptor` (Rust1.98.1, egui0.36.1), 2026-09-08. Workspace inicialmente sin Git; repositorio público inicializado por autorización posterior (ver PUBLISH). Host: `evidence/e0/initial-state.md`. Checkpoint continuación4: evidencia `evidence/e2/continuacion-04.md`; E2 abierta. latest-regression.json apunta a JSON215404 preparados, NO ejecutados; última regresión aceptada160410.

| ID | Requisito | Estado | Implementación / símbolo | Prueba | Evidencia |
|---|---|---|---|---|---|
| UI-01 | Ventana nativa y paquete ejecutable | verificado exe y paquete provisional en host; Windows limpio pendiente | main.rs, build_windows.py | E1 y smoke en TEMP Unicode/PATH aislado | evidence/e2/regression-20260908-160410/, package-smoke-20260908-120555/ (ZIP anterior a continuación4) |
| UI-02 | Diseño y estados usables | implementado-sin-verificar global; capturas reales con controles responsivos y toasts expirados, DPI OS pendiente | ui_panels.rs, ui_timeline.rs | selection-20260908-160410/caches.json y capturas inspeccionadas | evidence/e2/continuacion-03.md; dimensiones reales en logs, no DPI OS |
| UI-03 | Foco, texto y atajos | implementado-sin-verificar | `keymap.rs` (`keymap/1`, AltGr, conflictos), `app.rs::handle_keys` (`egui_wants_keyboard_input`) | `cargo test -p transcriptor` (4 tests) ; pendiente prueba IME/layout ES manual | tests |
| UI-04 | Paridad de acciones V1 | pendiente (inventario iniciado: 71 IDs registrados con defaults V1; handlers para 60; 6 devuelven aviso «pendiente E2/E3») | `keymap.rs::registry`, `app.rs::dispatch` | `keymap::tests::defaults_have_no_conflicts_and_keep_v1_ids` | `v1-action-inventory.md` (pendiente) |
| MED-01 | Import/probe/relink | verificado parcialmente en host: video/audio/imagen/Unicode/relink, rotación90 y worker import asíncrono; cancelación en IO bloqueado pendiente | ffmpeg.rs, import_jobs.rs, app.rs::poll_import, RelinkAsset | tests reales + kittest duplicados/edición concurrente/proyecto cambiado | evidence/e2/async-import.log, media-vfr-rotation.log, vfr-rotation-20260908-215336/ |
| MED-02 | Transporte | verificado (host): 1×/2×/3×/4×, skim 8× sin audio, pausa, seek, frame step exacto, loop IN/OUT, parada al final; política de audio visible. Pendiente: medir sincronía A/V (F-001) | `tv2-media::player`, `audio_policy` | `tests/scripts/e2-transporte.json` (asserts de posición/velocidad) | `evidence/e2/e2-transporte.log`, `transporte-*.json/png` |
| MED-03 | Timeline y gestos | verificado host: drag/cancel/trim/scrub, markers/snap/autoscroll, insert de grupo enlazado y undo; prueba manual pendiente | ui_timeline.rs, ui_markers.rs, timeline_index.rs, commands.rs | gesture_tests.rs, domain tests, guiones E1/E2 | evidence/e2/workspace-final.log, latest-regression.json |
| MED-04 | Composición y mezcla | verificado en host: dos videos+alfa+audio/PiP y mezcla/mute/solo/gain contra fuente, preview y WAV; revisión final integrada E2 pendiente | compositor.rs, AudioMixer, resolve.rs | PSNR 720/1080; mute_solo_and_gain_match_preview_and_pcm_export | evidence/e2/mix-equivalence.log, regression-20260908-160410/ |
| MED-05 | Mapping temporal | verificado en fixtures: flicks, fuera keyframe, TS inicio≈5s, audio+250ms, VFR y rotación; sincronía física pendiente F-001 | decoder/render/audio/time | 40 cuadros exactos contra PTS nativos; 298 exportados con PSNR mínimo28dB | evidence/e2/media-vfr-rotation.log, vfr-rotation-20260908-215336/verify.log, sync-export-measurement.txt |
| LAY-01 | Contratos y estados | verificado con fixture V1 auténtica (aceptado/propuesto/desactivado, `enabled` independiente de `accepted` en recortes, tombstone `item-borrado`, bloques sin aceptación) | `layers.rs`, `tv2-v1compat::{layers,trims}` | `crates/v1compat/tests/golden_v1.rs`, `trims::tests`, `commands::tests::layers_follow_v1_rules` | `tests/fixtures/v1/demo-a`, `evidence/e2/import-v1-*` |
| LAY-02 | Rango/jerarquía | verificado (multirrango + jerarquía en fixture V1; puntos solo en marcas del autor, D-0012; 32 niveles) | `layers::validate_items`, `tv2-v1compat::layers` | `layers::tests`, `golden_v1.rs` | `evidence/e2/import-v1-02-fuente-capas.png` |
| LAY-03 | Montaje V1 | implementado-sin-verificar → verificado parcialmente (aplanado idéntico al calculado por V1 en la fixture: tapado, compactación, repetición; conversión explícita D-0011; export del montaje 7,000 s). Pendiente: revelar en fuente por ocurrencia desde la GUI y comparación de píxeles V1↔V2 (V1 no ejecutado) | `tv2-v1compat::montaje` | `montaje::tests` (porta `test_montaje.py`), `golden_v1.rs` | `evidence/e2/import-v1-01-secuencia.png`, `import-v1-montaje-720p.mp4` |
| LAY-04 | Antes/después de modelos | implementado-sin-verificar (capa manual antes de transcribir verificada; master real pendiente E3) | `Command::CreateLayer`, `app.rs::add_range_to_layer` | guion E1 (`08-estado-capa.json`) | `evidence/e1/08-capa-manual-aceptada.png` |
| DAT-01 | Round-trip V1 | verificado parcialmente: fingerprint V2 == V1 sobre el mismo archivo real, `source_master_digest` igual, round-trip canónico idéntico de `editorial-layer/1` y `editorial-trims/1`; pendiente escritura de vuelta desde la GUI y montaje→V1 (E3) | `digest.rs`, `ffmpeg::v1_inventory_json`, `tv2-v1compat` | `golden_v1.rs`, `layers::tests::round_trip_*`, `trims::tests::round_trip_*` | `tests/fixtures/v1/demo-a/expected-v1.json` |
| DAT-02 | Durabilidad | implementado-sin-verificar (escritura atómica, parcial rechazado; crash en frontera pendiente) | `store.rs::atomic_write` | `store::tests` | tests |
| DAT-03 | Edición externa | pendiente | — | — | — |
| DAT-04 | Undo/redo | implementado-sin-verificar (gesto/batch/nueva revisión/stale verificados en tests y guion; export no borrado por undo por diseño: no hay inversa) | `session.rs` | `session::tests`, guion E1 (undo/redo Aceptar) | `evidence/e1/08-estado-capa.json` |
| AI-01 | Comandos comunes | implementado-sin-verificar (mismo `ProjectSession::execute` para GUI y guion; cliente externo E4) | `session.rs::CommandEnvelope` | guion E1 | — |
| AI-02 | Dry-run y autorización | implementado-sin-verificar (dry-run existe; permisos E4) | `session.rs::dry_run` | `session::tests::failed_batch_leaves_project_untouched_and_dry_run_does_not_commit` | tests |
| AI-03 | Idempotencia/conflictos | implementado-sin-verificar | `session.rs::execute` | `session::tests::stale_base_is_rejected_and_idempotent_retry_replays` | tests |
| AI-04 | Estado en tiempo real | pendiente | — | — | — |
| EXP-01 | Export exacto | verificado parcialmente: cortes/IN-OUT/selección/repetición/VFR/rotación/fronteras; A/V export medido; regresión completa de exe actual pendiente | ExportJob, extract_ranges, decoder | todos178cuadros VFR y120rotados contra PTS nativos; primero/último incluidos | evidence/e2/vfr-rotation-20260908-215336/verify.log, latest-selection.txt; latest-regression.json preparado no ejecutado |
| EXP-02 | Matriz de formatos | verificado host:13 presets reales H264720/1080/2160/vertical,HEVC,AV1,ProRes,VP9WebM,MKVH264,NVENC,WAV,FLAC,MP3; NVENC depende del hardware | export.rs::presets, ffmpeg::Capabilities | export tests reales/ffprobe | evidence/e2/format-matrix.txt, workspace-final.log |
| EXP-03 | Jobs y fallos | verificado parcialmente: cola/snapshot/cancel/fallos; cierre activo sin huérfanos/parciales; repetir smoke tras import worker | CancellableChild, CleanupPaths, shutdown_workers | FFmpeg bloqueado esperando stdin + cierre exe durante4K/cola/cache | evidence/e2/shutdown-smoke.log, shutdown-20260908-173501/, process-cancel-test.log, workspace-import-vfr.log |
| ML-01..03 | Inferencia/etapas/supervisión | pendiente (E5) | — | — | — |
| PERF-01 | Fluidez y recursos | implementado-sin-verificar global: P95CPU UI10k/100k0,424/0,433ms; reservas cache256MiB/cola64/2workers/import asíncrono; RAM total/VRAM/presentación/input/seek/reposo pendientes | resolve.rs, cache.rs, import_jobs.rs, timeline_index.rs | n200/caso release kittest, oráculo resolver anterior; admisión/liberación/reintento cache | evidence/e2/dense-ui-benchmark.log, cache-memory-admission.log, async-import.log |
| OPS-01 | Diagnóstico | implementado-sin-verificar (errores con código/acción, consola con filtro, log rotado, redacción de secretos) | `error.rs`, `console.rs` | — | `%LOCALAPPDATA%\Transcriptor\logs` |
| PKG-01 | Distribución | verificado paquete alpha en host, descarga FFmpeg separada; E6/licencias finales/Windows limpio V2 pendientes | build_windows.py --public-release, Install-FFmpeg.ps1, VERIFY | 575hashes, instalación idempotente, PSNRfuente42,6–42,8/visor30,6/hueco0 | evidence/e2/package-smoke-20260908-181823/, publication-build-final.log, PUBLISH.md |

## Requisitos descubiertos en V1 (se amplían en E2/E3)

| ID | Requisito | Estado |
|---|---|---|
| V1-KM-01 | 67 acciones `keymap.py` con IDs y defaults | implementado-sin-verificar (registro + 4 acciones nuevas: copiar/cortar/pegar/duplicar, ripple, `layers.add_range`, `file.*`) |
| V1-MT-01 | `editorial-montaje/1` insert/overwrite/ripple/split/trim semántica | implementado-sin-verificar sobre el modelo V2 (`commands::tests` reproduce `test_montaje.py` para add/split/trim/move/remove) |
| V1-IO-01 | `digest_json` y `fingerprint` compatibles | verificado (goldens Python 3.14) |
