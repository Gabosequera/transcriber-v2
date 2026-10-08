# Continuación 14 — E5 Python local, extracción real y bloqueo de inferencia

Frente delimitado: `workers/python/worker.py`, scripts específicos E5 y esta evidencia. El principal integra `tv2-pipeline`/GUI. El lock instalado por el principal se conservó sin cambios. V1 se consultó solo como fuente de contratos (`medios.py`); no se importó ni ejecutó, y no se modificaron sus archivos ni datos personales.

## Implementación

- Protocolo `tv2-worker/1`, NDJSON puro stdout UTF-8, diagnósticos stderr, IDs correlacionados, tamaño máximo de línea 1 MiB, hello/run/cancel/shutdown. run solo tras hello exitoso; un análisis por proceso en thread, cancelación cooperativa, EOF cancela y salida explícita. Python no escribe en el proyecto: devuelve `AnalysisResult` exactamente con job/project/revision/project_digest/source_sha256/model_digest/master_path/artifacts; Rust prepara/incorpora mediante comandos comunes.
- Hello comprueba **Python 3.12.13 y todas las distribuciones del lock** con versiones exactas; incompatibilidad `runtime_mismatch`. `capabilities.transcribe` distingue implemented/installed/runtime_state=not_loaded y dependency_status=installed_not_verified_for_native_execution; versiones instaladas no acreditan ejecución de DLL nativa. Forced alignment/risas/arousal/heurísticas se anuncian false. No arranca FFmpeg antes de run.
- Parámetros cerrados: idioma explícito, CPU, 1–4 hilos, beam 1–10 y pasos extract/transcribe. No red ni credenciales: se retiran tokens relevantes, offline HF, directorio local verificado y `local_files_only=True`.
- Hash completo del medio y digest canónico del mapa de los **cuatro archivos del modelo**, compatible con `tv2_pipeline::model_digest`, antes y después de análisis. Model digest observado: `3dff5642c8dd933e5ea720bb911c774406ad9b5800b16d319b59ed3bb7ff5643`.
- Extracción FFmpeg real de **cada** stream de audio, mono PCM16/16 kHz, reloj V1: Δ=start_time(audio)−T0, silencio prepend positivo en ms, atrim/asetpts negativo. Tiempo Rust = **705600000 flicks/s**, master/words/utterances V1 expresan segundos. Fuente intacta; temporales exclusivos, cancelación termina/recolecta FFmpeg y limpia su parcial.
- Adaptador faster-whisper real para tiny local CPU int8, word_timestamps; words/utterances/segments y conversación conservan track_id, IDs, probabilidades y tiempos ASR originales en metadata. No se finge forced alignment. Rangos vacíos se descartan con contador, se preservan tiempos ASR originales, tiempos V1 válidos se cuantizan a ms y se limitan a duración fuente. No se crean arrays de risas/arousal vacíos como evidencia de análisis inexistente.
- Checkpoints atómicos `.work/manifests`. Keys **por etapa**: extracción liga full source SHA, stream, Δ/T0/duración, algoritmo y hashes FFmpeg/worker; transcripción liga SHA audio normalizado, stream/reloj, modelo, opciones y runtime; master liga SHA transcripts y binding de solicitud/asset/proyecto. Los recibos se generan para la solicitud actual. Cambiar idioma/modelo/job/revisión no obliga a reextraer audio idéntico ni reasocia observaciones editoriales anteriores.
- Caché de extracción compartida en `work_root/.cache/extract/<digest>.wav+json`, con hash comprobado antes y después de copia atómica a cada job. Corrupción descarta cache/reextrae. Caché de etapa exige schema/stage/input_digest y **exactamente la ruta que después se leerá**; un manifest con hash válido de otro archivo no autoriza leer transcript/master sin verificar. Rutas relativas y escapes/symlinks fuera de job se rechazan.

## Fixtures y comandos

Voz local SAPI sobre **texto inventado**, dos WAV nuevos dentro de V2; no grabaciones personales. `tests/scripts/make_e5_speech.ps1` exige carpeta V2 nueva y conserva cualquier archivo existente. Primer intento con Windows PowerShell 5.1 `-File` fue rechazado por su ExecutionPolicy. No se cambió la política; la ejecución ordinaria con PowerShell 7 funcionó:

