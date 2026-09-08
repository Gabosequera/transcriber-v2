# 20 — Validación del AI Control Plane

Todos los comandos usan `CommandEnvelope` de `docs/07`, con `base_revision`, `precondition_digest`, actor, project, idempotency y risk.

| Comando | Precondiciones | Inversa/diff | Permiso |
|---|---|---|---|
| `create_track` | proyecto abierto, id único | remove track / track added | proposal o humano |
| `create_layer` | fingerprint/master válidos | tombstone layer / layer added | proposal |
| `insert_media` | asset importado, track compatible, range válido | remove clip | review |
| `move_clip` | clip/track unlocked, no colisión | move original | review |
| `trim_clip` | source content suficiente, límites válidos | restore range | review |
| `split_clip` | playhead interior | join/restore clip | review |
| `ripple` | tracks y policy declarados | inverse delta | human approval |
| `add_marker` | T0 válido | remove marker | auto |
| `create_proposal` | vista/digest actuales | delete proposal | auto |
| `apply_proposal` | proposal + digest/pass/stale checks | inverse transaction | human approval |
| `reject_proposal` | proposal existente | restore proposal state | auto |
| `set_property` | allowlist/type/range | old value | según propiedad |
| `launch_analysis` | capability/model/input digest | cancel/mark job | auto/review cost |
| `cancel_analysis` | job owned/running | none/resume possible | auto |
| `export` | timeline valid, destination approved | delete artifact only | human approval |
| `query_state/errors` | project scope | none | read-only |
| `undo/redo` | expected revision | opposite command | human/AI same bus |

Cada resultado registra affected IDs, expected effects, warnings, error code, events, new revision/digest y audit record. El flujo es `inspect → propose → validate → dry-run → diff → preview → apply → verify → audit`.

No se permite un comando genérico `execute_sql`, `write_file`, `run_shell`, `mutate_gstreamer` o `set_state`.
