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
