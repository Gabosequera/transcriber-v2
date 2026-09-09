# Decisiones de continuación 7

## D-0035 · Import editorial asíncrono y estricto

Un worker de lectura/probe prepara datos; GUI comprueba identidad y revisión capturadas y confirma todo en un batch externo protegido. Fallos de documentos reconocidos abortan, frente a importación parcial con avisos. Riesgo: edición concurrente exige repetir y clones grandes siguen en GUI. Prueba: import V1 inválido + test de worker compilado sin FFmpeg; aplicación no se ejecutó físicamente.

## D-0036 · Edición y portapapeles por comandos comunes

SetItemStructure cambia padre/rangos simultáneamente para evitar jerarquías transitoriamente inválidas; editor conserva borrador y base. PasteClips/PasteItems conservan propiedades y crean IDs nuevos, con un paso de undo. Copiar un árbol desliga sus raíces externas; conserva padres internos y descendientes. Alternativa rechazada: reconstrucción parcial de AddClip que perdía efectos y aceptación. Tests application de colisión/rollback/jerarquía/comentario/multirrango.

## D-0037 · Recibos y autosave auditado

Journal incorpora recibo opcional con proyecto/solicitud/resultado para cada clave idempotente. Reapertura valida coherencia; legacy sin recibo reserva la clave y falla retry explícitamente. No reproducir comandos para obtener IDs históricos. Autosave/1 combina proyecto y auditoría en un único replace; legacy project/1 sigue legible. Worker congela snapshot y eventos; guardado manual/apertura permanecen síncronos. Tests reopen, corrupción, retry, recuperación/undo y guardado posterior.

## D-0038 · Export V1 limitado por reversibilidad

Export documental GUI user/topics/ai; montaje sin cambios conserva original íntegro y digest de la proyección V2. Cambios de tiempo, efecto, audio o clips rechazan el inverso; no se simula reconstrucción del material tapado desde el flatten. La alternativa de descartar clips invisibles contraviene DAT-01. Riesgo: alcance limitado y duplicación de JSON; falta export de carpeta/montaje editado. Tests pérdida de precisión, original con campos desconocidos/disabled/oculto y mutaciones rechazadas.

---

# Decisiones de implementación

Registro breve de decisiones rutinarias y reversibles. Las que cambian GUI, motor, persistencia o compatibilidad enlazan un ADR. Formato: ID, fecha, decisión, alternativas, riesgo, prueba que lo demuestra.

## D-0030 · 2026-09-08 · Validación de snapshots y evidencia original

Validar estructura al cargar, recuperar y confirmar previews/comandos; IDs/referencias antes de operaciones temporales, invariantes de composición/capas y digest/identidad del master. Master V1 completo en Project.masters; proyecciones derivadas de solo lectura con registro original en extra.evidence. Se rechaza reemplazo de evidencia incorporada. Alternativa de editar las proyecciones descartada por divergencia. Schema project/1 se amplía con campo opcional; proyectos anteriores sin masters siguen legibles. No es sistema general de migraciones. Riesgo: coste de clones/hash en grandes masters, medir/optimizar pendiente. Tests validation (compilados, ejecución bloqueada), master/import y protection.

## D-0031 · 2026-09-08 · Intención durable para proyecto y auditoría

Publicar .pending-commit.json con snapshot/base/eventos antes de reemplazar project.json y journal.jsonl. Load completa idempotentemente si disco coincide con base o destino; un tercer contenido produce conflicto. Journal deduplica command_id iguales, rechaza contenido distinto/corrupción interna y solo repara cola incompleta. GUI drena eventos únicamente tras commit exitoso. Alternativa de append después de guardar perdía auditoría ante crash/error (after_change aún drenaba prematuramente, pese al texto de continuación 5). Prueba: store::recovery_finishes_each_commit_boundary_once_and_rejects_external_writes. Límite: lock cooperativo, no pérdida de energía/multidocumento V1/idempotencia durable. Reescritura journal limitada a 64 MiB; archivado pendiente.

## D-0032 · 2026-09-08 · Reconciliación explícita con dos bases

