# STATUS — Transcriptor V2

Checkpoint **continuación 6, 2026-09-08**, fuentes `2.0.0-alpha.3`, desde main limpio en `7267880`. Objetivo global **incompleto**: implementar hasta E4 y parar antes de E5. Testing físico, regresión multimedia y revisión general siguen aplazados por el usuario.

| Fase | Implementación | Aceptación |
|---|---|---|
| E0/E1 | Bases conservadas | Aceptadas históricamente en otro equipo |
| E2 | Funcionalidad principal + acciones editoriales de este incremento | Regresión integrada, A/V, rendimiento e interacción pendientes |
| E3 | Parcial; evidencia, validación y reconciliación ampliadas | Unitarios dirigidos; aceptación física pendiente |
| E4 | Pendiente | Sin transporte MCP ni cliente conectado |
| E5/E6 | Fuera del alcance vigente | No iniciar |

## Implementado en esta continuación

- `Project::validate`: schema/identidad, IDs duplicados, referencias, bases temporales, rangos, solapamientos, transformaciones, streams, jerarquías, tombstones y masters. Se usa al leer/guardar/recuperar y antes de confirmar previews/comandos. No requiere que los medios estén presentes. No sustituye auditoría exhaustiva de los argumentos aritméticos de todos los comandos.
- `MasterEvidence` conserva el documento V1 completo y su digest. Importación proyecta palabras, intervenciones por pista, risas, arousal y emociones a capas de solo lectura con IDs estables y registro original en evidencia. No se reemplaza un master incorporado por otro contenido; la gestión explícita de versiones queda pendiente.
- Protección común contra cambios de evidencia y decisiones humanas por actores externos, incluyendo batch/import/reconcile/undo externo. Edición respeta capas bloqueadas/de análisis. Tombstones no resucitan; rangos huérfanos inválidos se rechazan. Recortes conservan aceptación independientemente de activación al exportar/desactivar/reactivar.
- `save_with_journal`: intención durable antes de publicar proyecto y auditoría; recuperación idempotente de fronteras proyecto/journal, rechazo de terceros contenidos externos, reparación exclusiva de cola JSONL truncada, rechazo de corrupción interna. GUI retiene eventos hasta guardado exitoso. No cubre todos los contratos V1.
- Reconciliación de project.json: rescaneo cada 2 s en worker acotado, dos lecturas válidas iguales, merge de tres vías por IDs, conflicto de campos/orden y protección humana. GUI presenta resumen del diff y permite aplicar/posponer; aplicar revalida disco/sesión, usa Command::ReconcileProject y admite undo como nueva revisión. Sin watcher SO ni monitor de archivos V1 individuales ni editor de conflictos.
- Autosave de proyectos sin carpeta en configuración/recovery, por sesión, recuperación al arrancar y conservación de auditoría. Primer Guardar pide destino normal. Autosave de proyectos ya guardados mantiene formato legado; errores visibles y repaint para cumplir el intervalo en reposo.
- Salto de recortes en reproducción (Shift+T), sin alterar export: solo donde todos los clips contribuyentes están recortados; audio limitado antes del salto. Añadir tema completo, insertar multirrango sin huecos en un batch, navegar silencios importados y elegir ocurrencias desde inspector. Evidencia sin controles de edición activos.
- Corregidas rutas relativas en sesión al completar importación multimedia y slicing de fingerprints cortos que podía provocar panic.

## Controles y límites

Evidencia en `evidence/continuacion-06.md` y logs enlazados. Unitarios application/V1 sin medios; Clippy de todos los targets y formato. Windows Control de aplicaciones bloqueó el binario de tests de dominio (4551); no se eludió la política. Tests desktop siguen sin ejecución en este host. No se ejecutaron GUI, medios, V1 ni datos personales.

La prerelease es de fuentes. El build release local se registra separadamente: compilar no equivale a probarlo ni a preparar un paquete distribuible. Resultado exacto en `PUBLISH.md` y `evidence/publication-alpha3.json` cuando esté confirmado.

## Siguiente trabajo concreto

1. **Paridad E2/E3:** inventario trazable de acciones V1; importador editorial V1 asíncrono (sigue haciendo IO/probe en GUI); import/export keymap; export V1 desde GUI y montaje inverso con rechazo de pérdidas; chunks contiguos, adaptadores autor/bloques, edición de jerarquías/multirrango, derivación/manifests/requests/passes y carriles completos. Preservar masters/proyecciones; no inferir modelos.
2. **Durabilidad:** idempotencia entre aperturas e historial/jobs durables; migraciones explícitas; transacciones multidocumento V1. Auditoría de autosave de proyectos ya guardados. Pasar guardado/autosave y enumeración de recovery a workers; limitar/archivar auditoría sin perder reintentos. Watcher SO/archivos V1, diff detallado y resolución explícita. Lock cooperativo frente a editores ajenos.
3. **Protecciones:** distinguir completamente actor humano/AI al marcar edited/aceptación; protección actual conservadora, no autorización E4. Cubrir recuperación/actores y contratos restantes. PERF-01 pendiente: copias/hash de masters y escaneo de items siguen costosos en proyectos grandes.
4. **E4:** herramientas MCP específicas con schemas/capabilities, queries temporales paginadas, selección/transporte/jobs, permisos por sesión/proyecto/operación, propuestas con base/digest, dry-run/diff/preview/apply, eventos/auditoría, requests-respuestas JSON y cliente local conectado a GUI. Mismo núcleo; no herramienta genérica de shell/SQL/reemplazo de estado.
5. Parar al completar implementación E4, antes de E5. Si se interrumpe por contexto, commit/push/prerelease/traspaso. No cerrar requisitos por documentación o compilación.

Primer control: `./scripts/cargo.ps1 check --workspace --locked`. Leer este archivo, evidence/continuacion-06.md, matriz, RUN y traspaso vigente. Continuación 5 y sus evidencias permanecen como historia. V1 `../transcriber` estrictamente solo lectura.
