# Continuación 14 — E3: guardado con mtime real y muerte de proceso

2026-09-30. Base preservada: `main`, HEAD `0e4f10a4f90bc672f3264c080cc4449a44ced644`. Trabajo coordinado con E2/E4/principal; este frente modifica únicamente `crates/domain/src/asset.rs`, tests de `crates/application/src/store.rs` y esta evidencia. No cambia versiones ni publica. V1 fue leído exclusivamente; no se ejecutaron V1 ni sus entornos.

## Defecto reproducido y corregido

E2 detectó en el alpha.10 real (SHA-256 `9d391dbcb776ae1d61e5f7fceee91b1a1cd3f7e9977050fb46`) el error de guardado `JSON inválido: i128 is not supported`: `implementation/evidence/local/prepared-20260930-132329-077910/e2-escenario1.log`, paso 54 y log de aplicación a las `13:23:56Z`. El guardado fallido impidió reabrir el proyecto; los exports siguientes encontraron una secuencia vacía. El principal/E2 conservan esos artefactos y repiten el recorrido tras reconstruir.

La regresión `store::tests::file_mtime_roundtrips_through_import_audit_checkpoint_history_and_autosave` reproduce el mismo error con `mtime_ns` tomado de metadata de un archivo sintético nuevo. Serializar el envelope ImportAsset funciona; deserializarlo falla: `tagged ImportAsset with real nanosecond mtime must decode: Error("i128 is not supported", line: 1, column: 909)`. No es truncamiento del tiempo ni fallo del encoder: la deserialización de `Command` internamente etiquetado pasa por el buffer Content de Serde, que no implementa `deserialize_i128`. Los mocks antiguos llevaban `mtime_ns: None`.

`Fingerprint.mtime_ns` conserva `Option<i128>` y su JSON numérico. `deserialize_mtime_ns` lee el entero JSON firmado/no firmado de 64 bits y lo ensancha a i128 sin perder nanosegundos. Conserva ausencia y null; rechaza fracciones, strings y booleanos. No altera la identidad `size/hash_muestreado/inventario_sha256`, no convierte tiempos a flotante y no modifica medios.

La regresión ya ejecutada comprueba envelope, auditoría, save/checkpoint, reopen, recibo idempotente, undo/redo, autosave/recovery y Save As. `mtime_integer_contract_preserves_signed_unsigned_null_and_missing_values` verifica además -1, cero, i64::MAX, i64::MAX+1, u64::MAX, ausencia/null y rechazo de tipos incorrectos. La repetición del recorrido GUI en un ejecutable corregido corresponde a E2/principal; este frente no la declara aceptada a partir de tests.

## Durabilidad observada con procesos aislados

`store::tests::process_crash_recovers_commit_boundaries_and_releases_os_lock` crea fixtures propiedad de V2, lanza el mismo ejecutable de tests application como hijo, espera un handshake duradero y mata únicamente ese hijo conocido. El hijo mantiene el lock real del SO. Antes de matarlo, otra instancia de ProjectStore devuelve `ExternalConflict`; tras la muerte, el SO libera el lock y el mismo almacén puede recuperar.

Se ejecutaron diez escenarios: cinco fronteras para commit/1 y cinco para commit/2 con master sintético de 100 KiB externalizado. Fronteras: intención publicada, proyecto publicado, índice de auditoría publicado, history publicado y retirada final de intención. Cada uno comprueba nombre/revisión exactos, master cuando corresponde, un único evento/recibo, ausencia final de intención, segunda reapertura y undo/redo. El helper `crash_boundary_child` queda ignored en el listado normal y es invocado diez veces por la prueba padre; no representa una prueba pendiente.

Las fronteras se preparan explícitamente con las funciones de persistencia existentes; la muerte del proceso y el lock son reales. Esto verifica recuperación al morir con esos estados durables, sin afirmar corte eléctrico, rotura del disco ni interrupción aleatoria durante una llamada de escritura de la GUI. No se agregaron hooks ni switches de fallo al código de producción. Ningún binario fue movido/renombrado, ni se modificaron protecciones Windows.

## Ejecución exacta

Fixtures dentro de `C:\Users\gabri\Todo\transcriber-v2\.local\e3-14-fixtures`; los comandos configuraron TEMP y TMP a esa carpeta durante su ejecución. TempDir elimina solamente sus propias fixtures al finalizar. Logs archivados por el principal en esta carpeta de evidencia:

