# E5: entorno editorial separado para carga meta

2026-09-30. Root autorizó el incremento después de host03 Failed por `bad allocation`, tras llegar a `loaded_model` y `quantize_linears`; su evidencia/worker/host siguen bajo root. Este frente prepara únicamente el runtime, sin inferencia grande ni edición del worker. Recursos adicionales públicos anunciados: Accelerate1.3.0 y psutil6.1.1, 591.091 B de ruedas. Entorno nuevo `.local/e5-editorial-venv`, separado de arousal/MMS/ASR/risas. No se alteraron políticas, DLL ni entornos globales/V1.

## Preparación y procedencia

Espacio previo C:121.447.092.224 B (113,10 GiB); posterior117.944.430.592 B (109,84 GiB), medición global del volumen mientras otros frentes pueden escribir. UV0.11.32, ejecutable instalado en Microsoft.WinGet. Se creó el venv nuevo con el CPython3.12.13 administrado existente, sin descargas de Python: [log creación](e5-editorial-venv-create.log).

Primero se intentó reutilizar la caché mediante `uv pip install --offline --python .local/e5-editorial-venv/Scripts/python.exe -r workers/python/requirements-arousal.lock`. Falló resolución antes de instalar: varias ruedas no estaban disponibles y no se encontró charset-normalizer3.5.2. [Incidencia preservada](e5-editorial-venv-cache-install.log). No se cambiaron pins ni se descargaron esos25paquetes. **No se acredita reproducción desde un índice/caché vacío.**

Con aprobación de root se reutilizó el snapshot instalado: script `tests/scripts/prepare_editorial_venv_snapshot.py` copió físicamente17.535 archivos (3.287.881.984 B lógicos, 3,062 GiB) de Lib/site-packages a Lib/site-packages del venvnuevo en81,125 s. No hardlinks, no copias de Scripts/launchers viejos, y2.218 archivos pyc/cache se omitieron. Cada archivo copiado se verificó con SHA256, y el original volvió a verificarse después. Se verificaron todos los archivos no-pyc originales incluidos en el snapshot, no una supuesta invariancia de caches excluidas. [Log snapshot](e5-editorial-venv-snapshot.log), [inventario SHA por archivo](e5-editorial-runtime-snapshot.json). El script sólo crea archivos bajo el entorno nuevo/evidencia; no elimina fuente ni caché.

Los dos archivos .pth del entorno nuevo son `distutils-precedence.pth` (import shim setuptools) y `_virtualenv.pth` (import _virtualenv). No contienen rutas absolutas ni referencias al venv arousal. Sys.path observado al verificar no contiene arousal; sys.prefix es el venveditorialnuevo. Los entrypoints CLI de los25paquetes copiados no se reprodujeron; el backend sólo consume sus módulos desde Python y no esos entrypoints.

