# E5 risas — worker aislado e inferencia real sobre negativos

2026-09-30. V1 se consultó solo como texto; no se importó ni ejecutó. Los únicos audios procesados son PCM sintético de silencio y SAPI de texto inventado creados dentro de V2. ASR/PyAV permanecen bloqueados y no se reintentaron. No hay integración GUI, modificación de proyecto ni decisión editorial automática.

## Recursos y licencias fijadas

Antes de descargar se comunicaron al principal ~1.6 GB de tráfico, ~5 GB para entorno/cache, destinos `.local/models/laughter-omine` y `.local/e5-laughter-venv`, con ~126 GiB libres. Principal anunció recursos y autorizó preparación. Downloader explícito `scripts/download_laughter_model.py` no se invoca desde el worker; verifica tamaños/SHA completos antes de publicar con creación exclusiva y conserva recursos existentes.

- Código oficial [omine-me/LaughterSegmentation](https://github.com/omine-me/LaughterSegmentation/tree/a525292d26f744e14624e3a2f1fb5e3c7858d7b3), commit `a525292d26f744e14624e3a2f1fb5e3c7858d7b3`: MIT, Copyright (c) 2024 Taisei Omine. Texto completo en `workers/python/LAUGHTER-UPSTREAM-LICENSE.txt`.
- [Pesos oficiales](https://huggingface.co/omine-me/LaughterSegmentation/tree/cb10e3920766372f06bbd9657724f24dc39fa3e4), commit `cb10e3920766372f06bbd9657724f24dc39fa3e4`: `model.safetensors`, **1,261,816,628 bytes**, SHA256 `449b14f73c70db26da9b4a59ee77d9a9b29fbcaceb083dd7ea27cdfaa68442a0`. Licencia **research-only** según [README oficial fijado](https://github.com/omine-me/LaughterSegmentation/blob/a525292d26f744e14624e3a2f1fb5e3c7858d7b3/README.md#license), separada de la licencia MIT del código. No se redistribuyen pesos.
- Solo [configuración del backbone](https://huggingface.co/jonatasgrosman/wav2vec2-large-xlsr-53-english/blob/569a6236e92bd5f7652a0420bfe9bb94c5664080/config.json), commit `569a6236e92bd5f7652a0420bfe9bb94c5664080`: **1,531 bytes**, SHA256 `ffcc5c417fe11433447975d5053b2279fbeafd6bca03dd2753082e72ad2d36b7`, Apache-2.0. No pesos redundantes del backbone.
- `model-manifest.json` local SHA256 `ee1a588bff5264d7941947cd7387b0573605739c723d7b045d713937e43a7c02`, incluye URLs públicas fijadas, tamaños, hashes, revisiones y licencias. Descarga modelo/config terminó en **53.406s**.

Runtime propio CPython **3.11.15**, Torch **2.1.2+cpu**, Transformers **4.36.1**, safetensors **0.4.1**, tokenizers **0.15.0**, NumPy **1.26.3**, huggingface-hub **0.19.4**, y todas las transitivas exactas en `requirements-laughter.lock`. Python ≤3.11 y pins centrales siguen upstream; las transitivas se fijaron al resolver este entorno. Lock SHA256 `35326ce735a9ec5cfc654767b2efb68b84841b3ce5042872d358ebc51886bd38`. Python gestionado y cache aislados en `.local/laughter-python` y `.local/laughter-uv-cache`; no se modificaron otros entornos.

## Implementación y contrato

`workers/python/laughter_worker.py` usa `tv2-laughter/1`, stdout NDJSON, métodos hello/run/cancel/shutdown y un job activo. Hello verifica versión exacta Python/todas las distribuciones por metadata; anuncia implemented=true/runtime_state=not_loaded, research-only, CPU2 y ausencia de integración GUI. No importa Torch/NumPy hasta run; no descarga recursos ni accede a credenciales. Entorno offline y tokens conocidos eliminados al iniciar.

Se leyó `train/model.py` upstream pero **nunca se importó/ejecutó**: su forward contiene una llamada que podría terminar procesos ajenos. V2 define una clase mínima de inferencia con `audio_model = Wav2Vec2ForAudioFrameClassification(config local)`, head num_labels=1/problem_type single_label_classification y forward logits.squeeze(-1). Se cargan safetensors locales con **strict=True**, eval e inference_mode. La carga estricta y el forward acústico reales pasaron. No hay esa función ni entrenamiento en V2.

Request cerrado con job_id/project_id/revision/project_digest/asset_id/track_id/audio_index/audio_offset_ticks; SHA completo de fuente como lineage aportado por host, normalized_audio_path+SHA, source_duration_ticks/timebase=flicks/705600000, model/config/manifest paths absolutos y hashes. Parámetros cerrados: CPU2, 7s de ventana/2s solape, batch1..2, threshold0.01..0.99, min_dur0.02..10s, merge_gap0..2s y boost booleano. El host debe validar fuente original: este worker verifica **PCM normalizado**, modelo, config y manifest antes/después; worker/lock/runtime también se revalidan antes del resultado. No dispone de source_path original, y provenance lo declara.

Preprocesamiento stdlib wave/struct: mono PCM16/16k, silencio RMS270ms/-35dBFS/paso1ms, quantización y boost×5 con fades150ms según V1; normalización por pico global. Se escanea en bloques y las ventanas se leen del WAV por posición; no se carga todo el audio. Inferencia CPU2 en ventanas7s/hop5, sigmoid349frames, max-pool global de los solapes; umbral/merge estricto gap<0.2s/min_dur0.2 y confidencias máxima/media compatibles V1. Tiempos se desplazan por audio_offset_ticks y se limitan a sourceTicks. No se convierte voz inventada en risa ficticia.

Resultado único `laughter/events.json`, esquema tv2-laughter-events/1, bindings íntegros, eventos V1 (`t_ini,t_fin,tipo=laughter,conf,mean_conf,max_conf,dur`) más event_id/track_id y metadata ejecución/modelo/runtime/lineage. Receipt ofrece mapa SHA exacto del único artefacto. Checkpoint `.work/manifests/laughter.json` exige esquema/etapa/input_digest/mapa exacto/hash del archivo fijo. Key depende de todos los inputs/modelo/config/manifest/runtime/worker/lock/algoritmo, no de revisión sola; cache corrupta o entrada distinta no produce falso hit. Cancelación cooperativa entre bloques/verificaciones/ventanas; el forward nativo requiere deadline/kill del futuro supervisor host si no devuelve a tiempo.

E3 reprodujo un bloqueo CRT de Windows al importar NumPy mientras otro hilo espera stdin.buffer.readline. Se copió dentro del mismo worker el helper probado PeekNamedPipe→os.read **solo de bytes disponibles**, espera20ms, límite1MiB, EOF parcial y errores109/232. No se cambió versión ni política de seguridad. Tests de fragmentos, líneas múltiples, sobrelímite e incompleto EOF pasan; inferencia se ejecutó mientras stdin seguía disponible para cancelación.

## Pruebas y medición

`tests/scripts/test_e5_laughter.py`: unidades PCM/fades/pooling/eventos usan probabilidades inventadas **solo para probar algoritmos**, sin fingir inferencia. Protocolos hello/run/cancel/EOF usan procesos Python reales. Cancel durante verificación evita native. Cache unit comprueba bytes/schema/stage/key/artifacts exactos, incluyendo mapa sibling y SHA corrupto. Real suite ejecuta pesos oficiales sobre silencio2s y SAPI conocido9.5310625s, luego resume verificado e invalidación SHA sin otro forward.

Primera real `.local/e5-laughter-real-01`: **12 PASS /51.063s**, worker anterior `c7301ed06fa4c29190dba7ba65417de24718ad5ce8018692a9bf9a6d7293d303`. Se conserva resultado original SHA `3f7a6006487c7e749d2dabe169bbd63a31660bc303065f9df8f4f6eb8d973431`. Pico hijo real **2,768,388,096 bytes WS**, peak pagefile3,954,823,168. Tras esa prueba se corrigieron clamp absoluto de offsets, revalidación runtime y shutdown sin join duplicado; esos cambios justifican repetición con hash final.

```powershell
.local/e5-laughter-venv/Scripts/python.exe -I tests/scripts/test_e5_laughter.py --suite real --work-root .local/e5-laughter-real-02 --evidence .local/e5-laughter-real-02/result.json
```

Final real02 **12 PASS /35.704s**. Ventana nativa coordinada después de PERF E2; GUI humana6868 quedó intacta. No se usa esta prueba como benchmark aislado de máquina, pues puede haber editor/audio activos.

| Fixture real | Ventanas | Eventos | Probabilidad máxima frame | Tiempo job incl verificación/carga |
|---|---:|---:|---:|---:|
| Silencio PCM2s | 1 | 0 | 0.012364094145596027 | 12.891s |
| Voz SAPI inventada9.5310625s | 2 | 0 | 0.043197620660066605 | 17.578s |

Memoria medida con Toolhelp32+GetProcessMemoryInfo de **descendientes propios** y distingue launcher del intérprete. Hijo28928 pico WS2,750,820,352 bytes; hijo29272 pico **2,751,021,056 bytes (~2.562GiB)**, peak pagefile **3,937,808,384 bytes (~3.668GiB)**. Launchers ~5MiB no se presentan como memoria del modelo.

Hashes finales:

- worker: `b67519fd3d3c243dc76356d24a75aabaa78e7c162e13b7be4617d664c65358dd`.
- tests: `b51b4f02d88b396479193fbfda736bec79833ccd7f4ff823d6953221d8857967`.
- real02 report: `59b60b8bf6f953e0d62e5fff7cb1449a161a6c88ab2360c24e64bec8dd04b6af`.
- real02 silencio JSON: `4725be081016f609fd7015c03fb576ddba050ec517acf924fa72a48061cc0cad`.
- real02 SAPI JSON: `e8a576ef471badb2ffbb15ae4c689c3de2a35288331ae5536f4753ed3e599da1`.

Conclusión delimitada: **carga/inferencia del modelo real sobre dos negativos, cancel pre-native, protocolo, resume/invalidation y preprocesamiento pasan**. No hay fixture de risa positiva, validación de sensibilidad, cancelación dentro del forward, supervisor Rust de risas, incorporación de capas/prepared-command ni aceptación humana de risas. No se reclaman. Archivos de risas congelados con estos hashes; ASR/alignment/Rust/GUI no se cambiaron durante este incremento.

## Incremento host Rust en preparación

Por encargo posterior del principal se añadieron `crates/pipeline/src/laughter.rs`, `crates/pipeline/examples/laughter_host.rs` y `scripts/test-e5-laughter-host.ps1`. Root habilita pub mod y finalizador/generación por su cuenta; no se editaron lib.rs/alignment.rs/UI manualmente. Cargo fmt inicial se lanzó sobre package pipeline antes de acotarlo; después se usa rustfmt únicamente en archivos propios.

API pública: LaughterRuntime/Parameters/Payload/Result (Clone+Serialize+Deserialize, deny_unknown_fields), PROTOCOL/OUTPUT, enqueue/run/validate_result. Payload expone parentASR/parent_result/audio_index/runtime.work_root; result laughter_path/artifacts, compatible con el finalizador del principal. Parent debe estar Succeeded con recibo/hash de payload válidos; se revalidan fuente completa, modelo ASR/worker, todos sus artefactos y audio exacto `.work/audio/aN.wav`, nunca sibling arbitrario. Modelo/config/manifest/worker/lock de risas antes/después, runtime exacto3.11.15 y distribuciones de lock en handshake y metadata. Captura hash del record actual y rechaza alteración de payload sin actualizar digest. Manifests≤1MiB, fuente≤24h y rutas absolutas.

Importante: el extractor ASR ya normaliza `.work/audio/aN.wav` con adelay/atrim hacia el reloj canónico T0. Este host liga índice/track y envía **audio_offset_ticks=0**, conservando silencios/desfases ya aplicados. Sumar stream.start_time−T0 otra vez sería un defecto; el contrato standalone soporta offsets explícitos para futuros PCM recortados, pero no se reusan aquí.

El host reutiliza OwnedWorker real de root (Windows JobObject antes de reanudar proceso suspendido, escritor cancelable/deadlines/cierre). Artefacto único fijo con SHA; JSON tipado de binding/eventos, duraciones/orden/IDs/probabilidad/threshold, metadata de frames349/ventanas/modelo/license/runtime/lineage concordante. Ningún efecto de proyecto.

Ejemplo admite fixture parent nuevo o `-ExistingParentFixture` para **el mismo parent-asr.json exacto** de la cadena E3 MMS/arousal. Restringe audio a SAPI conocido inventado. Produce parent-record.json/laughter-record.json/laughter-result.json/host-result.json y prueba cancel pre-native y adversariales rehash (binding/manifest/offset/prob/range/ID); los eventos adversariales son datos de prueba, nunca positivos reportados como inferencia.

Build actual de ejemplo pasó (9.72s tras cambios de otros módulos). Clippy anterior pasó; Clippy actual quedó pendiente únicamente por unnecessary_unwrap/manual_is_multiple_of en arousal.rs de otro agente, avisado a su dueño. **La ejecución Rust real de risas todavía no se ha realizado en este punto**; espera cadena común para no rebind. La aceptación Python real02 sigue vigente y no se sustituye por esta preparación.

## Host Rust real sobre la cadena común — aceptación posterior

E3 terminó MMS→arousal sobre nuevo parent ASR inventado y abrió ventana nativa. Se ejecutó primera instancia del ejemplo Rust:

```powershell
pwsh -NoProfile -File scripts/test-e5-laughter-host.ps1 -OutputDirectory 'C:\Users\gabri\Todo\transcriber-v2\.local\e5-laughter-host-01' -ExistingParentFixture 'C:\Users\gabri\Todo\transcriber-v2\.local\e5-arousal-host-01\parent-asr.json'
```

**PASS exit0**, build3.10s, run risas19.9651124s incl lectura/verificación/carga. Parent **job-57fe5b97cf1c** se conserva byte-semánticamente idéntico al de E3; proyecto **proj-5789225831cf**, revisión1, digest `fd3aa86252b56c5d2ea37ab05ee0e747d267fdfdc39976e108a58fb0079439e8`, asset `asset-76f1df4dc491`. SAPI conocido9.5310625s, 2 ventanas, **0 eventos**, pmax0.043197620660066605. No se ejecuta ASR/tiny; sus bytes y recibo pertenecen al fixture declarado, mientras los modelos MMS/arousal/risas sí tienen inferencia real separada.

Job risas **job-1cd7f64c56b0** durable Succeeded; job **job-c441cc1b00e9** durable Cancelled en verify-inputs antes de native. Casos adversos: índice ausente/CPU4/revisión vieja/sibling/unknown-fields/payload threshold con digest antiguo; output alterado y rehashed para binding/manifest/offset/prob/rango/eventID se rechaza. Los artefactos se restauran a sus bytes originales y vuelven a validar; los eventos sintéticos adversariales no se muestran como positivos acústicos. Root OwnedWorker controla descendientes; no hay modificación de proyecto ni incorporación automática.

Clippy final propio **PASS1.28s /-D warnings** después de que E3 resolviera sus dos warnings; rustfmt solo archivos propios correcto. Hashes congelados:

- laughter.rs `09d28b588cc8e3c68816e4d8a71a2f355c690ad25543a095dbb54b51cdb28ea0`.
- laughter_host.rs `fa9a31b780de8644b8aad6e5d7592f90a99eb1828c991373e05a8c9c1cf72228`.
- debug laughter_host.exe `57f6a7f2ce32f6270b32ca392f2beda8361d0859c9e1e124e8feaa42c02732c2`.
- host-result.json `8961b0194783060883eec4e0dc85bb092a54a316c0ccc5954ab8fb16533a86a1`.
- laughter-record.json `2a2ba31a52404e6fd4d531ddaa959dd11f2d02804824281c38f1be5b16baa00e`.
- laughter-result.json `062a2f9e6e222a8902af78a44d6e9885746fcb9bb98efa3a9921dafd17484234`.
- Artefacto laughter/events.json `e593ae2b2372cf9926207a43dc334f696fdfd87b31c1d25005e9a5f88473d0f6`.

Los JSON durables para generación de root están en `.local/e5-laughter-host-01/run/{laughter-record,laughter-result}.json`, con parent exacto `.local/e5-arousal-host-01/parent-asr.json`. Stage no prueba positivo/sensibilidad ni cancel dentro forward ni GUI. Se conserva la preparación previa como historial; esta aceptación posterior cierra host real negativo con mismos bindings de la cadena.

## Revisión independiente de finalización

Por pedido del principal se revisaron generación/finalizer solo lectura. Repros propios preservados:

- `.local/e5-generation-review-01/report.json`: fingerprint.inventarioSHA real `c95d12f7…bec7` difiere de sourcefullSHA `90f6dfc2…397a`; assembler confundía ambos y metadata de MMS real rechazaba Stage source lineage mismatch. E2 corrige la fuente completa según lineage real.
- Mismo report: hello JSON válido sin newline en EOF era aceptado. Root corrigió envelope estricto; `.local/e5-generation-review-02/report.json` conserva after como E_PROTOCOL Incomplete NDJSON envelope at EOF.
- review02: módulo fixture propio fuente VALUE='approved' SHA `bd57a219e7ddff132f6fed6e70ddb7c30a7f89203f2ed68151461e10813f2ec7`, pero pycache mtime/size válido daba VALUE='unhashed' mediante SourceFileLoader, sin cambiar SHA fuente. Solo código benigno propio; ningún archivo de producto se alteró. Root reemplazó loader por compile/exec de bytes verificados e inyección de derivación verificada; comunicó11 pruebasPASS incluyendo mismo ataque de cache. No se afirma aceptar código importado no verificado ni se hizo bypass de seguridad.
- `.local/e5-generation-review-04/report.json` confirma independientemente el loader corregido sobre **la misma fixture pycache anterior**: devuelve VALUE='approved' y SHA fuente no cambia. Root informó aceptación del host compuesto17.258s sobre mismo parent+MMS/arousal/risas, incluyendo prepare/commit común/save-reopen/undo-redo y rechazo de texto rehashed; esa aceptación pertenece a root, no a este ejemplo de risas.
- `.local/e5-generation-review-03/report.json`: PCM real extraído a0 de la fixture multistream dura9.531s, sourceClock real10.222s. Assembler con transcript inventado conocido rechaza Canonical PCM must cover the source clock exactly. El extractor -t/adelay/atrim normaliza **origen** pero no apad trailing. Se comunicó a root/E2 para elegir compatibilidad de PCM corto≤source o incrementar extracción; no se alteró código de esos dueños ni se volvió a extraer/inferir. El host común de una sola pista9.5310625s no cubre este caso multistream.
