# Especificación funcional y contratos de Transcriptor V2

Complementa [las instrucciones de ejecución](00-CONSTRUIR-TRANSCRIPTOR-V2.md) y se verifica mediante [las entregas y pruebas](02-ENTREGAS-PRUEBAS-Y-CONTINUIDAD.md). Los requisitos de este documento se implementan; los hechos V1 citados son una referencia que debe revalidarse si cambia el repositorio.

## 1. Experiencia de escritorio

La aplicación abre una ventana propia desde `Transcriptor.exe` o un nombre equivalente documentado. No requiere navegador, WebView, Electron, frontend Tauri ni servidor web para presentar la interfaz. El paquete puede contener DLL, FFmpeg y plugins necesarios; «un .exe» no exige concentrar todas las dependencias en un único archivo autoextraíble.

Diseña una interfaz coherente en español: biblioteca de medios a la izquierda, visor y transporte en el centro, inspector contextual a la derecha, timeline amplio abajo y panel plegable de trabajos/consola/agente. Permite ajustar paneles, alturas de carriles y zoom; conserva el layout y ofrece restablecerlo. Adapta el espacio a pantallas de laptop. Evita fijar controles esenciales fuera de vista al reducir la ventana o aumentar el DPI.

Define tokens de color, tipografía, espaciado, iconos, selección y estados. Prioriza lectura de texto y rangos sobre decoración. Los estados se distinguen con texto/icono y patrón, además de color. La propuesta puede usar trama, el aceptado un check y el desactivado una apariencia atenuada. Incluye tooltips vinculados al keymap real y foco visible. El texto largo se trunca con acceso al contenido completo.

Implementa estados vacío, cargando, sin medios, medio ausente, error recuperable, sin resultados de análisis, conflicto externo y trabajo cancelado. Un botón visible debe funcionar o explicar por qué está deshabilitado. Los modelos aún no instalados se muestran como capacidad ausente, sin progreso o resultados ficticios.

## 2. Medios, timeline y operaciones

Importa video, audio e imágenes desde diálogos y drag-and-drop. Identifica contenedor, streams, duración, resolución, rotación, timebase, start time, frecuencia y canales de audio. La extensión por sí sola no determina compatibilidad. Un medio incompatible produce un error explicativo; un archivo movido permite relink validado. Los originales permanecen intactos.

El timeline de secuencia admite varias pistas de video y audio, imágenes con duración editable, overlays, orden vertical, nombres, visibilidad, bloqueo, mute/solo y ganancia. Define cómo interactúan estas propiedades con preview y export. La composición básica incluye posición, escala, ajuste de aspecto y opacidad; las imágenes con transparencia deben verse correctamente sobre video. Mezcla varias pistas de audio sin sustituirlas accidentalmente por la pista visual superior.

Implementa selección individual y múltiple, selección por área/carril, IN/OUT, mover en tiempo, mover entre pistas compatibles, copiar/cortar/pegar/duplicar, split, trim de bordes, eliminar con hueco y eliminar con ripple explícito, marcadores y undo/redo. Incluye linked audio/video y desvinculación explícita. Nuevos IDs al duplicar; conserva referencias al asset y procedencia. Establece la política de colisiones, overwrite y ripple antes de ejecutar cada operación.

El alcance inicial no exige un compositor de efectos ilimitado. Las operaciones normales descritas sí son obligatorias. Roll/slip/slide, transiciones avanzadas, multicámara y HDR completo se incorporan si se declaran soportados o la paridad lo exige; no añadas botones simulados ni permitas que ese alcance extra retrase los requisitos esenciales.

El mouse distingue selección, arrastre, trim, corte, scrub y pan mediante hit testing estable. Captura un gesto desde press hasta release, muestra una previsualización y confirma una sola transacción al soltar. Escape cancela sin residuo; un drag no genera cientos de pasos de undo. Define umbral de arrastre, handles usables, snapping configurable, autoscroll, rueda y zoom anclado al cursor. Prueba cambios de foco y pérdida de captura.

Mantén vistas Fuente y Montaje/Secuencia. «Revelar en fuente» debe llevar a la posición original correcta, también cuando el mismo segmento aparece repetido. El playhead, selección, rango de loop y rango del clip son conceptos distintos y deben mostrarse sin ambigüedad.

