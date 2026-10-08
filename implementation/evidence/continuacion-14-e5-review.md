# Continuación 14 — revisión cruzada E5 Rust/GUI

2026-09-30. Revisión independiente de `crates/pipeline/src/lib.rs`, `process_tree.rs`, `tests.rs`, `apps/desktop/src/pipeline_ui.rs` y, como dependencias, `application::{jobs,session}`, `domain::evidence`. Se leyó [evidencia Python/intensidad](continuacion-14-e5-python.md), [inventario](continuacion-14-e5-inventory.md) y el smoke de transporte. No se editó producción, no se lanzó GUI ni modelo y no se reintentaron suites bloqueadas.

Identidad de las fuentes revisadas: `lib.rs` SHA256 `48a6270972477035b744243190bec20a3ecd02666a56f778f77848ad44497d07`; `process_tree.rs` `f2f131797e572353513ddabcd845106a9ebbe3db03e03ad5a5b00f0721cdb74e0`; `pipeline_ui.rs` `cd298e463010ba31750e0bb6a81fe2e9b03e5336e99aaafce78f952e556618a6`. Son snapshots de revisión; cambios posteriores requieren relectura y no heredan automáticamente esta conclusión.

## Hallazgo accionable: escritura al worker sin plazo/cancelación — corregido y probado

**P1, defecto estático, reproducer pendiente de ejecución:** `crates/pipeline/src/lib.rs:216–219`, `OwnedWorker::send`, usa `serde_json::to_writer`, `write_all` y `flush` sobre `ChildStdin` bloqueante. No comprueba cancelación ni limita previamente el tamaño del envelope, ni tiene timeout. `run` envía el payload antes de entrar en `response`, que es donde se comprueban flag/plazo. `close`, línea 263, también intenta enviar `shutdown` **antes** de iniciar el grace de dos segundos.

Condición concreta: un worker que responde `hello` y después deja de leer stdin, junto con un request que supere el buffer del pipe (Asset.extra o inventario grande), puede bloquear `send(run)` indefinidamente. La GUI acepta Cancelar, pero el hilo de análisis no alcanza `response`, `close` ni Drop; el lease permanece Running y el proceso sigue vivo mientras el editor continúe abierto. Esto no implica que el worker Python normal ya haya fallado así; demuestra una ruta de supervisión que el contrato ML-03 no limita. El Job Object mata descendientes al cerrarse su handle, pero el hilo bloqueado conserva ese handle.

El smoke Windows exitoso existente no cubre esta condición: su fixture `transport-only-owned-child.py` usa `for line in sys.stdin`, ignora cancel/shutdown **después de leerlos**, y abre FFmpeg tras consumir el request. Verifica kill-on-close/grace para un worker que sigue leyendo, no backpressure en la escritura del host.

Corrección recomendada: serializar y validar el envelope contra el límite NDJSON antes de enviarlo; separar la escritura bloqueante de la supervisión cancelable y disponer de kill del árbol propio aunque el writer esté bloqueado. La espera/cancelación debe funcionar tanto durante send(run) como send(cancel/shutdown), sin esperar primero una escritura. No basta añadir un `check_cancel` antes del `write_all`, porque no interrumpe el write ya bloqueado.

Reproducer propuesto para una fixture exclusiva de transporte (sin ASR): responde hello y deja de leer stdin; request con ≥64 KiB de metadata inventada dentro del límite de protocolo; cancelar tras confirmar el worker vivo. Exigir retorno acotado Cancelled, lease terminal durable y handles del worker/descendiente señalados. Un caso adicional debe rechazar envelope >1 MiB antes de escribir bytes. Su ejecución posterior a la corrección se documenta abajo.

## Hallazgos menores de cancelación/revisión

