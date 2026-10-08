# Continuación 14 — arousal audEERING aislado

Fecha: 2026-09-30. Fuentes V2 alpha.11. Este frente implementa `workers/python/arousal_worker.py`, su lock propio, downloader público y pruebas específicas. V1 `prosodia.py` se leyó exclusivamente como texto; no se ejecutó/importó ni se modificó. ASR/PyAV, entornos globales, alignment/transcription venv y políticas de seguridad no se alteraron.

**Resultado comprobado:** inferencia audEERING real en voz SAPI inventada, cinco ventanas y asociación de 23 palabras conocidas ya alineadas por MMS. Las trece pruebas independientes/protocolo pasan. La GUI y Rust todavía no invocan esta capacidad: no representa transcripción exitosa, aceptación GUI, equivalencia numérica completa con V1 ni validación emocional de personas o exactitud en español.

## Modelo, licencia y preparación

Recurso oficial: [audEERING model card](https://huggingface.co/audeering/wav2vec2-large-robust-12-ft-emotion-msp-dim). Revisión fija `6eba34a2485ea31cb03600241787c3a5edab8626`. Modelo de regresión entrenado en English MSP-Podcast, licencia **CC-BY-NC-SA-4.0**, descrito para investigación. No se sustituyen esos términos por la licencia del código V2. [Card archivada](e5-arousal-model-card.md) y [manifest](e5-arousal-model-manifest.json). Los pesos/venv permanecen en `.local`, no se agregan a Git ni a assets de release.

El principal anunció 661 MB/destinos y 127 GiB libres antes de autorizar preparación. Se descargó públicamente, sin autenticación ni tokens, en `.local/models/arousal-audeering`. El downloader usa URLs de la revisión fija, verifica SHA/tamaño, conserva archivos existentes y publica el nuevo archivo mediante hardlink exclusivo después de verificar/fsync; no reemplaza uno existente.

| Archivo | Bytes | SHA256 |
|---|---:|---|
| model.safetensors | 661375508 | efa5ac1a13b2d2f42182738e44794b1eb4c0cdd221a8b4ae11304c3a5f5fae95 |
| config.json | 2344 | c0962c3d1f065972bbebbba0bbffb8016ef4e9aae4a9b07e5fec22f770d2cddb |
| preprocessor_config.json | 214 | 60ca5a31e13f69ee2fbf147504c8676db5f6398fd7a6b12294341dff838edfcf |
| README.md | 3913 | 5fe5eb9afb24ab938fb3facf0eebe64a0f8a8628eb93fd772ba5c7120a45365d |

SHA de pesos coincide con LFS/upstream SHA consultado en API oficial `?blobs=true`; config/processor SHA son mediciones de bytes descargados de la revisión oficial fija y quedaron fijados explícitamente en downloader/worker. Manifest SHA `93c9c89093955b3a515b80825457b9c49049707edb737e896b2bd1eb111c4de4`; digest canónico del mapa de tres archivos `18f38ce13568ec98a0173603e4bf653097e802ac8fb3c7fa34df751c97a3a36e`.

[Config oficial](https://huggingface.co/audeering/wav2vec2-large-robust-12-ft-emotion-msp-dim/blob/6eba34a2485ea31cb03600241787c3a5edab8626/config.json): Wav2Vec2, hidden1024, 12 capas/16 heads, stable layernorm, etiquetas en orden arousal/dominance/valence. Se implementa el head del card/V1: mean pooling temporal, dropout→dense→tanh→dropout→linear3. Carga exclusivamente config local y `safetensors.load_file`, estado `strict=True`; no pickle, código remoto ni descargas durante run.

## Runtime reproducible

Venv propio `.local/e5-arousal-venv`, CPython **3.12.13**, uv **0.11.32**, `include-system-site-packages=false`. [pyvenv.cfg](e5-arousal-pyvenv.cfg) conserva home exacto `AppData/Roaming/uv/python/cpython-3.12.13-windows-x86_64-none`. Ejecutable uv usado: `C:/Users/gabri/AppData/Local/Microsoft/WinGet/Packages/astral-sh.uv_Microsoft.Winget.Source_8wekyb3d8bbwe/uv.exe`; cache propia V2 `.local/uv-cache`. Torch2.8 CPU reutilizó su wheel cacheado; no se modificó alignment venv.

Todas las 25 distribuciones están fijadas en `workers/python/requirements-arousal.lock` (SHA `52853cb13a0bbf5d36a38aad9f34b1b300774043a57bc0aa48655b6e357c3aa1`): certifi2026.7.22, charset-normalizer3.5.2, colorama0.4.6, filelock3.32.3, fsspec2026.7.0, huggingface-hub0.34.4, idna3.20, jinja2 3.1.6, markupsafe3.0.3, mpmath1.3.0, networkx3.6.1, **numpy2.5.3**, packaging26.3, pyyaml6.0.3, regex2026.9.29, requests2.34.2, **safetensors0.7.0**, setuptools78.1.0, sympy1.14.0, **tokenizers0.21.4**, **torch2.8.0+cpu**, tqdm4.70.1, **transformers4.48.3**, typing-extensions4.16.0, urllib3 2.8.0.

No Torchaudio/PyAV/librosa/SciPy/FFmpeg es necesario: input WAV PCM16mono16kHz se lee por ventanas con stdlib. El primer `hello` comprueba versiones por metadata sin cargar ML; distingue `runtime_state=not_loaded` de inferencia. `main` quita tokens conocidos y fija HF/Transformers offline. Imports nativos solo en `run` con ventanas analizables.

Compatibilidad consultada en fuentes primarias: [Transformers4.48.3 setup](https://raw.githubusercontent.com/huggingface/transformers/v4.48.3/setup.py) exige NumPy≥1.17/Torch≥2.0 y tokenizers0.21; [Torch2.8 requirements](https://raw.githubusercontent.com/pytorch/pytorch/v2.8.0/requirements.txt) no fija un límite NumPy. Se consideró [NumPy2.2.6 Python3.10–3.13](https://numpy.org/doc/stable/release/2.2.6-notes.html), pero **no se cambió versión**: el diagnóstico siguiente encontró y resolvió la causa de transporte con los paquetes originales. `.local/e5-arousal-v2-venv` fue creado vacío durante investigación y no se instaló/utilizó; el entorno original fallido quedó preservado y [su freeze archivado](e5-arousal-runtime-v1-failed.lock).

Preparación ejecutada:

```powershell
& 'C:/Users/gabri/AppData/Local/Microsoft/WinGet/Packages/astral-sh.uv_Microsoft.Winget.Source_8wekyb3d8bbwe/uv.exe' venv --python 3.12.13 .local/e5-arousal-venv
# Torch primero desde el índice CPU; las demás dependencias desde PyPI.
uv pip install --python .local/e5-arousal-venv/Scripts/python.exe --cache-dir .local/uv-cache --index https://download.pytorch.org/whl/cpu 'torch==2.8.0+cpu'
uv pip install --python .local/e5-arousal-venv/Scripts/python.exe --cache-dir .local/uv-cache 'transformers==4.48.3' 'safetensors==0.7.0' 'tokenizers==0.21.4' 'huggingface-hub==0.34.4'
# Freeze preservado en lock; para reproducir usar todos sus pins, no resolver de nuevo los transitivos más recientes.
.local/e5-arousal-venv/Scripts/python.exe -I scripts/download_arousal_model.py .local/models/arousal-audeering
```

`uv` en los ejemplos representa el ejecutable absoluto anterior; PATH no lo contenía. [Descarga](e5-arousal-download.log) conserva tamaños/SHA verificadas.

## Contrato tv2-arousal/1

NDJSON envelope cerrado `protocol/id/method/params`, máximo1MiB. `hello`→`run`, un job activo, `cancel` correlacionado al job, `shutdown`. Request cerrado liga job `job-[0-9a-f]{12}`, project/revision/digest, asset, SHA de fuente y duración ticks, normalized PCM SHA, transcript SHA, modelo/mapa SHA y manifest SHA. Timebase explícita `canonical-normalized-pcm/1`: corresponde a WAV normalizado que conserva el origen temporal de fuente; worker no puede verificar la fuente original cuando solo recibe PCM, por lo que declara esa lineage como responsabilidad del host.

Parámetros cerrados CPU/threads1–4/batch1–8, ventana/hop0.1–30s (hop≤ventana), gap0–30/pad0–10 y scope speech-regions/full-audio. Defaults de aceptación: ventana4s/hop2s/batch1/threads2/gap1s/pad0.5s. Las regiones unen huecos≤1s, se acolchan/capan y vuelven a unir; offsets caen en grid global de hop (V1), con dedup. Ventana final se rellena con ceros antes de normalización del feature extractor, como V1; RMS usa solo samples válidos.

Eventos finitos contienen t_ini/t_fin redondeados3, arousal/dominance/valence6 y rms_dbfs3. Baseline es mean/population std; std0 se reemplaza1. Arousal_z se redondea3. Asociación por solapamiento semiabierto pondera la duración de cada ventana, devuelve null si no hay coincidencia y conserva orden/word_id/text/times/otros campos humanos mediante copia profunda. No acepta/desactiva/edita items del proyecto.

Artifact separado `arousal/arousal-words.json` conserva transcript y añade `editorial-arousal/1`+`tv2-arousal-words/1`; receipt incluye binding+artifact SHA. Checkpoint `.work/manifests/arousal.json` usa request/runtime/algorithm digest, artifact/path únicos y SHA; corrupciones no se aceptan como cache. Verifica inputs y tres archivos de modelo antes y después de inferir; cancel antes de publicar evita checkpoint válido. Escritura temp/fsync/replace ocurre únicamente dentro del job. Rutas rechazan traversal/backslash/colon/componentes no normales y escapes por canonicalización.

## Diagnóstico causal y corrección del lector

Primer análisis real01 alcanzó verify-inputs y load-arousal, pero superó120s antes de modelo/forward. [Resultado fallido](e5-arousal-real-01-result.json) y [log](e5-arousal-real-01.log) conservados. No se atribuyó a Smart App Control: no hubo evento CI correspondiente a Python/arousal/NumPy en la ventana consultada. No se reintentaron PyAV/ASR ni suites4551.

Diagnóstico02 añadió fases exactas+faulthandler30s: Torch._C inicializa NumPy `_multiarray_umath` mientras main espera `stdin.buffer.readline`. Se cerró exclusivamente árbol propio29076/29400/27288 para respetar ventana PERF anunciada, conservando [stack/result](e5-arousal-real-02-result.json) y [log](e5-arousal-real-02.log). Tres handles OS adquiridos antes de confirmar salida quedaron señalados. No hubo inferencia ni artifact/checkpoint.

Nuevo probe mínimo sin modelo aisló la diferencia, con mismas versiones, origen y políticas:

| Contexto | Tiempo import | Evidencia |
|---|---:|---|
| NumPy solo main | 0.094s | [log](e5-arousal-numpy-probe-01.log) |
| NumPy solo hilo | 0.094s | [log](e5-arousal-numpy-probe-thread.log) |
| Torch→NumPy main, sin lector bloqueado | 1.984s | [log](e5-arousal-torch-main.log) |
| Torch→NumPy hilo, sin lector bloqueado | 1.797s | [log](e5-arousal-torch-thread.log) |
| Torch→NumPy hilo, main readline bloqueado | 21.312s, desbloqueo stdin20.110s | [log](e5-arousal-torch-stdin.log) |
| Torch→NumPy main, reader hilo bloqueado | 21.282s, desbloqueo stdin20.187s | [log](e5-arousal-torch-reader-thread.log) |
| Torch→NumPy hilo, lector PeekNamedPipe/os.read | **1.703s antes de desbloqueo stdin20s** | [log](e5-arousal-torch-peek.log) |

Los stacks15s de ambos lectores bloqueantes coinciden en init de NumPy; pasar ML al hilo principal no basta. [PE dependencies](e5-arousal-numpy-pe.log) identifica OpenBLAS/CRT/Python DLLs; `_multiarray_umath` SHA `a9d5c4202cf109bf24305f3f1180e0509aaf661ba850612bd75df7cfd9f928b8`. Que la inicialización nativa espere el descriptor CRT es una inferencia causal de ese comparativo; no se instrumentó código C para demostrar su lock interno exacto.

`request_lines` usa [PeekNamedPipe oficial](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe) para obtener bytes disponibles y el único lector llama os.read hasta esa cantidad, sin read bloqueante cuando no hay datos. Buffer incremental acotado, mensajes parciales conservados, EOF109/232, nunca consume más de1MiB+1 antes del rechazo. Mantiene lector capaz de recibir cancel durante run; se compartió con E4. Synchronous pipe requiere lector único; los casos ejecutados prueban esta disposición, no cualquier topología de handles/plataforma.

## Validación real03 y límites

[13 tests independientes/protocolo](e5-arousal-protocol-final.log): PASS1.153s, sin ML en proceso de tests/protocol workers. Regiones/grid/RMS/population baseline/asociación half-open/copia humana/IDs/times/params/traversal/cancel antes inputs/hash checkpoint corrupto/partial lines/múltiples envelopes/EOF parcial/oversize. Py_compile de cuatro scripts y diff whitespace PASS; esto es compilación Python, no build Rust ni inferencia.

Luego, tras aviso del principal y término PERF, nueva ejecución con humano GUI349df PID6868 abierto como carga externa:

```powershell
.local/e5-arousal-venv/Scripts/python.exe -I tests/scripts/test_e5_arousal.py --root .local/e5-arousal-real-03 --real --audio .local/e5-python-tests-20260930/fixtures/speech-a.wav --transcript .local/e5-mms-real-02/real/mms-known-sapi/alignment/aligned-words.json --model .local/models/arousal-audeering --timeout 180
```

[Log real03](e5-arousal-real-03.log), [result completo](e5-arousal-real-03-result.json), [words](e5-arousal-real-03-words.json), [checkpoint](e5-arousal-real-03-checkpoint.json), [SHA fuentes/resultados](e5-arousal-sha256.txt): **exit0/PASS**, 13 tests1.131s y sesión real+resume+cancel+errors **21.485s**. No se usó un modelo ficticio.

- PCM mono16kHz **9.531s**, fuente SHA `90f6dfc24eca1116f879c5269e0dbec6cc5e0687e7bcd89ccea11344f641397a`; texto inventado conocido23palabras. MMS produjo sus tiempos, no ASR. El fixture añade edited=true/state=disabled/comment humano a la primera palabra; esos campos y todos los campos originales permanecen iguales en salida.
- Cinco eventos audEERING finitos, baseline **mean0.490142/std0.065602**; recomputar asociación de las23palabras coincide exactamente con salida. Artifact14759B SHA **8c29e0c28780345362d123730b19bc14971b573f84968d92319dfa7ab01de1db**. Flag native_inference_executed=true/execution_state=completed procede de forward real.
- Resume devuelve mismo receipt/SHA y cached=true sin evento load-arousal. Nuevo run cancelado en verify-inputs da E_CANCELLED y no checkpoint; audioSHA alterado da E_PRECONDITION antes de native loading. No prueba cancel dentro de forward: Torch termina su llamada antes de atender flag y el host deberá aplicar deadline/JobObject cuando se integre.
- Medición por Win32 Toolhelp32+GetProcessMemoryInfo del árbol propio: **hijo nativo3876 pico WorkingSet1663479808B=1.549GiB**, launcher10608 pico5058560B. Su hijo auxiliar29800 también se midió. Los handles de estos tres objetos OS quedaron **señalados3/3** tras shutdown; no se seleccionó/terminó proceso ajeno. Picos no son garantía para archivos grandes, batch8/ventana30s o GUI paralela.

Worker SHA de ejecución real03 `ad3e56279b79e579153db2a9b2874cc923379f5311f6d0952d28b2166fb6a29b`. La GUI349df no contiene integración arousal. Pendientes: supervisor Rust/prepare/import de regiones+word attributes con protecciones humanas y DAG/invalidation, selección/edición/revisión GUI, cancelación durante forward con deadline y handles del host, medición sobre medios grandes y evaluación de accuracy lingüística/numérica. Esta capacidad ejecutada no cierra por sí sola E5 ni sus demás requisitos obligatorios.

## Portado del lector ASR solicitado después de real03

El principal autorizó copiar el lector demostrado a `workers/python/worker.py`; E4 confirmó que ese archivo estaba libre. Se mantuvo la disposición run en hilo y se sustituyó solamente el loop lector por incremental `request_lines`, con cleanup EOF/oversize y shutdown return para no duplicar join. La copia queda dentro de cada worker: su SHA existente liga también el helper, y `python -I` no necesita añadir rutas de importación ni otro archivo que el runtime actual aún no verifica. Extraer un helper compartido requeriría ligar su SHA explícitamente en todos los payload/runtime identities; se dejó esa refactorización para cuando aporte una ventaja concreta.

Hash ASR anterior preservado `2d9952a504e8dfd699b2bc970ebd816bb389385552b3163ecb20420ef6f9019e`; los jobs/receipts históricos se conservan con esa identidad y **no se reatribuyen** al worker modificado. [SHA nuevo/result](e5-asr-reader-sha256.txt), [log](e5-asr-reader-independent.log) y [resultado completo](e5-asr-reader-independent-result.json). En root nuevo `run-reader-07`, la suite explícitamente `--suite independent` ejecutó **12/12 PASS6.072s**: extracción FFmpeg multistream/offsets, cache/corrupción, cancel/reap del FFmpeg propio, cambios modelo/lang reutilizan únicamente extracción, hello/runtime/closed capabilities, rechazo de precondiciones antes de FFmpeg, EOF/cancel y traversal. Fuente sintética intacta; py_compile y whitespace PASS.

**No se importó faster-whisper/PyAV ni se pidió inferencia ASR**. El cambio de lector no demuestra que las DLL PyAV bloqueadas hayan cambiado ni autoriza repetir esa inferencia; su bloqueo anterior sigue vigente. La nueva arousal real03 permanece ligada a su worker propio y no se alteró para incorporar este portado. MMS/laughter son responsabilidad de E4; sus nuevos hashes y regresiones se registran en sus informes.

## Host Rust ejecutado sobre cadena nueva común

El principal aprobó diseño y añadió `pub mod arousal`; este frente creó únicamente `crates/pipeline/src/arousal.rs` y `crates/pipeline/examples/arousal_host.rs`. DTO públicos Clone/Serialize/Deserialize cerrados, parent=JobRecord<AlignmentPayload>+AlignmentResult, runtime aislado y parámetros acotados. `crate::validate_record` comprueba schema/ID/digest del payload **antes de jobs::claim** y al validar resultado. Parent exige Succeeded/result exacto y revalidación alignment, que a su vez verifica ASR ancestor, fuente y PCM. Modelo/manifest/code/lock/input SHA se revalidan; lectura de artifact/transcript usa los mismos bytes hasheados y parseados.

El validator exige receipt cerrado, único artifact/path fijado y preservación exacta del objeto parent salvo atributos arousal. Metadata/events/baseline/runtime son DTO cerrados: campos nuevos no pasan. Relee header RIFF PCM16mono16kHz para samplecount, deriva regiones y grid de hop de las palabras alineadas, valida cardinalidad/rangos source-clock y recompone baseline/population std/z y asociación ponderada. No añade offset de stream porque PCM/words ya tienen T0 aplicado. Valida ID/text/times/provenance/asr_original y todos los campos humanos originales, incluso si un atacante reemplaza bytes y recalcula el SHA del receipt. No hay prepare/import/mutación del proyecto desde este módulo.

Builds del ejemplo previos a ejecución fallaron por ruta de `V1Master`/bound Serialize, parámetro projections(&AssetId) y nombre ErrorCode; [primero](e5-arousal-host-build.log), [segundo](e5-arousal-host-build-fixed.log), [tercero](e5-arousal-host-build-third.log) preservados. Se corrigieron esas referencias en el ejemplo. [Build final](e5-arousal-host-build-final.log) **PASS5.28s**, [Clippy](e5-arousal-host-clippy.log) **PASS1.47s** con -Dwarnings; rustfmt específico/whitespace correctos. Avisos iniciales Clippy del principal sobre modulo arithmetic/unwrap se corrigieron antes de este freeze. No se reconstruyó GUI abierta ni se repitió ejecutable/suite4551.

Después de confirmación E4 de ventana libre, primera ejecución nueva:

```powershell
scripts/cargo.ps1 build --locked -p tv2-pipeline --example arousal_host
scripts/cargo.ps1 clippy --locked -p tv2-pipeline --example arousal_host '--' -D warnings
target/debug/examples/arousal_host.exe C:/Users/gabri/Todo/transcriber-v2 C:/Users/gabri/Todo/transcriber-v2/.local/e5-arousal-host-01 C:/Users/gabri/Todo/transcriber-v2/.local/e5-mms-real-03/known-transcript.json
```

[Runtime](e5-arousal-host-runtime-01.log) **exit0/PASS** y [report](e5-arousal-host-host-result.json). El ejemplo importó el Asset con comandos humanos **solo en una session fixture en memoria** y guardó [project.json](e5-arousal-host-project.json) base `proj-5789225831cf`, revisión1 y asset `asset-76f1df4dc491`. Master y raw transcript comparten `tracks.a0`, 23words/IDs, un utterance con sus word_ids y conversation compatible; parser/proyecciones V1 real lo aceptaron. Sus tiempos iniciales son inventados uniformes y su ASR payload lleva fixture/no-decoding explícitos. Crear Succeeded para este parent fixture no representa un decoder real.

| Etapa | Job | Recibo final archivado | Resultado |
|---|---|---|---|
| ASR fixture, **no inferencia** | job-57fe5b97cf1c | [record](e5-arousal-host-parent-asr.json), [result](e5-arousal-host-parent-asr-result.json) | Master/PCM/transcript inventados estructurales ligados a worker actual B3bd11… |
| MMS **real** | job-0be1e8a200a1 | [record](e5-arousal-host-alignment-job.json), [result](e5-arousal-host-alignment-result.json) | Native alignment23 palabras, **11.9861937s**, worker7c59aa… |
| audEERING **real** | job-9df2fbd58fa5 | [record](e5-arousal-host-arousal-job.json), [result](e5-arousal-host-arousal-result.json) | Native arousal5 ventanas, **14.7873638s**, workerAD3e562… |
| audEERING cancel | job-af113a7927a3 | [record](e5-arousal-host-arousal-cancelled-job.json) | Durable Cancelled, ErrorCode::Cancelled en verify-inputs, sin inferencia de ese job |

Los records/resultados originales permanecen en `.local/e5-arousal-host-01/{parent-asr,alignment-job,arousal-job}.json` para consumo de E4/root sobre **la misma cadena exacta**. El principal preparará finalización con esa base; este frente no reasoció receipts anteriores cuyo worker había cambiado ni insertó observaciones al proyecto.

[Words nuevas](e5-arousal-host-words.json) SHA **b08b69aa500d384953c390b5aac8ffee433d0b3e4d8615f19389083d3e91637a**; es distinto del caso Python real03 por metadata/track/IDs de la nueva fixture, no una sustitución de sus bytes. Todos los atributos originales de la primera palabra (incluidos edited/state/comment y asr_original) quedan conservados. El proyecto de la session permanece idéntico a la base durante MMS/arousal/cancel.

Rechazos ejecutados: payload de record alterado antes de claim deja job Queued, más14 casos listados en report (revisión obsoleta, traversal, sibling, campos extra result/payload, word_id, asr_original, comentario humano, asociación, baseline, extra metadata/event, runtime lock y source lineage). Los nueve ataques a bytes actualizan deliberadamente SHA del receipt para alcanzar las comprobaciones semánticas; todos se rechazan. Se restauraron bytes originales y se revalidó el receipt durable. No se reclamó inferencia durante esos ataques.

[SHA finales de módulo/ejemplo/binario/report/words](e5-arousal-host-final-sha256.txt) fija esta ejecución. La comprobación de procesos después no halló ningún Python propio activo; los handles3/3 de Python real03 están documentados antes. Este ejemplo host canceló en verify-inputs: no demuestra por sí solo el plazo del host ante un forward Torch ya iniciado. Esa frontera/GUI/DAG/evaluación lingüística sigue pendiente aunque la capacidad nativa y el supervisor de este caso hayan pasado.