```powershell
pwsh -NoProfile -File tests/scripts/make_e5_speech.ps1 -OutputDirectory 'C:\Users\gabri\Todo\transcriber-v2\.local\e5-python-tests-20260930\fixtures'
$e5Ffmpeg=(Get-Command ffmpeg).Source
& $e5Ffmpeg -hide_banner -loglevel error -nostdin -n -i '.local/e5-python-tests-20260930/fixtures/speech-a.wav' -itsoffset 0.25 -i '.local/e5-python-tests-20260930/fixtures/speech-b.wav' -map 0:a:0 -map 1:a:0 -c:a pcm_s16le '.local/e5-python-tests-20260930/fixtures/multistream.mkv'
```

Tamaños: speech-a 305038 bytes; speech-b 319140; multistream 627061. SHA256 multistream: `1562864a6004e595fabc30f18a08b0c129c0ce83800fdd3bed88b2cec09bd5a1`.

## Resultado real de inferencia: bloqueado por entorno

Primera batería completa, antes de correcciones posteriores de cache/hello: `test_e5_python.py` sobre `run-01`, **8 tests intentados: 5 correctos, 2 failures y 1 error**. La extracción real de ambos streams terminó; no se alcanzó decoder ni master. Error de importación `_core`: `DLL load failed while importing _core: Una directiva de Control de aplicaciones bloqueó este archivo.` Los casos dependientes de transcript/cancel decoder no pudieron completarse; no son regresiones confirmadas del algoritmo ASR.

El principal identificó el bloqueo en [e5-code-integrity-block.xml](e5-code-integrity-block.xml): eventos 3077/3033, política `{0283ac0f-fff1-49ae-ada1-8a933130cad6}`, DLLs **PyAV** `av.libs/avformat-63-e8c4f8fc30664f2b7e85dcc683bd4b0d.dll` y `libgcc_s_seh-1-3e3b83c1f8de213bb810010a5622b180.dll`. El texto `_core` no permite atribuirlo a CTranslate2; la atribución se basa en esos eventos. El worker final clasifica ese mensaje como `E_DEPENDENCY_BLOCKED`; esta nueva clasificación está implementada, **no ejecutada contra la DLL bloqueada**.

No hubo reimport/retry del binario/DLL bloqueado tras conocer la causa, ni reubicación/renombrado/desactivación de seguridad. El principal solicita intervención administrativa concreta. Log local del intento: `.local/e5-python-tests-20260930/result-01.json`, SHA256 `50ee777ed8bdab36305c0533f8dccdb024eb2ca89a7d89e11eaf907a471f1b96`.

## Verificación independiente realmente ejecutada

Las revisiones de `--suite independent` sobre roots nuevos run-02/run-03/run-04 dieron 10/10/11 correctos mientras se añadía cobertura. Después de keys por etapa y cache compartida, comando final:

```powershell
$e5Ffmpeg=(Get-Command ffmpeg).Source
$e5Ffprobe=(Get-Command ffprobe).Source
& '.local/e5-venv/Scripts/python.exe' -I tests/scripts/test_e5_python.py --suite independent --source '.local/e5-python-tests-20260930/fixtures/multistream.mkv' --model '.local/models/whisper-tiny' --ffmpeg $e5Ffmpeg --ffprobe $e5Ffprobe --work-root '.local/e5-python-tests-20260930/run-05' --evidence '.local/e5-python-tests-20260930/result-05.json'
```

**12 tests correctos en 5.630 s.** Esta selección no importa faster-whisper/PyAV ni simula inferencia:

- FFmpeg real multistream; +250 ms son exactamente 4000 muestras de silencio a 16 kHz; −250 ms recorta exactamente 4000 muestras y coincide con PCM origen.
- Cache de etapa/reanudación; corrupción de copia job se recupera desde cache compartida verificada, corrupción de ambas fuerza reextracción real.
- Cambio de idioma/modelo/proyecto/revisión/job reutiliza solo extracción por contenido verificado; key ASR cambia y no aparece master reasociado. Los metadatos de modelos cambiados en este caso son una prueba de keys, sin ejecutar ese modelo.
- Cancelación antes de extracción preserva artefacto y fuente. Cancelación **después de crear un FFmpeg real vivo** termina/recolecta el proceso y limpia parcial; solo el instante de cancelación se inyecta mediante wrapper Popen, sin proceso/inferencia simulados ni medio grande.
- Manifest traversal/archivo relacionado equivocado/stage cambiado se rechazan; runtime versión incompatible se rechaza sin importación nativa.
- Subproceso Python real hello/runtime/capabilities, run sin hello, schema/pasos inválidos, SHA source/model incorrectos antes de FFmpeg, EOF cancela sin master exitoso y rutas inválidas.

