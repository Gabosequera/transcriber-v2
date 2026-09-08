# STATUS — Transcriptor V2

Checkpoint **continuación 5, 2026-09-08**, versión de fuentes `2.0.0-alpha.2`. Corte preventivo por contexto; objetivo global todavía incompleto. El usuario autorizó terminar la **implementación hasta E4**, parar antes de E5 y dejar testing real/revisión general para otra iteración. La publicación de este checkpoint no cierra fases pendientes.

## Estado de fases

| Fase | Implementación | Aceptación |
|---|---|---|
| E0/E1 | Bases conservadas | Aceptadas históricamente en el equipo anterior |
| E2 | Funcionalidad principal existente | Regresión integrada, rendimiento y validación física pendientes |
| E3 | Parcial; incremento descrito abajo | Pruebas unitarias dirigidas; uso real pendiente |
| E4 | Pendiente | Sin MCP/cliente conectado implementado |
| E5/E6 | Fuera del alcance vigente | No iniciar |

Leer primero este archivo, `evidence/continuacion-05.md`, `RUN.md` y la matriz. Los detalles históricos siguen en `evidence/status-continuacion-04.md` y `evidence/traspaso-continuacion-04.md`. No reiniciar E0/E1 ni sustituir rutas de evidence histórica.

## Entorno y cambios

Checkout actual `C:/Users/gabri/Todo/transcriber-v2`, remoto `Gabosequera/transcriber-v2`, base recibida `42a5226`. V1 `../transcriber` permanece solo lectura. Se instalaron Rustup/Rust 1.98.1, GitHub CLI y Windows SDK 10.0.26100; ya existía Visual Studio 18 Community. FFmpeg local por PATH: 8.1.2, distinto del 8.0.1 histórico. No se instalaron modelos ni se ejecutaron pruebas multimedia/GUI.

- `scripts/cargo.ps1`: descubre MSVC/Rust sin rutas personales, configura x64 y restaura directorio del llamador.
- Tests JSON con `${WORKSPACE}`/`${OUTPUT}`, preparadores de destinos nuevos y resolución FFmpeg configurable. Corregidos default V1, fuente TrueType configurable y quoting Unicode del shutdown smoke. La expansión de las plantillas fue comprobada sin ejecutar medios.
- Atajos: editar/capturar/desasignar/restaurar desde GUI; validación de combinaciones/conflictos, escritura atómica antes de cambiar mapa activo. Durante edición de atajos se suspenden comandos del editor.
- Sesión: undo/redo múltiples conservan continuidad y revisión monótona; validación de protocolo/digest/proyecto antes de replay; misma clave con otra solicitud se rechaza; máximo 10 000 claves sin evicción silenciosa; overflow de revisión rechazado. Diff incluye propiedades del proyecto, secuencias y capas eliminadas.
- Guardado: lock de SO y comparación de digest del documento leído; rechazo de archivos externos modificados/incompletos o destinos existentes no abiertos; límite de documento de 64 MiB en lectura y escritura. Session/history mantienen rutas absolutas; snapshots persistidos usan rutas relativas. Save As actualiza store solo tras éxito. Errores de journal visibles y eventos conservados si falla su escritura.
- Recuperación: autosave de mismo proyecto y revisión mayor se ofrece en GUI; aceptar crea revisión nueva y un paso de undo. El proyecto normal permanece guardado hasta la decisión explícita.

## Controles y publicación

Clippy workspace/all-targets con `-D warnings` y formato correctos. Las 13 pruebas application pasaron; el target de tests desktop compiló pero Windows bloqueó su ejecución (Control de aplicaciones, error 4551). Los tests keymap NO se ejecutaron. Resultados unitarios y publicación exactos en `evidence/continuacion-05.md` y `PUBLISH.md`. El primer check encontró dependencias ausentes y después un error de préstamo Rust corregido; logs conservados. No confundir ese log fallido con el Clippy final correcto.

**No hay nuevo paquete ni ejecutable release probado.** La prerelease alpha.2 conserva fuentes y documentación; no redistribuye el binario alpha.1 como si incluyera estos cambios. Los source archives los proporciona GitHub. Goldens/fixtures multimedia originales ausentes: no se regeneraron ni ejecutaron en esta continuación. El puntero histórico `latest-regression.json` sigue apuntando al equipo anterior hasta una nueva preparación explícita.

## Primera acción al continuar

Desde raíz: `./scripts/cargo.ps1 check --workspace --locked`. El binario desktop de tests está bloqueado por Windows Control de aplicaciones (4551), sin bypass. No iniciar regresiones multimedia/GUI sin nueva indicación: el usuario las aplazó. Continuar implementación E3 con estos pendientes, luego E4:

1. **Durabilidad/reconciliación:** validación integral de invariantes al cargar y recuperar; autosave de proyectos nuevos; reconciliación externa con diff/aprobación y protección humana; journal recuperable/idempotente y transacciones multidocumento V1; migraciones. El lock es cooperativo: un editor ajeno que lo ignore aún puede escribir entre comprobación y reemplazo. `project.json` y `journal.jsonl` todavía no se confirman juntos; append puede quedar parcial y requiere recuperación. No cerrar DAT-02/03.
2. **Paridad editorial:** master completo protegido y proyecciones palabras/hablantes/señales; import/export V1 desde GUI y montaje inverso con rechazo explícito de pérdidas; chunks contiguos, jerarquía/multirrango, comentarios/evidencia/derivación/protección humana y navegación de todas las ocurrencias; inventario de acciones V1 y handlers pendientes (p. ej. saltar recortes y añadir tema). Auditar import_v1_folder síncrono. No modificar V1.
3. **E4:** transporte local conectado a la GUI, herramientas específicas con schemas (no un único command_execute), capabilities y queries paginadas/temporales, permisos por sesión/proyecto/operación, propuestas con digest/base, dry-run/diff/preview/apply, eventos/auditoría, requests-respuestas JSON y cliente local documentado. El envelope actual es base interna, no autorización ni servidor MCP.
4. Al terminar implementación E4 parar. Si el contexto se aproxima al límite, publicar checkpoint con docs y prompt breve. No marcar el objetivo global terminado mientras queden los puntos anteriores.
