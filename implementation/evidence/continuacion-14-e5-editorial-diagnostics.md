# E5: diagnósticos causales del backend editorial

2026-09-30. Modelo Qwen2.5-1.5B-Instruct y runtime editorial27pins previamente anunciados/verificados. Todos los procesos de este informe fueron propios, dentro de JobObject con handshake previo a imports, cierre/kill limitado aesejob y deadline; no se modificaron worker, entorno, modelos, inputs ni V1. `accepted:true` en un supervisor diagnóstico significa que terminó el guion, **no aceptación editorial ni incorporación al proyecto**.

## Configuración de host04, revisión de lectura

Root preservó host04 Failed414,844 s, E_LIMIT1024tokens sinEOS, salida repetitiva noJSON. La revisión estática no encontró EOS/cache desactivados: generation_config usa EOS[151645,151643], el worker comprueba esa misma lista; use_cache=True en configuración de modelo y por defecto en GenerationConfig. Transformers4.48.3 crea DynamicCache y pasa num_logits_to_keep=1 aQwen2, evitando logits para toda la matriz del prompt. Greedy0 conserva temperature/top_p/top_k del archivo pero sus warnings dicen que son ignorados, sin convertirlos en causa de repetición. La ausencia inicial de telemetría de tokens impide derivar avance de generación a partir de CPU estable.

## Comparación real, cuatro forwards sin generación

Script `tests/scripts/probe_editorial_qwen_quantization.py`, SHA216757470e1076a23cf84006bf138c6311cf1c9c7dc599ce288e338e4d7e957c. Mismo chat inventado completo26tokens (máximo32), CPU2, cap8GiB, plazo180 s. Resultado PASS65,875 s, exit0/signaled, pico7.544.299.520 B. Recursos modeloSHA pre/post iguales. Cada forward devolvió151936logits finitos y DynamicCache28capas/26tokens.

| Perfil | Forward s | RMSE vsBF16 | Coseno vsBF16 | Primer token |
|---|---:|---:|---:|---|
| BF16 original |5,172|0|≈1|`{"` (ID4913), logit24,5|
| 196Linear cuerpo INT8, cabeza/embedding/norm FP32 |0,422|4,484917|0,310610|espacio (ID220)|
| Mismo cuerpo, cabeza INT8 per_tensor |0,406|4,523912|0,310673|espacio|
| Mismo cuerpo, cabeza INT8 per_channel |0,390|4,521173|0,310791|espacio|

La desviación se observa antes de cuantizar la cabeza. Cambiar sólo la cabeza a per_channel no la corrige. El perfil del cuerpo también cambia dtype de embedding/norm aFP32 respecto aBF16; sin baseline íntegra FP32 no se atribuye toda la diferencia exclusivamente a una operación concreta de cuantización. La microprueba con pesos aleatorios pequeña anterior validaba API/arquitectura y memoria, no esta fidelidad de los pesos entrenados. Ninguna forward constituye generación editorial.

Evidencia durable: [supervisor](e5-editorial-diag-quantization-supervisor.json), [fases y métricas](e5-editorial-diag-quantization-stdout.jsonl), [stderr](e5-editorial-diag-quantization-stderr.log). Algunos tokenlabels del stdout diagnóstico antiguo usan CP1252 de Windows; los campos numéricos/IDs son ASCII. No se presentó esa salida como propuesta.

## FP32 directo y primer stream: fallos conservados

| Intento | Resultado/fase | Tiempo s | Pico Job B | Forward/generación |
|---|---|---:|---:|---|
| fp32-01 | Error de harness CP1252 al leer JOBJSON UTF8, antesimports |3,015|13.787.136|0/0|
| fp32-02, low_cpu_mem_usage=True | AV0xc0000005 en load_direct_fp32, exit3221225477 |10,953|8.610.304.000|0/0|
| stream-fp32-01, ctor include_buffers=False | AV0xc0000005 durante carga; último contador240/338 |12,953|8.605.577.216|0/0|

Primer error fue del guion: Path.read_text sin encoding leyó CP1252 y aumentó el tamaño al volver aUTF8. Root verificó independientemente topics-agent-request.md496B SHA03ede24b0d3d5c3710cc39ca2cffe05072efd38b83b6a37b75cee887d5527174, intacto. Se corrigieron las cuatro lecturas de JSON/lock/previo aUTF8 y se repitió por cambio causal de harness, sin omitir comprobaciones. fp32-02 verificó los5inputs, workerf443/lockf168/modelManifest0661, y midió contexto real host04:1905tokens/3996B UTF8, digest48e9f24d5ed6081dafff00c36f68e29b219c3295d02be1fdfccb82ea8a062448. No llegó acargar modelo ni a forwards. No se repitió el mismo perfil tras AV/cap.

