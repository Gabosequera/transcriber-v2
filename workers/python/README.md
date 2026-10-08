# Workers locales opcionales

El editor manual no necesita Python. Los workers producen artefactos; solo los comandos preparados del núcleo incorporan resultados al proyecto. No descargan modelos al ejecutar inferencia. No enviar medios personales a servicios remotos.

## Entornos aislados

En esta continuación se usa CPython **3.12.13** para ASR/MMS/arousal y **3.11.15** para risas, en cuatro venv dentro de V2. Las distribuciones están fijadas en `requirements-{transcription,alignment,arousal,laughter}.lock`. `hello` comprueba Python/lock exactos y distingue instalación de carga nativa. Crear los venv con el Python fijado instalado o gestionado por uv, sin modificar entornos globales (si uv no está en PATH, usar la ruta de su instalación existente):

```powershell
uv venv --python 3.12.13 .local/e5-venv
uv pip install --python .local/e5-venv/Scripts/python.exe -r workers/python/requirements-transcription.lock
uv venv --python 3.12.13 .local/e5-alignment-venv
uv pip install --python .local/e5-alignment-venv/Scripts/python.exe --index https://download.pytorch.org/whl/cpu -r workers/python/requirements-alignment.lock
```

Los comandos de instalación son preparación explícita con red; anunciarlos y comprobar espacio antes de nuevos recursos grandes. El wheel Torch2.8.0+cpu de Windows descargado ocupa590.7MiB; el entorno instalado ocupa más. El runtime ASR tiene PyAV bloqueado por Smart App Control en este equipo: no repetir imports/inferencia hasta cambiar causalmente el bloqueo, ni mover DLLs o desactivar políticas. Torch/Torchaudio sí se cargaron en el segundo entorno.

## Modelos y derechos de terceros

Whisper tiny de `Systran/faster-whisper-tiny`, revisión `d90ca5fe260221311c53c58e660288d3deb8d356`, se preparó en `.local/models/whisper-tiny` (~75MB, MIT). El host exige config.json/model.bin/tokenizer.json/vocabulary.txt y sus SHA. El análisis corre offline y sin tokens de Hugging Face/OpenAI/OpenRouter.

