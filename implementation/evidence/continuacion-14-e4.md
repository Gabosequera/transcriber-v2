# Continuación 14 — E4, cliente y aceptación conectada

Autorización vigente: pruebas físicas y E5 autorizadas por el nuevo encargo; los aplazamientos de continuación 13 son históricos. Trabajo delimitado a cliente/scripts E4. No se inició V1 ni se modificó su checkout/datos personales. El agente principal coordina GUI y diagnóstico Windows 4551; este frente no reintentó binarios bloqueados.

## Defecto reproducido y corregido

AI-04 / `scripts/mcp-client.ps1`, rama stdio: `Invoke-RestMethod` representa la respuesta HTTP `202 Accepted` sin cuerpo como una cadena vacía. La condición anterior `$null -ne $result` serializaba esa cadena y escribía una línea `""` a stdout para `notifications/initialized`. El test anterior con puerto cero solo ejercitaba fallo HTTP, por lo que no cubría este caso exitoso.

Reproducción inicial con `scripts/test-mcp-client-http.ps1`: **falló**, `Successful stdio HTTP forwarding mismatch`. Salida observada: respuesta initialize, línea `""`, respuesta tools/call. Se conserva este intento fallido como defecto del cliente, independiente del bloqueo de binarios.

Corrección: el puente escribe respuestas únicamente para requests con ID y cuerpo no vacío. Una notificación nunca genera respuesta stdio, aunque el transporte retorne contenido. No se agregó retry automático de mutaciones.

## Pruebas realmente ejecutadas

Sobre el árbol que contiene la corrección:

- `pwsh -NoProfile -File scripts/test-mcp-client-http.ps1`: **PASS**, 2 escenarios HTTP loopback sintéticos. initialize conserva ID `árbol`; `tools/call` con rechazo de propuesta conserva ID 7 y `isError`; notificación HTTP202 no responde; mutación enviada una vez; cliente CLI initialize/notificación/verify recorre pending/null → complete/true, ID final 3. Listener TCP efímero y proceso cliente propio se limpian en finally. No editor, ControlEngine ni medios.
- `pwsh -NoProfile -File scripts/test-mcp-client.ps1`: **PASS**, parseo/solicitud inválida/error HTTP correlacionado, UTF-8 y notificación sin respuesta en fallo HTTP. Puerto cero, sin editor/medios.
- Parser PowerShell de `scripts/test-mcp-editor.ps1`: **sin errores**. Esto verifica sintaxis; no prueba ejecución de sus etapas contra editor.

No hubo cambios Rust en este frente; no se atribuyen tests control compilados a tests ejecutados. La suite de control mantiene su estado de bloqueo hasta el diagnóstico/cambio de evidencia del principal.

## Cliente de aceptación real preparado

`scripts/test-mcp-editor.ps1` usa el cliente HTTP existente y requiere editor vivo, fixture/config aisladas V2 y endpoint publicado por su panel. El token se solicita con `Read-Host -AsSecureString` cuando no está en el entorno; nunca se serializa en evidencia ni se envía al chat. El cliente no inicia el editor, concede permisos, revisa propuestas ni sustituye la aprobación local. `-BuildPath` y `-FixturePath` opcionales añaden SHA256 del ejecutable/project.json a la evidencia; no usar proyectos personales.

Recorrido corto para participación humana/principal:

1. Abrir build conocido sobre fixture V2 aislada; abrir **Control externo** e **Iniciar en lectura**. Copiar endpoint al comando. Ejecutar `pwsh -NoProfile -File scripts/test-mcp-editor.ps1 -Endpoint 'http://127.0.0.1:PUERTO/mcp' -EvidencePath '.local/acceptance-e4/read.json' -Stage Read`; pegar token solo en el prompt oculto. La carpeta de evidencia debe existir. Esperado: consultas reales de clips/capas/secuencias/events, sin mutación.
2. Conceder **propuesta y aplicación** en el panel, conservando alcance automático vacío para probar revisión. Ejecutar otro destino nuevo: `pwsh -NoProfile -File scripts/test-mcp-editor.ps1 -Endpoint 'http://127.0.0.1:PUERTO/mcp' -EvidencePath '.local/acceptance-e4/rename.json' -Stage Prepare -Interactive`. Esperado: preview/diff, contexto sin cambios, retry de propuesta conserva ID, apply sin review rechazado. Se preparan dos propuestas.
3. Revisar/aprobar **ambas** propuestas en GUI sin aplicar ni editar el proyecto; volver al prompt y pulsar Enter. Esperado: commit de la primera con exact_prepared_state, verify hash completo y recibo vivo, revisión +1, retry apply sin cambios y mismo recibo, apply de la segunda con E_STALE_REVISION, query de base antigua rechazada y evento proposal_applied. Observar visualmente que el nombre nuevo aparece en la GUI. El script registra respuestas, IDs/digests/revisión; no sustituye esa observación visual.
4. Para undo/redo remoto repetir Prepare/Interactive con `-CommandType undo` o `redo` y un archivo nuevo por escenario. Para undo GUI tras rename: Ctrl+Z y ejecutar `-Stage LocalUndo` sobre la evidencia rename; debe restaurar nombre anterior con nueva revisión. No mezclar este paso con otro cambio editorial.
5. Revocar **aplicación** en GUI y ejecutar `-Stage Revoked` sobre escenario preparado/aplicado. Esperado: apply deja de anunciarse y una llamada explícita se rechaza. Guardar proyecto, cerrar/reabrir/start-read-only, cambiar endpoint/token y ejecutar `-Stage Restart`: identidad de proyecto igual/sesión nueva, permisos de escritura/alcance automático revocados, propuestas conservadas sin review. Propuestas restauradas pendientes necesitan reprepare/review.

