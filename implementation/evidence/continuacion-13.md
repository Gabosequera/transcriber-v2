# Continuación 13 — revisión de brechas E2/E3/E4

Base comprobada el 19 de septiembre de 2026: `main` limpio en `a60e31fd0c908ded49c3a51f88d0f66aeed3c0c6`; `origin/main` coincide. Se conserva alpha.9 íntegra. No hay AGENTS.md aplicables en los ancestros ni dentro del checkout. V1 observado limpio, exclusivamente de lectura. No se ejecutan editor, FFmpeg, medios, modelos ni suites bloqueadas por Windows 4551.

## Inventario existente y clasificación

La implementación previa de inspector, gestos preparados, índices/composición, contratos V1, derivación, almacenamiento, watchers y MCP continúa vigente en [continuación 12](continuacion-12.md). Esta revisión contrasta recorridos concretos con sus símbolos; no cuenta registros de acciones como prueba de paridad. Tres subagentes separan E2, E3 y E4; el principal revisa multimedia e integra.

| Requisito | Brecha demostrada / símbolo | Resultado esperado y dependencia | Clasificación |
|---|---|---|---|
| LAY-02, MED-05 | `item_editor::RangeDraft`: el diálogo reparsaba todos los tiempos mostrados en milisegundos | Conservar ticks de cada borde no editado, también al eliminar filas; sin dependencia | Implementación corregida, tests desktop solo compilados |
| MED-02 | `resolve_jobs::step_target`: StepFrames diferido no alineaba la base al fotograma | Misma cuadrícula que player con composición pendiente | Implementación corregida, test desktop solo compilado |
| PERF-01 | `ui_panels::inspector`: recorría clips×rangos incluso con ocurrencias colapsadas | Consulta lazy en un worker, cache por selección/revisión, descartar obsoleto y virtualizar filas | Implementación corregida; medición física aplazada |
| DAT-03/04 | `ui_markers::MarkerEditor`: borrador sin base podía sobrescribir cambios concurrentes | Ligarlo a proyecto/revisión/secuencia y rechazar obsoleto | Implementación corregida; test desktop solo compilado |
| DAT-02/03 | `documents::validate_path_tree`: un archivo podía ser padre de otro output | Rechazar árbol imposible antes de intención y también en recovery; incluye aliases ASCII Windows | Implementación corregida; regresión application |
| AI-02, DAT-03 | `protection::check_transition`: batch desbloquear→editar→rebloquear y reconcile eludían bloqueo de pistas | Comparar pista y clips protegidos en la transición completa para actores no humanos | Implementación corregida; regresión application |
| LAY-02, DAT-03 | `protection::check_transition`: borrar capa vacía con tombstones eliminaba protección; reconcile reasignaba asset/tipo | Conservar tombstones e identidad de capas frente a Agent/External | Implementación corregida; regresión application |
| EXP-03, DAT-02 | `ExportJob::run`: temporales predecibles compartidos y `rename` tras un único exists | Carpeta temporal exclusiva; publicación `persist_noclobber`; SHA y flush previos | Implementación corregida; tests media solo compilados |
| EXP-01/03 | `wav_header`: tamaños RIFF truncados a u32 para mezclas largas | RIFF pequeño / RF64 grande sin truncamiento; WAV final usa `-rf64 auto` | Implementación corregida; tests de cabecera solo compilados |
| EXP-03, PERF-01 | Verificación ignoraba cancel durante probe/hash; stderr retenía salida completa | Probe y hash cancelables; drenar stderr conservando 16 KiB | Implementación corregida; tests media solo compilados |
| AI-02/04 | `ControlEngine::review`: no permitía rechazar propuestas obsoletas/restauradas | Rechazo local libera preparación independientemente de vigencia; aceptar conserva comprobaciones | Implementación corregida; tests control solo compilados |
| AI-04, OPS-01 | Cliente stdio omitía respuesta RPC ante error HTTP | Error correlacionado, sin retry automático de mutaciones; notificaciones sin respuesta | Implementación corregida; prueba de cliente separada |
| AI-01/02/03 | Undo/redo ausentes del contrato externo pese al requisito de sección 8 | Propuesta preparada, revisión/permisos/protección, recibo idempotente y movimiento real de pilas | Implementado; cuatro tests application dirigidos pasan, control solo compilado |
| PERF-01, AI-02 | Botón local Revalidar calculaba `session.digest()` en GUI | Calcular digest del snapshot en worker; clientes siguen obligados a suministrar digest | Implementación corregida; integración desktop compilada |
| AI-02/04 | Apply consumía preparación aun cuando fallaba commit y no ofrecía recuperación | Marcar needs_revalidation, revocar review/eligibilidad y publicar evento de fallo | Implementación corregida; regresión control solo compilada |

