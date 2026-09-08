# Inventario de contratos V1

| Contrato/archivo | Productor | Consumidores | Identidad/versión | Obligatorios e invariantes | Fuente de verdad / regenerable | Stale/conflicto |
|---|---|---|---|---|---|---|
| `editorial-master/1` (`*.editorial.master.json`) | `editorial_master.build_master`, pipeline | UI, chunks, layers, projects, export | media fingerprint + `source_master_digest` | schema, project, media, transcription, ≥1 track, conversation; tiempos finitos | master es fuente; markdown/vistas regenerables | digest de master |
| `editorial-trims/1` (`views/trims.json`) | `editorial_trims` | UI/export/layers | fingerprint media, revision | duration, revision, next_id, cuts; union de enabled; no mutar originales | trims editable; layer proyectada regenerable | fingerprint + revision |
| `editorial-trims-proposal/1` | AI/worker | `validate_proposal`, merge | source master digest, request/pass | cuts normalizados, chunk/utterance ids válidos, razones | propuesta no aplicada | digest mismatch rechazo |
| `editorial-layer/1` (`layers/*.json`) | `editorial_layers`/UI | UI, AI snapshot, projects | layer_id + media fingerprint + source master digest + revision | ids/rangos únicos, state, parent acíclico/≤32, child dentro parent | layer editable; `views/layers.json` regenerable | external digest/revision; tombstone |
| `editorial-layers-proposal/1` | AI | merge responses | master + layers digest | capas validadas, no resucitar deleted | propuesta no aplicada | ambos digests |
| `editorial-layers-view/1` | `write_snapshot` | AI | source master/layers digest | layers + digest calculado | vista regenerable | digest |
| `editorial-chunks/1` | local/Codex/AI | revisión, topics, trims | source master digest opcional/esperado | chunks contiguos, ids, t_ini/t_fin, first/last utterance | plan elegido por usuario; markdown regenerable | digest |
| `editorial-topics-request/1` / proposal | `editorial_topics.prepare` / AI | validator/import | request_id, master/layers digests, pass | scope y items jerárquicos; pass 1/2 | proposal no aplicada; layer resultante fuente | request/pass/digests |
| `editorial-montage-request/1` / proposal | `editorial_montaje.prepare` / AI | merge/export | request_id, master/layers/montage digest, pass | clips, source ranges, keep/repeat, limits | pass JSON + montage actual | digest/pass |
| `editorial-catalog/1` | `editorial_catalog` | discovery/UI | relative path + fingerprint + master digest | entries | cache regenerable | revalidate candidate |
| `keymap/1` | `keymap.py`/UI | shortcuts | schema + bindings | valid action/key names | config fuente, diff regenerable | validation |
| manifests de pasos | `editorial_pipeline` | resume/status/UI | step id + params/input/output digest | status, params, paths, hashes, error/progress | manifest + output checkpoint | key exacta e input digest |
| `editorial-derivation/1` | `editorial_projects` | child import/layers | parent digest/fingerprint + mapping | source ranges, segments, ancestor | child master fuente; mapping evidencia | parent digest |
| exports/fingerprints/digests | `podcast_export`, `medios`, `editorial_io` | resume/audit | SHA-256/identity | bytes/metadata coherentes | artifact + record | hash mismatch |
| undo/redo | `editorial_history.HistoryStack` | UI/controller | doc ids + expected revisions | before/after snapshots; depth 50; one action multi-doc | memoria, no persistencia V1 | `Stale` si revision difiere |

## Implicación V2

Estos contratos se convierten en fixtures, no se sustituyen por tablas opacas. Todo nuevo schema declara migración, compatibilidad y campo `generated_from` si es una vista.
