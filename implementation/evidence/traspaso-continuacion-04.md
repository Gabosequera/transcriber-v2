## Traspaso al siguiente agente — continuación 4, 2026-09-08

**Estado autoritativo: E0 cerrada; E1 cerrada en host; E2 abierta; E3 pendiente; E4–E6 no aceptadas.** El usuario pidió detener implementación y conservar progreso; después autorizó commit/push/release de V1 y crear repositorio público V2 con licencia propietaria. Esa publicación es un encargo separado y no cierra E2/E3. Leer `implementation/STATUS.md` y, para el resultado de publicación, `implementation/PUBLISH.md`. No reiniciar lo hecho por Fable/Astra.

### 1. Dónde quedó el trabajo, con evidencia por entrega

Workspace `G:/TODO/transcriptor-v2`; no tenía Git durante implementación. V1 HEAD de referencia `c3677ba568cfc7d5947ec4695c42fffef6d79e03`, árbol limpio al checkpoint; la autorización posterior de publicación permite cambios de release en V1. Sin AGENTS.md aplicable encontrado. Estado Git final en PUBLISH.md.

| Entrega | Estado | Evidencia real |
|---|---|---|
| E0 | Cerrada | implementation/evidence/e0/initial-state.md y experimento-multimedia.md |
| E1 | Cerrada en host | implementation/evidence/e2/regression-20260908-160410/e1-recorrido/;12s360frames, PSNR42,4–42,9dB |
| E2 | Abierta | continuacion-03.md y continuacion-04.md, pruebas de mezcla/selección/cache/cola/cierre/VFR/rotación/import; requisitos pendientes abajo |
| E3 | Fundamentos parciales, sin cerrar | v1compat, domain layers/commands, application session/store; master completo, chunks, export V1, atajos y durabilidad pendientes |
| E4–E6 | Pendientes | bases en bloques3/5, paquete provisional no implica cierre |

Exe al checkpoint `G:/TODO/transcriptor-v2/target/release/Transcriptor.exe`,21.505.536bytes, SHA256 `d7e4273f1c1bd7b33374a7f3ccf90b126ba58ecd84012495f2dbf3e2cb79ad6b`. Rust1.98.1/egui0.36.1/FFmpeg8.0.1. build-import-vfr.log y clippy-import-vfr.log correctos. workspace-import-vfr.log: **91 pasan +1 ignorado**, benchmark ejecutado explícitamente. Suite precede dos ajustes finales menores (iterador de test y error de import en guion); Clippy/build/recorrido nativo son posteriores.

Continuación4 implementó y probó: cierre de procesos propios y limpieza staging; resolver por barrido; reserva de caché256MiB/64pendientes; seek VFR correcto; rotación90; importación asíncrona con cancelación, deduplicación y protección al cambiar proyecto. `vfr-rotation-20260908-215336/verify.log`: todos178cuadros VFR y120rotados pasan, mínimos43,355/30,843dB, visor31,564–46,318dB. `shutdown-20260908-173501/`:1,357s, sin huérfanos/parciales (build previo a VFR/import). `dense-ui-benchmark.log`: n200, P95CPU0,424/0,433ms en10k/100k; no GPU. `async-import.log` y `cache-memory-admission.log` pasan. rot90.png inspeccionada: orientación correcta, toasts cubren controles; repetir captura limpia.

F-008 y F-009 de continuación3 siguen cerrados: paths demo correctos y export estricta ante assets requeridos ausentes/decoders fallidos. Selección/tramos todas las ocurrencias:24s/720frames y2s/60frames, PSNR42,7–43,0dB. Mezcla/mute/solo/gain contra fuente/preview/WAV, error≤0,000015258789.

**Primera acción de implementación al reanudar:** ejecutar `regression-20260908-215404/`, SOLO PREPARADA. latest-regression.json apunta allí, no a una prueba aceptada. Última completa aceptada160410. ZIP160510 anterior a continuación4; última smoke aceptada package-smoke-20260908-120555 (499hashes,14s420frames, PSNRfuente42,6–42,8/visor30,6,hueco0). Publicación posterior debe informar un artefacto actualizado en PUBLISH.md; no confundir ZIP antiguo con exe actual. ZIP fallido105323 histórico no entregable.

