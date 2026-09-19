# Continuación 13 — E4 y comandos de historial

Trabajo sobre la implementación existente alpha.9, integrado en alpha.10. No se reconstruye el servicio ni se inicia E5. V1/datos personales intactos; no GUI, medios ni tests bloqueados ejecutados.

| Requisito / brecha demostrada | Símbolo y corrección | Dependencia |
|---|---|---|
| AI-01/02/03, especificación §8: undo/redo ausentes de comandos MCP | `Command::Undo/Redo`, `ProjectSession::{preparation_snapshot,prepare_command,commit_prepared}`, schema MCP: preparación histórica, actor Agent, preview, precondición de entrada exacta, pilas y recibos comunes | Protección de efectos del núcleo y almacenamiento existente; batches de historial rechazados |
| AI-02/03: 32 propuestas antiguas no podían rechazarse tras edición, agotando capacidad | `ControlEngine::review(false)` y botón Rechazar: rechazo sin exigir estado ejecutable/base actual; aprobación mantiene precondiciones | Sin cambios al proyecto; persiste rechazo/evento |
| AI-03/DAT-02: fallo del commit consumía PreparedCommand pero UI no ofrecía revalidación | `ControlEngine::apply`: revoca reviewed/automatic_eligible, marca needs_revalidation, archiva error/evento | Core commit atómico; nueva preparación y autorización obligatorias |
| PERF-01: reprepare local calculaba digest de proyecto en GUI | `poll_control` y `draw`: petición local privada, snapshot/digest en worker; revisión exacta al aplicar | No habilita omisión de digest remoto ni expone bypass de schema |
| AI-04: error HTTP stdio dejaba request sin respuesta; codificación dependía del terminal | `scripts/mcp-client.ps1`: error JSON-RPC correlacionado, UTF-8 explícito, no retry mutaciones ni respuesta a notificaciones | Cliente PowerShell 7; servidor/GUI real siguen pendientes |

## Verificación ejecutada

- `scripts/cargo.ps1 test -p tv2-application --lib prepared_history --locked`: **4 correctos**, 76 filtrados, 0,03 s; compilación 10,76 s. Primera pasada anterior de tres tests pasó; se amplió por revisión del principal a persistencia real e intercalado.
- `prepared_history_commands_keep_preview_stacks_actor_and_durable_retry`: preview/diff exacto, recibos Agent, replay, save_checkpoint/load_session, undo/redo durable.
- `prepared_history_interleaves_local_history_and_survives_autosave_recovery`: tres entradas, undo/redo de GUI y preparados intercalados, pilas de dos/una entradas, reapertura, autosave/recovery y retry sin cambio de revisión.
- `prepared_history_rejects_stale_base_changed_entry_and_nested_batch_atomically`: cambio de historial con proyecto idéntico, base/digest antiguos y batch anidado; estado/pilas intactos.
- `prepared_history_agent_cannot_remove_or_resurrect_human_decisions`: agente no retira ni resucita aceptación/edición humana.
- `pwsh -NoProfile -File scripts/test-mcp-client.ps1`: PASS para parseo, solicitud inválida, error HTTP correlacionado, ID Unicode y notificación sin respuesta. Puerto cero no conecta a aplicación/medios.
- Check workspace/all-targets previo correcto (7,26 s). Clippy application/control/desktop all-targets `-D warnings` correcto (3,94 s) después de historial/reprepare; el principal registra compilación y controles del árbol final, que incluyen la posterior corrección del commit fallido.

## Solo compilación / aceptación pendiente

No se ejecutaron ni reintentaron tests dominio/desktop/V1compat/control por Windows4551. Tres tests nuevos de control (25 totales) cubren historial MCP worker/review/apply/verify, rechazo de propuestas obsoletas y recuperación tras commit fallido; la prueba existente de restauración también comprueba rechazo sin PreparedCommand. Su estado final de compilación se registra en los controles integrados del principal, sin atribuirles ejecución.

No se afirma aceptación de interoperabilidad MCP contra editor vivo, GUI/foco/rendimiento, reproducción/export ni crash físico. Recorridos reproducibles actualizados en `implementation/control.md`. El historial remoto tiene la protección deliberada del actor agente: la autorización de permisos no habilita deshacer decisiones humanas protegidas ni cambiar evidencia inmutable. No queda un dispatcher remoto de undo/redo que eluda esas protecciones.