## 3. Tiempo y reproducción

Usa unidades y tipos explícitos para tiempo fuente, tiempo secuencia y tiempo de presentación. Conserva offsets y la base temporal de cada stream. Calcula con enteros/racionales donde sea necesario; no acumules desplazamientos con floats de UI ni asumas que todo video tiene 30 fps. Define intervalos y redondeo para fronteras de clips; permite puntos en los contratos que los admitan.

La correspondencia fuente→secuencia puede devolver varias posiciones por repetición. Los cambios de velocidad de un clip, si se soportan, requieren un mapping propio. La velocidad de revisión del transporte no cambia el proyecto ni la velocidad del render final. Un proyecto visto a 2× se exporta a velocidad normal salvo un efecto de velocidad explícito.

Reproduce dentro del visor nativo: play/pausa, seek, scrub, frame step, loop IN/OUT, navegación a bordes/items, seguimiento del playhead y salto de recortes habilitado por el usuario. Conserva las velocidades V1 de 1×, 2×, 3×, 4× y 8×. En V1, 8× se presenta como skim de keyframes; no prometas decodificación exhaustiva a esa velocidad. Define y muestra la política de audio en cada modo; 2× debe tener audio sincronizado e inteligible, preferentemente preservando pitch.

Durante un scrub continuo prioriza la posición solicitada más reciente. Identifica generaciones de seek para descartar frames tardíos. Mantén preroll, sincronización A/V y colas acotadas. No inicies un decoder nuevo por fotograma ni realices IO, hashing, decode o inferencia en el event loop de UI. Al pausar no repintes continuamente si nada cambia.

Representa waveform y thumbnails progresivamente con cachés por asset y nivel de detalle. El índice temporal y la virtualización permiten consultar solo lo visible. No recorras todos los items ni leas todos los JSON en cada frame.

## 4. Dos familias de capas y una relación explícita

Las pistas multimedia determinan composición y audio. Las capas semánticas contienen transcripción, palabras, hablantes, bloques, temas/subtemas, risas, arousal, silencios, decisiones, propuestas y pedidos del autor. Dibujar una capa semántica sobre el timeline no implica quemarla sobre el video exportado.

Relaciona items semánticos y clips mediante IDs estables, asset/fingerprint, rangos fuente, procedencia y mapping de secuencia. Una capa puede contener items multirrango y jerarquías. Al cortar o repetir un clip, remapea su representación sin duplicar ni modificar la evidencia original. Si una transformación impide una correspondencia fiable, marca el resultado como obsoleto o sin vínculo y explica cómo recalcularlo.

Un cambio en el orden visual de carriles editoriales no cambia el orden de composición multimedia. El cambio de pista de un clip no reescribe los JSON de análisis. Los adaptadores de marcas, bloques o recortes deben escribir en su documento autoritativo y regenerar la vista; no crear una segunda copia editable que pueda divergir.

Permite crear capas y rangos manuales antes de transcribir. V1 ya ofrece `editorial_layers.media_context`, con `editorial-layer-context/1`, como contexto previo al pipeline. No inventes un master inferido para habilitar la UI. Al incorporar un master real, valida identidad, enlaza los datos existentes y muestra conflictos sin perder el trabajo manual.

## 5. Precisiones de V1 verificadas al preparar este encargo

Estado observado: HEAD `c3677ba568cfc7d5947ec4695c42fffef6d79e03`, working tree sin cambios reportados por `git status --short`. Lectura de código, no ejecución funcional de V1 en esta preparación. Los símbolos son referencias más estables que sus números de línea.