MMS_FA de Torchaudio2.8.0 usa el recurso público fijo `https://dl.fbaipublicfiles.com/mms/torchaudio/ctc_alignment_mling_uroman/model.pt`. `scripts/download_mms_model.py` prepara explícitamente `.local/models/mms-fa/model.pt` y su manifest, sin sobrescribir un modelo existente. Descarga **1262047414bytes**; reservar al menos tamaño+1GiB libre antes de ejecutarla. SHA medido: `20ef12963ab4924bef49ac4fc7f58ad5da2ee43b2c11bc8c853c9b90ecdbc680`. Es medición local y no firma upstream. Pesos bajo **CC-BY-NC4**, según [licencia MMS del commit de referencia](https://github.com/facebookresearch/fairseq/tree/100cd91db19bb27277a06a25eb4154c805b10189/examples/mms#license). No se incluyen pesos/venv en Git ni como assets de release; la licencia del código propio no sustituye sus términos.

## Alcance comprobado

`worker.py`: extracción FFmpeg real, transcripción faster-whisper preparada y actualmente bloqueada al cargar PyAV, derivación stdlib de intensidad/pausas/invocaciones/conversación. Sus tests PCM/palabras inventadas prueban fórmulas/contratos; no representan ASR exitoso.

`alignment_worker.py`: contrato separado `tv2-alignment/1`, solo run carga Torch; PCM16mono16kHz y transcript originales verificados, ventanas CPU limitadas, fallos/fallback explícitos, cache por contenido y resultados con lineage. Alineación acústica real de23 palabras conocidas en voz SAPI inventada correcta, también mediante Rust/jobs. Medición del hijo Torch, no launcher: pico WorkingSet **2.532GiB** en ese caso; no garantiza el consumo de otros medios. La GUI actual aún no invoca MMS.

`arousal_worker.py`: audEERING `wav2vec2-large-robust-12-ft-emotion-msp-dim`, revisión `6eba34a2485ea31cb03600241787c3a5edab8626`,661375508bytes de pesos, CC-BY-NC-SA4, entorno `.local/e5-arousal-venv` y recursos `.local/models/arousal-audeering`. Primera ejecución correcta:5ventanas/23palabras MMS sobre voz inventada, picoWS1.549GiB. Son medidas del modelo entrenado con habla inglesa, sin validación de precisión en español ni afirmaciones emocionales sobre personas.

`laughter_worker.py`: omine-me/LaughterSegmentation revisión `cb10e3920766372f06bbd9657724f24dc39fa3e4`,1261816628bytes, entorno `.local/e5-laughter-venv`, recursos `.local/models/laughter-omine`. Código upstream MIT y pesos restringidos a investigación según su ficha conservada; no redistribuidos. Wrapper de inferencia estricto, sin importar el código de entrenamiento que termina procesos aleatorios. Silencio y voz inventada produjeron0eventos en la prueba real; sensibilidad con risa positiva aún pendiente. PicoWS2.562GiB en la repetición final.

Los lectores de los tres workers de modelos usan PeekNamedPipe antes de consumir stdin en Windows: el lector bloqueante original interfería con la inicialización nativa NumPy. El diagnóstico y las pruebas conservan los fallos anteriores y las mismas versiones; no se cambió la política de seguridad. El finalizador stdlib `finalize_worker.py` y el ensamblador `finalize.py` se integran por separado para producir una nueva generación verificable, sin reescribir los jobs anteriores. Su aceptación se registra en la evidencia específica, no por la mera presencia de estos archivos.

La cancelación cooperativa no interrumpe un forward nativo ya iniciado. El host debe aplicar su plazo y terminar exclusivamente el árbol propio. En Windows se usa JobObject; las otras plataformas no heredan esa garantía automáticamente. Ver los informes específicos en `implementation/evidence/continuacion-14-e5-*.md` para build, comandos, fallos y límites.

## Propuestas editoriales locales en integración

`editorial_worker.py` y el adaptador Rust `editorial` usan los contratos existentes de capas/temas/recortes/montaje, con jobs ligados a request/snapshot/modelo. El modelo propone contenido; el núcleo valida, prepara y espera revisión antes de aplicar. La primera pasada de temas sólo produce evidencia validada para la segunda. El editor manual sigue sin necesitar este backend.

Qwen2.5-1.5B-Instruct, revisión `989aa7980e4cf806f80c7fef2b1adb7bc71aa306`, se preparó en `.local/models/qwen2.5-1.5b-instruct`: pesos3.087.467.144bytes con SHA upstream verificado, Apache-2.0 según ficha/archivoLICENSE. Descarga explícita por `scripts/download_editorial_model.py`; no se descarga al arrancar. Manifest/locks y consumo se registran en [evidencia editorial](../../implementation/evidence/continuacion-14-e5-editorial-native.md).

El entorno editorial separado27pins está preparado en `.local/e5-editorial-venv`, con Accelerate1.3.0/psutil6.1.1, preservando arousal.26 pruebas Python stdlib y7 de protocolo real pasan; no ejecutan inferencia. Cada cambio del worker/lock invalida los caches. La GUI integrada pasa comprobaciones de código pero todavía no tiene build nativo aceptado.

Los primeros intentos fallidos se conservan en la evidencia: alias Windows corregido, falta de memoria y degradación de salida al cuantizar a INT8. No produjeron propuestas aceptadas. Host06 ya generó dentro de8GiB al fijar OMP/MKL a2 antes de imports, pero devolvió Markdown y un rango incompleto; se rechazó sin modificar el proyecto.

Perfil vigente: carga por tensor BF16→FP32, construcción meta incluyendo buffers, restauración explícita de RoPE y atención SDPA CPU, con OMP/MKL2 antes de imports y Torch2/interop1. Modelo y dimensiones limitados al Qwen2.5-1.5B verificado. Cabecera, inventario, formas y tamaños se validan antes de asignar pesos; lecturas de hasta1MiB permiten cancelar y comprobar el plazo de carga/generación. El prototipo pasó dos forwards (26/1905tokens) y generó `{"tema":"saludo"}` con EOS, pico8.349GB bajo8GiB; no equivale a aceptación editorial completa. La generación estructurada usa repetition_penalty1 para no penalizar puntuación JSON ya presente en el prompt. La plantilla de temas usa el ámbito real; montaje recibe restricciones de duración/reordenación y las capas nuevas reciben un identificador sugerido propio por pedido. No se eliminan fences ni se reparan respuestas para presentarlas como válidas.

Después del fallo de memoria en host07, el prefill usa bloques256 y conserva todo el contexto hastaN−1; generate recibe IDs/máscara completos. La penalización1.1 se aplica sólo a la salida generada. Micro del helper real sobre513tokens verifica cache y tokens equivalentes; el consumo del modelo real se mide aparte. El adaptador rechaza cobertura incompleta de palabras y temas duplicados después de snap. La plantilla de segunda pasada conserva source_item_ids, rangos y jerarquía del mapa anterior. Timeout puede archivar texto parcial como diagnóstico, nunca como propuesta válida; no recupera texto de procesos terminados externamente.