Evidencia final local: `.local/e5-python-tests-20260930/result-05.json`, SHA256 `e05184dbf90f365ddeb5eb67c52afeb7b23366b14f04333a716fd8de7b4d325f`; fuente intacta según full SHA antes/después. `py_compile` de worker/test final correcto. SHA256 worker verificado: `bf6af05b3c7380f51c1d70658001ef181470809148c205b7115128845b434268`.

## Pendiente real

Transcripción tiny/inferencia decoder, cancelación durante decoder y resume/invalidation de outputs ASR requieren que cambie la causa de bloqueo PyAV. La batería completa está preparada con `--suite all`, **no repetirla sin esa evidencia**. No se declara E5 cerrada por extracción ni metadata/runtime handshake. Forced alignment real, risas/arousal, integración de resultados y reproducción GUI bajo carga siguen pendientes. Las heurísticas e intensidad se implementan en el incremento independiente descrito al final; su ejecución posterior a ASR real permanece pendiente. La integración y protección del proyecto/actor/jobs/tree lease pertenece al frente Rust del principal; se recomendó revalidar source SHA al preparar resultado si pudo cambiar después de terminar el worker.

## Smoke real del supervisor Rust y árbol Windows

Tras integrar la API Rust, se añadió `crates/pipeline/examples/runtime_smoke.rs` y `scripts/test-e5-host.ps1` para ejercitar el host **sin importar las DLL bloqueadas**. La API pública `capabilities` usa el worker real; `OwnedWorker` privado no expone EOF, de modo que el recorrido EOF del ejemplo reutiliza el **mismo archivo `process_tree.rs` de producción** mediante `#[path]` al abrir el worker real con stdin propio.

El recorrido de descendientes usa una fixture NDJSON **solo de transporte/lifecycle**, generada en la carpeta nueva de smoke, que espera hello/run antes de abrir un FFmpeg nativo real. FFmpeg procesa silencio lavfi con `-re`, duración limitada y salida null: no realiza inferencia ni escribe medios. La fixture ignora cancel/shutdown deliberadamente para probar el grace de dos segundos y kill-on-close del host. No se cambian `lib.rs`, `process_tree.rs` ni worker para esta prueba.

Comando ejecutado:

```powershell
pwsh -NoProfile -File scripts/test-e5-host.ps1 -OutputDirectory 'C:\Users\gabri\Todo\transcriber-v2\.local\e5-host-smoke-20260930-02'
```

El script compila con `scripts/cargo.ps1 build --locked -p tv2-pipeline --example runtime_smoke` y ejecuta el ejemplo debug. Build de este árbol: paquete **alpha.11**, build dev 3.00 s, runtime **3.6882638 s**, **PASS**:

- `tv2_pipeline::capabilities` inicia Python real, negocia hello/lock exacto, y cierra sin inferencia. Resultado distingue installed/not_loaded.
- `capabilities` con flag previo de cancelación devuelve **ErrorCode::Cancelled** y cierra su worker propio.
- Worker real con hello y posterior EOF: PID **13916**, exit code **0**, dentro del grace de 5 s.
- `tv2_pipeline::enqueue/run` con fixture de transporte: evento correlacionado del job **job-02e8523f5f1d**, Python PID **27424**, FFmpeg PID **23488**. Se adquieren handles `PROCESS_SYNCHRONIZE` de estos procesos **antes** de cancelar; tras terminar run/drop ambos handles quedan señalados. Esta prueba usa la identidad del mismo objeto del sistema operativo, sin confundir un PID reutilizado con el proceso original.
- El resultado conserva **Cancelled**, y `jobs::discover` encuentra el recibo durable de ese job en estado **cancelled**. La fixture no puede producir un `AnalysisResult` válido; no se afirma éxito de modelo.
- Resultado contiene `accepted_host_smoke=true`, `inference_accepted=false` y `transport_fixture_only=true`.

