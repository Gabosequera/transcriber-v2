# STATUS — continuación 9, fuentes 2.0.0-alpha.6

Publicación verificada: [v2.0.0-alpha.6](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.6), código/tag `48784e6ef9a6cfb1c13fcee1e82a1f0989f98a7b`. Prerelease no draft, sin assets binarios; tag y código main confirmados en remoto. Evidencia en `implementation/evidence/publication-alpha6.json`. 60 tests dirigidos, check/Clippy/formato/build release correctos; sin ejecución física ni cierre de E2/E3/E4.


Base: `main` limpio en `87d9a26`; V1 `bb7012c`, solo lectura. **Objetivo hasta E4 incompleto. E2 abierta, E3 parcial, E4 pendiente. E5 no iniciada.** Este es un checkpoint de continuidad; la aceptación física, multimedia y revisión general siguen aplazadas.

## Implementado en este incremento

- Caja Corte semántica: B+arrastre crea/estira/une; Shift resta, recorta, divide o borra. Ctrl+arrastre mueve; handles siguen recortando y S sigue dividiendo. Una transacción por release, Escape/captura/base de revisión protegidos. En Fuente y dentro de una ocurrencia en Secuencia. Selecciona superviviente y abre editor de pedido nuevo en user/ai. Como V1, caja opera items de un rango; cruces de jerarquía incompatibles se rechazan sin cambios. No declarar cobertura física de gestos.
- Coalescencia de recortes: solape estricto por enabled/carril; actor conserva ID/origen/aceptación (sin actor: mayor duración/ID). Razones y warnings unidos; evidencia y extras conservados con procedencia de absorbidos; tombstones. AddItem, bordes, nudge e inspector la usan. Movimiento entre carriles del mismo medio y eliminación de carril conservando recortes en destino o borrándolos con undo. main/ai de fábrica no se eliminan. Menú de cabecera conectado.
- Marcas del autor: adaptador schema 1/timebase media_elapsed_v1, identidad completa, revisión/contador, punto/región/decisión, prompt/label, campos desconocidos y cuarentena de marcas inválidas. Selección por revisión; empate divergente exige selección explícita de archivo. Import por carpeta o Archivo → Importar marcas del autor V1, incluso sin master; export sidecar. Una colección normalizada, no se convierte la evidencia del master en otra copia editable. No inspecciona automáticamente el store global V1 ni resuelve su conflicto con un diálogo especializado.
- Bloques: SetItemStructure y SetItemProps ajustan ambos vecinos (inspector), además de trim/nudge existentes. SnapBlockBoundaries recalcula palabras ±40 ms, risas ±120 ms y utterances de todas las pistas; scoring global-safe/2, radio explícito, first/last utterance y ajustes. Sin corte duro seguro rechaza entero. Menú de cabecera prepara el comando en worker y confirma exactamente su resultado si la base sigue vigente.
- Exportar bloques produce master derivado, plan seleccionado, view/Markdown y archivos transcript/señales/intensidad/pedido/momentos por bloque. El master de evidencia en sesión no cambia. Una intención recuperable publica el conjunto, verifica todas las bases antes de escribir, registra recibo y se elimina al final. Solo carpetas nuevas propiedad de V2; rutas portables, sin traversal/alias/enlaces. Archivo → Recuperar exportación V1 interrumpida conecta la recuperación en worker. No es export completo de carpeta V1 ni transacción sobre fuentes V1.
- Autosave: recupera la pila completa undo y redo junto con auditoría/recibos; revisión nueva y frontera revalidada. Reapertura posterior conserva ambas pilas. Legacy sin history conserva recuperación de un paso. El historial recuperado sustituye la entrada artificial de recuperación; undo recorre las ediciones del candidato.
- PreparedCommand conserva IDs de dry-run/preview para commit, con campos privados y validación de base/identidad/recibos. GUI snap e import de marcas trabajan en worker. MasterEvidence usa documento inmutable compartido con digest cacheado, sin duplicar el árbol JSON en cada snapshot/undo/worker ni recalcular su hash por cada validación. Serialización no cambia; un documento deserializado se vuelve a validar. Otros clones, proyecciones e historial serializado siguen costosos: PERF-01 abierto.



Build release final alpha.6: **OK en 2m04s**, exe compilado y no ejecutado; tamaño/SHA256/tiempo del wrapper en `evidence/build-alpha6.json` (desde evidence: `build-alpha6.json`). Sin paquete binario nuevo aceptado.

## Controles y límites

