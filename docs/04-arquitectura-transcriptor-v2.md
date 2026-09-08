# 04 — Arquitectura Transcriptor V2

```text
GUI nativa (egui/wgpu) ─┐
                        v
                  Command Bus ── Audit/Event Log ── SQLite interno
                        │                    └── JSON contracts / snapshots
                        ├── Domain: T0, clips, tracks, semantic layers, revisions
                        ├── Media Service: probe, decode, clock, cache, proxies, export
                        ├── Scheduler: decode/render/AI/export/background queues
                        └── Worker Supervisor ── Python processes (stdio NDJSON)
                                               └── external AI adapter (MCP/HTTP)
```

## Invariantes

El dominio solo acepta tiempos finitos, rangos `0 ≤ t_ini < t_fin`, identidad de media por fingerprint y operaciones con precondición de revisión/digest. El UI no llama FFmpeg ni modelos. El agente solo ve capabilities, vistas y comandos permitidos. La media original es inmutable; export y proxies son artefactos.

## Fronteras

- GUI→core: llamadas locales o channel tipado; no JSON como autoridad interna.
- Core→media: servicio con playback clock único; preview y export consumen el mismo `ResolvedTimeline`.
- Core→Python: proceso por worker/capacidad; contrato NDJSON versionado, stdout exclusivamente protocolo, stderr solo logs.
- External AI→core: MCP/JSON-RPC adapter traduce a `CommandEnvelope`; nunca expone SQL, path arbitrario o procesos.
- Persistencia: SQLite optimiza consulta y recuperación; JSON es contrato portable. Reconciliación explícita y unidireccional por operación.

## Crítico

GES no es la fuente de verdad: además de su API no thread-safe, impone reglas de solape y commit. El dominio V2 calcula primero un grafo válido; el adaptador GES puede rechazarlo y debe devolver error estructurado. En ausencia de plugins/GPU se degrada a FFmpeg/software sin bloquear la GUI.
