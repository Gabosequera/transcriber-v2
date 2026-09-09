# Continuación 7 — implementación editorial, portapapeles y recibos durables

Fecha: 2026-09-08. Base main limpio/sincronizado `9287a0212bc55f861c2757af795c4cbaeb2f394a`. V1 `bb7012c5bc22403908b6dfb48fe721c67045b6b5`, limpio; solo lectura de keymap/chunks y fuentes relacionadas, sin ejecutar módulos, modelos ni medios personales. Checkout V2 existente conservado.

## Cambios y alcance

STATUS y decisiones D-0035–D-0038 describen contratos y límites. E2/E3 abiertas; E4 pendiente; E5 no iniciada. Este checkpoint no certifica implementación completa ni aceptación de entregas.

- Import V1 asíncrono/cancelable, worker único y canal bounded(1), revisión/proyecto capturados, batch con asset/master/capas/montaje. Error de documento reconocido aborta. Fallos y cambio de revisión señalan error de guion; cancelación descarta resultado. Las lecturas bloqueadas no son interrumpibles inmediatamente; se conserva el slot hasta terminar.
- Editor persistente de texto/comentario/multirrango/padre con base de revisión y batch. Clipboard semántico con descendientes y nuevos IDs; padres internos remapeados, raíces desligadas de padres no copiados. Cortar solo después de copiar. Ctrl+A por capa. Copia de clips conserva props y enlaces nuevos; duplicación/borrado multicapa atómicos.
- Import/export/restauración keymap. Auditoría AST **69 V1 / 81 V2**, ninguna V1 ausente; cambio Ctrl+E para export. Corrige conteos históricos; no es inventario completo de botones/gestos ni prueba funcional.
- Export documental en GUI/worker: capas user/topics/ai y montaje importado intacto. Original completo conservado para no perder clips tapados/desactivados. Edición V2, falta de original, medios múltiples, precisión subms y carriles sin adaptador dan error. No exporta carpeta V1 completa ni resuelve inverso editado.
- Journal con recibos request/result/project; recuperación valida y reserva claves legacy. Reapertura y Save As conservan auditoría/reintentos. Autosave/1 snapshot+audit en replace único, legacy legible, worker y recuperación explícita con undo. Historial/jobs no durables todavía; guardar manual sigue síncrono.

## Controles exactos

- `check --workspace --locked`: pasa al integrar, repetido tras cambios de comandos/GUI. Primer control al iniciar también solicitado; no hay log persistente de su salida inicial, por lo que no se usa como evidencia de cierre.
- `test -p tv2-application -p tv2-v1compat --lib --locked`: **24 + 15 = 39 pasan** en alpha.4, `unit-continuacion-07.log`.
- `clippy --workspace --all-targets --locked '--' -D warnings`: pasa; `clippy-continuacion-07.log`. Compila tests desktop (keymap y worker) sin ejecutarlos.
- `fmt --all --check`: exit 0, sin salida; registro transcrito en `fmt-continuacion-07.log`.
- `build --release --locked -p transcriptor`: **OK, 2m51s**, `build-continuacion-07.log`, tamaño/SHA256 en `build-alpha4.json`. Exe no ejecutado ni empaquetado.

Los nuevos tests de application cubren rollback de colisión al pegar, props/enlaces después de cortar, copia de jerarquía/multirrango/comentarios, edición de padre/ciclo/hijos/undo, recibos tras reapertura y corrupción/retry legacy, autosave con auditoría y recuperación/undo/reapertura. V1 añade inverso intacto con material invisible y campos desconocidos, rechazo de cambios, export de capa con precisión/padre/comentario y rechazo de carpeta reconocida inválida.

No se intentó ejecutar dominio/desktop por el bloqueo Windows 4551 previo; no se alteró ni eludió ninguna política. No hubo prueba GUI/multimedia/medios/performance/revisión general, conforme al aplazamiento.

Fallos intermedios corregidos: conversión IO→DomainError en GUI; visibilidad de after_change; ruta DomainResult; helper de test mal nombrado; conversión String→ItemId; Clippy collapsible_if/useless_conversion. Un test de export comparaba representación interna de `origin` duplicada en extra con el input manual; se corrigió para comparar documentos V1 canónicos más parent/comment, que es el contrato probado. No se cambiaron goldens usando V2. Logs finales identificados arriba sustituyen esas ejecuciones fallidas como resultado vigente.

## Límites siguientes

Split/trim/nudge de items y ciclo autor, chunks/autor/bloques/trims completo/orden/derivación/manifests/requests/passes, inverso editado/carpeta V1/multidocumento, historial/jobs/migraciones/archivado de auditoría, watcher SO/V1/diff/resolución y E4 completo. Protección de decisiones humanas se conserva, pero atribución AI de edited/aceptación aún no completa. Slots de worker limitan concurrencia; no hay límites globales de lectura de todos los JSON, ni benchmark de clones/hash de masters. Worker autosave conserva snapshot, pero falta auditoría de dos instancias/terminación de proceso en cada frontera. UI review queda aplazada, no debe confundirse con implementación cerrada.

## Publicación

Checkpoint de fuentes alpha.4 autorizado por interrupción de contexto; sin assets binarios ni datos personales. Resultado remoto, commit/tag y verificación se registran en PUBLISH y publication-alpha4.json al completar publicación. Main llevará después el commit documental del resultado.