Se instalaron exclusivamente dos ruedas mediante UV `--no-index --no-deps`, URL pública con fragmento SHA256 fijado. [Log instalación](e5-editorial-venv-additions-install.log): resolución269ms, preparación207ms, instalación71ms. Fuentes/hashes oficiales [Accelerate1.3.0 PyPI](https://pypi.org/project/accelerate/1.3.0/) y [psutil6.1.1 PyPI](https://pypi.org/project/psutil/6.1.1/):

| Archivo | Bytes | SHA256 | Licencia |
|---|---:|---|---|
| accelerate-1.3.0-py3-none-any.whl |336.647|5788d9e6a7a9f80fed665cf09681c4dddd9dc056bea656db4140ffc285ce423e|Apache|
| psutil-6.1.1-cp37-abi3-win_amd64.whl |254.444|f35cfccb065fff93529d2afb4a2e89e363fe63ca1e4a5da22b603a85833c2649|BSD-3-Clause|

El hash es el publicado y fijado en la URL de instalación de UV; no se afirma una segunda descarga manual independiente. Psutil no tiene dependencias base; Accelerate requiere numpy>=1.17,<3, packaging>=20, psutil, PyYAML, Torch>=2, huggingface-hub>=0.21 y safetensors>=0.4.3. Los25pins anteriores más estos2 satisfacen sus rangos. UV pipcheck verificó27paquetes, todos compatibles, en6ms: [log](e5-editorial-venv-depcheck.log).

## Verificación y congelación

La comprobación native pequeña importó Torch2.8.0+cpu, Accelerate1.3.0, psutil6.1.1 y Transformers4.48.3, verificó inventario de27distribuciones exacto con lock, y creó únicamente una Linear4×2 bajo `init_empty_weights` (peso en meta, sin datos). No `from_pretrained`, pesosQwen, generación ni PyAV. RSS propio al finalizar comprobación256.901.120 B. Proceso terminó exit0. [Imports](e5-editorial-venv-imports.log), [inventario runtime27pins](e5-editorial-runtime-inventory.json). Esto prueba disponibilidad del loader meta y compatibilidad de imports, **no aceptación del modelo grande ni solución probada del pico**.

Pyvenv.cfg nuevo quedó fijado explícitamente a `home=.../cpython-3.12.13-windows-x86_64-none`, version_info3.12.13, include-system-site-packages=false. UV había resuelto inicialmente la junction `cpython-3.12-windows-x86_64-none`, cuyo Target es ese3.12.13; ambas rutas Pythonbase tienen los mismos bytes. Después del ajuste exclusivo del venvnuevo se verificó sys.version3.12.13 y sys.prefixnuevo sin imports de ML: [log final Python](e5-editorial-venv-final-python.log). Sys.base_prefix puede informar la alias junction aun con home explícito; ambas rutas fueron verificadas iguales.

| Recurso congelado | SHA256 |
|---|---|
| workers/python/requirements-editorial.lock (27pins) |f168f99ed58200cc1532217b00c6f27c8b0a68c98ba5899491875ee0538c99cb|
| .local/e5-editorial-venv/Scripts/python.exe launcher |c40630e8d077aba7ac3850e9b69947c4c978e2dd40df9d7acdcee31f3b972da1|
| .local/e5-editorial-venv/pyvenv.cfg explícito |1b9bec3d249ac5eb50a921d1afd32b9fec17a8bedc8cea93d39f26a86795cdd2|
| Pythonbase CPython3.12.13, ambas rutas |f598fb950a86a895d8f9b4755fc9b38c48adc7a15732a342e55c17a3c3499602|
| workers/python/requirements-arousal.lock antes/después |52853cb13a0bbf5d36a38aad9f34b1b300774043a57bc0aa48655b6e357c3aa1|
| .local/e5-arousal-venv/pyvenv.cfg antes/después |1b9bec3d249ac5eb50a921d1afd32b9fec17a8bedc8cea93d39f26a86795cdd2|

Nuevo lock sólo añade Accelerate1.3.0 y psutil6.1.1 a los25pins originales, sin variar ningún anterior. Inventario completo en JSON citado; los archivos nativos importados pertenecen al venvnuevo y Pythonbase permitido ya instalado. Arrousal se usó sólo como fuente de lectura y launcher para el script stdlib, sin instalaciones ni imports ML nuevos en su entorno.

## Integración y límites

Root recibió freeze inmediato del runtime/cfg/lock antes de protocolos y host04. No se hicieron nuevos imports nativos ni cambios del entorno después. Root cambia el loader a low_cpu_mem_usage=True: Transformers4.48.3 instalado exige Accelerate>=0.26.0 para esa ruta, ahora disponible; la identidad debe volver a calcular Python/worker/lock/modelManifest/dependencies sobre las rutas nuevas. WorkerSHA y lockSHA cambian las claves/recibos, sin reutilizar los jobs Failed anteriores como resultados válidos.

Root conserva cap8GiB y fases explícitas, mide host04 y valida propuestas. La preparación presente no demuestra inferencia editorial ni calidad de tópicos/pasadas/chunks; el resultado de host04 pertenece a la evidencia native de root. La limitación de reproducción freshindex permanece: se tiene un snapshot exacto verificable y paquetes adicionales públicos fijados, no las25ruedas originales archivadas.
