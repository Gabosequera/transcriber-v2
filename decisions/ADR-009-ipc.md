# ADR-009 — IPC por frontera

- Estado: provisional | Fecha: 2026-09-07

NDJSON/stdio para control y metadatos de workers; archivos de trabajo/shared memory para frames/tensores; named pipes/Unix sockets para procesos persistentes; gRPC/protobuf solo para servicio remoto. JSON-RPC/MCP se limita a integración de agente. P6 debe medir latencia/backpressure y recovery.