| Comando / resultado | Evidencia |
|---|---|
| `scripts/cargo.ps1 test -p tv2-application --locked file_mtime_roundtrips '--' --nocapture`: antes del fix, un test ejecutado/fallido por i128 | [e3-mtime-before.log](e3-mtime-before.log) |
| Misma orden después del fix: 1 test correcto en 0,27 s; compilación 39,11 s | [e3-mtime-after.log](e3-mtime-after.log) |
| `scripts/cargo.ps1 test -p tv2-application --locked process_crash_recovers '--' --nocapture`: 1 test correcto, diez kills/reopen; 0,71 s; compilación 19,05 s | [e3-process-crash.log](e3-process-crash.log) |
| `scripts/cargo.ps1 test -p tv2-application --locked`: **83 unitarios ejecutados correctos**, 1 helper ignored, 14,51 s; **3 integraciones correctas**, 0,00 s; 0 doctests. Compilación 36,67 s | [e3-application-final.log](e3-application-final.log) |
| `scripts/cargo.ps1 clippy -p tv2-application -p tv2-domain -p tv2-v1compat --all-targets --locked '--' -D warnings`: correcto, 5,05 s | [e3-clippy-final.log](e3-clippy-final.log) |
| `scripts/cargo.ps1 fmt -p tv2-application -p tv2-domain -p tv2-v1compat --check`: exit 0 | Salida vacía de éxito |
| `git diff --check -- crates/application crates/domain crates/v1compat`: exit 0 | Solo aviso LF→CRLF habitual |

Identidad al terminar esos controles (antes de cambios posteriores del principal): SHA-256 de `asset.rs`: `293f4e3e75dfc7845853435b96305e06684e06e73a9b3a69c0f5352a9b47c4c3`; `store.rs`: `a9b1faf57dd962e0185c16a0fd6513cfe2fcded3e4f17fb0a602327cb3248e69`; ejecutable application `target/debug/deps/tv2_application-761e5409f1147582.exe`: `9f580f029b39d6cf3b1af208818cb61110030b750ff33732423f1e274fb075f9`.

Intentos conservados: el primer comando dirigido omitió comillas alrededor de `--` en PowerShell y Cargo rechazó `--nocapture` antes de compilar; se corrigió el argumento. Un primer full-test `--locked` se detuvo antes de compilar porque el principal estaba incorporando `crates/pipeline`; se ejecutó correctamente tras su actualización de Cargo.lock. Dos pruebas exploratorias suponían que cualquier Disabled AI/edited=false debía protegerse como decisión humana; fallaron y se retiraron al comprobar V1 en lectura (`editorial_layers.py:410`, `editorial_montaje.py:1003–1004`) y discutir la semántica con el principal. No se cambió la protección global basándose en esa hipótesis; [e3-disabled-before.log](e3-disabled-before.log) conserva sus fallos. Los contratos de propuestas siguen teniendo sus propias reglas más conservadoras de merge.

## Backlog E3 vigente

| Requisito / símbolo | Resultado esperado / dependencia | Estado |
|---|---|---|
| DAT-02/04, `Fingerprint`, auditoría/history de ImportAsset | Guardar/reabrir con metadata real y preservar recibos/historia | Defecto reproducido y corregido; núcleo ejecutado. GUI corregida en repetición E2/principal |
| DAT-02/04, `ProjectStore`, commit/1 y /2 | Lock de segunda instancia y recovery tras muerte abrupta en fronteras durables | Diez escenarios ejecutados correctos; no prueba eléctrica ni crash aleatorio GUI |
| DAT-01, LAY-01/02/03, `tv2-v1compat` contratos/passes/derivación/interchange | Suite de compatibilidad y recorridos de copia V1 completa/temas dos pasadas/derivación real/NLE | All-targets compilado por Clippy; suite V1compat no ejecutada por bloqueo Windows4551 vigente. No se trasladaron tests antiguos a otro binario |
| DAT-03, reconciliación/watchers | Cambios válidos/parciales, conflicto humano, revisión obsoleta con GUI abierta | Núcleo application ejecutado; recorrido watcher/GUI físico restante |
| DAT-01/02/04, Save As/bundles | Traslado, pérdida de fuente auxiliar, reapertura/export/undo coherentes | Tests existentes application ejecutados; recorrido GUI completo del build final pendiente |
| DAT-02, publicación documental | Crash de proceso durante publicación multidocumento y conflictos externos en copia V2 | Fronteras sintéticas existentes application ejecutadas; muerte de proceso sobre ese publicador/GUI pendiente |

