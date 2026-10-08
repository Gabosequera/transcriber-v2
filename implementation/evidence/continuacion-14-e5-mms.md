# Continuación 14 · E5 MMS independiente de PyAV

Se implementó `workers/python/alignment_worker.py` standalone y `tests/scripts/test_e5_alignment.py`. V1 `align.py` se consultó como texto, sin importarlo ni ejecutarlo; no se modifica `worker.py` de ASR, Rust/GUI, V1, locks o seguridad Windows. No hay descarga automática, red o credenciales desde el worker. MMS alineación forzada no sustituye ni demuestra ASR.

## Runtime/modelo local

El principal instaló `.local/e5-alignment-venv` CPython3.12.13 y Torch/Torchaudio2.8.0+cpu; primer import nativo y disponibilidad MMS_FA/forced_align confirmados. Lock `workers/python/requirements-alignment.lock` SHA256 `29d21883e56c3fbf5db7ca6b2af1dca514e832855bb9f52d8b75fe84e174eab8`. El worker exige versión Python y todas las distribuciones exactas por metadata, sin importar Torch en hello.

MMS local `.local/models/mms-fa/model.pt`, 1,262,047,414 bytes, SHA256 `20ef12963ab4924bef49ac4fc7f58ad5da2ee43b2c11bc8c853c9b90ecdbc680`. El manifiesto del principal registra URL pública oficial, ETag, Last-Modified y licencia CC-BY-NC4; ese SHA es una medición del download, no digest firmado upstream. El modelo queda local, no redistribuido.

Se leyó la API de la wheel2.8 instalada: `_get_model(bundle._model_type,bundle._params)`, carga **local** `torch.load(weights_only=True,map_location='cpu',mmap=zipfile.is_zipfile(...))`, eliminación de `bundle._remove_aux_axis` mediante `_remove_aux_axes`, `load_state_dict(strict=True)` y `_extend_model(normalize_waveform=...,apply_log_softmax=True,append_star=True)`. Nunca se usa `bundle.get_model`, que descargaría pesos. Tokenizer/aligner de la misma bundle. CPU fijada a dos threads. Lectura PCM stdlib wave/struct → torch.tensor, sin NumPy/PyAV.

## Contrato standalone

Lanzamiento `python -I -u alignment_worker.py --work-root ROOT`. NDJSON puro `tv2-alignment/1`, hello/run/cancel/shutdown, respuesta ID correlacionado result/error y progreso job. Carga nativa solo en run con palabras alineables, un job/thread activo, cancelación/EOF/shutdown cooperativos. Una llamada nativa forward no admite interrupción interna; el futuro host debe poseer JobObject y deadline. Se declara `gui_integration=false`, `asr=false`, `project_mutation=false`, output `tv2-aligned-words/1`; no se integra a GUI/orquestación en este incremento.

Run cerrado: job/project/revision/project_digest/asset_id, normalized_audio_path+SHA, transcript_path+SHA, model_path/manifest_path/model_sha256, source_sha256/source_duration_ticks/timebase=`flicks/705600000`; parámetros CPU2, batch_words1..120, margin0..1 y max_batch_seconds≤30. **source_sha256 es lineage del host**: sin source_path este worker no vuelve a medir el medio original; lo explicita en metadata. Normalized PCM/transcript/model sí se revalidan antes/después, modelo SHA/tamaño/bundleversión contra manifest, checksum del manifest antes/después. WAV mono PCM16/16kHz, duración normalizada ≤sourceDuration+una muestra y clamp al menor, palabras IDs únicas/timesordenados/positivos dentro del audio.

Normalización NFKD sin acentos, `[a-z']`; batches con span máximo30s más margen≤2s. El máximo fin dentro del grupo controla palabras ASR solapadas; una palabra patológica >30s falla explícitamente antes de Torch. Alineación conserva ID/text/extra/probabilidad y copia `asr_original` por palabra. Fuente `mms`, `mms_interpolated`, `whisper_unalignable` o `whisper_fallback`; fallos de lotes conservan tiempos ASR y generan `failed_batches`, sin declararlos alineados. Interpolación usa fórmula V1 cuando produce rango válido, con clamp al medio. Rangos de utterances originales se retienen y se documenta que el host debe rederivar master/proyecciones.