| Evidencia V1 | Comportamiento que debe conservarse o migrarse explícitamente |
|---|---|
| `editorial_layers.py`: `STATES`, `validate_items` | Estados de item: `proposed`, `accepted`, `disabled`; rangos ordenados, validación temporal y jerarquía de hasta 32 niveles |
| `editorial_layers.py`: `LayerStore` y merge | Borrado de capa mediante `deleted`; items eliminados mediante `deleted_item_ids` en el merge. No serializar un estado nuevo `deleted` bajo `editorial-layer/1` |
| `editorial_layers.py`: `cut_state`; `editorial_trims.py` | `enabled` determina la aplicación del recorte. `accepted` registra revisión humana; un propuesto habilitado también corta |
| `editorial_layers_ui.py`: `accept` | Los `bloques` no admiten aceptación; aceptar un item aceptado no lo vuelve propuesto. Un docstring menciona A y toggle, pero el código y el registro de acciones no hacen eso |
| `keymap.py`: registro `ACTIONS` | E acepta; Shift+E acepta y avanza; X desactiva según contexto; P activa; Delete/BackSpace/D borran; S divide; A/V seleccionan; B activa corte |
| `keymap.py` y `editor_medios.py`: transporte, `SPEEDS` | Espacio play/pausa; K pausa; L acelera; J desacelera y en 1× pausa; teclas 1–4; Shift+L skim 8×. J no significa reproducción inversa |
| `keymap.py`: acciones de vista/montaje | Shift+T salta recortes; Ctrl+M Fuente/Montaje; Ctrl+Shift+Up/Down mueve pista; Ctrl+N crea capa. Evitar reemplazar estos defaults por convenciones genéricas sin migración |
| `editorial_montaje.py`: `SCHEMA`, `flatten`, `source_to_seq`, `seq_to_source` | Archivo `views/montaje.json`, schema real `editorial-montaje/1`; fuente y secuencia distintas; repetición posible; pista superior sustituye video y audio y el resultado compacta huecos |
| `editorial_montaje.py`: `REQUEST_SCHEMA`, `PROPOSAL_SCHEMA` | Los schemas de intercambio usan `editorial-montage-request/1` y `editorial-montage-proposal/1`. No normalices inadvertidamente «montaje» a «montage» en el documento persistido |
| `editorial_history.py`: `HistoryStack` | Historial de cambios multidocumento con comprobaciones de revisión; no equivaler undo con restaurar ciegamente un archivo antiguo |
| `editorial_cycle.py`: `status` | Ciclo de requests/respuestas y aviso de pedido viejo al cambiar `source_layers_digest` |
| `editorial_io.py`: `digest_json`, `hash_file` | Canonicalización Python concreta para JSON; hash de medios grandes puede muestrear bloques e incluir metadata. No llamarlo hash completo de contenido |

Estas precisiones corrigen simplificaciones de la documentación anterior. Añade tests que las distingan; no las conviertas en texto decorativo. Revalida los símbolos si el estado de V1 cambia.

Al importar un montaje V1, conserva su resultado observable mediante un adaptador o perfil explícito. Un timeline V2 con huecos negros, audio mezclado y overlays tiene otras reglas. Documenta la conversión, muestra sus efectos y no cambies el resultado de un proyecto heredado silenciosamente. La conversión debe poder reproducir fragmentos visibles/audibles originales incluso cuando dos pistas se solapan.

## 6. Paridad de interfaz y shortcuts

Construye un inventario trazable de botones, menús, gestos, estados y acciones leyendo `app.py`, `automatico_ui.py`, `editor_medios.py`, `editorial_layers_ui.py`, `editorial_montaje_ui.py`, `toolbar_ui.py`, `keymap.py`, `keymap_ui.py` y las utilidades de navegación/edición relevantes.

Por cada acción registra símbolo V1, precondición, comportamiento, comando V2, ubicación en UI, shortcut y test. Reorganiza los controles para mejorar uso, conservando capacidades y defaults. Si aparece una acción sin equivalente, implementa su comportamiento o registra una decisión explícita de compatibilidad; no la omitas porque el editor de referencia no la tenía.

Implementa Ajustes → Atajos con búsqueda, categorías, captura de combinaciones, conflictos, restauración de defaults, import/export `keymap/1` y aplicación inmediata. Conserva IDs existentes. Un solo registro alimenta teclado, toolbar, menú contextual, paleta de comandos y documentación de capabilities.

Las teclas de edición de timeline no deben dispararse al escribir una etiqueta o comentario. Verifica layouts español/inglés, AltGr, modificadores, numpad, foco, IME y escalado. Distingue acciones disponibles por contexto: bloque, recorte, item semántico, clip multimedia, texto o ninguna selección. Los atajos nuevos de copiar/pegar deben respetar ese contexto.

