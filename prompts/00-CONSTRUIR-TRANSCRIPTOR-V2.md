# Construir Transcriptor V2: instrucciones de ejecución

> **MARCADOR DE CONTINUACIÓN (actualizado 2026-09-08, continuación 3).**
> Estado: **E0 cerrada · E1 cerrada (host) · E2 avanzada y abierta · E3–E6 pendientes.**
> Fuente de verdad del progreso: `implementation/STATUS.md`, `implementation/requirements-matrix.md`, `implementation/decisions.md` (D-0001…D-0021), `implementation/evidence/e0|e1|e2/`.
> Al reanudar: lee el anexo «Traspaso al siguiente agente» al final de este archivo, luego `STATUS.md`, y continúa por el primer punto de «Qué sigue» sin rehacer lo verificado.

Fecha de preparación: 2026-09-07. Este es el punto de entrada para el agente implementador. Lee también, completos, [la especificación](01-ESPECIFICACION-EDITOR-Y-CAPAS.md) y [las entregas y pruebas](02-ENTREGAS-PRUEBAS-Y-CONTINUIDAD.md), situados en esta misma carpeta.

## Encargo y resultado esperado

Pongámonos manos a la obra. Implementa Transcriptor V2 como aplicación nativa de escritorio en Rust, con un ejecutable Windows utilizable, una interfaz moderna y un editor multimedia funcional. Conserva la semántica editorial, los proyectos JSON y las funciones valiosas de V1. El usuario y un agente externo deben poder controlar las mismas operaciones mediante un dominio común.

Construye primero un recorrido completo de importar, reproducir, editar, guardar, reabrir y exportar. Integra desde ese recorrido las estructuras y los comandos necesarios para las capas editoriales; completa después su paridad con V1 y su control externo. A continuación incorpora los modelos y pipelines Python existentes: transcripción, alineación, heurísticas, risas, intensidad, arousal y las demás capacidades inventariadas. El editor debe funcionar sin instalar ni cargar modelos.

El resultado solicitado incluye implementación, pruebas, corrección de fallos, revisión visual, documentación y distribución. Continúa automáticamente entre entregas cuando sus criterios estén satisfechos. No te detengas al tener un plan, un scaffold, una ventana vacía, capturas bonitas o una demo con datos simulados. Tampoco declares completada una entrega apoyándote únicamente en que compila.

## Autoridad y alcance

- Producto e investigación V2: `G:\TODO\transcriptor-v2`. Puedes implementar aquí. Conserva la documentación y cualquier trabajo de otros agentes.
- V1: `G:\TODO\transcriptor-installer-v0.2.0`, estrictamente de solo lectura. Sus medios, modelos, configuración y proyectos reales también permanecen intactos. Ejecuta comparaciones que puedan escribir exclusivamente contra copias aisladas en V2.
- `references/`: fuentes de consulta. Reutiliza los clones existentes; descarga solo los que falten y comprueba origen, revisión y licencia. No conviertas los clones en el código del producto ni los modifiques como efecto lateral de builds.
- Los permisos anteriores para solo investigar quedan ampliados a implementar V2 por este encargo. No quedan ampliados a modificar V1, publicar servicios, subir datos, comprar recursos o distribuir públicamente el programa.
- Adopta decisiones de implementación rutinarias y reversibles. Solicita una decisión únicamente si falta autoridad o una elección de producto imprescindible que cambie materialmente el encargo; continúa mientras tanto los trabajos independientes.

Lee las instrucciones locales aplicables. Al iniciar registra los estados Git de ambos directorios, si son repositorios, y las modificaciones existentes. V2 era un workspace documental cuando se preparó este encargo: no presupongas que contiene `.git` o `Cargo.toml`. Si ya hay implementación al comenzar, evalúala y continúa desde ella; evita generar un segundo producto en paralelo.

## Lectura inicial y uso del research

Lee estos tres archivos antes de implementar. Después consulta:

1. `reports/contradicciones-y-correcciones.md`, `reports/validacion-documentacion-existente.md` y `reports/trazabilidad-afirmaciones.md`.
2. `docs/18-matriz-conservar-refactorizar-migrar.md` a `docs/24-backlog-de-prototipos-tecnicos.md`.
3. Los ADRs de GUI, motor, persistencia, dominio, comandos, IPC y migración.
4. `docs/03-inventario-de-reutilizacion.md`, `docs/14-licencias-y-procedencia.md`, `research/repositories-lock.json` y los mapas de fuentes correspondientes.
5. Los archivos V1 relevantes a la primera entrega y sus tests; amplía la lectura al entrar en cada subsistema.

