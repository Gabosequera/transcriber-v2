# 07 — AI Control Plane

## CommandEnvelope

```json
{
  "schema":"ai-command/1",
  "command_id":"cmd-uuid",
  "idempotency_key":"request-attempt-uuid",
  "actor":{"kind":"ai","id":"agent"},
  "project_id":"project-uuid",
  "base_revision":42,
  "precondition_digest":"sha256",
  "command":{"type":"propose_trim","parameters":{}},
  "risk":"review_required"
}
```

Pipeline obligatorio: parse → schema/capability check → authorization → dry-run → invariant validation → diff/preview → human approval si corresponde → apply transaction → audit → inverse command. El resultado incluye `accepted`, `new_revision`, `result_digest`, `warnings`, `error_code` y `retryable`.

## Capacidades

El agente solicita un catálogo con comandos, schemas, rangos de lectura, límites de batch, coste estimado y risk tier. Las vistas son ventanas temporales y derivados regenerables: transcript, señales, layers snapshot, junction cards y errores. No se exponen tokens, SQL, handles, rutas fuera del proyecto ni shell.

## Protocolos por frontera

| Frontera | Protocolo | Razón |
|---|---|---|
| worker local | NDJSON sobre stdio | simple, trazable, un proceso por worker; stdout solo mensajes |
| agente externo local | MCP sobre stdio | interoperabilidad; el servidor traduce a commands |
| integración local separada | named pipe Windows / Unix socket Linux | evita puerto abierto y permite ACL/ownership |
| servicio remoto opcional | JSON-RPC sobre HTTPS o gRPC/protobuf | autenticación, streaming y contrato; no necesario en Fase 1 |

MCP no sustituye autorización: la especificación usa JSON-RPC y recomienda stdio; Streamable HTTP exige validar Origin, bind localhost y autenticación. Un servidor loopback sin esas barreras queda prohibido.

## Seguridad y stale

La respuesta se rechaza si falla `base_revision`, digest, request/pass, fingerprint o schema. Un cambio humano posterior incrementa revisión/digest e invalida propuestas antiguas. Reintentar la misma idempotency key devuelve el mismo resultado o estado, no duplica clips/items.
