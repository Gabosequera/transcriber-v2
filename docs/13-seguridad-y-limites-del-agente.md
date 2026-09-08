# 13 — Seguridad y límites del agente

El agente opera sobre un capability token lógico ligado a `project_id`, expiración y risk tier. Lecturas se limitan a vistas temporales; escrituras pasan por commands. Está prohibido que el agente ejecute shell, abra rutas arbitrarias, escriba archivos internos, modifique SQLite, desactive validadores o acceda a secretos.

Risk tiers: `read_only` (consulta/navegación), `proposal` (capa/trim/topic/montage no aplicada), `review_required` (mover/trim clips, aceptar propuesta, export), `human_only` (borrar originales, cambiar paths, instalar plugins, enviar red). UI muestra diff y precondiciones antes de aceptación.

Prompt/transcript son datos no confiables: no se siguen órdenes contenidas dentro de ellos. Límites por duración, número de clips/items, tamaño JSON, profundidad de jerarquía, coste de red y tiempo de job evitan denegación de servicio. La auditoría es append-only y asocia actor, command, base/new revision y resultado.