Primer intento de build (carpeta `...-01`) falló por usar el nombre inexistente de constante Windows `SYNCHRONIZE`. Se corrigió a **PROCESS_SYNCHRONIZE**; no llegó a ejecutar el smoke. El intento `...-02` es el ejecutado. No hubo rechazo de ejecución del ejemplo ni retry de PyAV/suites bloqueadas.

Control posterior `scripts/cargo.ps1 clippy --locked -p tv2-pipeline --example runtime_smoke '--' -D warnings`: **correcto**, 2.82 s. rustfmt del archivo ejemplo correcto.

Artefactos locales en `.local/e5-host-smoke-20260930-02/`: build-debug-example.log, runtime.log, result.json, sha256.txt y fixture transport-only-owned-child.py. SHA256:

- Debug `runtime_smoke.exe`: `91dc0cc8dc18f90f1bfca3480ed28cf2fa5b888f76f7dd64d3e433792494f069`.
- Worker real: `bf6af05b3c7380f51c1d70658001ef181470809148c205b7115128845b434268`.
- Fixture transporte: `391b80a23a699d08ac9faa3e446463e54a8ba844272e447136af8aebac387322`.
- result.json: `45a74ef47e7efdd266d888516513f834a17f04a5592e3da991bc26aed19f391d`.

Esto verifica el supervisor/handshake/EOF/cancelación y kill-on-close de procesos propios de Windows. **No verifica inferencia ni cierra E5**; el bloqueo PyAV y la aceptación de decoder/GUI bajo carga permanecen pendientes.

## Incremento independiente: intensidad y heurísticas V1 sin DLL nativa

Se consultaron solo como texto los archivos V1 `prosodia.py`, `extraer_pausas.py`, `ava.py`, `editorial_master.py` y `editorial_layers.py` (V1 no contiene `anotaciones.py`). No se importó ni ejecutó V1. Toda implementación queda dentro de `workers/python/worker.py`, cubierta por la identidad SHA del supervisor, con stdlib `wave`, `struct`, `math`, `unicodedata` y `SequenceMatcher`; no se altera el lock, decoder ASR ni imports nativos.

Algoritmo versionado `v1-intensity-pauses-ava-conversation/1`:

- Intensidad por palabra 1:1 sobre PCM16 mono 16 kHz normalizado: RMS/peak dBFS, piso local con contexto ±350 ms y promedio de dB anterior/posterior, contraste, z-score poblacional y énfasis sigmoidal con coeficientes V1 0.72/0.18/0.10. Lee bloques acotados de 32768 muestras y comprueba cancelación; no duplica el offset de pista ni T0. Conserva IDs y probabilidades, enriquece seis métricas y señales por intervención. Silencio y bordes mantienen valores finitos.
- Pausas ≥300 ms con cursor de fin máximo (intervenciones anidadas no inventan silencio), contexto léxico/puntuación y z-score de duración. Sin silencio final fabricado.
- Invocaciones Ava/Eva/aba/eba con normalización de acentos, repetición, gap 1.2 s, tope 20 s y contexto de cinco palabras. Una confianza ausente sigue `null`; no se declara una instrucción aceptada ni ediciones humanas.
- Conversación con solapamientos semiabiertos y detección conservadora de captura duplicada entre pistas: ≥0.80 de solapamiento respecto al intervalo menor, similitud ≥0.92 y selección primaria por confianza ASR y longitud. Retiene todas las observaciones e IDs; `clean_utterance_ids` es una proyección y no elimina la fuente.

La derivación se ejecuta obligatoriamente **después de cargar palabras ASR** dentro del recorrido existente `steps=['extract','transcribe']`; no se añade todavía selección de pasos ML a la API Rust. `analysis.completed_steps` distingue `word_intensity` y `deterministic_heuristics`; `analysis.derivation` documenta selección fija y ausencia de decisiones editoriales. `hello.capabilities.word_intensity` y `.heuristics` informan `implemented=true`, `runtime_state='stdlib_ready'`; transcripción sigue `not_loaded`. Esto expresa disponibilidad del código, no aceptación de inferencia ni de GUI. Forced alignment, risas y arousal siguen no disponibles.

