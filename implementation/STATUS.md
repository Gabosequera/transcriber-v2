# STATUS — Transcriptor V2

Checkpoint **continuación 7, fuentes `2.0.0-alpha.4`**, desde `main` limpio y sincronizado en `9287a02`. Objetivo hasta E4 **incompleto**; E5 no iniciada. Testing físico, multimedia y revisión general siguen aplazados por el usuario, separadamente del cierre de implementación.

| Fase | Implementación | Aceptación |
|---|---|---|
| E0/E1 | Conservadas | Histórica |
| E2 | Abierta: correcciones reales de portapapeles/contextos y editor de items | GUI/A-V/rendimiento pendientes |
| E3 | Parcial: import worker, export documental limitado, recibos durables y autosave auditado | Unitarios dirigidos; física pendiente |
| E4 | Pendiente | Sin MCP ni cliente conectado |
| E5/E6 | Fuera del alcance vigente | No iniciar |

## Incremento implementado

- Import editorial V1 en worker único, canal acotado, cancelación y espera del scripting. Lee/probea fuera de GUI; valida proyecto/revisión al completar; medio/master/capas/montaje se confirman en un único batch. Documento reconocido inválido aborta la importación completa. El worker no escribe en V1. Una edición concurrente obliga a repetir la importación; no hay merge automático.
- `SetItemStructure` + editor persistente de texto/comentarios/multirrango/padre desde inspector y F2/Enter. Validación de ciclos, rangos de hijos y referencias; un paso de undo. Un borrador con base vieja falla expresamente.
- `PasteClips` conserva nombre, ganancia, transformación, enabled, extras y procedencia; IDs/grupos de enlace nuevos. Pegado y duplicación de selección enlazada son atómicos. `PasteItems` conserva comentarios/evidencia/multirrango, remapea padres internos y copia descendientes. Raíces copiadas se desligan del padre no copiado. Cortar no borra si Copiar falla; borrado multicapa es batch. Ctrl+A respeta capa seleccionada. No implica que todos los gestos editoriales estén completos.
- Ajustes permite importar/exportar `keymap/1`, restaurar todo y aplicar inmediatamente. Importación completa rechaza schema, acción, acorde y conflictos inválidos. Auditoría estática: **69 acciones V1, 81 V2, ninguna V1 ausente**, único default añadido Ctrl+E en montage.export. Los conteos históricos 70/71 eran incorrectos; registro no acredita funcionamiento.
- Archivo permite **exportar un documento de capa user/topics/ai** o un **montaje importado sin cambios**. Se escriben nuevas carpetas, en worker, sin sobrescribir originales. Montaje conserva ahora todo el JSON original (clips tapados/desactivados y campos desconocidos); el inverso rechaza edición V2, medios múltiples y proyectos antiguos sin original. Capas rechazan precisión submilisegundo y tipos que requieren adaptador autoritativo. No es exportación de carpeta V1 completa ni inverso de montajes editados.
- Recibos idempotentes request/result/proyecto en journal; reapertura restaura resultado/IDs sin ejecutar otra vez. Claves antiguas sin recibo quedan reservadas y rechazan retry; no se inventa un resultado. Guardar como conserva auditoría previa. Historial undo/redo sigue en memoria.
- `transcriptor-autosave/1` publica proyecto y auditoría juntos; lee autosave legado y recupera con recibos/undo. Autosave de ambos tipos de proyecto en worker. Guardar manual, abrir y descubrimiento recovery siguen síncronos. Snapshots/auditoría se clonan en GUI: PERF-01 no cerrado.

## Controles

`evidence/continuacion-07.md`: **24 application + 15 V1compat pasan**, Clippy workspace/all-targets y formato. Tests desktop nuevos (keymap/worker) compilados por Clippy, no ejecutados. No se reintentó ni eludió el bloqueo Windows 4551 de dominio/desktop. Build release **OK, 2m51s**. Alpha.4 publicada sobre `7e050bc`, verificada sin assets binarios; `evidence/publication-alpha4.json` y PUBLISH. No equivale a GUI probada ni paquete aceptado.

## Siguiente implementación concreta

1. **E2 real:** acciones semánticas S/dividir, bordes/nudge y ciclo X de autor aún no tienen paridad completa; gestión/orden/gestos de carriles e inventario de botones/contextos V1. Confirmar handlers con fixtures, no contar IDs como cierre. Incorporar estos recorridos al mismo núcleo de comandos.
2. **E3 contratos:** chunks contiguos y adaptadores autor/bloques, export trims conservando cabecera/IDs/metadatos, orden de carriles, requests/passes/manifests/derivación. Inverso de montaje editado sin pérdida, export de carpeta completa y transacciones multidocumento. No reemplazar silenciosamente fuentes autoritativas con capas genéricas.
3. **Durabilidad:** historial/jobs entre aperturas, migraciones versionadas de proyecto, archivado de auditoría conservando recibos, guardado/apertura en workers, watcher SO/documentos V1/diff detallado/resolución explícita. Autosave worker usa snapshot congelado pero no reduce clones/costes de validación de masters. Revisar dos instancias escribiendo autosave y cierre durante worker.
4. **E4:** MCP específico, schemas/capabilities/queries paginadas, permisos por sesión/proyecto/operación, propuestas/base/digest/dry-run/diff/preview/apply, eventos/jobs/requests y cliente local conectado a GUI. Completar actor humano/AI para edited/aceptación. No añadir shell/SQL/reemplazo arbitrario de estado.
5. Parar antes de E5. Si se interrumpe por contexto, commit/push/prerelease/traspaso. V1 y datos personales estrictamente solo lectura.

Lectura inicial: este STATUS, evidence/continuacion-07.md y continuacion-06.md, matriz, RUN, decisiones D-0035–D-0038 y traspaso final del prompt 00. Main incluye el registro posterior a la publicación; no reanudar desde el tag sin esos documentos.
