# Flujos funcionales reales de V1

| Flujo | Entrada → salida | Código/artefactos | Riesgo y destino |
|---|---|---|---|
| importar medio | path → info/fingerprint | `medios.inspeccionar`, `medios.fingerprint`, `app.py` | ffprobe/paths; conservar contrato, Rust media |
| metadata | media → streams/resolution/t0 | `medios.py`, wizard/UI | offsets/codec; golden fixture |
| abrir/crear proyecto | source/spec → project dir/master | `editorial_pipeline._validate_spec`, `editorial_master`, `app_paths` | folder/stale; Rust importer |
| transcripción | audio tracks → whisper JSON | `editorial_pipeline` + `core`/faster-whisper | native model; Python worker |
| alignment | whisper words → aligned words/utterances | `align.py`, `core.align_transcription` | MMS checkpoint/stale; Python worker |
| signals | aligned audio → laughter/arousal/intensity | `laughter.py`, `prosodia.py`, `escena_audio.py` | optional steps/VRAM; Python worker |
| master JSON | track artifacts → master | `editorial_master.build_master/write_package` | schema authority; Rust core adapter |
| layers | master + marks/plan/trims → layers/views | `editorial_layers.adapters`, `LayerStore`, `write_snapshot` | projection vs source; preserve |
| manual layer edit | selection/gesture → revisioned layer | `editorial_layers.LayerStore.save`, UI modules | external change; Rust GUI/core |
| AI proposal | request/view → proposed JSON | `editorial_topics`, `editorial_trims`, `editorial_montaje` | stale/pass/invalid ids; command bus |
| preview/playback | master/media → FFplay/GStreamer frames | `editor_medios.py`, `medios.py`, `playback_clock.py` | orphan process/AV drift; Rust media |
| trims | silence/AI/user cuts → enabled segments | `editorial_trims`, `podcast_export.export_plan` | boundary/audio drift; preserve algorithm |
| export | resolved segments → media + records | `podcast_export.py`, FFmpeg scripts, EDL/FCPXML | VFR/keyframes/licence; process adapter |
| resume | manifests + partial artifacts → next step | `editorial_pipeline`, `_StepStore`, `editorial_projects` | stale output/repeat model; supervisor |
| invalidation/error | changed fingerprint/digest/crash → reject/retry | validators, `marcas`, `editorial_history`, logs | error codes currently inconsistent; redesign |

## Estado de persistencia

Los outputs pesados se publican por etapa, los manifests llevan claves de entrada y la UI detecta propuestas por polling. La condición de carrera principal no es el thread de cálculo sino la edición externa/AI entre lectura y guardado; V2 debe convertirla en revisión transaccional.
