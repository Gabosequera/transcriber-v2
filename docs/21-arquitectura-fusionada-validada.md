# 21 — Arquitectura fusionada validada

## Decisión de fusión

Conservar contratos y semántica editorial V1; refactorizar UI/orquestación; portar dominio/validación/undo/scheduler a Rust; mantener inferencia Python; usar GStreamer/FFmpeg como servicios aislados; tomar de Cutlass commands/staging, de Gausian crate boundaries/render ideas, de OpenCut/Kdenlive/Shotcut conducta observable y de Kerf diff/queue conceptuales.

## Componentes

```mermaid
flowchart LR
 GUI[egui/wgpu GUI] --> BUS[Command Bus]
 BUS --> CORE[Domain + Timeline + Layers]
 CORE --> DB[SQLite/Event Log]
 CORE --> JSON[JSON Contracts/Snapshots]
 CORE --> MEDIA[Media Service]
 CORE --> SCH[Hardware Scheduler]
 SCH --> PY[Python Workers]
 MEDIA --> GST[GStreamer/GES Owner]
 MEDIA --> FF[FFmpeg Processes]
 BUS --> AUD[Audit + tracing]
```

## Flujos

```mermaid
sequenceDiagram
 participant U as Usuario/AI
 participant B as Bus
 participant C as Core
 participant P as Persistencia
 U->>B: command envelope
 B->>C: parse/validate/dry-run
 C->>P: transaction + revision
 P-->>C: digest/new revision
 C-->>U: diff/result/audit
```

```mermaid
flowchart TD
 O[Abrir proyecto] --> I[Import JSON + validate]
 I --> D[Verify fingerprint/digests]
 D --> R[Reconcile si externo cambió]
 R --> S[SQLite transaction]
 S --> V[Regenerate views]
```

```mermaid
flowchart TD
 E[Edición humana/AI] --> C[Typed command]
 C --> V[Validate invariants]
 V --> X[Diff + inverse]
 X --> A[Apply revision]
 A --> Q[Preview/export snapshot]
```

```mermaid
flowchart TD
 AI[Agente] --> Cap[Capabilities + views]
 Cap --> Prop[Proposal]
 Prop --> Dry[Dry run/diff]
 Dry --> H{Approval?}
 H -->|sí| Apply[Apply command]
 H -->|no| Reject[Reject]
 Apply --> Audit[Audit + verify]
```

```mermaid
flowchart LR
 Job[Analysis job] --> Sup[Rust supervisor]
 Sup --> W[Python worker]
 W -->|hello/progress/checkpoint| Sup
 W -->|result/error| Sup
 Sup --> Store[Manifest + artifact digest]
```

```mermaid
flowchart LR
 T[ResolvedTimeline] --> PG[PreviewGraph]
 PG --> G[GStreamer decode/clock]
 G --> W[WGPU texture/compositor]
 T --> EG[ExportGraph]
 EG --> F[FFmpeg process]
 PG -. frame/hash/drift .-> EG
```

```mermaid
flowchart TD
 E[Export request] --> V[Validate revision/digest]
 V --> G[Compile ExportGraph]
 G --> F[FFmpeg supervised process]
 F --> R[Export record + artifact digest]
 R --> A[Audit / verify duration, frames, A/V drift]
```

```mermaid
flowchart TD
 Cmd[Command] --> Tx[SQLite transaction]
 Tx --> Snap[Deterministic JSON snapshot]
 Snap --> Out[Atomic replace]
 Out --> Dig[Digest + revision]
```

```mermaid
flowchart TD
 Err[Error/event] --> Redact[Redact secrets/paths]
 Redact --> Trace[tracing span/event]
 Trace --> UI[User console]
 Trace --> Log[Rotating JSONL]
 Trace --> Crash[Crash report opt-in]
```

## Qué se diseña desde cero

El vínculo clips↔evidencia, `ResolvedTimeline` común preview/export, authority/reconcile SQLite↔JSON, capabilities/risk policy AI, scheduler VRAM/RAM y packaging reproducible. Son piezas sin reutilización segura o con objetivos específicos de Transcriptor.