### 2. Cómo compilar y ejecutar con rutas reales, guiones y verificador

Guía actualizada: `G:/TODO/transcriptor-v2/implementation/RUN.md`.

```powershell
Set-Location G:/TODO/transcriptor-v2
& C:/Users/gabri/.cargo/bin/cargo.exe test --workspace --locked
& C:/Users/gabri/.cargo/bin/cargo.exe fmt --all --check
& C:/Users/gabri/.cargo/bin/cargo.exe clippy --workspace --all-targets --locked -- -D warnings
& C:/Users/gabri/.cargo/bin/cargo.exe build --release --locked -p transcriptor
& G:/TODO/transcriptor-v2/target/release/Transcriptor.exe
```

FFmpeg en packaging/third-party/ffmpeg; resolución TRANSCRIPTOR_FFMPEG_DIR → junto al exe → workspace → PATH. RUN.md tiene comandos PowerShell completos para ejecutar los JSON preparados desde TEMP con Start-Process -WindowStyle Hidden y variables TRANSCRIPTOR_CONFIG_DIR/CACHE_DIR/LOGS_DIR aisladas. Los logs se escriben junto al JSON. Exigir fallos=false, inspeccionar capturas y ejecutar:

```powershell
$runs = Get-Content implementation/evidence/e2/latest-regression.json -Raw | ConvertFrom-Json
python -X utf8 tests/scripts/verify_export.py (Join-Path (Split-Path $runs.'e1-recorrido') 'export-e1-720p.mp4') $runs.'e1-expect'
python -X utf8 tests/scripts/verify_export.py (Join-Path (Split-Path $runs.'e2-escenario1') 'escenario1-720p.mp4') $runs.'e2-expect-escenario1'
python -X utf8 tests/scripts/verify_export.py (Join-Path (Split-Path $runs.'e2-escenario1') 'escenario1-1080p.mp4') $runs.'e2-expect-escenario1'
python -X utf8 tests/scripts/verify_vfr_rotation.py ((Get-Content implementation/evidence/e2/latest-vfr-rotation.txt -Raw).Trim())
./tests/scripts/shutdown_smoke.ps1
```

`prepare_regression.py` y `prepare_selection.py` generan destinos nuevos. VFR tiene run.json reproducible guardado, no prepare_vfr_rotation.py; copiar JSON cambiando destinos para repetir, esperar5500ms antes de screenshots. Otros guiones e2-transporte/e2-import-v1/e2-caches. Demo de workspace: implementation/evidence/e2/cache-markers-demo.transcriptor/project.json. Benchmark: cargo test -p transcriptor --release dense_ui_benchmark --locked -- --ignored --nocapture.

Paquete local: python -X utf8 packaging/build_windows.py, luego tests/scripts/package_smoke.ps1. Builder deja package-build.json; VERIFY.ps1 e INICIO.md dentro. No sustituir validación visual/media por exitcode. Publicación y licencias revisadas en PUBLISH.md. Goldens: tests/fixtures/v1/make_v1_fixture.py ejecuta copias aisladas de módulos V1. Medios: tests/fixtures/media/make_fixtures.py; no regenerar fixture-a innecesariamente (fingerprint golden depende de bytes).

### 3. Mapa de arquitectura implementada por crate