## 7. JSON, proyectos y persistencia

Preserva los contratos inventariados en `reports/v1-contract-inventory.md` y verifica sus nombres reales en código. Incluye masters, trims, capas, vistas, chunks, temas/subtemas, montaje, requests/proposals/passes, orden de carriles, manifests, derivación padre/hijo, keymap y registros de exportación.

Inventaría productores y consumidores antes de decidir paths. En V1 existen, entre otros, `editorial/views`, `layers`, `chunks/<chunk_id>`, `.work/manifests`, `.work/staging` y documentos seleccionados/propuestos. Mantén la organización compatible o implementa un importador/exportador con mapping documentado. La estructura de código del producto es distinta de la estructura de proyectos del usuario.

Un JSON normalizado no garantiza el mismo digest que Python: prueba floats, exponentes, Unicode, campos opcionales, listas, redondeo y normalización. Conserva el algoritmo y metadatos necesarios para validar requests V1, o introduce una versión nueva con compatibilidad explícita. No recalcules digests incompatibles y etiquetes luego todos los resultados como stale.

Usa paths portables cuando el contrato lo permita, identidad de assets, relink y validación de rutas. Separa configuración de usuario, secretos, logs, cachés, modelos y datos del proyecto. Las cachés deben ser regenerables y tener presupuesto; no copies gigabytes de modelos/runtime dentro de cada proyecto ni dentro del binario del editor.

Define qué operación confirma una revisión y qué almacenamiento es autoritativo. La transacción SQLite y el replace de un JSON no forman una transacción atómica conjunta: implementa journal/outbox o protocolo equivalente con recuperación idempotente. Prueba interrupciones antes y después de cada frontera. Varios JSON reemplazados individualmente tampoco garantizan atomicidad de una edición multidocumento.

Edición externa: detecta cambios con watcher y rescaneo de respaldo, espera a una lectura estable, valida schema/revisión/digest, prepara diff e importa como comando. Trata un archivo incompleto temporalmente como tal; no borres el estado válido ni sobrescribas la edición humana. Si cambió la base durante el diff, vuelve a validar. La GUI recibe eventos de actualización al confirmar, sin reiniciar el proyecto.

Define política para dos instancias y para autosave/recovery. Undo restaura contenido mediante una nueva revisión auditable, sin rebobinar identificadores de revisión ni borrar archivos exportados como efecto implícito. Las tareas de export/inferencia tienen cancelación y resultados, no una inversa ficticia que borra archivos ajenos.

Compatibilidad significa preservar semántica, IDs, rangos, procedencia y campos, no necesariamente bytes de pretty-print. Una exportación V1 de una función exclusivamente V2 debe informar pérdida o rechazarla; nunca descartar silenciosamente pistas o efectos.

## 8. Control por AI desde el dominio

Todos los cambios de proyecto usan la misma capa de aplicación, tanto si los inicia la GUI, el teclado, un archivo externo o un agente. El contrato debe existir desde E1; el adaptador externo se completa en E4 antes de instalar modelos.

Implementa un registro de comandos tipados con argumentos, schema, targets, precondiciones, permisos y efecto. Incluye proyecto/assets, tracks/clips, capas/items, marcadores, selección, transporte, trims, aceptación, montaje, undo/redo, import/export y trabajos. Separa consultas, cambios persistentes, controles efímeros y tareas largas: mover el playhead no necesita una revisión persistente por frame ni ensucia undo.

Los cambios persistentes incluyen `command_id`, versión del protocolo, actor, proyecto, revisión base, digests necesarios, `idempotency_key` y payload tipado. Resultado: IDs afectados, diff, warnings, eventos y nueva revisión, o error estructurado. Un batch de edición es todo-o-nada; un duplicado devuelve su resultado anterior; una base antigua produce conflicto explicable.

Expón capabilities reales, estado de proyecto, ventanas paginadas del timeline, selección, contexto transcript/señales/procedencia, búsqueda, jobs y errores. No envíes un transcript completo por cada movimiento. Señala datos derivados o stale. La AI debe poder referirse a una palabra/item por ID y obtener su evidencia temporal.