Artefacto único `alignment/aligned-words.json` por job, recibo ligado a input y artifactsSHA. Cache `.work/manifests/alignment.json` exige schema/stage/input_digest y exactamente la ruta prevista con SHA; key incluye request/runtime/worker/lock/manifest/algoritmo, no reutiliza por revisión sola. No se modifica el transcript original ni un proyecto.

## Pruebas y primera alineación acústica real

Input SAPI público/inventado existente `.local/e5-python-tests-20260930/fixtures/speech-a.wav`, 16kHz mono. Texto conocido de `make_e5_speech.ps1`: “This is a synthetic recording for a local software test. The blue notebook is on the table. We will meet tomorrow at nine.” Son **23 palabras conocidas**, tiempos iniciales uniformes y probabilidad null, sin ASR fingido.

Primera batería independiente `.local/e5-mms-independent-01`: 7/7 correctos 0.674s, sin imports Torch. Tras binding/clamping acordado, primera corrida acústica `.local/e5-mms-real-01`: **8/8 correctos 14.472s**, MMS23/23, failed_batches vacío, cambios acústicos reales de timestamps, resume con mismos hashes. La medición inicial memoria6MB correspondía al launcher venv, **no a Torch**; se conserva y se corrige explícitamente.

Se corrigió solo monitor de prueba para enumerar descendientes propios Toolhelp32 y consultar PeakWorkingSet con GetProcessMemoryInfo. Se añadió prueba real stdlib de etapa unalignable/cache/corrupción/manifest con artefacto equivocado y cancelación NDJSON antes de cargar modelo; no se simula modelo. Corrida final:

```powershell
& '.local/e5-alignment-venv/Scripts/python.exe' -I tests/scripts/test_e5_alignment.py --suite real --work-root '.local/e5-mms-real-02' --evidence '.local/e5-mms-real-02/result.json'
```

**9/9 correctos en22.188s** (batería completa incluye repetidas lecturas SHA del modelo de1.2GB para cache/invalidación). Worker Torch real hijo PID24736, launcher8784, conhost25588. PeakWorkingSet Torch **2,718,699,520 bytes (2.532GiB)**, WorkingSet al consultar1,458,614,272 bytes y PeakPagefile3,621,777,408 bytes. Memoria representa este PCM corto/modeloCPU2, no un límite demostrado para32s/podcasts. Proceso cerrado por shutdown al terminar prueba; no se tocó GUI humana.

Salida acústica:23 alineadas,0 interpoladas,0 failed_batches, execution_state=`completed`. Ejemplos This .14–.32, is .38–.50, synthetic .64–1.16, recording1.22–1.80. Confianza ASR sigue null; no se inventa certeza de transcripción. Se verifica igualdad de IDs/text/originales, SHA artefactos y fuente/transcript intactos. El output final coincide en bytes con la primera corrida, evidenciando determinismo de esta fixture. No se realizaron retries de PyAV ni suites4551.

SHA256:

- Alignment worker: `e4a247a4e13c69210036ab1ef0b1ba68cff28912d19abd68c9bbe484bf080fec`.
- ASR worker intacto: `2d9952a504e8dfd699b2bc970ebd816bb389385552b3163ecb20420ef6f9019e`.
- `.local/e5-mms-real-02/result.json`: `4d5d0fa674329bd3d439945bce8f7231f4ce75d35a1aad53f7f33f85cc5ff51c`.
- `.local/e5-mms-real-02/real/mms-known-sapi/alignment/aligned-words.json`: `c4049dd52a9731a6645bdd3dde886ebe13a403ed6e8a58434420cb31994640a0`.