Ver `evidence/continuacion-09.md` y logs. Solo tests application/V1compat ejecutados; Clippy all-targets compila desktop/dominio, no ejecuta sus tests bloqueados por Windows 4551. Sin GUI, medios, modelos ni pruebas físicas. Publicación exacta en PUBLISH/evidence.

## Pendientes reales para continuar

1. E2/E3: revisión de controles/gestos dinámicos completa; integración y resolución de candidatos globales de autor; reglas editoriales restantes y pruebas físicas aplazadas. Caja multirrango no soportada (igual a V1); no presentarla como edición general de jerarquías.
2. E3 contratos: derivación padre/hijo y mapping, manifests/requests/proposals/passes, export carpeta V1 completa, montaje inverso editado con material invisible. El montaje editado sigue rechazado: falta implementar la inversa. Materialización nueva de bloques no equivale a materializar todos los contratos/proyectos previos.
3. Durabilidad: jobs durables, archivado de auditoría/recibos de sesión y migraciones generales; watcher SO/documentos V1, diff por campo y resolución explícita. Recuperación de exports es explícita desde menú; no se ha ensayado crash real. Persisten límites 200 entradas, 64/128 MiB y snapshots serializados completos. Compartir masters reduce clones/hash; no elimina todo el trabajo costoso de GUI.
4. E4 completo pendiente: MCP específico, schemas/capabilities reales, queries paginadas/contexto/evidencia/selección/transporte/jobs, permisos, propuestas con digest/revisión/dry-run/diff/preview/apply/verificación, eventos/auditoría y cliente local conectado. PreparedCommand es base interna, no implementación de MCP.

Continuar sobre main hasta completar E4, sin pedir nueva autorización y detenerse antes de E5. No reiniciar E0/E1. Traspaso vigente al final del prompt principal.


---

# STATUS — Transcriptor V2

Checkpoint **continuación 8, fuentes 2.0.0-alpha.5**, desde `main` limpio/sincronizado `acecb37`. Objetivo hasta E4 **incompleto**. E5 no iniciada. Aceptación física, multimedia y revisión general siguen aplazadas.

| Fase | Implementación | Aceptación |
|---|---|---|
| E0/E1 | Conservadas | Histórica |
| E2 | Abierta; edición semántica y carriles ampliados | GUI/A-V/rendimiento pendientes |
| E3 | Parcial; bloques contiguos, export completo de trims, historial y workers | Tests dirigidos; física pendiente |
| E4 | Pendiente; atribución AI corregida en núcleo | Sin MCP ni cliente conectado |
| E5/E6 | Fuera del alcance vigente | No iniciar |

## Incremento implementado

- `SplitItem` conserva los otros rangos en el original y divide/reasigna descendientes cuando cruzan el corte. Conserva comentario, estado, extras/evidencia y jerarquía. `TrimItem` valida hijos/rangos; `ShiftItems` incluye descendientes una vez y limita el delta al medio. S, corchetes, nudge y menú contextual usan esos comandos en Fuente/Secuencia con mapping al medio. Estados multicapa y trim de clips enlazados son batch; Shift+E avanza solo si se confirmó aceptar.
- `CycleAuthorDecision`: nota→incluir→excluir→nota. Punto con decisión pasa a región ±2 s, limitada al medio; mínimo 200 ms. E/X/P y validación impiden una decisión sobre un punto. Copias/splits nuevos de marcas/recortes usan prefijos V1 numéricos.
- Arrastre de item y handles confirman un comando al soltar, con guía temporal, snapping y base de revisión; Escape cancela. Herramienta Corte usa el mismo split, también para clips A/V enlazados. La guía de gesto no es un preview multimedia ni certifica rendimiento.
- Carriles: orden completo sin IDs repetidos; subir/bajar desde biblioteca y menú de cabecera; selección de cabecera limpia el contexto anterior. Crear permite notas/pedidos, recortes, temas y marcas. Visibilidad/bloqueo existentes conservados. **140 constructores/bindings V1** inventariados por AST en siete archivos; no equivale a 140 recorridos probados ni inventario dinámico completo.
- Recortes: cabecera autoritativa normalizada conservada una sola vez; cada carril conserva sus metadatos y sus cortes. Exportar cualquier carril escribe el **documento completo trims.json** del medio, incluyendo otros carriles, aceptación independiente, contador, campos desconocidos y etiqueta V2 mediante `tv2_label`. Crear recortes antes de master funciona. Sin copia editable adicional de cuts. Proyectos alpha.4 que perdieron cabecera requieren reimportación explícita; jerarquía/multirrango no son válidos en trims; mínimo 50 ms.
- Bloques: importa `.work/chunks.selected.json` con prioridad sobre `views/chunks.json`, como cabecera + colección única. Valida cobertura contigua 0..duración, máximo 50 minutos, sin aceptación/desactivación/jerarquía. Dividir exige ≥1 s a cada lado; trim/nudge ajustan vecinos. Export documental desde GUI. **Parcial**: planes editados con evidencia de bordes/snap antiguo rechazan export hasta recalcular; falta materialización multidocumento del master/chunks y snap seguro completo. No presentar ese rechazo como adaptador completo.
- `history.json` (`transcriptor-history/1`) guarda undo/redo en la misma intención recuperable que proyecto+journal. Reabre IDs y ambos stacks; valida identidad, revisión, continuidad y frontera. Proyectos antiguos sin archivo migran a historial vacío (no se inventa el pasado). Autosave incorpora y valida historial; recuperación explícita conserva por ahora el único undo hacia la versión guardada, **no restaura toda la pila del candidato**.
- Apertura/guardado/Save As en un worker por GUI y canal de resultado bounded(1). Apertura revalida proyecto/revisión; guardado reconoce solo la revisión/auditoría congeladas y conserva cambios posteriores como sucios. Historial se relocaliza junto con assets al Save As. Apertura de proyecto/auditoría/historial toma un lock común. Guardar y salir espera confirmación del worker; no cierra con escrituras activas. Descubrimiento inicial de recovery sigue síncrono.
- Autosave toma lock, revalida disco y rechaza sustituir recovery más nuevo de otra instancia. La sesión puede guardar explícitamente para establecer nueva base. No se ensayó terminación real de procesos/pérdida de energía.
- Actor Agent no fabrica `edited` ni aceptación humana. Sus cambios siguen editables por AI; decisiones humanas previas permanecen protegidas. No sustituye permisos de sesión/MCP de E4.