Flujo de propuesta: inspeccionar → proponer → validar → dry-run → diff/preview → aplicar → verificar → auditar. Validación/apply se vuelven a comprobar contra la revisión vigente; el preview no es autorización perpetua. Presenta una bandeja de propuestas y el estado del ciclo V1. Importar JSON manualmente es un cliente más del mismo flujo.

Ofrece permisos por sesión/proyecto y clase de operación. La persona puede autorizar edición automática dentro de un alcance; no pidas confirmación modal por cada movimiento ya autorizado. Conserva protecciones de decisiones humanas y tombstones frente a merges de propuestas. Una orden humana explícita para cambiar su decisión sí debe poder ejecutarse.

Expón un transporte local adecuado y documentado, con un cliente de prueba. Si eliges MCP, úsalo como adaptador, con capacidades y aislamiento apropiados a su transporte; no incrustes dependencia de un proveedor LLM en el dominio. No abras puertos públicos. No expongas ejecución arbitraria de shell/SQL/escritura de archivos como herramienta de edición.

Texto de transcripción, nombres de archivos y propuestas son datos, no instrucciones privilegiadas. Valida tamaños, rangos, IDs, paths y tipos en la frontera. Un agente puede editar mediante comandos o intercambio de archivos validado; nunca escribiendo memoria de widgets o estado GStreamer.

## 9. Render y exportación real

Preview y export consumen la misma revisión resuelta del timeline con selección de streams, mapping temporal, recortes, orden, composición, ganancia y formato de proyecto. La gráfica compartida reduce divergencias pero no prueba equivalencia: compárala con medios reales en E2.

Exporta proyecto completo, IN/OUT, selección o segmentos/chunks definidos. Congela revisión y preset al iniciar. La edición posterior no altera a mitad de render el trabajo lanzado. Muestra progreso, destino, cancelación, errores y resultado verificable. Escribe en staging y publica solo un archivo válido; no sobrescribas originales ni confíes en un exit code sin inspeccionar la salida.

Implementa una matriz de capacidades de codecs, contenedores y presets. «Todos los formatos» significa cobertura amplia y extensible con las capacidades realmente incluidas, no una promesa literal. El baseline distribuible debe cubrir video MP4/H.264/AAC, audio WAV/PCM, imágenes PNG/JPEG de entrada, exportación JSON y presets 720p, 1080p, 2160p y vertical 1080×1920, con licencias/configuración verificadas. Añade MKV, WebM, FLAC, HEVC, AV1, ProRes u otros según encoder/muxer probado; documenta cada combinación. Las capacidades ausentes se indican con una causa; no simules formatos cambiando extensiones.

Configura resolución, aspecto/padding/crop, frame rate válido, calidad/bitrate, canales, sample rate y streams. Conserva exportadores EDL/FCPXML V1 dentro de sus límites, indicando transformaciones o datos que no representan. Preserva metadata temporal relevante y tratamiento explícito de color; un modo SDR limitado se comunica como tal.

Los cortes específicos requieren tratamiento correcto de timebases, GOP y audio. Stream copy no garantiza cortes arbitrarios exactos; recodifica cuando haga falta. Prueba primer/último frame, duración y sincronía, también con VFR y B-frames. No prometas igualdad binaria de salidas con codecs con pérdida o backends hardware diferentes; define tolerancias sobre contenido y tiempos.

## 10. Modelos y pipeline posterior al editor

Tras E4 integra las capacidades reales de V1, empezando por sus módulos y contratos, sin reescribir los modelos en Rust por preferencia de lenguaje. Revisa `core.py`, `align.py`, `laughter.py`, `prosodia.py`, `escena_audio.py`, `editorial_trims.py`, `editorial_pipeline.py`, `pipeline.py` y dependencias. Inventaría también capacidades opcionales restantes; conserva su disponibilidad mediante adaptadores si no se migran internamente.

Conserva Whisper/faster-whisper y el alineador que V1 utiliza realmente, con su selección de modelo, idioma y dispositivo. Integra las LLM mediante adaptadores configurables al control de propuestas ya construido; conserva los flujos de pasadas de temas, recortes y montaje cuando existan en V1. El editor manual debe seguir funcionando offline y sin credenciales. La selección de un proveedor o modelo no altera los schemas del dominio ni obliga a regenerar análisis válidos.