**P2, defecto estático de cancelación durante preparación:** `lib.rs:400–426` deja de comprobar cancel después de la última verificación SHA de fuente. Leer master, parsear, generar proyecciones y `session.prepare_command` pueden ser costosos; cancelar en ese intervalo no impide devolver `Ok(PreparedCommand)`. `pipeline_ui.rs:122–125` entrega ese resultado tal cual, y poll lo muestra como incorporable. No hay aplicación automática ni pérdida de datos: el usuario todavía debe pulsar Incorporar. Añadir comprobaciones entre fases y después de prepare garantiza que una cancelación tardía conserve el job exitoso para revisión explícita y no publique un preview como operación activa completada. La cancelación inicial ya tiene test; la cancelación a mitad de preparación no está ejecutada aquí.

**P2, recuperación de preview tras error:** `pipeline_ui.rs:227–229` hace `ready.take()` antes de `prepared?`/`commit_prepared`. Si una edición humana vuelve obsoleto el preview, se muestra el error pero se elimina de memoria la entrada revisable. El resultado no se pierde del disco: job Succeeded permanece y «Revisar en revisión actual» puede reconstruirlo, por lo que no se clasifica como pérdida permanente. Conviene conservar record/result o permitir reprepare explícito en el panel de error; nunca aplicar automáticamente sobre la nueva base. Ya existe botón de revisión de resultados Succeeded y rebase explícito, que reduce el impacto del fallo.

## Comprobaciones favorables y límites

- El worker escribe fuera del proyecto. El resultado exige job/project/revision/project_digest/source_sha256/model_digest idénticos; master debe aparecer en artifacts. Las rutas rechazan componentes no normales/backslash y canonicalización que salga del job. Los SHA de artifacts se comprueban antes de preparar.
- La fuente se verifica por SHA completo al run y al preparar, y modelo/worker se ligan a la solicitud. El master se vuelve a leer/hashear sobre **los mismos bytes** que se parsean; se exige fingerprint/duración correspondientes al asset. Se importan únicamente master y artifacts declarados, no siblings editoriales arbitrarios. El bundle conserva SHA/tamaño/locators y el almacenamiento vuelve a comprobar archivos al publicar.
- Strict prepare rechaza cambio de proyecto/revisión/digest; rebase requiere la acción local explícita y conserva la identidad exacta del asset/fuente. El comando común atribuido External mantiene protecciones de master inmutable, aceptación/desactivación humana, ediciones y tombstones. `commit_prepared` revalida base exacta antes de cambiar el proyecto; los tests existentes prueban rechazo tras edición humana y conservación/replay/historia/Save As. No se halló una ruta nueva de sobreescritura humana o pérdida de proyecto en esta lectura.
- Windows Job Object usa `KILL_ON_JOB_CLOSE` y attach ocurre antes de hello/run; fallo de attach mata/recolecta el child propio. Drop mata/recolecta el worker. La prueba real anterior conserva handles de objetos OS, no solo PID, y acreditó worker/FFmpeg terminados y job Cancelled. Su alcance y hash son los de aquella ejecución. La implementación no Windows de ProcessTree es no-op: no se acredita misma garantía de descendientes allí.
- Readers de stdout/stderr tienen buffers acotados y stdout tiene límite de línea/canal. Envelope/progress/IDs están correlacionados. Eso limita lectura, pero no resuelve el hallazgo de escritura bloqueante anterior. Stderr registra un tail de 16 KiB al terminar; no hay certificación de OOM/decoder nativo a partir de esos límites.
- Jobs mantienen lock real, attempts y resultado durable; discovery solo marca Interrupted al adquirir lock de Running sin propietario. Revisión Succeeded reconstruye command con snapshot actual y no vuelve a ejecutar modelo. Cancelar/cerrar el panel no equivale a cerrar procesos arbitrarios: Running conserva ownership y flag del hilo propio.

## Evidencia disponible, sin atribuir ASR aceptado

La evidencia leída separa: 12 pruebas independientes de extracción/caché/protocolo/cancelación FFmpeg; host smoke Windows real de hello/EOF/árbol propio; 14 pruebas stdlib de intensidad/pausas/Ava/conversación y golden PCM; interoperabilidad Rust del master hecho con **palabras inventadas**. Las 12 regresiones de pipeline con fixtures sintéticas verificaron identidad, protección, tampering, rebase y durabilidad. Esta revisión no las volvió a ejecutar.

