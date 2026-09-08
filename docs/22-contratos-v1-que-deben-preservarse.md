# 22 — Contratos V1 que deben preservarse

| Contrato | No romper | Migración |
|---|---|---|
| `editorial-master/1` | media fingerprint, duration/t0, tracks, words/utterances/conversation, provenance | Rust serde adapter + golden round-trip |
| `editorial-trims/1` | identity media, revision/next_id, enabled/edited/origin/evidence y unión de cortes | importar a `SemanticLayer::Trims`, export exacto |
| propuestas trims/topics/layers/montage | request_id, digests, pass, IDs source y validación stale | envelopes externos convertidos a commands |
| `editorial-layer/1` | item multirrango, estado, parent_id, tombstones, revision | validator común, nombres/path compatibles |
| chunks/topics/subtopics/lanes | ids, contigüidad/jerarquía, source digest, lane order | fixtures y migradores puros |
| derivación padre/hijo | source mapping, ancestor, baselines y no inferencia duplicada | child adapter con mapping temporal |
| manifests | step id, params, input/output digests, status, checkpoint | supervisor Rust conserva ids/keys |
| vistas AI | transcript/signals/layers/current regenerables y digest | versionar `generated_from` |
| exports/fingerprints/digests | media original no tocado, record de formato/shift/hash | adapter FFmpeg y auditoría |
| undo/redo | multi-document transaction y stale por revision | commands serializables + inverse |
| config/keymap | defaults, bindings y secrets fuera de manifests | migración explícita; secret store |

Los campos nuevos son opcionales hasta subir schema. Toda escritura es atómica y produce backup/diff cuando migra.