Esto acredita **MMS acústico real sobre voz y transcript conocidos inventados**, y lifecycle/cache independiente. No acredita ASR, alineación de salida ASR real, cancelación dentro de forward nativo, integración de editor o GUI bajo carga. El siguiente incremento del principal define host Rust aislado con lineage al job ASR y verificación íntegra antes/después, sin incorporación automática a proyecto.

## Host Rust aislado con recibos y lineage

Se añadió `crates/pipeline/src/alignment.rs`; el principal habilitó `pub mod alignment` y expuso dentro del crate el supervisor por protocolo. No se editaron `lib.rs`, `process_tree.rs` ni Desktop desde este frente. El principal cerró la carrera del launcher venv con CREATE_SUSPENDED/JobObject attach/ResumeThread del hijo propio antes de autorizar esta corrida.

`AlignmentRuntime`, `AlignmentParameters`, `AlignmentPayload` y `AlignmentResult` tipados rechazan campos desconocidos en su nivel. `enqueue` parte de parent AnalysisRecord final+AnalysisResult verificados, con payload digest y recibo final exactos. Captura lineage parent-job/project/revision/projectdigest/Asset y fullsourceSHA/duración Flicks. Selecciona **solo** `.work/audio/aN.wav` / `.work/transcripts/aN.json` del audio_index existente en Asset, bajo root de ese job; no recibe rutas alternas ni elige siblings.

Host verifica bytes fuente original, modelo/worker ASR del parent, todos sus artefactos recibidos, modelo MMS/manifest/worker/lock antes y después. Negocia hello `tv2-alignment/1`, CPython3.12.13 y todas las versiones exactas del lock; valida las mismas identidades de runtime en metadata final. Reutiliza writer cancelable, respuesta/eventos correlacionados, JobObject y close2s del mismo OwnedWorker y jobs::claim/lease/finish persistidos. No descarga modelo ni incorpora resultados a proyecto/master.

Resultado permite **exactamente** `alignment/aligned-words.json` con un artefacto y SHA. Se verifican IDs/binding completo, duración/rangos positivos, cardinalidad, metadata model/manifest/audio/transcript/source/runtime concordante y estado/contadores/fallos de lote. El transcript original leído se rehashea contra el recibo (protege lectura concurrente). Cada palabra debe ser exactamente una copia de la original salvo tiempos y los campos `alignment_source` / `asr_original`; ID/text/probabilidad/extra no cambian. Resto del track (incluidas intervenciones) permanece idéntico. `completed` con cero palabras MMS se rechaza; un resultado fallback declara fallos.

### Primera ejecución del host y corrección de boundary del ejemplo

Nuevo `crates/pipeline/examples/alignment_host.rs` y `scripts/test-e5-alignment-host.ps1`. El ejemplo crea un **parent ASR inventado declarado** con PCM SAPI y transcript conocido; no invoca ASR/PyAV y el recibo fixture incluye esa clasificación. El modelo tiny solo se verifica como lineage de parent, no se carga. Todo proyecto del ejemplo queda en memoria sin cambio y no hay GUI.