Artefactos: `.work/transcripts/aN.json` permanece raw ASR; `.work/editorial/aN.json` es la copia enriquecida con hashes de audio normalizado/transcript/worker y algoritmo. El checkpoint `editorial-aN` exige artefacto exacto, hashes actuales y versión; su key depende de audio, transcript y worker. La key master incluye hashes editoriales y conserva el binding de proyecto/job/revisión. Master compatible `editorial-master/1` contiene `tracks.aN.words`, `.utterances.signals`, `.baselines.intensity` y `conversation`; los datos auxiliares `tracks.aN.intensity` y `.heuristics` describen observaciones. `AnalysisResult.artifacts` incluye el nuevo JSON editorial. No se escriben proyectos: cualquier efecto editor sigue requiriendo importación/prepared command del host.

### Verificación de fórmulas y contrato

Nuevo `tests/scripts/test_e5_editorial.py`, PCM sintético y palabras inventadas. Comando final:

```powershell
& '.local/e5-venv/Scripts/python.exe' -I tests/scripts/test_e5_editorial.py --work-root '.local/e5-editorial-stdlib-03/fixtures' --evidence '.local/e5-editorial-stdlib-03/result.json'
```

**14 tests correctos en 1.322 s.** Valores dorados calculados analíticamente sobre PCM de 0.25/0.5 FS, no generados por V2 para compararse consigo mismo: RMS −12.041/−6.021, intensidad z −1/+1 y énfasis .354/.769. Cubre silencio, vacíos, bordes/ventanas, offsets, multitrack, solapamientos anidados, confianza ausente/ inválida, etiquetas, cancelación y no mutación del transcript de entrada. Cubre cache editorial/hash exacto/corrupción/mapa manifest alterado/cambio de audio o probabilidades; reanudación devuelve los mismos recibos verificados.

La prueba de payload master siembra explícitamente **PCM y transcript inventado** mediante manifests con hashes reales y ejercita el ensamblador `Analysis.run` sin cargar modelo/DLL. El master local `.local/e5-editorial-stdlib-03/fixtures/invented.editorial.master.json` permite revisar el contrato de importación; **no es resultado de ASR ni aceptación real**. El test valida todas las rutas y SHA del `AnalysisResult`, el enriquecimiento y la reanudación. Este fixture no se importa automáticamente en un proyecto.

Las ejecuciones anteriores `e5-editorial-stdlib-01` (12 casos) y `...-02` (14) se conservan; `...-03` corrige el binding del fixture de modelo antes de construir Analysis y es la evidencia final. Report SHA256 `c55ca2ff0ac287bb14e0026e5598d4f9a019523ce18d4f1aab439cecc229afa2`; master inventado SHA256 `991e74368284f0652df795538da1bfe1dba9f58be7e14e9b6e88e84038c36293`.

### Revalidación tras cambiar identidad worker

SHA256 actual `worker.py`: **`2d9952a504e8dfd699b2bc970ebd816bb389385552b3163ecb20420ef6f9019e`**. Los hashes anteriores y sus resultados permanecen históricos. Se repitieron las **12 pruebas independientes** de extracción real/lifecycle/hash/runtime/EOF/cache descritas arriba con el worker nuevo, sustituyendo root/evidence por `run-06` / `result-06.json`: **12 correctas en 5.798 s**, fuente intacta; report SHA256 `584d8b851b3b09b84ed6084f1003e0dc5d70844219b5886e73efa21d0c2d3d73`. No se reintentó importar PyAV/faster-whisper.

El host Rust valida identidad worker: solicitudes/fixtures encoladas con SHA anterior requieren una solicitud nueva; no deben reasociarse resultados viejos. El smoke Rust histórico `...-02` documenta el árbol con worker anterior; no se presenta como repetido con este hash. Inferencia real y GUI bajo carga siguen pendientes de cambio demostrado en la causa de bloqueo PyAV.

## Corrección del fixture e interoperabilidad Rust realmente ejercitada

La revisión del principal encontró que el fixture editorial `...-03` no incluía `media.fingerprint.size`: los tests Python verificaban el ensamblador y métricas, pero **no acreditaban importabilidad por Rust**. `V1Master::parse_fingerprint` exige ese campo `u64`. Producción recibe el Asset Rust completo y ya copia el fingerprint; se corrigió exclusivamente el fixture y sus asserts, **sin cambiar `worker.py` ni su SHA**. El master `...-03` y su resultado quedan como evidencia histórica de la limitación, no como fixture importable validado.