El primer intento de inferencia tiny sigue bloqueado por DLLs de PyAV/Control de aplicaciones y no alcanzó decoder/master ASR. Los golden PCM y el fixture de master no prueban inferencia. La selección del request Rust continúa `extract+transcribe`; las derivaciones stdlib añadidas no son todavía un DAG configurable. MMS está siendo integrado en otro frente; no se acepta aquí por sus descargas/compilación ni se reclama paridad de risas/arousal/LLM/GUI bajo carga.

Resultado inicial de revisión: un problema prioritario de supervisión durante escritura y dos mejoras concretas de cancelación/recuperación de preview; sin nuevo defecto de pérdida permanente de datos o vulneración humana encontrado. P1 se corrigió y su nuevo reproducer pasó como se registra a continuación; el estado actualizado de ambos P2 se registra en el párrafo siguiente.

Actualización del principal posterior a la prueba writer: ambos P2 ya están **implementados y compilados**. `prepare_result_impl` comprueba cancel después de read/parse, en artifacts, después de projections y después de `prepare_command`. GUI aplica primero `ready.as_ref()` y `matches_base` y conserva el resultado si la base está obsoleta; ante error de commit restaura record/result para revisión. Workspace all-targets Clippy PASS en `e5-writer-clippy.log`; los 12 contratos pipeline existentes pasan en `e5-pipeline-writer-contracts.log`. Se releían esos símbolos, pero no se lanzó todavía GUI reconstruida ni nuevo test de cancelación inyectada a mitad de parse/preparación: la compilación y las regresiones existentes no prueban esa nueva frontera por sí solas.

## Regresión nueva ejecutada después de la corrección del writer

El principal corrigió `lib.rs`: serializa el envelope y rechaza tamaño ≥1 MiB, writer dedicado con cola y ACK acotados, espera cancelable cada ≤50 ms/plazo10 s, cancel best-effort250 ms, shutdown500 ms dentro del grace total2 s. El frente de revisión añadió **solo** `crates/pipeline/examples/blocked_stdin.rs` y `tests/scripts/blocked_stdin_worker_fixture.py`, y esperó el mensaje «writer listo» antes de construir/ejecutar. No se rehízo el ejecutable GUI abierto: esta prueba usa código posterior al build nativo `8ba...` de aquella GUI.

Fixture de transporte: Python 3.12.13 real, recibe hello, abre un hijo Python propio que duerme (sin modelos/media/credenciales), registra ambos PID en root nuevo, devuelve hello y **no vuelve a leer stdin**. El ejemplo adquiere handles OS de ambos procesos antes de activar cancel; así comprueba el mismo objeto aun si se recicla su PID. Asset.extra contiene 128 KiB de texto inventado; envelope medido **132892 bytes**, mayor64 KiB y menor1 MiB. Modelo tiny solo se lee para el digest requerido por enqueue/run; no se importa Torch/faster-whisper/PyAV ni se llama inferencia.

Primer build falló antes de ejecutar por un método inexistente `ErrorCode.as_str` usado en el mensaje del ejemplo; se sustituyó por formato Debug. [Build inicial](e5-blocked-stdin-build.log) conservado. [Build corregido](e5-blocked-stdin-build-fixed.log) PASS alpha.11, 2.71 s; [Clippy del ejemplo](e5-blocked-stdin-clippy.log) PASS, 1.61 s; Rustfmt específico y diff whitespace correctos.

Comandos ejecutados sobre root nuevo `.local/e5-blocked-stdin-20260930-111646` (FFmpeg localizado sin modificar instalación):

```powershell
scripts/cargo.ps1 build --locked -p tv2-pipeline --example blocked_stdin
target/debug/examples/blocked_stdin.exe .local/e5-venv/Scripts/python.exe tests/scripts/blocked_stdin_worker_fixture.py .local/models/whisper-tiny .local/e5-blocked-stdin-20260930-111646/work tests/fixtures/media/fixture-a.mp4 .local/e5-blocked-stdin-20260930-111646/result.json
scripts/cargo.ps1 clippy --locked -p tv2-pipeline --example blocked_stdin '--' -D warnings
```