Los fallos de etapas posteriores escriben una evidencia `.failed-UUID.json` y preservan el escenario preparado para diagnóstico/retry explícito. Read/Prepare exigen destino nuevo para conservar evidencia existente. Los PASS de cada etapa solo atribuyen los asserts ejecutados sobre la sesión, no audio físico/foco/fluidez/persistencia general.

## Aceptación pendiente y dependencias

Este frente **no ejecutó** estas etapas contra editor; MCP conectado, revisión/aplicación/GUI, revocación/reinicio y persistencia quedan pendientes de acción humana o automatización GUI del principal. La GUI alpha10 fue lanzada por el principal en su frente independiente; esto no acredita E4. Tests sintéticos HTTP y parseo de script no cierran E4.

## Ejecución cliente HTTP → editor nativo

`scripts/test-mcp-editor-scripted.ps1` ejecutado realmente con config/proyecto/medio sintéticos aislados. Resultado final [log](mcp-real-continuacion-14-http-fixed.log), folder `local/mcp-real-20260930-103816-2ac201d0`, SHA exe `8ba5193593d6d7eaf406169bf88158af3ecdbf98a6d4fc5f25cc9ac9c4aabd26`, fixture SHA `ffb48c13e1b626ec2f73c3144147b689a153cf4a3d5017ce233fd5b3a4061f67`. **PASS en14.16s**. El guion nativo termina **fallos=false**, guardado/reapertura revisión5. Token entregado a memoria por pipe CurrentUserOnly; no escrito en evidencia.

- Read consulta clips/capas/sequences/events reales.
- Rename, undo y redo: prepare/dry-run, commit exacto, revisión única, verify SHA/receipt completo, replay mismo receipt sin revisión extra, stale rechazado y query de revisión antigua rechazada; eventos presentes.
- Grants automáticos limitados a esos tres comandos desde `--script` local, autoridad equivalente al panel. No se declara aprobación visual de cada propuesta por una persona.
- Revocación local elimina apply anunciado y rechaza llamada. Puede revocarse solo apply o todo permiso; el cliente no presupone permiso de lectura después de revocar todo.
- Save/Stop/Open/servicio read-only: sesión nueva, proyecto igual, propuestas persistidas sin reviews ni grants automáticos.
- [TCP demorado](local/mcp-real-20260930-103816-2ac201d0/delayed-http.json): conecta200ms antes de headers, divide headers100ms y cuerpo100ms; pingHTTP200 correlacionado,470ms. Cliente y servidor reales.

Fallos previos preservados: primer escenario JSON incompleto de Permissions no llegó al guion (helper recogió solo su proceso); segundo recorrido logró tres Apply y falló el cliente al esperar tv2_context después de revocar todos los permisos; tercero tuvo reset de conexión después de apply exitoso; build instrumentado identificó RPC initialize fallido47ms/Socket10054 y servidor **headers WouldBlock/WSA10035**. No se reintentaron mutaciones inciertas dentro de un mismo proyecto.

La conexión aceptada Windows heredaba FIONBIO del listener. `serve` pasa expresamente a blocking con timeouts3s en pool acotado; la GUI no bloquea. [Microsoft accept](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept) documenta propiedades heredadas. La prueba demorada y todo el recorrido pasan después de esta corrección. Diagnóstico registra únicamente fases/tipos/códigos, no token/header/argumentos/endpoint. Clippy integrado correcto; no ejecución de suite control4551.

La revisión automática rechazó una prueba adicional de privacidad por subprocess sintético con `blocked by policy`, sin detalle. No se ejecutó ni se eludió; whitelist/hash de IDs/no messages se revisaron en código y los dos clientes sintéticos sí se ejecutaron. No presentar esa prueba extra como PASS.

Backlog AI-01–04 restante: observación/revisión GUI humana, transporte/jobs/audit/import/export/evidencia completa, terceros MCP y LLMs, suite Rust control pendiente diagnóstico4551. El recorrido nativo descrito acredita sus casos concretos, no aceptación global E4 ni E5.