Stream01 leyó header verificado estricto (nombres/shapes/BF16/offsets/cobertura), sin mmap completo, embeddingprimero; BF16 buffer→FP32 por tensor→set_module_tensor_to_device CPU; borró referencias locales. A240tensores el payloadCPU calculado era4.675.747.840 B; fallo posterior antes de contador280 (payload279=5.293.950.976 B), por lo que la hipótesis de mmap como causa única fue insuficiente. AV/cap no prueba por sí solo la instrucción nativa que falló.

Logs conservados bajo prefijos `e5-editorial-diag-harness-cp1252`, `e5-editorial-diag-fp32-direct` y `e5-editorial-diag-stream-false` (supervisor.json/stdout.jsonl/stderr.log). Scripts respectivos permanecen separados; fuentes/hash en cada supervisor.

## Diagnóstico parcial120 y constructor meta

Parcial sólo120tensores, sinforward/generación, cap8GiB/deadline180/CPU2: PASS12,656 s, pico5.438.054.400 B, PID11988exit0/signaled; SHAmodelo pre/post iguales. Script `probe_editorial_qwen_stream_partial.py` SHAcc97d6b96ccfb153d4537274ce4375f1da3efd8a61334f3cabcb91af606770fb. Sumó bytes de parámetros CPU reales y midió psutil.private (memoria comprometida) aparte de RSS.

| Fase | Private B | RSS B | PayloadCPU B |
|---|---:|---:|---:|
| Antes constructor False |324.964.352|280.006.656|0|
| Después constructor False/gc |2.280.853.504|286.896.128|0|
| Embedding BF16buffer+conversiónFP32 |3.689.914.368|1.687.433.216|0 antesasignación|
| Embedding asignado y buffer borrado/gc |3.222.253.568|1.220.685.824|933.494.784|
| 120tensores, borradas referencias/gc |5.408.350.208|3.093.114.880|2.803.834.880|
| Modelo borrado/gc |4.400.152.576|2.266.615.808|0|

gc no redujo private en esos puntos; assignment verificó compartir storage con el FP32 suministrado (booleano, sin registrar direcciones). La construcción dejó≈1.956GB adicionales comprometidos con0bytes de parámetros CPU. Accelerate1.3 big_modeling.py116 activa torch.device(meta) directamente para include_buffers=True; False usa register_parameter para convertir CPU→meta y nn.Linear crea torch.empty antes del registro. Ese efecto de constructor/allocador se midió, aunque no se identificó el detalle interno de reserva/purga del allocador nativo.

Root autorizó constructor-only True, cap1GiB/deadline60/CPU2, sinpesos/forward: PASS5,25 s, pico333.275.136 B, PID1248exit0/signaled. Private327.331.840→331.001.856 B (incremento3.670.016 B). Todos los parámetros meta; único buffer observado `model.rotary_emb.inv_freq` se reconstruyó usando Qwen2RotaryEmbedding(config,device="cpu") fuera del contexto:64valores FP32/256B, finitos, `original_inv_freq` misma referencia. No se improvisó otro buffer. Script `probe_editorial_qwen_meta_constructor.py` SHA8dc443b4d316da68b6b262298f633c7b56f8f9fbc34623442ccac2cbe2cae01b.

Logs durables prefijos `e5-editorial-diag-partial-120` y `e5-editorial-diag-constructor-true`.

## Stream con constructor True, carga completa observada

Cambio causal autorizado: nuevo script `probe_editorial_qwen_stream_fp32_meta.py` SHA108b6a1fd0e3c6cb8006a3778985aae8076ec2489c08d55260fd1195bd0b7190; sin editar worker/env. Cargó338/338tensores en2,703 s. Verificó1.543.714.304 parámetros (6.174.857.216 B CPU FP32), ningúnmeta, buffersCPU/FP32, aliashead/embedding y EOS[151645,151643]. Private7.704.207.360 B; picoJob7.711.064.064 B (<8GiB).

El guion terminó11,25 s con exit1/signaled por UnicodeDecodeError al leer stdoutlegacy anterior comoUTF8 (byteBFCP1252), antes de forwards. **Se acreditó la carga completa y sus invariantes, no inferencia ni hashes postfinales.** Logs `e5-editorial-diag-stream-meta-01-*` conservados. No fue OOM. El nuevo archivo `probe_editorial_qwen_stream_fp32_meta_utf8.py` selecciona sólo fila BF16ASCII previa y emite JSONASCII seguro; no relaja lectura de inputs/modelo/worker. Root autorizó su ejecución manteniendo cap/plazo/perfil y originales.

