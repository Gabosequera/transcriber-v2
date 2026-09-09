# v2.0.0-alpha.5 — checkpoint parcial del editor

Continúa desde alpha.4 con edición semántica por contexto, división de jerarquías/multirrango, trim/nudge y ciclo del autor; orden/tipos de carriles y gestos conectados al núcleo común. Exporta el documento completo de recortes conservando cabecera y metadatos. Añade importación de planes de bloques contiguos y export documental con rechazo explícito de evidencia/snap desactualizado.

Historial undo/redo durable en transacción recuperable proyecto/auditoría/history. Apertura, guardado y Save As en workers; reconocimiento de la revisión congelada sin limpiar ediciones nuevas; cierre espera escritura. Autosave con lock/CAS y protección del recovery de otra instancia. Actor AI separado de revisión/aceptación humana.

Verificación: 51 tests dirigidos (33 application + 18 V1compat), Clippy workspace/all-targets y formato pasan. Build release local registrado en evidence. Sin ejecución GUI/multimedia ni paquete binario nuevo; aceptación física/general aplazada. Tests desktop/dominio solo compilados, bloqueo Windows4551 no eludido.

**E2/E3 siguen abiertas y E4 pendiente.** Faltan adaptador sidecar autor completo, snap/materialización chunks, coalescencia trims, caja Corte, carpeta V1/montaje inverso editado/transacciones multidocumento, requests/passes/derivación, jobs/archivado/watcher/conflictos y MCP/cliente GUI E4. Recuperación de autosave aún no restaura toda su pila histórica. E5 no iniciada. Consulta STATUS y el traspaso vigente al final del prompt00.

Prerelease de fuentes, sin assets binarios ni datos personales. V1 conservada en solo lectura.
