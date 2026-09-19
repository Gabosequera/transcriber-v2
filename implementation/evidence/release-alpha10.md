# Transcriptor V2 2.0.0-alpha.10

Prerelease de fuentes sobre main, conservando E0/E1 y la integración E2/E3/E4 de alpha.9. No contiene assets binarios. No inicia E5 ni declara aceptación física.

- E2: conserva tiempos exactos al editar items; evita sobrescribir marcadores obsoletos; iguala pasos de fotograma; consulta ocurrencias en worker con invalidación de sesión. Export usa temporales exclusivos y no reemplaza destinos concurrentes; incorpora RF64 para audio largo, cancelación durante verificación y diagnóstico stderr acotado.
- E3: rechaza árboles documentales imposibles antes de publicar/recuperar; preserva pistas bloqueadas, tombstones e identidad de capas frente a operaciones externas.
- E4: undo/redo preparados con permisos, actor, pilas y recibos idempotentes; rechazo de propuestas obsoletas y recuperación de fallos; revalidación local fuera de GUI; cliente stdio responde errores RPC correlacionados.

Verificado: 80 unitarios application y 3 integraciones correctos; cliente sintético, check/Clippy workspace all-targets y formato correctos. Build release local registrado en evidencia, ejecutable no lanzado ni distribuido. Tests domain/desktop/V1compat/control no ejecutados por bloqueo Windows4551; media tampoco ejecutada. GUI, multimedia, NLE, MCP contra editor y mediciones físicas permanecen aplazados.

Backlog resuelto y límites: implementation/evidence/continuacion-13.md. Recorridos futuros: implementation/RUN.md y implementation/control.md. No queda una brecha de código demostrada abierta en la revisión realizada; esto no sustituye la aceptación global de las etapas.