Alternativa HF estándar investigada sólo por lectura: un contexto externo torch.device(meta) evita factoriesCPU, mientras safetensors.load_file fija deviceCPU y set_module_tensor_to_device usaCPU explícito. Deben comprobarse buffersCPU, ningúnmeta y picos antes de elegirla para producción; no se ejecutó esa variante ni se cambiaron fuentes productivas aquí.

## Stream FP32 eager: corto válido, contexto real sobrepasa el límite

Intento meta-02, script SHA `4c522ea044df3dce646b657436fc8ba7845cd8cd4e3226782dbcb2b964ecb63b`: cargó los 338 tensores en 2,625 s; parámetros CPU FP32 6.174.857.216 B, private 7.703.416.832 B. El forward corto de 26 tokens terminó en 2,109 s con logits finitos y DynamicCache de 28 capas/26 tokens. Primer token `{"` (ID4913), logit 24,05507469; top10 IDs [4913,73594,515,1,13874,3925,90,6257,39814,11578].

Falló durante el forward del contexto real de 1905 tokens: 41,281 s, exit 3221225477 (0xc0000005), pico Job 8.683.937.792 B, PID30156 signaled. No terminó ese forward ni ejecutó generación corta. Los logs originales se preservan como `e5-editorial-diag-stream-meta-02-*`. La comprobación externa posterior de los nueve archivos del modelo coincidió con sus hashes anteriores, registrada en `e5-editorial-diag-post-failed-probes-model-sha.json`; worker f443 y lock f168 seguían iguales en ese momento. No se repitió eager tras este fallo.

## Variante SDPA CPU: prueba causal completada

Root autorizó un único cambio de atención eager→sdpa manteniendo constructor meta True, reconstrucción del único buffer RoPE CPU, carga FP32 tensor por tensor, límite 8 GiB, 180 s y dos hilos CPU. Script `tests/scripts/probe_editorial_qwen_stream_fp32_sdpa.py` SHA `778fca519aa67b82b3f2d5e3839deec827b23e5183cdc7da7ca4f941bdf612a5`. La comparación añadida sólo observa logits sobre los diez IDs guardados del corto eager; no altera la carga ni la inferencia.

Lectura del código instalado Transformers 4.48.3: Qwen2 declara soporte SDPA, enruta a `scaled_dot_product_attention` cuando output_attentions=False y omite la máscara explícita en este caso sin padding y con DynamicCache. [Código oficial Torch 2.8 de atención](https://github.com/pytorch/pytorch/blob/v2.8.0/aten/src/ATen/native/transformers/attention.cpp) ofrece SDPA CPU con FP32. El registro confirma implementación sdpa y device CPU; no se perfiló el kernel concreto y no se atribuye Flash/CUDA a esta ejecución.

**PASS técnico**, 57,140 s, exit0, PID26576 cerrado/signaled, pico Job **8.348.938.240 B**, inferior a 8.589.934.592 B por 240.996.352 B. Cargó 338/338 tensores en 2,672 s y verificó 1.543.714.304 parámetros/6.174.857.216 B CPU FP32, todos sin meta, buffers CPU y FP32 si flotantes, alias cabeza/embedding y EOS [151645,151643].

| Operación | Tokens | Tiempo s | Resultado observado |
|---|---:|---:|---|
| Forward corto |26|2,125|Logits finitos, DynamicCache 28 capas/26 tokens; mismos top10 IDs que eager; máxima diferencia absoluta sobre los diez IDs guardados 0,00000762939453125|
| Forward del contexto host04 |1905|36,860|Logits finitos, DynamicCache 28 capas/1905 tokens; top1 ID4913 `{"`, logit27,09157562|
| Generación corta, límite16 |8 generados|4,094|Texto `{"tema":"saludo"}` y último token151645/EOS|

La fase finished declara dos forwards y resources_unchanged=true tras comprobar hashes de recursos/inputs al finalizar. Evidencia durable: [supervisor original](e5-editorial-diag-stream-sdpa-01-supervisor.json), [fases originales](e5-editorial-diag-stream-sdpa-01-stdout.jsonl), [stderr original](e5-editorial-diag-stream-sdpa-01-stderr.log). **El campo generation_executed:false del supervisor es heredado y queda conservado, pero contradice la fase short_generate16 del stdout; sí hubo generación corta de ocho tokens.** editorial_accepted:false es correcto: no se generó ni validó la propuesta editorial completa ni se ejecutaron sus dos pasadas.

Los probes, modelo y entorno quedan congelados; no hay más ejecuciones ML de E3. Root recibió el cierre y tomó el worker productivo para integrar el cambio causal y verificarlo por separado. Su fuente puede cambiar después de esta evidencia: no se compara el worker actual contra el hash histórico f443 ni se reejecutan probes que lo exigen. El margen de este ensayo es reducido; otro contexto mayor debe conservar el mismo límite y controles.