La documentación anterior es evidencia de investigación, no una garantía de exactitud. En especial, un título como «arquitectura validada» no demuestra que haya un prototipo integrado. Resuelve discrepancias mediante implementación, tests, versiones verificadas y fuentes oficiales. Registra las correcciones en un ADR o nota con archivo y símbolo de origen.

No repitas toda la investigación antes de producir software. Limita cada experimento a una pregunta de integración, una métrica y una decisión. Los 14 prototipos previos se distribuyen entre entregas; no constituyen 14 bloqueos obligatorios antes de abrir una ventana nativa.

## Base técnica y decisiones reversibles

Parte de Rust para dominio, comandos, GUI y supervisión. La hipótesis de GUI es egui con eframe y backend wgpu; eframe puede gestionar winit. No dupliques event loops ni añadas una integración manual de winit sin necesidad demostrada. Fija toolchain y versiones compatibles en los archivos del producto.

La hipótesis multimedia es GStreamer-RS para reproducción/decodificación y FFmpeg/FFprobe externos para probe/export. Valida pronto que el video se presenta dentro de la ventana y que el paquete Windows encuentra sus dependencias. GES es opcional y, si se usa, vive en su hilo propietario; el dominio propio continúa siendo la autoridad. La integración de texturas y el supuesto zero-copy necesitan evidencia real en el backend Windows elegido.

Mantén estas decisiones detrás de interfaces pequeñas. Si una hipótesis falla, reproduce el fallo, documenta la causa y prueba la alternativa más pequeña compatible con el encargo. Una licencia pendiente de un repositorio de referencia no debe detener el producto: utiliza código propio o una dependencia cuya licencia esté clara.

Define una política única de persistencia antes de escribir proyectos. SQLite es candidato para transacciones, índices, revisiones y trabajos; JSON conserva los contratos externos. Implementar simultáneamente un event store completo, una base duplicada y un sistema de sincronización genérico requiere una necesidad demostrada. Elige el mecanismo más sencillo que satisfaga las pruebas de recuperación y edición externa del archivo 01.

## Reutilización con procedencia

Consulta las rutas y commits de `research/repositories-lock.json` como punto de partida. Comprueba también los encabezados y licencias de cada archivo que realmente incorporarás.

| Referencia | Uso previsto | Condición |
|---|---|---|
| Cutlass | Comandos, inversas, validación de AI y componentes multimedia candidatos | Revisar MIT/Apache y dependencias concretas; portar solo lo necesario con atribución |
| Gausian | Diseño de editor, límites de módulos, representación visual y render | Conflicto README/`LICENSE` registrado; no copiar implementación o assets mientras siga sin resolverse |
| OpenCut | Comportamiento de timeline y editor | No copiar sin licencia verificada |
| Kerf | Flujo conceptual de propuestas/diff y control externo | PolyForm Noncommercial registrado; no incorporar código bajo el supuesto de uso comercial permitido |
| GStreamer-RS/GES | Dependencias e integración multimedia | Separar licencia de bindings, runtime y plugins distribuidos |
| MLT | Comparación de motor y semántica NLE | Revisar componentes y opciones; `COPYING` local contiene LGPL-2.1, por lo que «todo MLT es GPL» es una conclusión incorrecta |
| Kdenlive/Shotcut | Referencias de comportamiento y usabilidad | No copiar código GPL al producto bajo una licencia incompatible |

Fusionar significa integrar responsabilidades y comportamientos compatibles. No concatenes aplicaciones enteras, distintos command buses ni sistemas de persistencia competidores. Leer código restringido y reescribirlo de memoria no convierte automáticamente el resultado en una implementación clean-room; no hagas esa afirmación. Implementa a partir de requisitos propios cuando la reutilización no esté autorizada.

Registra cada incorporación en `implementation/reuse-ledger.md`: origen, commit, archivo, licencia, código incorporado, modificaciones y dependencias. Conserva avisos requeridos y genera el inventario de terceros del paquete final. No supongas que ejecutar FFmpeg como subproceso elimina las obligaciones de distribución.

## Organización del producto