**Primera ejecución del ejemplo: exit0, PASS**, [runtime](e5-blocked-stdin-runtime.log) y [resultado](e5-blocked-stdin-result.json):

- Cancel flag 300 ms después de handshake/handles; `run` termina **Cancelled en 0.6240197 s** (<5 s), `job-34e6941edb85` durable `cancelled`.
- Worker PID4160 e hijo Python PID24684: ambos **handles señalados** después de terminar run/Drop. No se seleccionó ni terminó proceso ajeno; cleanup del ejemplo usa exclusivamente handles de su fixture.
- Otro job en root separado usa padding1 MiB; devuelve **Invalid, «Petición Python excesiva», en 0.2835699 s** (<5 s), `job-39356c124ed8` durable `failed`. No afirma inferencia ni resultado editorial.

[SHA256 archivados](e5-blocked-stdin-sha256.txt): lib.rs corregido `0a407239eafd9b6aca8b95f68b19ae009018c2c44f9c076e1277d482bbf9040f`; ejemplo `5faefb12b1b03ed8619e48180e4276adcb8810d7048dbadfc5e6cfca9264898e`; fixture `89e042bc3b3e1460b976ec3d9e64a7853a069d7e26657e8200ec2e265b034761`; binario debug `5b788d547d2bde1d5dfa2dddeb5302ded7a514995ad133f6611817f0c92c23c6`; resultado `cc372fee6527c98401ebf74dbd08e707d8ee385109e86d45d7f1ae002f8c5027`. La fecha de modificación de lib.rs precedía al binario compilado; este registro no acredita los cambios futuros de MMS/arousal u otras rutas todavía no ejecutadas.

No se repitió ASR/PyAV ni suites Windows4551 ni se movió/renombró un ejecutable bloqueado. Es un nuevo caso necesario de backpressure del supervisor, con fixture transparente y `inference_accepted=false`; no cierra E5 ni su aceptación GUI.

## Revisión y corrección de generación compuesta

Revisión posterior autorizada de `generation.rs`, `finalize.py` y `finalize_worker.py`. La primera generación integrada del principal había pasado sobre parent ASR explícitamente inventado y resultados MMS/arousal/risas reales de la misma ascendencia. Ese éxito de integración no acreditaba la conservación de todas las extensiones ni el rechazo de una derivación modificada con SHA recalculado.

Se reprodujo pérdida de campos desconocidos dentro de `conversation` y `tracks.a0.heuristics`: la derivación sustituía ambos objetos completos. La entrada quedaba intacta y `asr_original` conservaba los campos, pero no la proyección final. [Fixture stdlib anterior a la corrección](e5-generation-review-extensions.json) registra `conversation_extra_preserved=false`, `heuristics_extra_preserved=false`, `analysis_extra_preserved=true`, sin inferencia nativa. La lectura Rust también encontró una frontera demasiado amplia: excluía `analysis`, `conversation`, `heuristics` e `intensity` completos de la comparación y no comprobaba `analysis.generation.modules`. Así, comprobar solo SHA no distinguía una derivación forjada cuyo recibo se había recalculado.

El principal cedió exclusivamente los cinco archivos de este incremento al frente E3. Corrección congelada:

