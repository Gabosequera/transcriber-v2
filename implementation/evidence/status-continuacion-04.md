# STATUS — Transcriptor V2

Checkpoint: **2026-09-08, continuación 4, detenido por petición expresa del usuario**. El encargo vigente al reanudar es completar **E0–E3** conforme a los tres prompts. **E0 cerrada; E1 cerrada en host; E2 abierta; E3 pendiente de completar; E4–E6 fuera del objetivo inmediato.** Esta pausa no es un cierre de entrega ni un bloqueo técnico general.

Leer este archivo, `evidence/e2/continuacion-04.md`, la matriz y el anexo de seis bloques al final de `prompts/00-CONSTRUIR-TRANSCRIPTOR-V2.md`. Sustituyen las instrucciones de continuidad anteriores; la evidencia histórica se conserva.

## Workspace y estado del proceso

Raíz `G:/TODO/transcriptor-v2`: no tenía Git durante la implementación; se inicializó `main` para la publicación posterior autorizada. Remoto nuevo público: `https://github.com/Gabosequera/transcriber-v2`. V1 `G:/TODO/transcriptor-installer-v0.2.0` estrictamente solo lectura: HEAD `c3677ba568cfc7d5947ec4695c42fffef6d79e03`, `git status --porcelain=v1` vacío al corte. Durante implementación no se modificaron V1 ni references. Después, por autorización expresa de commit/push/release, se añadió documentación de release en V1: commit bb7012c y tag v0.4.0 publicados; no se modificaron datos personales. Sin AGENTS.md aplicable encontrado. Se conservaron los avances de Fable y Astra; no reiniciar E0/E1.

Al corte no se observaron procesos de Transcriptor/cargo/rustc/FFmpeg/Python del workspace activos. No se mató ningún proceso. El usuario pidió parar y documentar: la implementación permanece pausada. El encargo posterior de publicar ambas versiones está terminado y se registra en PUBLISH.md.

## Terminado en esta continuación

1. **Cierre y cancelación**: procesos propios con kill/wait y watcher cancelable; cierre espera export/cache/player; limpieza RAII de staging/audio. Cancelación propagada a decoders y mixer de export. Prueba real de FFmpeg bloqueado esperando stdin y cierre nativo durante export 4K + cola/cachés, sin huérfanos ni parciales.
2. **Resolver eficiente**: barrido de bordes O(n log n + salida) y conjunto activo; se conserva algoritmo anterior solo en tests como oráculo. Benchmark release UI kittest con 10k/100k clips, 200 frames por caso: P95 CPU 0,424/0,433 ms. No mide presentación GPU.
3. **Presupuesto de caché**: 256 MiB de reservas compartidas para buffers CPU de waveform/thumbnails; 64 pendientes, 2 workers. Reserva hasta el último consumidor, rechazo explicativo y reintento al liberar; prueba real de recuperación. No es límite de RAM de FFmpeg ni de VRAM.
4. **VFR corregido y rotación verificada**: `-noaccurate_seek` + `fps=start_time=0:round=up` conserva el cuadro vigente al buscar entre PTS. Ocho seeks/40 cuadros contrastados píxel a píxel con decodificación nativa sin seek/fps; rotación90 contra transpose explícito. Ejecutable exportó 178 cuadros VFR y 120 rotados; todos contra PTS nativos, PSNR mínimo 43,355/30,843 dB. Visor/export también pasa. Audio conserva seek exacto.
5. **Importación asíncrona**: sondeo/fingerprint en un worker cancelable; cola máxima 64 pendientes. La UI aplica ImportAsset al completar, comprueba duplicados contra estado actual, conserva ediciones concurrentes y descarta resultados de otro proyecto/cancelados. Indicador/cancelar en biblioteca; --script espera la cola. Kittest cubre duplicados, edición concurrente y cambio de proyecto.

Se conservan los cierres **F-008** (demo portable) y **F-009** (export estricta ante medios ausentes/decoders fallidos), selección de clips/tramos con todas sus ocurrencias, mezcla/mute/solo/gain, cachés/markers/snapping/insert enlazado/cola y 13 presets de las continuaciones anteriores. Detalles en continuacion-03.md y matriz.

## Build y evidencia exactos

Rust1.98.1, egui/eframe0.36.1, FFmpeg8.0.1 gyan GPLv3, Windows11/RTX4070; hardware completo en evidence/e0/initial-state.md.

**Exe del checkpoint de implementación (reemplazado después solo por texto de licencia; ver publicación abajo)**: `G:/TODO/transcriptor-v2/target/release/Transcriptor.exe`, 21.505.536 bytes; SHA256 `d7e4273f1c1bd7b33374a7f3ccf90b126ba58ecd84012495f2dbf3e2cb79ad6b`. `build-import-vfr.log` correcto (43,61 s), `clippy-import-vfr.log` limpio con -D warnings.

- `workspace-import-vfr.log`: **91 pruebas pasan + 1 ignorada** (desktop17, application6, domain25, media28, v1compat11+4goldens). El benchmark ignorado se ejecutó explícitamente en release. Tras esta suite solo se ajustaron la forma equivalente de iterar en un test para Clippy y marcar fallos de importación en el guion; Clippy/build y recorrido nativo posteriores pasan. No atribuir a esa suite una nueva ejecución tras esos dos cambios.
- `media-vfr-rotation.log`: 28 tests reales de media, incluyendo 13 presets; oráculo PTS/rotación, offsets/sync y negativos.
- `latest-vfr-rotation.txt` → `vfr-rotation-20260908-215336/`: run.json/run.log/verify.log, dos exports y frames/capturas reales, `fallos=false`, verificador OK. Se inspeccionó rot90.png: orientación correcta; varios toasts tapan controles. **Repetir captura dejando expirar toasts**, no presentar esa imagen como revisión visual limpia.
- `latest-shutdown.txt` → `shutdown-20260908-173501/`, `shutdown-smoke.log`: cierre 1,357 s, procesos propios observados sin huérfanos/parciales. Esta smoke usó el build de cierre anterior a VFR/import asíncrono; repetir al integrar release final.
- `dense-ui-benchmark.log`: frío 5,663/80,983 ms (10k/100k), P95 0,424/0,433 ms, 200 frames/caso. Solo CPU de kittest.
- `cache-memory-admission.log`, `cache-budget-tests.log`, `process-cancel-test.log`, `async-import.log`: pruebas específicas pasan.