El principal investigó Smart App Control y no encontró evidencia de cambio de la causa que autorice reintentos idénticos de domain/V1compat; E3 no reintentó esas suites. Su compilación no se presenta como ejecución. E3 permanece abierta a las comprobaciones de compatibilidad/aceptación pendientes; los éxitos de application no certifican toda la etapa ni E5.

## E4: diagnóstico del reset HTTP e instrumentación segura

Revisión del intento nativo `implementation/evidence/local/mcp-real-20260930-101224-a4c00bb5`: el cliente completó `tv2_context` y `tv2_apply` (`matches_preview=true`, `replayed=false`) y falló en menos de un segundo al enviar la siguiente llamada. No identifica todavía la causa ni demuestra fallo dentro de `tv2_verify`: `mcp-client.ps1` realiza `initialize` y `notifications/initialized` antes de cada tools/call. El log del editor únicamente contiene un conflicto de guardado durante la limpieza posterior del harness. Los límites de lectura 3 s/respuesta 30 s/cliente 35 s no explican por sí solos un fallo tan temprano. Pool y llamadas secuenciales tampoco prueban saturación.

Se añadió exclusivamente diagnóstico a `crates/control/src/transport.rs` y `scripts/mcp-client.ps1`, sin cambiar timeouts, cierre/lectura/escritura, replies, retries ni autorización. `trace_io` devuelve el mismo `io::Result` y escribe solo fase fija (`headers`, `body`, `response_header`, `response_body`), `io::ErrorKind` y `raw_os_error` en stderr; errores al escribir el diagnóstico se ignoran. No escribe contenido HTTP, auth ni endpoint.

`Send-Rpc` escribe JSON de diagnóstico por RPC en stderr: método/herramienta de listas cerradas, ID entero o SHA256 de ID string, duración, complete/failed y cadena acotada de tipos de excepción; SocketException añade código/número del SO. Métodos/herramientas desconocidos se sustituyen por `other`/`none`; no se escriben argumentos, mensajes de excepción, headers, token ni endpoint. Se retiró el diagnóstico stderr previo que imprimía el mensaje bruto de excepción de stdio. Stdout conserva exclusivamente las respuestas RPC existentes, IDs UTF-8 originales y respuestas silenciosas para notifications. Las mutaciones no se reintentan.

| Control dirigido | Resultado / evidencia |
|---|---|
| `scripts/cargo.ps1 check -p tv2-control --all-targets --locked` | PASS, compilación 6.14 s; [e4-transport-check.log](e4-transport-check.log). No ejecución de tests control |
| `scripts/cargo.ps1 clippy -p tv2-control --all-targets --locked '--' -D warnings` | PASS, 3.51 s; [e4-transport-clippy.log](e4-transport-clippy.log). No ejecución de tests control |
| `scripts/test-mcp-client.ps1` | PASS: parse/invalid request/fallo HTTP correlacionado/ID UTF-8/silencio notification; [e4-client-diagnostic-stdio.log](e4-client-diagnostic-stdio.log) |
| `scripts/test-mcp-client-http.ps1` | PASS: forwarding HTTP/UTF-8/rechazo, 202 silencioso, mutación enviada una vez, polling pending→complete; [e4-client-diagnostic-http.log](e4-client-diagnostic-http.log). Endpoint sintético, no ControlEngine/editor |
| Rustfmt específico, parse sintáctico PowerShell y diff whitespace | PASS después de ajustar formato de llamada; aviso habitual LF→CRLF |

Una prueba adicional ad hoc de privacidad con subprocess/inputs inventados fue rechazada antes de ejecución por la revisión automática del comando: razón devuelta `blocked by policy`, sin explicación más específica. No se reintentó ni se eludió. La whitelist/ausencia de mensajes/headers y hash de ID string están revisados en código; esa prueba extra no se declara ejecutada. No se ejecutó todavía el siguiente harness nativo con instrumentación, ni suites control bloqueadas4551. La instrumentación permite identificar cuál de initialize/notification/tools-call y qué fase IO falla; no constituye una corrección causal del reset.