Ejecuta Python en procesos supervisados con entorno versionado. NDJSON/stdio puede llevar control/progreso; stdout queda reservado al protocolo y stderr a logs. Frames/tensores grandes pasan mediante archivos de trabajo o memoria compartida con lifecycle explícito. Implementa handshake, capability discovery, IDs de job, cancelación, timeout, progreso, checkpoints y cierre sin procesos huérfanos.

Presenta las etapas como pasos configurables con dependencia visible: extracción, transcripción, alineación, análisis heurístico de ausencia de palabras/silencio, risas, intensidad/arousal y generación editorial. Una ausencia de palabras no prueba silencio acústico; conserva las fuentes y thresholds reales de V1. Las heurísticas generan propuestas de cortes sin borrar evidencia.

Los pasos marcables/desmarcables deben explicar dependencias y permitir reutilizar resultados válidos. Invalida solo etapas afectadas por cambios de parámetros/entradas/modelo. Conserva nombres, manifests y organización de artefactos. Un resultado calculado sobre una revisión antigua se conserva como resultado del job y se reconcilia antes de aplicarse al proyecto actual.

Las capas de análisis existentes ya deben visualizarse y editarse según su tipo antes de conectar inferencia real. Al conectar modelos, comprueba que producen los mismos contratos y una equivalencia razonable con V1; no exijas bytes idénticos a inferencia no determinista. Documenta versión, dispositivo y tolerancias.

## 11. Rendimiento, distribución y observabilidad

Prioriza interacción/playback por encima de thumbnails, análisis y export en background. Usa concurrencia acotada, presupuestos de RAM/VRAM, backpressure, cancelación y liberación de modelos. Si la memoria GPU disponible es incierta, utiliza un presupuesto conservador y mide; no declares reservas garantizadas por una estimación.

Usar al máximo el dispositivo significa buen throughput con UI receptiva, consumo razonable y degradación visible. No fuerces 100% de CPU/GPU mientras el usuario edita. Un fallback software válido es parte del diseño; registra por qué se eligió. No prometas zero-copy hasta medir la ruta decoder→textura→presentación real.

Mide binario, paquete del editor, runtime Python, modelos y cachés por separado. El paquete básico debe arrancar sin Python/modelos y sin Rust/FFmpeg globales. Resuelve dependencias incluidas desde la instalación y datos desde rutas apropiadas al usuario; no hardcodees `G:\TODO` en producción. En Windows, el lanzamiento ordinario no abre consolas auxiliares; ofrece diagnóstico explícito y conserva stderr de procesos supervisados.

Usa eventos tipados y errores con código estable, mensaje localizable, contexto, causa y acción de recuperación. `tracing` es el candidato actual; correlaciona proyecto, comando, job y worker. La consola UI filtra severidad y muestra progreso limpio; los detalles técnicos quedan desplegables. Rotación de logs, límites de volumen, supresión de repetición y redacción de secretos. Los textos pueden estar en un catálogo; no conviertas mensajes humanos de stdout en un protocolo.

## 12. Fuentes técnicas consultadas al preparar los prompts

Consulta: 2026-09-07. Son apoyo a requisitos concretos; el implementador debe fijar versiones de build.

- [egui, repositorio oficial](https://github.com/emilk/egui): eframe ofrece ejecución nativa y egui mantiene APIs en evolución; la idoneidad del editor se verifica con su propio prototipo.
- [GES, documentación de Rust](https://gstreamer.freedesktop.org/documentation/rust/stable/latest/docs/gstreamer_editing_services/): documenta que la API de GES no es thread-safe. No generalizar esa restricción a todo GStreamer.
- [FFmpeg, opciones de seek](https://ffmpeg.org/ffmpeg.html): diferencia accurate seek en transcodificación y stream copy; respalda la prueba obligatoria de fronteras de corte.
- Evidencia local de MLT: `references/media-engines/mlt/COPYING`, encabezado LGPL-2.1. Comprobar módulos y build antes de concluir la licencia del paquete completo.

La presente preparación no certifica que se hayan ejecutado los benchmarks del research, probado una GUI V2 o resuelto licencias pendientes. Los criterios del archivo 02 son obligaciones futuras de implementación.
