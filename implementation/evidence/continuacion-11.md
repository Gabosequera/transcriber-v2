# Continuación 11 — carpetas V1 y publicación binaria

El mismo encargo continúa después de alpha.7 (código 293556e8ed0a0ac1ceeff602fbc82722aa6c8337; registro documental 4f97e53). Fuentes alpha.8. **E2 abierta/E3 parcial/E4 pendiente/E5 no iniciada. Objetivo global incompleto.** Sin aceptación GUI, multimedia ni revisión general; V1/datos personales solo lectura.

## Implementación integrada

- Importar carpeta captura documentos textuales auxiliares, desconocidos, manifests y propuestas en MasterEvidence.source_bundle, inmutable y compartido por Arc. Conserva bytes UTF-8/BOM/CRLF. Los adaptadores leen el snapshot; segunda captura detecta cambios. Rechaza master ambiguo. Binarios auxiliares inventariados con localizador, tamaño y SHA-256 por streaming, fuera del historial. Ninguna escritura en origen.
- folder::export usa la evidencia original y los adaptadores de capas/trims/autor/bloques/montaje. Conserva IDs, tombstones y campos desconocidos; regenera vistas de bloques y conserva moments/pedidos existentes. Archiva documentos sustituidos por SHA-256 en .work/tv2-original, sin anidación sucesiva. Master intacto salvo plan chunks exportado, excluido del source digest; evidencia en sesión intacta.
- Revisa editorial-step/1 por result_digest y tamaño/SHA de outputs. Manifests inválidos archivados con requires_revalidation; propaga invalidación de referencias a otros manifests retirados. No inventa ejecución de pasos. Requests/proposals/passes se preservan, pero validación y aplicación siguen pendientes.
- Archivo → Exportar carpeta documental V1 → capas, o capas con montaje activo del mismo medio. Prepara en worker, crea carpeta nueva propiedad de V2 y publica transaccionalmente. Referencia el medio principal y copia los auxiliares.
- documents::publish_with_files: intención transcriptor-documents/2; preflight de orígenes/destinos, temporal por bloques de 64 KiB, verificación tamaño/SHA y del destino antes de persistir. Recuperación idempotente no exige un origen cuyo destino ya está publicado; rechaza terceras versiones externas. Conserva lectura de intenciones /1 y menú de recuperación. Recibos distinguen SHA binario y digest canónico textual.
- Digest completo cacheado para comparar master del bundle sin reparsear/rehashar en cada comando. Protección de evidencia contra reemplazo de cualquier actor. Test de persistencia/undo/redo/reapertura/corrupción del bundle.

## Límites reales

No cierra DAT-01 ni PERF: captura textual de 32 MiB/20000 entradas, intención textual 128 MiB y límites previos de proyecto/autosave. Enlaces/nombres no portables se rechazan. Binarios requieren localizador original hasta exportarlos: falta traslado por Save As y recuperación de localizadores. Dos recorridos/hash de origen; falta cancelación cooperativa interna y conservación de directorios vacíos.

Proyectos previos sin bundle se leen, pero exportar carpeta completa exige importar V1 en proyecto nuevo: AttachMaster protege evidencia ya existente. No resalvar bundles con alpha.7 o anteriores, que no conocen el campo. Migración general y negociación de lector pendientes. Intenciones binarias /2 no recuperables con versiones anteriores.

Pendientes de código: derivación padre/hijo y mapping con streams conservados, requests/proposals/passes y generación de manifests nuevos, watcher/reconcile V1 individual, archivado general de auditoría/recibos, migraciones, límites y costes restantes, inventario dinámico/preview geométrico. E4 MCP funcional no implementado; el aplazamiento físico no causa esos pendientes.

Primera acción: derivación desde mapping de exportación verificada, conservando pistas, palabras/intervenciones, procedencia y recálculo de overlap/duplicados sin inferencia. Leídos editorial_projects time_map/map_range/derive_master/derive_layers/publish_child, editorial_master _event_average/_overlap_groups/_deduplicate_bleed y editorial_pipeline manifests. El exportador actual mezcla audio: no atribuir varias pistas de evidencia a un hijo que solo conserva una.

## Controles

47 application tests ejecutados/pasan, 1.15s. Incluye fixtures sintéticas de publicación binaria con interrupciones en todas sus fronteras, origen cambiado antes de publicar, destino externo y preservación de origen. Log unit-continuacion-11.log.

Clippy/all-targets compila los tests nuevos de V1compat; NO ejecutarlos por el Windows 4551 observado anteriormente. Dominio/desktop tampoco se ejecutan. Sin reintentos, renombrados, rutas alternativas o cambios de política. Tests application solo usan fixtures temporales V2.

Comandos finales: scripts/cargo.ps1 test -p tv2-application --lib --locked; clippy --workspace --all-targets --locked '--' -D warnings; check --workspace --locked; fmt --all --check; build --release --locked -p transcriptor. Logs por comando en esta carpeta; metadata del exe en build-alpha8.json. Exe no ejecutado. Publicación exacta en PUBLISH y publication-alpha8.json cuando se confirme.

Resultado final: Clippy/all-targets OK (8.34s), check/workspace OK (7.12s), fmt/check OK; build release OK (2m21s), sin ejecutar Transcriptor.exe. V1 observado limpio al finalizar. 47 application tests pasan; resto no ejecutado.

Publicado/verificado v2.0.0-alpha.8 sobre 2355655a658cc512ac5ed612c4950275873eef10; tag coincide, prerelease no draft, sin assets. Registro publication-alpha8.json. Main incluye después el commit documental de esta verificación.