## Límites conservados

Permanecen los límites deliberados de continuación 12: metadatos, historia, undo de 200 operaciones, COW por capa, costes lineales de hidratación/validación y jobs en configuración. El worker de ocurrencias evita repetir el producto rangos×clips en GUI, pero el snapshot inicial de mapping aún recorre los clips. No hay certificación global PERF-01.

La publicación multimedia evita reemplazar un destino concurrente y limpia solo temporales propios. No promete atomicidad universal ni resistencia a corte eléctrico del directorio. El hash cancelable comprueba entre bloques; una llamada de IO bloqueada al sistema operativo conserva sus límites. Referencia del formato RF64: [FFmpeg wavenc](https://www.ffmpeg.org/doxygen/7.1/wavenc_8c_source.html), tamaños de 64 bits en ds64; consultada sin ejecutar FFmpeg.

## Verificación y aceptación

Application es la única suite Rust ejecutada aquí. Desktop/domain/V1compat/control y los tests media nuevos se compilan, sin ejecutarse. El cliente sintético no acredita conexión al editor. RUN y control.md conservan los recorridos reproducibles pendientes. E5 no se inicia.

La revisión cruzada corrigió dos invalidaciones adicionales del inspector: reapertura de copia con mismos IDs/revisión y cambio de rango anterior a la consulta dentro del mismo frame. El epoch de sesión y la consulta de rangos vigentes evitan publicar o reutilizar resultados antiguos. Otra revisión cruzada detectó el estado sin preparación tras commit fallido en MCP; ahora exige revalidar y revisar de nuevo.

Cuatro tests dirigidos de historial preparado pasaron: preview/pilas/actor/retry, stale/entrada histórica modificada/batch prohibido, protección humana y ciclo de tres entradas con undo/redo humano intercalado, guardado/reapertura real y autosave/recovery. Antes de los controles integrados, los tres arreglos E3 pasaron pruebas application; la regresión del árbol documental falló antes de corregirse por persistir la intención inválida. Los logs finales siguientes identifican el árbol integrado, sin presentar controles parciales como aceptación.

Estado del backlog de implementación revisado: todas las brechas demostradas de esta tabla están corregidas. No se identificó otra brecha concreta de código en los recorridos examinados. Esto no equivale a revisión general aceptada: las pruebas automáticas bloqueadas y la aceptación física pueden revelar defectos nuevos. Contratos/passes V1 examinados conservaron la implementación previa; no se ampliaron automáticamente sus límites.

## Controles integrados finales

| Comando | Resultado y evidencia |
|---|---|
| `scripts/cargo.ps1 test -p tv2-application --locked` | 80 unitarios correctos en 14,65 s; 3 integraciones correctas en 0,00 s; 0 doctests. Compilación 23,47 s. [Log](application-continuacion-13.log) |
| `scripts/cargo.ps1 check --workspace --all-targets --locked` | Correcto, 9,52 s. [Log](check-continuacion-13-final.log) |
| `scripts/cargo.ps1 clippy --workspace --all-targets --locked '--' -D warnings` | Correcto, 4,71 s. [Log](clippy-continuacion-13-final.log) |
| `scripts/cargo.ps1 fmt --all --check` | Exit 0, sin diferencias. [Log vacío de éxito](fmt-continuacion-13-final.log) |
| `scripts/test-mcp-client.ps1` | PASS: parseo, solicitud inválida, error HTTP correlacionado, ID Unicode y notificación sin respuesta. [Log](client-continuacion-13.log) |
| `git diff --check` | Exit 0; sin errores de whitespace |

All-targets comprueba también los seis tests desktop nuevos, los tres control nuevos (25 totales) y los cuatro media nuevos; **no se ejecutaron**. No se ejecutó ningún FFmpeg/FFprobe, editor ni suite bloqueada. V1 observado limpio al terminar las comprobaciones. Ver detalles [E2](continuacion-13-e2.md) y [E4](continuacion-13-e4.md).

Build release final correcto: `scripts/cargo.ps1 build --release --locked -p transcriptor`, 3 min 03 s de Cargo / 185,77 s con wrapper. Ejecutable de 30.676.992 bytes, SHA-256 `9d391dbcb776ae1d61e5f7fceee91b1a1cd3f7e0958eeebee3f7e9977050fb46`. [Metadata](build-alpha10.json) y [log](build-continuacion-13.log). Ejecutable **no lanzado**, no asset binario de prerelease. Publicación de fuentes autorizada; identidad remota se añade tras verificarla.
