# Continuación 4 — checkpoint solicitado por el usuario

Fecha 2026-09-08. Objetivo vigente al reanudar: terminar E0–E3. Pausa explícita, E2 abierta. Resumen autoritativo: ../../STATUS.md. No se completó E3 ni se inició E4/E5.

## Cambios propios y archivos

- `crates/media/src/process.rs` **nuevo**: propiedad de Child, watcher con token cada20ms, wait sin mutex bloqueado, Drop kill/wait/join. output drena stdout/stderr por separado; prueba real FFmpeg esperando entrada cancelado.
- `crates/media/src/{audio,decoder,render,player,export,cache,ffmpeg,lib}.rs`: decoders/probe/import cancelables, propagación a mixer/renderer/player/export, staging/audio con limpieza RAII, workers cache esperados al salir. ExportJob::run recibe &Arc<AtomicBool>.
- `crates/domain/src/resolve.rs`: barrido de eventos y BTreeSet activo; igualdad exacta con algoritmo original conservado bajo cfg(test). Complejidad O(nlogn + salida); benchmark de UI densa aprovecha este cambio.
- `cache.rs`: 256MiB de reservas, 64 pendientes, 2 workers, carga/pruning disco existentes; reserva en Arc hasta último consumidor, reintento tras rechazo, cotas de buckets para waveform. No mide ni limita toda la memoria de procesos/GPU.
- `decoder.rs`: noaccurate_seek + fps round=up evita cuadro futuro VFR. Oráculo usa ffprobe PTS y decodificación nativa sin seek/fps, compara píxeles exactos. Rotación90 compara contra -noautorotate + transpose=cclock.
- `apps/desktop/src/import_jobs.rs` **nuevo**, `main.rs/app.rs/ui_panels.rs/scripting.rs/gesture_tests.rs`: worker de import, cola y cancelación visible; UI aplica comandos al completar. Identidad del proyecto y token evitan incorporar resultados tras abrir/nuevo; rutas portables se calculan al completar contra store vigente. Duplicados se comparan entonces, preservando ediciones concurrentes. Guion espera cola y marca fallo de importación. Test prepara la fixture esperando el mismo recorrido asíncrono.
- `app.rs`: RunningExport propietario del JoinHandle; shutdown_workers limpia cola y espera import/export/cache/player. Test de cierre con export4K activo y pendiente, sin salida ni staging.
- `tests/scripts/shutdown_smoke.ps1`, `inspect_vfr_seek.py`, `verify_vfr_rotation.py` **nuevos**. Guion nativo VFR/rotación guardado en vfr-rotation-20260908-215336/run.json; fue preparado por Python inline, aún no existe prepare_vfr_rotation.py. Para repetir, clonar el JSON con destinos nuevos (RUN.md).
- Documentación reemplazada en STATUS/RUN/anexo y filas de matriz; decisiones D-0022–D-0025. Cambios de continuación3 conservados y explicados en continuacion-03.md. Manifiesto continuacion-04-files.json conserva hashes de fuentes y exe al corte.

## Resultados y límites de prueba

| Caso / comando | Resultado | Artefacto |
|---|---|---|
| cargo test --workspace --locked | 91 pasan, 1 ignorado (benchmark explícito aparte) | workspace-import-vfr.log |
| cargo clippy --workspace --all-targets --locked -- -D warnings | Limpio tras sustituir chunks_exact por as_chunks en test | clippy-import-vfr.log |
| cargo build --release --locked -p transcriptor | Correcto43,61s, exe21.505.536bytes | build-import-vfr.log |
| cargo test -p tv2-media --locked -- --nocapture | 28 pasan, 13 presets reales incluidos | media-vfr-rotation.log |
| cargo test -p transcriptor --release dense_ui_benchmark --locked -- --ignored --nocapture | 10k/100k, n200, P95CPU0,424/0,433ms | dense-ui-benchmark.log |
| cargo test -p transcriptor async_import --locked -- --nocapture | edición concurrente/duplicados/cambio proyecto pasan | async-import.log |
| tests/scripts/shutdown_smoke.ps1 | 1,357s; sin descendientes propios ni parciales; build anterior a VFR/import | shutdown-smoke.log, shutdown-20260908-173501/ |
| exe --script vfr-rotation-20260908-215336/run.json | fallos=false; VFR178frames, rotación120frames | run.log, exports, PNG |
| python -X utf8 tests/scripts/verify_vfr_rotation.py <carpeta anterior> | todos los frames contra PTS nativos; min43,355/30,843dB, visor31,564–46,318dB | verify.log |

La suite completa precedió dos ajustes pequeños finales: iterador equivalente en test y propagación de fallo de import al guion. Clippy/build y recorrido nativo sí son posteriores. No se ejecutó otra suite completa después de ellos. Un primer test VFR pedía un cuadro exactamente en EOF5,933333; se corrigió el caso a seek5,75. vfr-native-pts.log conserva ese intento fallido; la prueba corregida pasa en media-vfr-rotation.log/workspace-import-vfr.log. Conservar histórico, no citar el log fallido como aceptación.

Experimento antes del fix: seis seeks entre cuadros devolvían PTS futuro con accurate_seek, hasta100ms adelante. Se consultó documentación oficial FFmpeg -ss/accurate_seek/autorotate: https://ffmpeg.org/ffmpeg.html. Filtro round=up no adelanta la vigencia del siguiente PTS; preroll negativo se descarta en fps. Audio mantiene accurate seek. La fixture VFR tiene duración real5,933333s, no6s redondos.

Revisión funcional: cancelación, cache admission, import y VFR/rotación comprobados. Revisión técnica: reserva hasta último handle, proyecto cambiado, revisión concurrente, canales/procesos y oráculo independiente. Revisión de experiencia parcial: rot90.png inspeccionada, orientación y thumbnails correctos; toasts acumulados cubren controles. Repetir captura tras expirar toasts y revisar presentación. Estas pasadas de incremento NO cierran las tres revisiones de E2 completa.

## Estado al parar

No procesos del workspace observados activos al corte. V1 limpio, mismo HEAD. Build y logs terminados. No hubo nueva petición de control del escritorio. No se modificaron datos personales ni references.

`latest-regression.json` apunta a regresión215404 SOLO PREPARADA. Ejecutarla es la primera acción. La última regresión E1/E2 completamente aceptada es160410. ZIP160510 probado anteriormente NO contiene continuación4. No se reconstruyó paquete nuevo. El usuario pidió documentar y detenerse, por lo que no se lanzaron más recorridos de implementación.

Pendientes E2/E3 detallados en STATUS y anexo. Riesgos nuevos a revisar: cotas de output ffprobe, cancelación en IO del fingerprint, import V1 síncrono, toasts superpuestos y prueba negativa de import en modo script. No se convierten limitaciones físicas A/V/DPI/IME en verificado por pruebas lógicas.
