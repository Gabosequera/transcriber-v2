# ADR-004 — workers Python por proceso y NDJSON/stdio

- Estado: provisional
- Fecha: 2026-09-07
- Nivel de confianza: alto

Rust supervisa workers que anuncian capacidades, emiten progreso/heartbeats y conservan checkpoints. NDJSON/stdio minimiza packaging y superficie de red. Named pipes/Unix sockets quedan para procesos persistentes; gRPC/protobuf se reserva para deployment remoto o streaming complejo. MCP traduce solicitudes externas al mismo command bus y no obtiene acceso directo a persistencia.