Ubica el workspace Rust en la raíz de V2, separado de `references/`, `docs/` y `research/`. Una organización inicial razonable es `apps/desktop`, `crates/domain`, `crates/application`, `crates/persistence`, `crates/media`, `crates/agent`, `workers/python`, `tests/fixtures`, `packaging` e `implementation`.

Es una guía de responsabilidades, no una orden de crear crates vacíos. Comienza con pocos paquetes cohesionados y extrae límites cuando existan consumidores o necesidades de aislamiento reales. El dominio no importa GUI, GStreamer, procesos Python ni widgets. Las capas de infraestructura dependen de los contratos del dominio. La GUI presenta estado y emite intenciones; los callbacks no escriben directamente JSON ni construyen comandos FFmpeg.

Define primero los tipos que requieren estabilidad: identidad de medios, tiempo, clips, tracks, items semánticos, selección, revisión, comandos, jobs y errores. Usa tipos de dominio y validación en fronteras; conserva los campos JSON desconocidos cuando la compatibilidad lo requiera. Evita un `serde_json::Value` universal, registros globales mutables, strings mágicos dispersos y mutexes que abarquen operaciones lentas.

Un adaptador de compatibilidad V1 es una parte válida de la arquitectura. Dale contrato, pruebas y límites. Corrige las causas de los fallos; no ocultes errores con `catch` amplio, `unwrap` sobre entradas externas, reintentos infinitos, espera arbitraria o fallback silencioso. Evita también abstraer un framework general antes de necesitarlo.

## Bucle de trabajo obligatorio

Para cada requisito o defecto:

1. Identifica su ID en el archivo 02, el comportamiento observable y el estado actual.
2. Lee la implementación y las pruebas relevantes. Si hay una decisión dependiente de versión, consulta código/documentación oficial y registra fuente, fecha y versión.
3. Escribe una nota breve: decisión, alternativas pertinentes, riesgo y prueba que demostrará el resultado. Expón conclusiones y evidencia; no es necesario volcar razonamiento interno paso a paso.
4. Implementa el incremento completo a través de dominio, persistencia, UI y salida multimedia cuando corresponda.
5. Ejecuta las pruebas pertinentes. Inspecciona la aplicación real para interacciones y presentación; comprueba el medio exportado para operaciones de render.
6. Revisa datos, concurrencia, rendimiento y paridad V1. Corrige los fallos encontrados y repite las pruebas afectadas.
7. Actualiza matriz, evidencia y punto de continuación. Pasa al siguiente requisito pendiente.

Haz tres revisiones distintas por entrega: funcional contra el encargo, técnica contra contratos/concurrencia/errores y de experiencia contra el ejecutable. Repite una revisión si encuentra problemas; evita repetir comandos idénticos sin cambio ni incertidumbre nueva. Cada pasada debe terminar en evidencia, correcciones o una limitación explícita.

Si hay agentes auxiliares, asígnales límites de archivos y resultados concretos. Comparte contratos antes de desarrollar consumidores, conserva las ediciones ajenas y reserva la integración y aceptación al responsable principal. No permitas que cada agente invente su timeline, su registro de comandos o su formato de proyecto.

## Continuidad y criterio de finalización

Mantén `implementation/STATUS.md` con entrega activa, build, requisitos verificados, fallos abiertos, decisiones, comandos de reproducción y siguiente acción concreta. Usa `implementation/requirements-matrix.md` para trazabilidad y `implementation/evidence/` para resultados. No marques un requisito como completo si solo existe documentación o una prueba con mocks de su dependencia principal.

Al reanudar, lee estos archivos y continúa desde el primer requisito pendiente. Guarda avances antes de una interrupción. No conviertas la duración de una sesión en criterio de finalización. Si una dependencia externa imprescindible impide continuar, registra qué falta, intentos y trabajos independientes completados; no certifiques lo que no se probó.

Entrega el ejecutable, el paquete y las instrucciones de ejecución con rutas reales; informa pruebas, limitaciones y diferencias de compatibilidad. Windows es el primer destino verificable. Mantén límites portables para Linux y documenta su estado real; no declares soporte multiplataforma sin pruebas en esas plataformas.

Empieza por E0 y E1 del archivo 02. La prioridad es que una persona pueda editar y exportar desde una ventana nativa y que esa misma operación quede representada en contratos utilizables por la AI.

---

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
