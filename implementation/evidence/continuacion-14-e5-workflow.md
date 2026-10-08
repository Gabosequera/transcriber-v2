# Continuación 14 — E5 workflow durable (2026-09-30)

## Alcance y API

Implementación nueva en `crates/pipeline/src/workflow.rs`, con contratos en `workflow_tests.rs`, ejemplo `workflow_host.rs` y guion `scripts/test-e5-workflow-host.ps1`. Este frente no editó lib.rs, UI, workers Python, otros stages, V1 ni versiones; root declaró el módulo e integró la UI por separado.

DTO públicos Clone/Serialize/Deserialize y cerrados: AlignmentStep, ArousalStep, LaughterStep, WorkflowPlan, WorkflowPayload y WorkflowResult. API `enqueue(asr, plan, work_root, cancel)`, `run(record, resume, Arc<AtomicBool>, progress)` y `validate_result(record, result, cancel)`. El resultado contiene el JobRecord/GenerationResult final tipado. No incorpora un master ni muta proyecto; la incorporación conserva el camino de prepared command existente.

Plan secuencial: ASR original, por audio_index ordenado MMS opcional, arousal opcional que exige MMS del mismo plan, risas opcionales, y generación al final. Índices duplicados/vacíos y parámetros fuera de contrato se rechazan. Roots de roles distintos pueden ser descendientes, pero no el mismo root físico.

Al encolar se capturan SHA de fuente completa, modelo ASR, Python, FFmpeg, worker/lock ASR, código/Python de generación y todos los modelos/manifiestos/locks/workers/Python de stages seleccionados. Se revalidan antes y después de etapas. El parent común legacy conserva `ffmpeg_path="ffmpeg.exe"`: se resuelve ese nombre en PATH para capturar/revalidar bytes, sin modificar el payload ni ejecutarlo. Solicitudes nuevas con path absoluto siguen ese path. Cambios de recursos requieren workflow nuevo.

## Checkpoint y recuperación

Checkpoint atómico `.work/workflow.json` ligado a schema, workflow ID, payload digest, plan digest y resources digest. Mantiene referencias ID/payload digest de children en prefijo del orden exacto. Cada ID se persiste antes de ejecutar su child; la carga valida el JobRecord real contra parent original, parámetros/runtime, índice y hashes capturados. Un child ausente o modificado no se reemplaza silenciosamente.

Succeeded se verifica profundamente y se reutiliza sin ejecutar. Failed/Cancelled/Interrupted requieren resume explícito. Running sólo se recupera con lease propia después de detectar propietario perdido; un propietario activo conserva su bloqueo. Checkpoints no adoptan/religan jobs MMS/arousal/risas de un workflow distinto: el workflow crea sus children y reutiliza únicamente sus propias referencias durables.

La revisión de root identificó un mkdir durante carga/validación. Está corregido: resolver y load son readonly; validate exige checkpoint existente y workflow Succeeded con receipt exacto. Sólo save crea directorios tras validar ancestros, rechaza symlinks/reparse points/junctions en job/.work/archivo y repite la validación antes de atomic_write. Cancelación se comprueba tras guardar el child y emitir progreso, antes de ejecutar, y tras completar un child; así un child Succeeded puede recuperarse si el workflow fue cancelado justo después.

## Contratos ejecutados

`scripts/cargo.ps1 test --locked -p tv2-pipeline --lib workflow::tests`: **12 PASS**, 0 fallos, 0.28 s (compilación incremental 0.43 s). Log `.local/e5-workflow-contract-final.log`. Recursos inertes y jobs genéricos propios; no proceso Python, protocolo de inferencia falso ni librería nativa/modelo ejecutado.

Cubren lectura sin crear directorios; validación Succeeded con checkpoint ausente y sin mutación; .work de tipo archivo; junction Windows real hacia destino temporal propio que permanece intacto; headers/digests/parent/etapas ajenas; recurso cambiado antes de claim; child Queued persistido al cancelar antes de ejecutar; recuperación explícita de Failed/Cancelled/Interrupted; Succeeded sin nuevo execute/attempt; child perdido sin recreación; roots/índices/arousal→MMS; campos desconocidos y payload cambiado.

Primer lote tuvo 8/9 PASS: el test intentaba finish(Interrupted), operación correctamente rechazada por jobs. Se ajustó para drop lease + discover, el camino real de recuperación. Un intento posterior de compilación encontró E0583 editorial/tests.rs de otro frente mientras se creaba; no se modificó ese módulo y se esperó confirmación. Ambos resultados originales se distinguieron de la ejecución final.

`scripts/cargo.ps1 clippy --locked -p tv2-pipeline --lib --tests --example workflow_host`: **PASS**, sin warnings, 8.07 s; `.local/e5-workflow-clippy-final.log`. No suites bloqueadas 4551, cambio de política, release ni GUI.

## Host real de workflow, generación solamente

`.local/e5-workflow-host-02/run/host-result.json`: **PASS 6.7923905 s**, build 28.08 s incluyendo espera del lock Cargo. ASR parent inventado exacto `job-57fe5b97cf1c`, project `proj-5789225831cf`, revisión 1; no ASR inferido. Reutiliza receipt existente de `.local/e5-arousal-host-01` sin alterar sus bytes ni estado. Generación usa el worker stdlib real y el runtime instalado.

Workflow `job-422d7e03bf4c` termina Succeeded con 3 attempts. Generación `job-f1b2920875d8` termina Succeeded con **1 attempt**:

1. Cancelación después de checkpoint-child pero antes de run: child permanece Queued, sin attempts.
2. Resume=false rechazado. Resume=true genera con worker real; cancelación sólo tras el receipt durable del child deja workflow Cancelled y generación Succeeded.
3. Resume=true reutiliza ASR y generación verificadas, sin ejecutar otro child ni añadir attempts a generación.

Inventario completo de archivos propios (rutas, tamaños, mtimes y SHA) idéntico antes/después de validate_result; parent JSON exacto intacto. Resultado conservado en workflow-record.json/workflow-result.json y checkpoint. No incorporación, mutación de proyecto, ASR, MMS/arousal/risas ejecutados ni GUI en esta aceptación. El DAG completo con selección ML queda pendiente de aceptación del workflow, aunque sus hosts individuales y generación compuesta tienen evidencia separada.

Host01 se conserva: el filtro del guion canceló el progreso final del worker antes de finish durable, por lo que generación quedó Cancelled y falló la expectativa Succeeded. Se corrigió el filtro para distinguir boundary del workflow (detail sin event) de evento del worker y se ejecutó host02 en directorio nuevo. No fue AppControl ni retry de binario bloqueado.

Hashes del host02: report `7b3106c0ebf03d297aa9966bffdaf6774e926ad011cbfaa4b2e3850a63d09be5`; debug example `f721156c81856b556bf39c5a1adbfe16508aed8bacb17fb7911b24359e2c7101`. Fuentes y guion congelados tras los contratos/Clippy finales; no se cambiaron workers ni sus SHA de identidad.
