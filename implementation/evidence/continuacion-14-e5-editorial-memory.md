# E5: diagnóstico de memoria editorial y microprueba INT8

2026-09-30. No se modificaron el worker editorial de producción ni el entorno arousal/editorial. No se instaló ni descargó ningún paquete/modelo. V1 no se ejecutó. Este informe no acredita inferencia del modelo Qwen de 1,5B ni calidad editorial.

## Host02: hechos y causalidad pendiente

La evidencia original permanece en `.local/e5-editorial-host-02`. El recibo del job fija workerSHA `9bcae1d0f89bbef99e8b776bc8fc58fdd7511c9fe5f77895adb4f3da08ac2c1f`, modeloManifestSHA `066140930ba42fc71368e5770db47b2482d454826e926df3121ea2481e68366a` y lockSHA `52853cb13a0bbf5d36a38aad9f34b1b300774043a57bc0aa48655b6e357c3aa1`. El análisis siguiente describe ese intento, anterior al cambio de loader/conversión de root para host03. El supervisor terminó tras 24,656 s, con `accepted:false`, límite JobObject 8.589.934.592 B (8 GiB) y pico de memoria del job 8.996.986.880 B (8,379 GiB). El host propio PID29196 terminó con código1. `stdout.log` sólo tiene progreso `load_editorial_model`; `stderr.log` registra `Python cerró sin respuesta`. El job `job-b2a88615ad47` conserva intento Failed 17:37:46–17:38:06 UTC. No hubo propuesta/incorporación.

Consulta acotada del registro Application (17:36–17:40 UTC) identificó Application Error1000 a17:38:04 UTC: Python PID14992 (0x3A90), CPython3.12.13 administrado por UV, módulo VCRUNTIME140.dll14.44.35211.0, excepción0xc0000005, desplazamiento0x128a9. El FILETIME de inicio corresponde a17:37:48 UTC, dentro del intento. WER1001 a17:38:05 y17:38:08 UTC comparte informe `9d393fae-e8d0-47b3-aa17-4552a3d70aa6`. La correlación temporal/ruta del runtime es fuerte; el supervisor no archivó el PID hijo para demostrar completamente la genealogía. El Report.wer no fue accesible por permisos y no se elevó ni insistió. No hubo evento Resource-Exhaustion en esa ventana. No se atribuye este AV a CodeIntegrity ni a PyAV.

La presión de memoria es una hipótesis respaldada por el pico y la ruta de conversión, **no una explicación probada del acceso inválido**: faltan stack/dump y fases separadas para distinguir `from_pretrained` de `model.float()`. Los límites del JobObject afectan memoria comprometida, no únicamente working set; [Microsoft documenta ambos límites y picos](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information). El supervisor configura `JOB_OBJECT_LIMIT_JOB_MEMORY`, comprueba SetInformation y asignación antes del handshake, y sólo cierra su job propio. El pico observado no debe reinterpretarse como RAM física ni como motivo para aumentar el límite.

## Lectura exacta del cargador y pesos