Guardar baseline leído; worker toma dos lecturas estables y prepara merge de tres vías. Colecciones con IDs se combinan por identidad, rangos/listas restantes son atómicos; orden concurrente incompatible falla. Resumen de diff en GUI, aplicar revalida sesión y disco y ejecuta Command::ReconcileProject como actor externo. Baseline CAS se avanza solo tras éxito; undo conserva nueva revisión. Sin sobrescribir decisiones humanas/evidencia. Alternativa de reload completo pierde edición local. Tests reconcile y external_diff_revalidates_both_bases_and_is_undoable_then_saveable. Rescaneo 2 s sin watcher SO; falta diff detallado/resolución y documentos V1.

## D-0033 · 2026-09-08 · Recortes, evidencia y montaje multirrango

Aceptación de recorte persiste aunque esté desactivado; reactivar conserva revisión humana. V1 enabled determina corte. Salto de revisión se deriva de timeline resuelto: solo salta intersección de trims de todos los contribuyentes; no oculta medios independientes ni modifica export. Player limita audio antes de frontera. Tema/selección añade sus rangos reales en un batch, sin convertir gaps en contenido. Inspector navega intersecciones por clip incluso repetido. Tests trims y review (este último compilado/no ejecutado); A/V e interacción pendientes. Protección de transiciones se aplica a actores externos; distinguir completamente autoría edited/aceptación queda para E4.

## D-0034 · 2026-09-08 · Recuperación de proyectos aún sin guardar

Store separado en configuración/recovery, nombre aleatorio por sesión/proyecto; conserva snapshot y auditoría sin elegir por el usuario un destino normal. Inicio enumera candidatos; recuperación explícita crea revisión nueva y primer Guardar pide carpeta. Guardado exitoso marca recovery como resuelto sin borrar archivos. No se modifica V1 ni datos personales. Autosave legado de proyectos guardados conserva JSON anterior, todavía sin journal. Riesgo pendiente: IO síncrono de autosave/guardado/enumeración; trasladar a workers y probar múltiples instancias. Se compila GUI, no se acredita interacción física.

## D-0026 · 2026-09-08 · Reanudación en otro equipo y alcance hasta E4

El usuario aplaza testing real/revisión general y autoriza implementación hasta E4. Las fases distinguen implementación de aceptación. Preparadores materializan `${WORKSPACE}`/`${OUTPUT}` en carpetas nuevas; evidencia histórica no se reescribe. MSVC se descubre con vswhere y Developer PowerShell; conservar lock/versiones del proyecto. Prueba dirigida de expansión en Unicode y compilación; medios reales diferidos.

## D-0027 · 2026-09-08 · Identidad de reintentos e historial

Un retry conserva protocolo/actor/proyecto/base/digest/comando; solo puede cambiar command_id. Otra solicitud con la misma clave se rechaza. Catálogo de 10 000 claves por sesión: no evictar para evitar duplicados silenciosos, rechazar nuevas claves al llenarse. Digest del snapshot incluye revisión. Undo/redo actualiza la revisión esperada de la próxima entrada; overflow rechazado. Todavía no hay persistencia de idempotencia entre sesiones ni permisos de transporte E4. Tests en session.rs.

## D-0028 · 2026-09-08 · Guardado y recuperación incremental E3

ProjectStore comparte observación de digest entre clones, serializa escritores con lock de SO y compara archivo actual antes de reemplazar. Lock liberado al crash; ningún borrado de lock por adivinación de PID. Rutas absolutas dentro de sesión/historial y relativas solo en snapshots persistidos eliminan retargeting tras Save As. Recuperación explícita conserva guardado como undo. Alternativa de sobrescribir silenciosamente descartada por pérdida de datos. Límite: escritores externos no cooperativos y commit multidocumento/journal siguen pendientes; esto no cierra DAT-02/03.

## D-0029 · 2026-09-08 · Atajos editables con candidato completo

Validar mapa completo antes de guardar; conflictos explicados y rechazados. Vacío desasigna, None restaura defaults. Captura y texto usan normalización común; se acepta memoria nueva solo si la escritura atómica termina. Interacción real/AltGr/IME permanece para la siguiente iteración.

## D-0025 · 2026-09-08 · Import de medios fuera de la UI (MED-01/PERF-01)