`e5-mms-host-01` compiló y ejecutó el ejemplo, pero rechazó el root propio porque comparaba un canonical Windows con prefijo `\\?\` contra ruta normal. No hubo bloqueo de ejecución/4551 ni carga Torch. Se corrigió exclusivamente el ejemplo para comparar parentcanonical y repocanonical, manteniendo rutas normales hacia herramientas/runtime. Se conservaron build/runtime logs del intento; el nuevo root `...-02` corresponde al código corregido. Sin eludir seguridad.

Comando final:

```powershell
pwsh -NoProfile -File scripts/test-e5-alignment-host.ps1 -OutputDirectory 'C:\Users\gabri\Todo\transcriber-v2\.local\e5-mms-host-02'
```

Build debug alpha.11 **correcto3.39s**; host real MMS **PASS**, inferencia y validación del run13.4547197s.23 palabras alineadas, failed_batches vacío. Job MMS `job-109e10d06a7f` durable **succeeded**. Job adicional `job-63b311561aa5` cancelado al evento verify-inputs antes de native, durable **cancelled**, código Cancelled. Se confirmó proyecto de sesión idéntico y no incorporación de master/capas. Uso de CPU se coordinó con E2 tras su medición base; GUI humana26948 no se tocó.

Casos adversos ejecutados por el ejemplo: audio_index inexistente, CPU4, resultado de revisión diferente, artefacto sibling adicional y unknown-fields en Result/Payload se rechazan. Además se alteran ID de palabra, probabilidad y manifest-lineage en el JSON de salida, **recalculando el SHA del recibo para llegar a los controles semánticos**: los tres se rechazan. Artefacto se restaura a sus bytes exactos originales y vuelve a validarse. No se modifican audio/transcript/modelo original. Estos casos no ejecutan otro forward nativo. No se han ejecutado suites4551 ni aceptado GUI/ASR por estos resultados.

Clippy ejemplo/módulo `-D warnings` y rustfmt correctos. Clippy inicial detectó field-reassign-with-default en el ejemplo; se corrigió con initializer struct, sin allow. Check final0.76s. Hashes:

- `alignment.rs`: `e8b5e0485f643c3f178e9067ddd067cf72e44c87d00746fd087727be6602d762`.
- debug `alignment_host.exe`: `602c7a63087b0d9c0366d26714014d513acc9f059755accd221261c7dd12a374`.
- `.local/e5-mms-host-02/run/host-result.json`: `22cab5640d927d45c29375a2271f4d33ce52395034ee0b7dd75c6f16a4a0330f`.
- Python alignment worker sigue `e4a247a4e13c69210036ab1ef0b1ba68cff28912d19abd68c9bbe484bf080fec`; ASR worker sigue intacto.

El host acredita **MMS real con lineage protegido sobre parent inventado** y cancelación antes de native. Alineación de un ASR real, cancelación durante forward nativo, rederivación del master enriquecido, aceptación humana/prepared-command e integración GUI siguen pendientes y no se simulan.

## Portado lector Windows tras diagnóstico CRT de E3

E3 reprodujo bloqueo de import NumPy cuando otro hilo mantenía stdin.buffer.readline esperando bytes y verificó PeekNamedPipe→os.read de bytes disponibles con mismas versiones/políticas. Por instrucción explícita del principal se portó ese helper dentro de alignment_worker.py (sin módulo externo fuera del SHA), serve incremental y shutdown con un único join. No se cambiaron ASR, dependencias, modelo, algoritmo, alignment.rs ni GUI humana6868.

Se añadieron pruebas reales de envelopes parciales, múltiples líneas, EOF incompleto y sobrelímite1MiB a test_e5_alignment.py. `.local/e5-mms-real-03`: **10 PASS /21.937s**, incluye inferencia acústica real de las23 palabras del SAPI inventado, cache/resume, invalidación/cancel pre-native y protocolos. Pico intérprete propio **2,718,797,824 bytes WS (~2.532GiB)**. Los resultados previos y sus hashes permanecen originales; nuevos cache keys incorporan el nuevo worker SHA.

- Nuevo worker alignment congelado: `7c59aa50d81727e3390f9bbfa4d3796f0464f68d15333406ea7e0c554a32c1ce`.
- Nuevo test script SHA: `4d3dc6854be00cae0c6f4ca9868a4b7773af10aa418b0e2e0516fa7121797120`.
- real03 report SHA: `5e8b2638266487822fe879f77f8e2569b66c9e57d7e86dd4efdb86a106671909`.
- aligned words SHA: `4c958bd476c8b3145583e94b5ac79f6d3a10fb1b8704a8dabe93095ead8f6fe1`.

Este incremento acredita Python/MMS real con lector nuevo; no implica haber repetido el ejemplo Rust host previo con este SHA ni inferencia ASR bloqueada. Root/E3 pueden crear parent fixture nuevo con sus workers actuales para la siguiente cadena validada. No se reejecutó ningún binario bloqueado ni hubo cambios de seguridad.
