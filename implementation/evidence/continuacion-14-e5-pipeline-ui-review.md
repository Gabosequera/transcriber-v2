# Continuación 14 — revisión cruzada de pipeline UI y namespace ASR

Revisión de fuentes de `apps/desktop/src/pipeline_ui.rs` y lectura de los gates correspondientes de workflow/generation/application, sin ejecución de modelos, GUI o tests. El único cambio de código autorizado por root fue pipeline_ui.rs.

## Hallazgo y corrección

En `TranscriptorApp::start_analysis`, una solicitud nueva persistía primero un ASR Queued dentro de `analysis/jobs`, y después llamaba a `workflow::enqueue`. Esta segunda operación captura hashes de recursos y puede fallar o cancelarse antes de persistir el workflow. `scan_analysis` sólo ocultaba ASR ya referenciados por workflows existentes. Por tanto, ese ASR huérfano aparecía como transcripción antigua recuperable: `Reanudar solicitud` recorría SavedAnalysis::Asr y permitía preparar/incorporar sólo ASR, sin el plan MMS/arousal/risas que se había elegido.

Root autorizó separar el namespace. Los ASR de solicitudes nuevas por workflow ahora se encolan con `runtime.work_root/<workflow-asr>`, dejando el descubrimiento legacy no recursivo en `runtime.work_root/jobs`. La carga del workflow usa la ruta capturada del parent; sus checkpoints, bindings y recursos conservan esa identidad. Si falla/cancela la captura previa al workflow, el ASR Queued queda como evidencia en el namespace separado y no se ofrece como análisis completo. Un reintento explícito nuevo crea su propio job. Los jobs legacy existentes mantienen su ruta.

## Otras comprobaciones por lectura

No se identificó otro defecto accionable en la conservación o incorporación: resultados finalizados se conservan durables; errores de preparación quedan asociados al record y pueden revisarse explícitamente; revisión en snapshot actual prepara en background; Apply exige matches_base y commit_prepared vuelve a comprobar la base; errores de commit conservan el record; cancel usa token owned y su Drop; progreso se limpia al terminar; los callbacks de progreso son no bloqueantes; controles de incorporación no aplican automáticamente. La verificación del grafo completo y los hashes se ejecuta fuera de la GUI. matches_base usa igualdad del Project compartido, sin serializar JSON en cada frame.

La revisión no acredita interacción GUI, cancelación física/crash en ese punto ni inferencia ASR. El camino de fallo descrito se demuestra por el orden de llamadas y rutas/discovery presentes en las fuentes; no se ejecutó un reproducer que cargase DLL bloqueadas. No se añadieron pruebas que sólo reprodujesen la implementación; la compilación valida la integración, y la exclusión del namespace se apoya en la lectura no recursiva de jobs::discover.

## Verificación y freeze

`scripts/cargo.ps1 clippy --locked -p transcriptor --all-targets`: **PASS**, 13.10 s, sin warnings. Log `.local/e5-pipeline-ui-asr-namespace-clippy.log`. Se compilaron targets; no se ejecutaron suites 4551 ni binarios de escritorio. No cambios a app/scripting/semantic_jobs, otros módulos, workers Python, modelos, versiones o V1.

pipeline_ui.rs congelado SHA256 `5fa721eb62ced8bd37a04b8fc9494d5f13daaa64e8267bf5bdbc8af3dd4e10f7`.