- Un worker cancelable, cola64, aplica ImportAsset en UI tras comprobar proyecto/token/identidad actuales. La ruta portable se calcula contra store vigente al completar. Se preservan cambios concurrentes sin descartar una import válida por cualquier revisión nueva. Nuevo/abrir cancela los pendientes; cierre espera al worker. Guion espera la cola. Evidencia: async-import.log y recorrido nativo VFR/rotación. El importador editorial V1 sigue síncrono; completar en E3.

## D-0024 · 2026-09-08 · Seek VFR conserva el cuadro vigente (MED-05/EXP-01)

- Video usa -noaccurate_seek y fps=start_time=0:round=up: preservar preroll y no anticipar PTS futuros. Audio no cambia su seek exacto. Alternativa anterior descartaba el cuadro vigente entre PTS y adelantaba hasta100ms en fixture VFR.
- Fuente oficial: [FFmpeg -ss, accurate_seek y autorotate](https://ffmpeg.org/ffmpeg.html), consultada2026-09-08, build8.0.1. Oráculo independiente con PTS nativos y pixels sin seek/fps. Rotación90 contrastada con transpose=cclock sin autorotate. Todos los178/120cuadros exportados pasan PSNR28dB fijado antes de medir; mínimos43,355/30,843. Evidencia: vfr-seek-experiment.txt, media-vfr-rotation.log, vfr-rotation-20260908-215336/verify.log.

## D-0023 · 2026-09-08 · Barrido temporal y reservas de caché (PERF-01)

- Resolver por eventos ordenados y conjunto activo, O(nlogn + salida); oráculo anterior bajo cfg(test). Benchmark release kittest n200 por10k/100kclips, P95CPU0,424/0,433ms; no presentación GPU.
- Reservas conservadoras waveform/thumbnails compartidas256MiB, cola64 y2workers; ownership hasta último consumidor y reintento después de liberar. La reserva cubre buffers CPU propios; no RAM de FFmpeg, ni GPU, ni toda la app. Pruebas de admisión/liberación/reintento con FFmpeg real. Ver dense-ui-benchmark.log y cache-memory-admission.log.

## D-0022 · 2026-09-08 · Propiedad y cierre de procesos (EXP-03/PERF-01)

- CancellableChild supervisa token, termina/recolecta solo el Child que creó y espera watcher al liberar; desbloquea pipes aunque FFmpeg espere datos. Evitar wait bajo mutex prolongado. Decoders, cache, player/export propagan el token; coordinadores esperan workers al cerrar. CleanupPaths elimina staging/audio en toda salida, sin borrar export publicado.
- Pruebas: process-cancel-test.log, shutdown-tests.log y shutdown-smoke.log nativo, cierre1,357s con export4K/cola/cachés, sin procesos propios huérfanos ni parciales. Esta smoke precede import asíncrono: repetir integrada antes de cerrar E2.

## D-0021 · 2026-09-08 · Export de selección y tramos (E2 / EXP-01)

- `ResolvedTimeline::extract_ranges` une rangos solapados/adyacentes y los concatena en orden de secuencia. Preserva todas las pistas visibles/audibles y sus mappings fuente, sin editar el proyecto. Clips seleccionados definen intervalos; items/bloques fuente se proyectan en cada ocurrencia habilitada del montaje. A/V enlazados no duplican intervalos.
- El diálogo explica estas reglas y el snapshot se congela al encolar. Alternativa descartada: exportar solo las pistas seleccionadas, pues perdería la mezcla y overlays del montaje. Los contratos/importación de chunks V1 completos siguen en E3; este incremento exporta los bloques/items que ya existen en el dominio.
- Pruebas: resolución con solapes desordenados, kittest de selección y bloque repetido, guion `prepare_selection.py` con exports comparados contra la fuente mediante PSNR.

## D-0020 · 2026-09-08 · Export estricto y demo portable (F-008/F-009, EXP-03/PKG-01)

- Validar al ejecutar el job los assets de los tramos resueltos que intersectan el rango solicitado; video solo si el preset lo consume. Abrir el fichero comprueba accesibilidad incluso si desapareció tras encolar. No cambiar el contrato ProjectStore: paths relativos al padre de .transcriptor.
- Export aborta ante fotograma incompleto o error de decoder de audio, sin publicar staging. Preview mantiene su tolerancia. EOF de audio con proceso correcto admite streams más cortos y silencio legítimo; fallo de proceso no equivale a silencio.
- Alternativa descartada: rechazar todos los assets del proyecto, pues impediría exportar audio o rangos válidos por medios no utilizados. Riesgo: distinguir EOF normal/fallo y la última imagen válida; pruebas reales negativas, regresión de medios y smoke en TEMP Unicode.

## D-0001 · 2026-09-07 · Unidad de tiempo: flicks (1/705 600 000 s) en `i64`

- **Decisión**: `Ticks` (flicks) para tiempo fuente, de secuencia y de presentación; `Rational` para frecuencias; `TimeRange` semiabierto. Conversión exacta con ms (JSON V1), con 24/25/30/50/60/23.976/29.97/59.94 fps y con 44.1/48/96 kHz.
- **Alternativas**: nanosegundos (no divisibles por 29.97 → deriva), racional por valor (más complejo en serde y ordenación), segundos `f64` (acumulación de error, lo que el encargo prohíbe).
- **Riesgo**: contratos V1 en segundos float; se convierte con redondeo a ms al importar/exportar y se documenta en DAT-01.
- **Prueba**: `tv2-domain time::tests` (exactitud por rate, sin deriva en 3 h a 29.97, round-trip ms).

## D-0002 · 2026-09-07 · IDs como cadenas tipadas

- Newtypes sobre `String` con prefijo (`clip-…`, `layer-…`, `item-…`) compatibles con el patrón V1 `[a-zA-Z][a-zA-Z0-9_-]{0,79}`. Evita reescribir IDs V1 al importar. Prueba: `ids::tests`.

## D-0003 · 2026-09-07 · Multimedia: FFmpeg por proceso + compositor/mezclador propios (amplía ADR-002/006)

- **Decisión**: decodificación de video (`-ss` exacto, `fps`, `scale`, `rawvideo` RGBA por pipe), audio (`f32le`, `atempo`) y codificación (`rawvideo` por stdin + WAV mezclado) con el FFmpeg vendorizado. Compositor CPU y mezclador en Rust (`tv2-media`), usados por el visor **y** por la exportación sobre la misma `ResolvedTimeline`.
- **Motivo**: (1) GStreamer no está instalado en el host y su distribución en Windows exige runtime + devel MSVC + decenas de DLL/plugins, mientras que FFmpeg ya es obligatorio para probe/export; (2) con un único compositor la equivalencia visor/export se cumple por construcción y se mide en E2 (EXP-01); (3) GES no es autoridad (ADR-008) y una composición multipista con solapes en GStreamer requeriría `compositor`/`audiomixer` dinámicos.
- **Alternativas**: GStreamer-RS (queda como alternativa detrás de `VideoDecoder`/`AudioDecoder`, ver ADR-002); bindings libav (licencia/enlazado más comprometidos).
- **Riesgo**: coste de spawn por seek (~30–80 ms medido en E0 con fixture 640×360), copia CPU por fotograma (sin zero-copy; no se promete), rendimiento 4K. Mitigación: reutilización de decodificadores en lectura secuencial, decodificación al tamaño del visor, `skip_frame nokey` para 8×.
- **Prueba**: `tv2-media` tests con medios reales (`decoder`, `render`, `audio`); evidencia E0 en `evidence/e0/`.

## D-0004 · 2026-09-07 · Persistencia E1: `project.json` atómico + `journal.jsonl` (compatible con ADR-003)

- Un documento autoritativo por proyecto (`<nombre>.transcriptor/project.json`, schema `transcriptor-project/1`), escritura tmp+fsync+rename con reintentos, campos desconocidos conservados (`#[serde(flatten)]`). Journal append-only de comandos confirmados (auditoría AI). SQLite se incorporará en E3 solo si las pruebas de DAT-02/DAT-03 (multidocumento V1, índices, jobs) lo exigen; los JSON V1 seguirán siendo contrato externo.
- Prueba: `tv2-application store::tests` (round-trip, sin temporales residuales, parcial rechazado, journal truncado tolerado).

## D-0005 · 2026-09-07 · Historial por snapshots con revisión esperada (como `HistoryStack` V1)

- Cada comando se aplica sobre una copia; al confirmar se guarda `before/after` y la revisión que debe tener el proyecto para que la entrada siga válida. Undo/redo restauran como **nueva revisión**; si la revisión no coincide (edición externa reconciliada) la entrada se descarta con `E_STALE_REVISION`. Comandos inversos explícitos se añadirán si el tamaño del proyecto (10k+ items) hace caro el snapshot; medición pendiente en PERF-01.
- Prueba: `session::tests`.

## D-0006 · 2026-09-07 · Política de colisiones y ripple

- Por defecto un clip no puede solapar a otro de su pista (`MovePolicy::Reject`, error `E_OVERLAP` explicable). `Overwrite` recorta/parte lo que tapa; `Insert` reproduce el `move(mode="insert")` de V1 (borde más cercano, desplaza posteriores, cierra hueco de origen). Ripple solo con `RemoveClips { ripple: true }` (Shift+Supr). Prueba: `commands::tests`.

## D-0007 · 2026-09-07 · Composición: la pista de video más alta tapa; el audio se mezcla

- Orden de composición = índice de pista (mayor = arriba). Video: se dibujan de abajo hacia arriba con transformación (fit/fill/stretch/native, escala, posición, opacidad) y alfa. Audio: suma con ganancia lineal clip×pista, mute/solo por pista, limitador duro ±1. La regla V1 «la pista superior sustituye video y audio» se conserva para montajes V1 mediante adaptador explícito (LAY-03, E3), no como regla V2.

## D-0008 · 2026-09-07 · Política de audio por velocidad (V1 conservada)

- 1×: audio normal; 2×/3×/4×: `atempo` (tono preservado, cadena ≤2.0 por etapa); 8×: skim de fotogramas clave (`-skip_frame nokey`) sin audio. Se muestra en el transporte (`audio_policy`). Reloj: audio cuando hay clips audibles y rate ≤ 4; si no, reloj de pared.

## D-0009 · 2026-09-07 · Rust 1.98.1 fijado en `rust-toolchain.toml`; egui/eframe 0.36.1

- Versiones tomadas de crates.io el 2026-09-07 (`cargo search`). eframe 0.36 cambió la API: `App::ui(&mut Ui)`, `egui::Panel` unificado, `Context::global_style`. Registrado para futuras actualizaciones.

## D-0010 · 2026-09-07 · Rutas de usuario fuera del proyecto

- Config `%APPDATA%\Transcriptor` (`ui-state.json`, `keymap.json`), logs `%LOCALAPPDATA%\Transcriptor\logs` (rotación 5 MB), cachés del proyecto en `<proyecto>.transcriptor/cache/`. Variables de entorno `TRANSCRIPTOR_CONFIG_DIR`, `TRANSCRIPTOR_LOGS_DIR`, `TRANSCRIPTOR_FFMPEG_DIR` para pruebas aisladas. Nada apunta a `G:\TODO` en producción.

## D-0011 · 2026-09-08 · Montaje V1: perfil explícito «aplanado» (LAY-03)

- **Decisión**: `views/montaje.json` (`editorial-montaje/1`) se importa con `tv2-v1compat::montaje` portando literalmente `flatten` de V1 (aritmética en segundos, `EPS=1e-3`, redondeo a 3 decimales): la pista superior sustituye video **y** audio y los huecos se compactan. El resultado se materializa como una secuencia V2 con una pista de video y una de audio por stream, clips con `provenance.v1_clip_id` y `extra.v1` (estado, origen, temas, pista y posición colocada). La secuencia lleva `extra.v1_montage_profile`.
- **Alternativas**: conservar las pistas V1 y añadir a la resolución V2 una regla «la pista superior silencia las inferiores» (rompe la mezcla V2 y mezcla dos semánticas); pedir al usuario que elija (innecesario: el resultado observable es único).
- **Riesgo**: se pierde la estructura de capas del montaje V1 (se conserva en `extra.v1`); la exportación de vuelta a `editorial-montaje/1` (E3) reconstruye clips a partir de `v1_clip_id` + `seq_ini_placed`.
- **Prueba**: `montaje::tests` reproduce `test_montaje.py` (tapado inicio/medio/fin, huecos, repetición) y `tests/golden_v1.rs` compara con el aplanado calculado por el propio V1 sobre la fixture generada con sus módulos.

## D-0012 · 2026-09-08 · Puntos solo en marcas del autor (regla V1)

- V1 persiste capas con `validate_items(allow_points=False)`; los puntos existen solo en el adaptador de marcas del autor. V2 iguala la regla: `LayerKind::allows_points()` es cierto solo para `Author`. Detectado al generar la fixture con V1 (`ValueError: rango fuera del medio, vacío o solapado` con un punto en capa `user`).

## D-0013 · 2026-09-08 · Fixtures V1 auténticas

- `tests/fixtures/v1/make_v1_fixture.py` copia los `.py` de V1 a una carpeta temporal (`PYTHONDONTWRITEBYTECODE=1`, PATH con el FFmpeg vendorizado, cachés redirigidas) y ejecuta `medios.fingerprint`, `editorial_layers.LayerStore`, `editorial_trims` y `editorial_montaje` para escribir `tests/fixtures/v1/demo-a` y `expected-v1.json`. El checkout V1 no se toca. Los goldens no se generan con código V2.

## D-0014 · 2026-09-08 · Grupo de enlace al dividir

- `SplitClip` asigna a la mitad derecha `link_group = "<grupo>@<at>"` (determinista): las mitades derechas de video+audio divididas en el mismo instante quedan enlazadas entre sí e independientes de las izquierdas. Detectado por el guion del escenario 1 (borrar un tramo eliminaba los seis trozos). Test `split_keeps_halves_linked_but_independent_between_them`.

## D-0015 · 2026-09-08 · Generación de seek = generación de la salida de audio

- El reproductor usaba dos contadores (local y de la salida cpal) que divergían tras `StepFrames`/loop; los bloques de audio quedaban etiquetados con una generación que el callback descartaba y el reloj se detenía (posición fija en el loop). Ahora `next_generation()` toma siempre la de la salida de audio cuando existe. Verificado por `e2-transporte.json` (posición avanza dentro del loop 2–3 s).

## D-0016 · 2026-09-08 · Gestos con estado del puntero, no con `drag_started`

- La captura de gestos del timeline usa `primary_pressed/primary_down/primary_released` y `press_origin` de egui: el gesto empieza en el press exacto y termina en el release, sin depender del umbral interno de arrastre de egui (que retrasaba el `Pending` y desplazaba el origen). Verificado con `egui_kittest` (`gesture_tests.rs`).

## D-0017 · 2026-09-08 · Cachés e índice temporal

- Se integra la base de cachés de Fable. Carga de disco y FFmpeg en workers limitados; claves SHA256 versionadas, persistencia con temporal único, validación de cabeceras/tamaños, cancelación cooperativa sin liberar prematuramente el slot. Waveform multirresolución y miniaturas consultan solo el rango visible; texturas limitadas a 256. El índice por pista usa árbol de intervalos con `max_end`, invalidado por proyecto/secuencia/revisión. Un test con 100001 clips devuelve 11 intersecciones visitando menos de 100 nodos. Esto no acredita todavía el benchmark de toda la UI ni un presupuesto global de RAM con múltiples medios largos.

## D-0018 · 2026-09-08 · Origen temporal de FFmpeg

- Reproducción de fallo con MPEG-TS que empieza cerca de 5 s: el renderer sumaba `start_time` a `-ss` aunque FFmpeg ya interpreta ese seek relativo al inicio. MAE 8,137 antes de corregir (`evidence/e2/start-time-before.log`). `source_t` se pasa directamente; `fps=start_time=0` normaliza el vídeo. Audio usa `aresample:async=1:first_pts=0` antes de `atempo`, conservando silencios de streams retrasados y escalándolos con la velocidad. Las cachés cambian su clave a v2 para invalidar datos anteriores.
- Fuente oficial consultada: [FFmpeg, opción seek_timestamp](https://ffmpeg.org/ffmpeg.html), 2026-09-08; build probado 8.0.1. Fixtures `fixture-start5.ts` y `fixture-audio-delay.mp4`; pruebas con seek y 2× en `media-offset-tests.log`. Export de corte fuera de keyframe: diez destellos/pulsos, diferencia A/V de 0,104 ms (`sync-export-measurement.txt`). No sustituye medición física del reproductor.

## D-0019 · 2026-09-08 · Cola de export y rechazo esperado en guiones

- La GUI congela revisión, assets, timeline resuelto y preset al encolar; máximo 16 pendientes y un worker activo. Historial acotado; cancelar un pendiente no escribe salida. Test real cancela activo y pendiente, edita el proyecto y verifica que el job restante conserva la duración de su revisión. Persistencia de trabajos y recuperación quedan para E3.
- `exec_checked` conserva el error tipado del mismo recorrido usado por `exec`. El paso `shift` del guion puede declarar `expect_error`; exige código exacto y revisión inalterada. E1 declara `OVERLAP` en su prueba negativa. Un fallo inesperado sigue invalidando el recorrido.

## D-0039 · Edición semántica y estados por contexto

SplitItem conserva rangos no afectados del original, divide descendientes cruzados y reasigna hijos íntegros sin perder sus IDs. Trim valida contención; nudge expande descendientes y aplica delta común. Marcas tienen nota/incluir/excluir y decisión solo en región ≥200 ms; un punto se expande ±2 s. Recortes requieren un rango ≥50 ms sin jerarquía. GUI, teclado y gestos construyen estos comandos; estados multicapa/trim enlazado son batch. El actor Agent no puede crear aceptación humana; edited se atribuye a la persona. Tests semantic_tests; GUI compilada, no ejecutada. No equivale a adaptador author sidecar ni permisos E4.

## D-0040 · Historial/1 dentro de la intención recuperable

history.json serializa undo/redo con proyecto/revisión y fronteras comprobadas. La intención de commit contiene proyecto, auditoría e historial; se elimina después de los tres archivos. Cuatro fronteras sintéticas probadas, sin crash físico. Apertura conjunta bajo lock; ausencia de history en proyectos antiguos migra a pila vacía, incoherencia no se ignora. Autosave incluye historial y lo valida; recuperar mantiene todavía un único undo a la versión guardada. Riesgo: snapshots grandes (200 entradas, intención 128 MiB/autosave 64 MiB), sin deltas/archivado. No se promete historial ilimitado ni jobs durables.

## D-0041 · IO de proyecto y conflictos de autosave

Un worker de apertura/guardado por GUI, canal bounded(1), proyecto/revisión congelados. Abrir revalida base al publicar; guardar reconoce solo el prefijo de auditoría capturado y no limpia revisiones nuevas. Save As transforma paths en todos los snapshots. El cierre normal espera escrituras; guardar y salir no marca limpio antes del resultado. Autosave usa el mismo lock y verifica base en disco; identifica writer y no reemplaza un recovery más nuevo de otra instancia. Riesgos restantes: editores ajenos al lock, lecturas bloqueadas, clones iniciales y descubrimiento recovery síncrono. Tests de ACK, dos instancias y store; no ejecución GUI de cierre.

## D-0042 · Documentos V1 normalizados sin colección duplicada

Trims conserva cabecera una sola vez y metadata en cada carril; cuts existen únicamente como items autoritativos normalizados. Export desde cualquier carril emite todos los carriles del medio, conserva extras/aceptación, incrementa revisión por cambios de carril y ajusta next_id. `tv2_label` es extensión explícita para etiqueta (V1 no tiene campo equivalente); IDs nuevos cut-/m numéricos. Proyectos que ya perdieron la cabecera exigen reimportación; no se inventa esa información. Coalescencia y administración de carriles V1 completas siguen pendientes.

Chunks usa el plan seleccionado antes que la view; una cabecera + colección única, cobertura 0..duración/max50min. Split/trim/nudge preservan partición y vecinos. Summary original se conserva como metadata; comment editorial es independiente. `tv2_original_range` permite rechazar export de metadata de bordes/snap desactualizada tras editar. Esta negativa es un límite explícito, no implementación de recalcular evidencia/snap ni materializar master/chunks. Export sigue documental, no carpeta; montaje inverso editado sigue pendiente.

D-0039, frontera temporal: un trim de fin exclusivo consulta el intervalo inmediatamente anterior al corte; trim de inicio consulta el siguiente. Clip::seq_edge_to_source y test application evitan usar el inicio de otra ocurrencia o rechazar el fin de secuencia.