**Atención al puntero de regresión:** `latest-regression.json` ahora apunta a `regression-20260908-215404/`, cuyos JSON se prepararon justo antes de la pausa y **NO se ejecutaron**. La última regresión completa E1/E2 aceptada sigue en `regression-20260908-160410/` (E1 12s/360frames, PSNR42,4–42,9; E2 visor/export34,9–37,9). No confundir "latest preparado" con "verificado".

Última selección verificada: `selection-20260908-160410/` (24s/720frames y tramos repetidos2s/60frames), `mix-equivalence.log` máximo error0,000015258789.

## Paquete histórico y publicación posterior

ZIP histórico probado en host: `G:/TODO/transcriptor-v2/dist/Transcriptor V2 prueba ñ 20260908-160510.zip`, SHA256 `af2503b08f867bdf0a3fbb6470e49280be857028e16425e40a33446223d8a02f`. **Es anterior a todos los cambios de continuación4; no contiene el exe actual.** ZIP85.541.598bytes; expandido225.633.502bytes. `package-smoke-20260908-120555/`: 499 hashes, export14s/420frames, PSNR fuente42,6–42,8/visor30,6, hueco negro0. Windows limpio/licencias finales E6 pendientes. ZIP fallido105323 histórico, no entregable.

## Primera acción al reanudar y orden de trabajo

1. Ejecutar los JSON ya preparados en `regression-20260908-215404/` con el exe actual y verificar E1 y E2 720/1080p; inspeccionar capturas/logs. RUN.md contiene comandos. No hace falta volver a generar destinos.
2. Repetir shutdown_smoke.ps1 con import worker integrado; cubrir cancelación/import fallida en guion y recuperación de cola. El sondeo V1 dentro de import_v1_folder y los diálogos de archivo siguen síncronos: auditar al avanzar E3. `CancellableChild::output` acumula stdout/stderr para ffprobe; revisar cotas para entradas adversas antes de declarar recursos globales acotados. Fingerprint lee tres muestras de hasta8MiB; no certifica cancelación de IO de disco/red colgado.
3. Cerrar pendientes E2: medición RAM total/VRAM/colas con medios largos y apertura/cierre/seek repetidos; presentación GPU/latencia de input/seek frío-caliente/reposo; sincronía física A/V y cambios de velocidad F-001; DPI OS100/150/200, foco/IME/layouts F-002. El control del escritorio fue denegado en sesión anterior; aquí no hubo nueva solicitud/denegación. --script/kittest/zoom no sustituyen esas pruebas. Revisar toasts superpuestos, capturas limpias y las tres revisiones obligatorias de entrega.
4. Rebuild/paquete nuevo + smoke desde TEMP Unicode cuando el código integrado esté validado; no entregar el ZIP viejo como build nuevo.
5. **E3**: contratos V1 completos (export GUI y montaje inverso), master/palabras/hablantes/señales protegidos; chunks contiguos/jerarquía/multirrango/comentarios/evidencia/derivación y decisiones humanas; revelar y volver a cada ocurrencia; inventario V1 y atajos configurables; autosave/recovery/migraciones/atomicidad lógica multidocumento, crash testing, reconciliación de edición externa y ausencia de lost update. Hay bases, no entrega aceptada. Continuar luego hasta cerrar E0–E3; E4–E6 quedan documentadas en anexo.

No cambiar etiquetas a "verificado" o "cerrada" para eludir pruebas pendientes. Actualizar STATUS antes de cualquier nueva interrupción.

## Publicación posterior expresamente autorizada

V1 v0.4.0 publicada, CI Linux/Windows e instalación desde cero correctas; ZIP remoto contrastado con manifiesto y82hashes. V2 público, licencia propietaria/todos los derechos reservados por elección del titular, V1 mantiene PolyForm NC. Se completaron avisos de24crates; el ZIP público excluye FFmpeg binario y ofrece descarga oficial separada con hashes. El primer paquete público pasó smoke tras explicitar pausa/seek y comprobar posición. El paquete final con licencia en Acerca de pasó smoke181823:575hashes, descarga/idempotencia FFmpeg, export14s420frames, fuente42,6–42,8dB/visor30,6dB/hueco0. Exe actual SHA2562eb8464f6800dd84cb111c942115a6658df85dbb92a11a5aad64864030fdaabc. ZIP final6278e97df38597b7f7809bb61c54c9909f9439264ef1a6fe38420ddb8ddab1f5. Resultado final, commits, URLs y hashes en PUBLISH.md: esta sección no sustituye ese registro ni cierra E2/E3/E6.

**PUBLICACIÓN TERMINADA:** V1 v0.4.0 y V2 v2.0.0-alpha.1 publicadas, commits y releases en PUBLISH.md. V2 tag5cd55a7; ZIP y checksum verificados en GitHub. Usuario reiteró parar tras publicar: no seguir desarrollo hasta nuevo mensaje. Traspaso conservado; E2 abierta/E3 pendiente.