- `finalize.py:31` superpone campos derivados y conserva recursivamente extensiones desconocidas en objetos de conversación, heurísticas/provenance, intensidad, baselines y finalization. `finalize_worker.py:154` conserva las extensiones de `analysis.generation`, sustituyendo su ascendencia/módulos por los actuales. Las listas de observaciones se recalculan completas: no se reasigna una anotación antigua a un nuevo evento por posición. La copia íntegra anterior permanece en `asr_original`. Las afirmaciones históricas de stages alignment/arousal/laughter se retiran antes de superponer el plan actual para no resucitar una etapa omitida; las anotaciones independientes se conservan.
- `generation.rs:345` compara campos desconocidos de analysis, conversation y metadata, y `:394` los de heuristics/provenance/intensity, antes de la verificación costosa. `analysis.generation.modules` exige exactamente las referencias del request. Lecturas del master y JSON de stages están limitadas a 128 MiB y SHA se calcula sobre los bytes que se parsean.
- `finalize_worker.py:186` introduce método `verify` de solo lectura. Vuelve a cargar exclusivamente bytes SHA verificados del finalizador/derivación, inyecta la derivación aprobada y rederiva desde las entradas verificadas **ignorando todos los checkpoints de salida**. Compara JSON canónico completo, incluido tipo de dato, señales, conversación, heurísticas, metadata y extensiones. Rehash de todas las entradas/código/master antes y después; cancel comprobado antes/después de derivar y antes de responder. No crea directorios, escribe artefactos ni actualiza checkpoints.
- `generation.rs:272` usa un worker propio para hello/verify después de checks baratos, con transporte, deadline y cancelación existentes; Drop/close conservan ownership del árbol. Revalida inputs/código y SHA del master después de la respuesta. Python no llama al validador Rust, por lo que no hay recursión. Cada validación incurre en una derivación stdlib adicional y en las verificaciones de hashes: debe ejecutarse fuera del hilo GUI. No ejecuta ASR, Torch ni modelos opcionales.

Validación ejecutada sobre fuentes congeladas, separada de compilación:

| Comprobación | Resultado | Evidencia |
|---|---|---|
| Suite pure finalize, incluyendo PCM corto 9.531s frente a vídeo10.222s, extensiones y no resucitar etapas | **19 PASS, 0.402s** | [Log](e5-generation-extension-tests-final.log) |
| Suite protocolo/fronteras, conservando los 11 casos anteriores | **17 PASS, 1.355s** | [Log](e5-generation-verify-tests-final.log) |
| Tests Rust de generación, identidad/probabilidad/extensiones y rechazo de pérdida/forgery de extras | **3 PASS, 0.01s** después de compilar5.76s | [Log](e5-generation-review-rust-tests.log) |
| Clippy pipeline all-targets, `-D warnings` | **Compilación PASS, 2.06s** | [Log](e5-generation-review-clippy.log) |
| Py_compile de los cuatro archivos Python y diff whitespace | **PASS** | Verificación local de esta ejecución |

Los casos nuevos incluyen un proceso Python real con hello/verify/shutdown y snapshots de todos los archivos antes/después; nueve alteraciones de derivaciones/campos humanos con SHA nuevo; caché manipulado con target+checkpoint SHA coherentes que `run` reutiliza pero `verify` rechaza; cancelación antes y después de derivar sin mutar salidas; cambio de input entre derivación y rehash; envelope de verify cerrado. Las pruebas Rust atacan lineage, analysis extra, generation extra, conversation extra, heuristics y provenance, conservación obligatoria y añadidos no autorizados. Son fixtures pequeñas inventadas y stdlib, no resultados ASR nuevos.

[SHA256 de los cinco archivos congelados](e5-generation-review-freeze-sha256.txt): generation.rs `5d98071022bc458061eee615fdfb78d3d2463b63bfab56cc608bc66337fd7812`; finalize.py `6582fcc513a724cfa1b203f4c989c663fc9cd3728cfc899cbbb423d14bdc5584`; finalize_worker.py `c2daff13c196fbbb96b91a647fe40eab9d0bf8fba546045678dfa02731ce5323`; test_e5_finalize.py `9cdbabf113bcbabf760b0175e98445865212c91652577991ed5fa00cdc3929c9`; test_e5_finalize_protocol.py `a90a0639838bfe03dd146c5a3664616b6ac77129f5b2b57ea15fef40e05ec196`.

Fuentes entregadas al principal para construir/ejecutar **generation-host-02**, capturando los nuevos hashes al encolar y conservando el mismo parent y recibos MMS/arousal/risas. Esa ejecución integrada posterior y sus ataques no se atribuyen a estos tests hasta disponer de su evidencia. No se repitieron modelos, PyAV ni suites bloqueadas; no se cambió GUI, workflow ni política de seguridad. E5 completo y aceptación GUI continúan pendientes de los incrementos coordinados por el principal.
