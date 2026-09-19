# E2 — auditoría de editor, 2026-09-19

Incremento sobre main posterior a alpha.9, conservando implementación previa. No se ejecutaron GUI, medios ni tests de desktop/dominio/V1compat/control; Windows 4551 no se reintentó ni se eludió.

## Brechas demostradas y corregidas

| Requisito | Archivo/símbolo | Defecto observado en código | Resultado y dependencia |
|---|---|---|---|
| MED-05, DAT-01 | `item_editor.rs::ItemEditor::command`, `RangeDraft` | Al editar solo etiqueta/comentario, todos los tiempos se reconstruían desde la representación en milisegundos, perdiendo ticks originales. | Cada fila conserva su rango original y cada borde sin cambios conserva ticks exactos; quitar filas no reasigna bases. Borde editado usa validación existente. Mantiene preparación/comando común y política explícita de snap de bloques. |
| MED-02 | `app.rs::player_send`, `resolve_jobs.rs::step_target` | StepFrames pendiente sumaba el salto desde una posición subframe; player normal usa primero floor_to_frame. | Misma cuadrícula, duración de fotograma y clamp que el player; sin dependencia E3/E4. |
| PERF-01, MED-05 | `ui_panels.rs::inspector`, `inspector_occurrences.rs` | Cada frame recorría rangos × clips y ordenaba ocurrencias, incluso con sección cerrada. | Consulta solo al desplegar, un worker, snapshot de clips sin evidencia extra, resultados compartidos, filas virtualizadas, clave proyecto/revisión/secuencia/capa/item, descarte tardío, error/reintento. Conserva mapping de `Sequence::range_occurrences`. |
| DAT-03, DAT-04, MED-03 | `ui_markers.rs::MarkerEditor`, `draw` | El borrador de marcador se guardaba completo contra la revisión actual aunque hubiera cambiado el proyecto desde su apertura. | Base proyecto/revisión/secuencia capturada; aviso y guardado deshabilitado cuando stale; envelope con revisión base, mismo núcleo y undo. Solo convierte tiempos desde float cuando se cambia su control. |

## Verificación automática

- `scripts/cargo.ps1 check -p transcriptor --all-targets --locked`: correcto, 3,53 s tras las primeras tres correcciones; 2,57 s tras marcador.
- `scripts/cargo.ps1 fmt --package transcriptor`: correcto.
- `scripts/cargo.ps1 clippy -p transcriptor --all-targets --locked '--' -D warnings`: primer intento detectó ubicación de dos módulos de test; movidos al final. Repetición correcta, 4,04 s.
- Seis tests nuevos sintéticos: dos de conservación exacta de rangos/edición de un borde/eliminación de filas; uno de pasos diferidos en 30000/1001 y límites; dos de resultado de ocurrencias obsoleto (selección y reapertura de copia con mismas IDs/revisión); uno de borrador de marcador con cambio de proyecto/revisión/secuencia. **Solo compilados**, no ejecutados.

La revisión cruzada E3 detectó dos invalidaciones adicionales en el nuevo cache: reapertura de copia divergente con mismas IDs/revisión, y comando de autor que cambia el rango desde un control anterior del inspector en el mismo frame. Se añadió epoch de sesión incrementado en los tres reemplazos de sesión y el worker lee rangos actuales desde el proyecto al capturar su clave; no usa el clon anterior del inspector. Clippy posterior detectó una variable ya innecesaria, eliminada; control final desktop/all-targets correcto en 2,01 s (alpha.10).

## Límites y aceptación pendiente

El worker del inspector aún toma una copia lineal de campos de presentación al iniciar una consulta y mantiene en RAM los resultados de esa selección; no se anuncia PERF-01 aceptado ni memoria ilimitada. No se añadió una nueva política de mapping ni se fusionaron ocurrencias A/V distintas de las que devolvía el dominio.

Recorridos físicos futuros: editar etiqueta de rango submilisegundo y reabrir; pasar frame desde seek subframe mientras cambia composición y comparar con estado estable; expandir inspector de item multirrango/repetido y cambiar rápidamente de selección/revisión; abrir marcador, cambiar proyecto vía núcleo/MCP, comprobar bloqueo stale y reabrir para editar/undo. Ejecutar únicamente cuando se levante el aplazamiento físico.
