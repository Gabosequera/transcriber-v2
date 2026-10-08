# Fixture de risa humana: investigación y descarga bloqueada

2026-09-30, E3. Investigación pública de fuentes oficiales; sin cuentas, credenciales, medios personales, imports nativos ni ML. Root autorizó descargar hasta1MiB en carpeta nueva tras anunciar76.134B/6,293s/CCBY3; no autorizó todavía procesamiento ni inferencia.

Candidato: **Small group laughter**, autor Ch0cchi, grabación de grupo en estudio publicada2006-01-31. [Fuente del autor en Freesound](https://freesound.org/people/Ch0cchi/sounds/15294/) y [copia Commons](https://commons.wikimedia.org/wiki/File:Small_group_laughter.ogg) coinciden en descripción/procedencia; ambas identifican CC BY3.0. Commons publica76.134B,6,29333333333333s y SHA1 `4b1f54f2797a06b776fa0ef7eb02ad8b9d75f4c1`. No son todavía medidas locales.

La [licencia oficial CC BY3.0](https://creativecommons.org/licenses/by/3.0/) permite redistribuir y adaptar, también comercialmente, conservando atribución/licencia e indicando cambios en una derivación. Atribución propuesta: «Small group laughter / SMALL GROUP LAUGHTER.wav — Ch0cchi, Freesound15294, CC BY3.0; copia Commons; conversión aPCM/downmix/resampling [si se realiza]». Guardar las páginas de procedencia/licencia en el recibo futuro; no publicar binario de audio en evidencia global.

## Resultado de descarga

La solicitud inicial a `https://commons.wikimedia.org/wiki/Special:Redirect/file/Small_group_laughter.ogg` devolvió429; no escribió audio ni carpeta. No se capturó Retry-After en esa primera solicitud. Después se leyó la página oficial de descripción (HTTP200) para obtener el enlace literal Original file:

`https://upload.wikimedia.org/wikipedia/commons/e/eb/Small_group_laughter.ogg?utm_source=commons.wikimedia.org&utm_campaign=index&utm_content=original`

Una solicitud a ese enlace, con la misma identidad User-Agent, sin cookies/credenciales/proxy ni redirects, respondió también429, sin Retry-After. No se reintentó el CDN ni se modificó la identidad. El lector preparado exigía cabeceras antes de cuerpo, límite1MiB tanto por Content-Length como durante lectura, plazo30s y coincidencia exacta tamaño/SHA1 antes de escribir. **Audio descargado0B; SHA256 no disponible; descarga pendiente por rate-limit.** Carpeta local `.local/e5-laughter-positive-fixture-01` contiene sólo README/manifest de investigación, ningún audio.

## Riesgo de etiqueta y criterio de smoke positivo futuro

La etiqueta del autor identifica humanos riendo, pero no aporta anotación temporal experta ni prueba de espontaneidad; una risa actuada sigue siendo voz humana real para smoke. Es risa de grupo y copia Ogg de un WAV, con compresión; no representa una conversación individual limpia. La descripción dice stereo y el metadato Freesound dice mono: verificar canales localmente y documentar la conversión. Antes del detector, escuchar y anotar intervalos de risa; no considerar la etiqueta de categoría como ground truth de cada frame. Excluir como candidato En-us-laugh.ogg: es pronunciación de la palabra, aunque figure en categoría de risa.

Propuesta acotada, pendiente de autorización/descarga: usar worker local de risas por su protocolo standalone y modelo/runtime actuales verificados, audio PCM propio ligado porSHA, sin crear ASR reconocido ni inyectar proyecto. Fijar de antemano defaults actuales CPU2, threshold0,5, min_dur0,2s, merge_gap0,2s, ventanas7s/overlap2s/batch1. Exigir salida validada con al menos un evento de duración>=0,2s, índices/rangos finitos dentro de duración, hashes y worker/model/runtime coherentes y solapamiento>=0,1s con un intervalo de risa anotado antes del run. Conservar probabilidades/eventos/recibo, tiempo, pico y cierre de proceso propio. Si devuelve0eventos, documentar fallo del smoke; no bajar umbral después para declararlo pasado. Una detección no acredita exactitud, recall, límites temporales precisos ni calidad general. Los negativos ya documentados siguen siendo evidencia separada.

Alternativa de licencia libre: [Laughter and clearing voice, ezwa, dominio público](https://commons.wikimedia.org/wiki/File:Laughter_and_clearing_voice.ogg),135.301B/8,777142857s publicados. Mezcla carraspeo con risa y tiene procedencia histórica PDSounds; resulta menos limpia como primer positivo. [ESC-50 oficial](https://github.com/karolpiczak/ESC-50) incluye clase Laughing y WAVs de5s, pero su distribución completa es CC BY-NC3.0: no sustituye una fixture sin restricción comercial. No se descargó ninguna alternativa.

## Recurso alternativo publicado por el autor, sólo metadatos

Tras el429 root autorizó investigar la preview oficial original, sin descargarla ni usarla para eludir el límite de Commons. La página Freesound del autor respondió200 y publica `https://cdn.freesound.org/previews/15/15294_33253-hq.mp3` (también variantes lq.mp3 y lq.ogg). El enlace de descarga del WAV original requiere login; no se usó.

Una única consulta HEAD al hq.mp3, misma identidad, sin cookies/credenciales/redirects, obtuvo HTTP200, Content-Length **111408B**, Content-Type audio/mpeg, ETag `"4db2904e-1b330"`, Last-Modified **2011-04-23T08:39:42Z**. No se pidió cuerpo, no se escribió audio. Metadatos guardados localmente en `preview-metadata.json`. La licencia de la grabación en la página del autor sigue siendo CC BY3.0. Es otra codificación del recurso del autor, con bytes/hashes diferentes: no puede compararse su SHA1 contra el Ogg de Commons, ni se conoce aún SHA256. Duración de la fuente publicada6,293s; duración efectiva de la preview pendiente de verificación local.

Este recurso requiere anuncio/decisión posterior de root antes de descargar. Si se autoriza, aplicar el límite1MiB/plazo30s, conservar cabeceras+URLTLS, exigir tamaño anunciado cuando corresponda y guardarSHA256 propio. Mantener la atribución y documentar que se empleó previewMP3 y cualquier conversiónPCM. El criterio positivo anterior no cambia y no se ejecutó todavía.

## Descarga separada de preview autorizada y completada

Root anunció111KB/CC BY3 y autorizó el GET público de la preview. Un único GET al URL anterior respondió200 con Content-Length111408 y Content-Type audio/mpeg. Se limitó la lectura por cabeceras y contador a1MiB/plazo30s, sin redirects/cookies/credenciales. Se creó un nombre nuevo `small-group-laughter-freesound-preview-hq.mp3` únicamente después de comprobar111408B; archivo flush y SHA256 de memoria/archivo coincidieron: **85569b4951cc0127a13cbea44dd1cd698beab0db68e0167b699196eadf2fa502**. Hora de escritura2026-09-30T19:48:52.9320821Z. Este hash es local; no se obtuvo un hash upstream independiente y el SHA1Commons no corresponde a esta codificación.

La escritura del recibo inicial falló al acceder LastModified.Value después de guardar y verificar el audio. Se recuperó `preview-download.json` desde el archivo ya persistido y los metadatos HEAD anteriores, sin efectuar otro GET; ETag/Last-Modified están etiquetados como HEAD, no como cabeceras GET capturadas. Evidencia pública contiene sólo el recibo, no audio: `e5-laughter-positive-fixture-preview-manifest.json`.

HttpClient no recibió configuración de proxy por la tarea ni cambio de identidad, pero conservó su configuración ambiental por defecto; DefaultProxy.IsBypassed devolviófalse al consultar después. Esto no acredita conexión directa ni identifica el transporte efectivo. Root recibió este límite explícito; no se alteró ningún proxy ni se usó un recurso personal para superar429. Espacio libre al registrar115.999.690.752B, sin atribuir variaciones globales del volumen sólo a este archivo.

Al concluir esa descarga todavía no se había decodificado, escuchado ni procesado audio ni ejecutado ML/ASR. Los siguientes incrementos se autorizaron por separado y quedan registrados a continuación.

## Preparación PCM autorizada

FFprobe real: MP3 mono48000Hz,6,293333s,111408B. FFmpeg produjo WAV PCM signed16 little-endian mono16000Hz sin recorte: **100693frames/6,2933125s**,201430B, SHA256 **3ac02b42fe6d0a7e1e726a4d5b69323da426df16afb73ad329b12148be597380**. La fuente ya era mono; se decodificó y resampleó, sin inventar stereo. Diferencia de duración≈20,833µs por discretización a16k. El request standalone usa el reloj PCM normalizado T0,4440561300Flicks; no se incorpora a un proyecto ni se afirma un ASR reconocido.

Procesos propios FFprobe22144/FFmpeg21944/FFprobe29984 exit0/signaled,0,599/0,460/0,372s, deadline30s y salida<=1MiB. Fuente MP3 conserva suSHA. [Recibo público PCM y comandos](e5-laughter-positive-pcm-manifest.json); no contiene audio binario.

## Supervisor: fallos previos y cambio causal probado

El primer intento real01 no llegó a GO ni a crear el worker: [resultado01](e5-laughter-positive-result-01.json), WinError5,0,125s,pico798720B. No hubo imports ML ni inferencia. Micro stdlib01 instrumentó fase: launcher se asignó al JobObject propio pero el intérprete real tenía IsProcessInJob(actual, nuestroJob)=false y AssignProcessToJobObject(actual) falló5; OpenProcess SET_QUOTA|TERMINATE|QUERY_LIMITED había abierto correctamente. [Micro fallido](e5-laughter-positive-handshake-01.json),1GiB/30s,0,125s/pico802816B. No se omitió el límite ni se repitió inferencia.

La documentación [Microsoft AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject) exige un job vacío o una jerarquía compatible para un proceso que ya pertenece a otro job. [Código oficial CPython venvlauncher, rama main](https://github.com/python/cpython/blob/main/PC/venvlauncher.c) crea un JobObject interno y le asigna el intérprete. Ese código explica una hipótesis de jerarquía, pero no acredita por sí solo el binario exacto3.11.15 local. La diferencia observada se cerró creando el launcher **CREATE_SUSPENDED**, asignándolo al límite propio antes de cualquier hilo ejecutado, y reanudando sólo su hilo primario propio (único, suspend count previo1 exigido). Después del READY se exige miembro propio para launcher e intérprete antes de GO; no se reasigna un miembro que ya heredó el límite.

[Micro corregido02](e5-laughter-positive-handshake-02.json) PASS stdlib1GiB/30s,0,297s,pico13549568B, launcher3408/interprete31240, ambos en job propio, exit0/signaled. [Micro EOF](e5-laughter-positive-handshake-eof-01.json) PASS como rechazo esperado,0,250s,pico13852672B, launcher2492/interprete29520, exit1 por faltar GO antes de imports ML; cierre propio verificado. No se alteraron políticas, binarios protegidos, runtime ni modelo. Script final `tests/scripts/run_e5_laughter_positive.py` SHA **076ca898a4c7c3433e0de40274ab46f33ceb14dc5fba512dfd6462d06bfc7516**.

## Smoke real02: detección técnica en muestra etiquetada por autor

Root reservó ventana y autorizó una inferencia real con cap8GiB/plazo240s/CPU2, sin ejecuciones concurrentes ML de otros agentes. Comando exacto ejecutado desde raízV2:

```powershell
& '.local/e5-laughter-venv/Scripts/python.exe' -I -B 'tests/scripts/run_e5_laughter_positive.py' --work-root '.local/e5-laughter-positive-run-02' --evidence 'implementation/evidence/e5-laughter-positive-result-02.json'
```

**PASS técnico**,15,140s, exit0/signaled, picoJob **3487940608B** (<8589934592B). Handshake confirmó launcher2980 e intérprete24468 dentro del JobObject antes de GO; el worker y sus descendientes se crearon bajo esa jerarquía. [Supervisor](e5-laughter-positive-result-02.json), [recibo/resumen técnico](e5-laughter-positive-technical-02.json), [artifact nativo](e5-laughter-positive-native-events-02.json), [checkpoint durable](e5-laughter-positive-stage-checkpoint-02.json), [stdout](e5-laughter-positive-stdout-02.jsonl), [stderr](e5-laughter-positive-stderr-02.log).

| Evento | Rango s | Duración s | mean_conf | max_conf |
|---|---|---:|---:|---:|
|a0-laugh-00001|0,682–4,854|4,172|0,807|0,992|

Parámetros intactos: threshold0,5,min_dur0,2s,merge_gap0,2s,amplitude_boost=true,ventana7s/overlap2s/batch1. Una ventana; frame_duration0,02005730659025788s; maximum_frame_probability **0,9923580288887024**. El worker no persiste la serie completa de probabilidades por frame; sólo se archivaron máximos/medias y eventos realmente disponibles. No se inventaron ni reconstruyeron probabilidades ni anotaciones.

Artifact SHA **db6721a46ae1eed47e2160691388c531692b68ebb425fc81a24d1df5bebb0ee8**, coincidente con recibo. ModeloSHA **449b14f73c70db26da9b4a59ee77d9a9b29fbcaceb083dd7ea27cdfaa68442a0** y configSHA **ffcc5c417fe11433447975d5053b2279fbeafd6bca03dd2753082e72ad2d36b7**, verificados por el worker pre/post. ManifestSHA **ee1a588bff5264d7941947cd7387b0573605739c723d7b045d713937e43a7c02**. WorkerSHA **b67519fd3d3c243dc76356d24a75aabaa78e7c162e13b7be4617d664c65358dd** y lockSHA **35326ce735a9ec5cfc654767b2efb68b84841b3ce5042872d358ebc51886bd38**, iguales al plan preparado y lectura final. Runtime actual CPython3.11.15/Torch2.1.2+cpu/Transformers4.36.1/NumPy1.26.3/Safetensors0.4.1. El artifact registra el inventario exacto. No se mutó ningún entorno ni se descargó otro modelo. Pesos siguen research-only, distinto de la licenciaCCBY3 del audio.

**Este resultado sólo acredita detección técnica en el clip etiquetado por el autor.** Sin escucha humana ni intervalos temporales de referencia, no se calculó solapamiento/IoU ni se declaró aceptación positiva completa. Flags reales conservados: full_positive_acceptance_complete=false,human_listening_executed=false,temporal_ground_truth_verified=false,ASR/GUI=false. Las condiciones iniciales de anotación/groundtruth siguen pendientes; no se rebajaron para llamar completo al positivo. No se ajustó umbral después del resultado, no hubo otra inferencia de risas y no se modificó un proyecto.