- **domain**: flicks705600000/s, IDs, assets/secuencias/clips/pistas, linked A/V, layers/items/jerarquía/multirrango/estados/tombstones/markers. Comandos/batches atómicos, Reject/Overwrite/Insert y ripple explícito. ResolvedTimeline común por barrido de bordes; extract_ranges une/concatena selección preservando mapping y todas las pistas.
- **application**: ProjectSession con revisión monótona, actor/base/idempotencia/dry_run; undo/redo como revisión nueva. ProjectStore project.json atómico + journal.jsonl; rutas relativas al **padre** de .transcriptor. Recovery/transacciones multidocumento/edición externa aún no aceptados.
- **media**: FFmpeg/FFprobe por procesos propios cancelables; compositor CPU y mezclador propios comunes a preview/export. VideoDecoder VFR, AudioDecoder/atempo/offsets, TimelineRenderer, player propietario y salida cpal/WASAPI. cache.rs workers/LOD/disco/pruning/cancelación/reserva256MiB. export.rs staging/ffprobe/capacidades13presets/rechazo de decoders fallidos. process.rs supervisión/kill/wait; output ffprobe aún necesita revisión de cotas globales.
- **v1compat**: master raw conservado en adaptador, layers/trims import/export y montaje con **flatten portado literalmente antes de flicks**. read_v1_editorial produce comandos; fingerprint/digest/estados/puntos/flatten con goldens auténticos. Persistencia/proyección completa master, export GUI y montaje inverso pendientes E3.
- **desktop**: eframe/wgpu, dispatch/exec_checked comunes UI/atajos/script. ImportJobs nuevo: un worker y64pendientes, UI aplica resultado con identidad/token/dedup actuales. Cola export16pendientes, un worker, snapshot fijo e historial limitado; shutdown espera jobs. Gestos por estado del puntero, índice intervalos/snapedges, media visible y256texturas, markers/lista/editor. Modo --script captura exe real y espera import; gesture_tests usa egui_kittest. keymap/console/paneles responsivos existentes.
- Fable aportó base cachés/presets/capacidades/tests y política ShiftClips; Astra continuó integración y correcciones. Conservar procedencia en reuse-ledger.md.

### 4. Nueve hallazgos que ahorran investigación

1. **egui0.36/paneles**: App::ui/Panel/global_style/dropped_files cambian; painter no reserva altura, set_min_height evita encogimiento. horizontal_wrapped evita recortes del transporte. eframe guarda app.ron por app_id independientemente de TRANSCRIPTOR_CONFIG_DIR; zoom2 no acredita DPI OS200%.
2. **Gestos por puntero**: primary_pressed/down/released y press_origin evitan umbral tardío de drag_started. Confirmar una sola transacción al release; autoscroll ajusta origen temporal. kittest cubre drag/cancel/snap/insert/cola.
3. **Dos defectos históricos del player**: comandos perdidos durante espera corregidos con carry; generaciones de seek/audio divergentes corregidas con contador único de salida. F-001 de sincronía física sigue abierto. Nuevo cierre propaga cancel y espera procesos propios.
4. **Grupos al dividir**: derecha usa grupo determinista independiente de izquierda; A/V derechos siguen enlazados. Insert ordena tracks antes de dedup. MoveClip individual conserva contrato individual.
5. **Compatibilidad exacta V1**: fingerprint size+3muestras de8MiB+inventario Python; digest canonical y repr floats Python; puntos solo autor; flatten mantiene EPS1e-3 y round(x,3) antes de flicks. Goldens generados por V1, nunca por V2.
6. **FFmpeg/fixtures**: drawtext requiere escapado; alfa con color+pad. TS -ss ya relativo al inicio, no sumar start_time; audio offset usa aresample:first_pts=0 antes de atempo. FFmpeg8 rotación se fija con display_rotation al copiar streams. VFR real dura5,933333s; accurate_seek descartaba cuadro vigente: noaccurate_seek+fps round=up corrige. Build gyan GPLv3; licencia propietaria del producto no reemplaza licencias ajenas.
7. **Verificación sin OCR**: PSNR y controles de fuente, oráculo PTS sin fps/seek para VFR, transpose explícito para rotación. Todos298cuadros exportados contrastados; sync export de destellos/pulsos da0,104ms, audio lógico1–4× máximo25,9ms. Nada de ello certifica pantalla/altavoces físicos.
8. **Control de escritorio denegado** en sesión previa, sin nueva solicitud/denegación aquí. --script/capturas/kittest permiten avanzar; foco/IME/layouts/DPI real y A/V físico siguen pendientes.
9. **Herramientas/paquete**: usar python -X utf8 y encoding explícito (cp1252 falla con español). ZIP timestamps<1980 requieren strict_timestamps=False; publicar desde parcial. Demo abre aunque falten medios: comprobar cachés/píxeles/export. Paths relativos al padre de .transcriptor. Inventario293crates incluye dev/opcionales y avisos faltantes declarados: auditar distribución. No confundir puntero latest preparado con evidencia aceptada ni ZIP viejo con exe actual.