`analysis_fixture` ahora construye Asset completo: kind/path, probe PCM16 mono/16 kHz/duración/un stream, fingerprint `size=32044`, `mtime_ns`, hash muestreado calculado con tres bloques y tamaño, e inventario SHA256 calculado desde el inventario sintético. Se conservan explícitamente palabras inventadas. Test14 exige tamaño entero no negativo dentro de `u64`, coherencia con tamaño real y fingerprint Asset, hashes de 64 caracteres y exporta tanto `invented.asset.json` como el recibo `invented.analysis-result.json` para inspección.

Comandos ejecutados (sin suites bloqueadas, sin GUI ni importación PyAV):

```powershell
& '.local/e5-venv/Scripts/python.exe' -I tests/scripts/test_e5_editorial.py --work-root '.local/e5-editorial-stdlib-04/fixtures' --evidence '.local/e5-editorial-stdlib-04/result.json'
pwsh -NoProfile -File scripts/cargo.ps1 build --locked -p tv2-pipeline --example editorial_payload
& 'target/debug/examples/editorial_payload.exe' '.local/e5-editorial-stdlib-04/fixtures/invented.editorial.master.json' '.local/e5-editorial-stdlib-04/fixtures/invented.asset.json' '.local/e5-editorial-stdlib-04/rust-interop.json'
pwsh -NoProfile -File scripts/cargo.ps1 clippy --locked -p tv2-pipeline --example editorial_payload '--' -D warnings
```

**Python 14/14 correctos en 1.429 s**, build debug alpha.11 correcto 3.37 s. **Primera ejecución del nuevo ejemplo correcta, exit0** (no reintento de ejecutable bloqueado). `crates/pipeline/examples/editorial_payload.rs` valida el payload exacto producido por Python contra código Rust real:

- `V1Master::parse`, fingerprint con mismo tamaño del PCM, misma identidad Asset y duración. Regresión explícita elimina size y luego lo hace negativo: ambos payloads se rechazan.
- `V1Master::projections`: dos capas (palabra e intervención) bloqueadas, propuestas, sin editar; IDs y `asr_prob=.91`/`rms_dbfs=-12.041` retenidos en evidencia original.
- `MasterEvidence::validate` sobre un proyecto fixture con el Asset del mismo análisis.
- `ProjectSession::prepare_command` con actor externo y Batch AttachMaster/ReplaceLayer no modifica ni siquiera el proyecto en memoria. `commit_prepared` valida y aplica una revisión; undo humano por vía de comandos deja revision2 y restaura ausencia de master/capas, conservando Asset. Todo ocurre en memoria; **no se escribe ni abre un proyecto**.
- Documento del master permanece idéntico en bytes y como evidencia. El reporte distingue `fixture_only=true`, `inference_accepted=false`, `gui_exercised=false`, `project_written=false`.

rustfmt del ejemplo y clippy `-D warnings` correctos (0.97 s). Sin cambios en lib/tests/UI ni suites 4551. SHA256 locales:

- Worker, sin cambio: `2d9952a504e8dfd699b2bc970ebd816bb389385552b3163ecb20420ef6f9019e`; siguen vigentes las 12 independientes `run-06`.
- `.local/e5-editorial-stdlib-04/result.json`: `120ef969c626a46a408377a96e752dcd7567605d0887c26ef0c117e15909e0b3`.
- Master inventado nuevo: `aa31e59ece174c116bb37c7bda7d6e6ad6954f6dcd19f071aaf7f6e12cc49dc5`.
- Report Rust `rust-interop.json`: `ada648f01c1b5f977567f8271fa9ec7ff3273e5586e71cbff61ba4ebcfa91ac5`.
- Debug example `editorial_payload.exe`: `8d787ddcc5ade58f63396d13dfa554be726689639311641977b49629dbb34163`.

Este recorrido acredita interoperabilidad de la **fixture inventada** de E5 con parser/proyecciones/evidencia/comandos Rust; no acredita inferencia, carga del decoder o aceptación GUI.