Lectura estática de Transformers4.48.3 instalado: `modeling_utils.py:3993` carga safetensors antes de construir el modelo; `:4061` libera la referencia inicial si se habilita low_cpu_mem_usage; `:4083` usa `init_empty_weights`; `:4744` permite asignar buffers si los dtypes coinciden, comprobado en `:374`; `:801` convierte cada tensor al dtype solicitado en la ruta meta. `utils/import_utils.py:91` exige Accelerate>=0.26.0. El worker actual solicita BF16, local-only, safetensors y eager, y luego convierte todo aFP32 (`editorial_worker.py:207–209`). Por tanto no basta activar low_cpu_mem_usage en el mismo BF16 para asegurar que desaparezca el pico posterior de float(). La [documentación de Transformers4.48](https://huggingface.co/docs/transformers/v4.48.0/main_classes/model) describe la carga mediante meta y advierte que, con igual precisión, low_cpu_mem_usage por sí solo puede ser redundante.

Se leyó únicamente el header safetensors con stdlib, sin importar Torch/safetensors ni cargar pesos. Header38.528 B, todos los tensors BF16: 1.543.714.304 parámetros, 3.087.428.608 B de payload. FP32 del mismo payload serían 6.174.857.216 B; la suma BF16+FP32 sería 9.262.285.824 B, antes del runtime. Esta suma ilustra un posible solapamiento, **no una medición de memoria comprometida**: el mapping, la asignación compartida, referencias y liberación por tensor importan.

La configuración tiene `tie_word_embeddings:true`: embedding151936×1536 (466.747.392 B BF16), sin `lm_head.weight` separado en el archivo. Hay196 matrices Linear del cuerpo, 2.620.391.424 B BF16. La conversión global de PyTorch recorre módulos y convierte los parámetros (`module.py:925,986,1166`). El uso de DynamicLinear.from_float convierte/observa una Linear y la empaqueta; su forward devuelve el dtype de entrada. El constructor qint8 utiliza `_empty_affine_quantized`, no un tensor FP32 del modelo completo. [API oficial PyTorch2.8 DynamicLinear](https://docs.pytorch.org/docs/2.8/generated/torch.ao.nn.quantized.dynamic.Linear.html).

## Microprueba nativa acotada

Script nuevo `tests/scripts/test_e5_editorial_int8_micro.py`, SHA256 `a1b11015e6f102331043e630821e4b0ed090dae7f125654c896cfaae62b8fade`. Comando:

```powershell
& .local/e5-arousal-venv/Scripts/python.exe -I -B tests/scripts/test_e5_editorial_int8_micro.py .local/e5-editorial-int8-micro-01
```

PASS4,828 s, Torch2.8.0+cpu, engine x86, threads2/interops1. JobObject propio cap1GiB, deadline60 s, handshake anterior aimports, kill-on-close y timeout limitado aesejob; PID26096 exit0, proceso propio signaled. Pico del job325.959.680 B (310,86 MiB). No `from_pretrained`, pesos Qwen, red, PyAV ni modelos nuevos. Se creó Qwen2 diminuta aleatoria (2 capas, hidden64, vocab128), BF16; referencia FP32 diminuta para comparar. La ruta visitó nombres de hijos sin retener lista global de módulos/pesos, convirtió y reemplazó inmediatamente15 Linear mediante default_dynamic_qconfig/from_float y convirtió finalmente embedding/norms/buffers restantes aFP32. El lm_head se procesa después del cuerpo; su peso está compartido con el embedding antes de convertirlo. Logits FP32 finitos, error absoluto máximo0,0145382136 frente a referencia (<0,15), generación de4tokens aleatorios. Estos tokens no tienen significado editorial ni prueban calidad.

Logs durables: [supervisor](e5-editorial-int8-micro-supervisor.json), [stdout](e5-editorial-int8-micro-stdout.log), [stderr vacío](e5-editorial-int8-micro-stderr.log). Original `.local/e5-editorial-int8-micro-01` conservado.

Proyección aritmética para modelo completo con esa ruta: bodyINT8 1.310.195.712 B + embeddingFP32 933.494.784 B + lm_headINT8 233.373.696 B + restoFP32≈579.584 B =2.477.643.776 B (2,3076 GiB) de payload. **Excluye packing transitorio, observers, asignador, mapping original, KVcache, logits/contexto y runtime**; no garantiza pico ni cap8GiB. Convertir el cuerpo antes del head evita tener todo el modelo FP32 simultáneamente. No usar quantize_dynamic global con deepcopy sobre el modelo grande ni conservar oldweights globales. La nueva ruta altera números/calidad: debe quedar ligada a workerSHA y perfil/engine reproducibles, claves de cache y recibos, sin reutilizar resultados de una ruta distinta.

Siguiente prueba grande sólo tras cambio causal y anuncio/coordinación root: instrumentar fases loaded_model/quantize_linears/remaining_float, conservar cap8GiB y supervisor propio, medir memoria/exit/semántica. Si la carga previa aconversión ya falla, la microprueba no resuelve ese punto y hay que cambiar loader antes de repetir.

## Alternativa low_cpu_mem_usage sin instalar

Consulta sólo metadata oficial PyPI; opción fijada compatible con las versiones actuales, no instalación ni garantía de ejecución:

| Paquete adicional | Archivo | Tamaño comprimido | Licencia | SHA256 |
|---|---|---:|---|---|
| [Accelerate1.3.0](https://pypi.org/project/accelerate/1.3.0/) | accelerate-1.3.0-py3-none-any.whl |336.647 B|Apache|5788d9e6a7a9f80fed665cf09681c4dddd9dc056bea656db4140ffc285ce423e|
| [psutil6.1.1](https://pypi.org/project/psutil/6.1.1/) | psutil-6.1.1-cp37-abi3-win_amd64.whl |254.444 B|BSD-3-Clause|f35cfccb065fff93529d2afb4a2e89e363fe63ca1e4a5da22b603a85833c2649|

Total adicional591.091 B comprimidos; tamaño instalado pendiente, y un entorno aislado puede duplicar archivos del runtime aun usando cache UV. Accelerate requiere Python>=3.9, numpy>=1.17,<3, packaging>=20, psutil, PyYAML, Torch>=2, huggingface-hub>=0.21, safetensors>=0.4.3. Los25pins actuales cumplen los rangos excepto psutil ausente; psutil no tiene dependencias base adicionales. WindowsABI3 admite CPython3.12, pero su import nativo aún no se verificó. Entorno candidato nuevo `.local/e5-editorial-venv` conservaría los25pins+estos2; ningún entorno existente se modificaría. Usar carga meta directamente al dtypefinalFP32 evita la conversión global posterior, pero aún debe medirse el solapamiento mapping/conversión por tensor con cap8GiB. La alternativa INT8 probada en mini evita esta instalación adicional; su aceptación grande permanece pendiente.