### 5. Qué sigue en orden, y qué ya existe

1. **E2, integración inmediata**: ejecutar regresión215404 preparada con exe actual; repetir shutdown integrado y negativos de import en guion. Capturas limpias tras expirar toasts. VFR/rotación/barrido/import asíncrono/reservas cache ya hechos: no rehacer investigación.
2. **E2 restante**: RAM total/VRAM/colas y cache de medios largos, apertura/cierre/seek repetidos, CPU y presentaciónGPU, latencias input/seek frío-caliente/reposo; A/V físico bajo carga/cambio velocidad F-001; DPI OS100/150/200/foco/IME/layouts F-002. Cachés waveform/thumbnails, markers, snapping, autoscroll, insert enlazado, cola/cancelación y13formatos están implementados/probados en los alcances anotados. Completar tres revisiones funcional/técnica/experiencia antes de cerrar.
3. **E3**: import/export V1 completos desde GUI; master/transcripción/palabras/hablantes/señales protegidos; chunks contiguos/temas/jerarquía/multirrango/carriles/comentarios/evidencia/derivación y protección humana. layer_to_v1/trims_to_v1 existen; montaje inverso desde provenance.v1_clip_id/extra.v1.seq_ini_placed pendiente. Revelar fuente y volver a cada ocurrencia. Inventario botones/gestos V1 y atajos configurables completos (registry/save_overrides base). Autosave/recovery/migraciones/atomicidad lógica multidocumento/crash testing/watcher/diff/reconciliación/sin lost update; project.json atómico/journal básicos no cierran DAT-02/03. Revisar import_v1_folder síncrono. No exigir SQLite sin evidencia.
4. **E4**: transporte externo, capabilities/queries paginadas/dry_run/apply/eventos/permisos/propuestas/requests-responses y cliente real. Envelopes/revisión/idempotencia existen; flujo externo completo no.
5. **E5**: workers Python NDJSON y pipelines core/align/laughter/prosodia/escena_audio/editorial_pipeline, runtimes/modelos separados, progreso/cancel/resume/digest/output→comandos. No modelos integrados actualmente.
6. **E6**: distribución reproducible/licencias transitivas/procedencia/diagnóstico/tamaños/Windows limpio/otro hardware. Builder/VERIFY/demo/guía/smoke host existen. Nueva publicación expresamente autorizada no equivale a estabilizaciónE6. Linux no probado.

### 6. Reglas que deben mantenerse

- V1 y datos personales solo lectura para implementación V2; excepción explícita posterior: commit/push/release V1 autorizados por el usuario, documentar cambios en PUBLISH.md. Comparaciones que escriben siempre sobre copias aisladas en V2.
- No copiar código/assets de references; conservar procedencia y licencias de terceros. V2 pasa a todos los derechos reservados por instrucción expresa posterior; V1 conserva PolyForm NC.
- Verificado exige caso/comando/build/artefacto; goldens generados con código V1. No equiparar mocks/compilación/zoom/audio lógico a evidencia física.
- Conservar Fable/Astra/evidencia histórica; un único anexo actualizado, no un segundo producto. Actualizar STATUS/matriz/evidencia antes de parar.
- E0–E3 sigue siendo objetivo de implementación al reanudar. La pausa y una release alpha no completan los criterios pendientes.