## Verificación

Controles exactos y publicación: `evidence/continuacion-08.md`, logs y `PUBLISH.md`. Tests application/V1compat dirigidos; Clippy all-targets compila desktop/dominio sin ejecutar sus binarios. No se eludió ni reintentó el bloqueo Windows 4551. No se ejecutó la GUI, medios, modelos ni pruebas físicas.

## Pendientes reales de implementación

1. **E2/E3 editorial:** caja Corte de creación/unión/resta V1, completar revisión de controles dinámicos y gestos; editor de rango de bloques con adaptación de vecinos en todos los caminos; snap seguro/utterances y materialización del plan. Adaptador autor desde sidecar/master aún pendiente; el ciclo interno no lo sustituye. Coalescencia V1 de recortes y reglas de administración/movimiento entre carriles pendientes.
2. **Contratos E3:** derivación padre/hijo, manifests/requests/proposals/passes, export carpeta completa, inverso de montaje editado conservando material invisible, transacciones multidocumento V1. Original de montaje intacto conservado desde alpha.4; inverso editado sigue rechazado.
3. **Durabilidad/fluidez:** recuperar pila histórica del autosave, jobs durables, archivado de auditoría/recibos, migraciones generales, watcher SO + documentos V1, diff por campo y resolución explícita. Descubrimiento recovery inicial síncrono; snapshots/history y validación/hash de masters siguen costosos en GUI. Límites de historial/commit pueden rechazar proyectos grandes; falta almacenamiento por deltas/compartido. No cerrar PERF-01.
4. **E4 completo:** MCP específico, schemas/capabilities reales, consultas temporales paginadas/selección/transporte/contexto/evidencia/jobs, permisos por sesión/proyecto/clase, propuestas/base/digest/dry-run/diff/preview/apply/verificación, eventos/auditoría/requests y cliente conectado a GUI. No añadir shell/SQL/reemplazo arbitrario de estado como control AI.
5. Continuar hasta E4 y parar antes de E5; aceptación física aplazada no justifica dejar implementación pendiente. Este checkpoint por contexto no declara cumplido el encargo.

V1 `bb7012c` observado limpio; solo lectura AST/texto, sin ejecutar sus módulos ni tocar datos personales. Traspaso vigente al final de `prompts/00-CONSTRUIR-TRANSCRIPTOR-V2.md`.

Controles finales alpha.5: **51 tests (33 application +18 V1compat), Clippy/all-targets y formato OK; build release OK en 2m13s**, sin ejecutar el exe. Metadatos en evidence/build-alpha5.json.

Publicación verificada: [v2.0.0-alpha.5](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.5), código/tag `83987b68a8dae139a8f70a5dba71c407b6fd7de8`, main subido. Prerelease no draft, sin assets binarios; `evidence/publication-alpha5.json` y PUBLISH. 51 tests, Clippy/formato/build release final OK; sin ejecución física ni cierre global de E2/E3/E4.
