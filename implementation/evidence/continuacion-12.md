# Continuación 12 — integración E2/E3/E4, fuentes alpha.9

Base: main limpio en 6aefc03f6f26cbd44d062ac4ac972d6a5853ea06, posterior al tag alpha.8. Se conserva el proyecto y se integra el trabajo de los tres frentes. E0/E1 no se reinician; E5 no se inicia. V1 y datos personales son de solo lectura. No se ejecutan GUI, medios, modelos ni inferencia.

## E2 implementado

- Inspector con borradores persistentes para texto, ganancia, transformación y duración de imágenes. Motivo/estado editorial de clips, E/Shift+E/P/X, grupos enlazados y protección de revisión humana. Split, propiedades y link/unlink respetan pistas bloqueadas.
- Gestos multimedia/semánticos preparan el comando en worker, muestran su geometría y confirman el mismo resultado/IDs contra la base exacta. Escape/foco/error cancelan. Snap considera ambos bordes; mover pistas distingue audio/video. Selección por área incluye items; autoscroll horizontal/vertical conserva el ancla. No se cruza un mapping ambiguo entre ocurrencias.
- Índices de clips/proyecciones en workers, con invalidación y error/reintento. resolve_jobs reconstruye composición y rangos de salto en un worker, descarta resultados antiguos y difiere seek/play hasta la composición vigente. El visor señala que conserva el último resultado mientras calcula. Exportar espera a esa composición y reutiliza su snapshot. El player conserva el reloj de audio si timeline/assets son iguales.
- Paleta Ctrl+Shift+P con búsqueda/contextos del registro de teclado/menús; contexto MCP publica atajos efectivos desde ese registro. No ofrece dispatch arbitrario remoto.
- Reenlace con probe cancelable; descubrimiento del medio mediante lectura estructurada parcial del master. Cerrar importación no espera indefinidamente un IO bloqueado.
- Exportación configurable: resolución, fps racional, frecuencia/canales y bitrate donde se admite; validador común GUI/MCP. EDL/FCPXML en perfil representable V1, rechazando efectos/mapping no representables. Waveform con agregación adaptativa de picos y caché versionada para medios largos.

## E3 implementado

- Derivación padre/hijo desde mapping y recibo de export verificado. Repeticiones, orden, IDs, procedencia y evidencia original conservados. El exportador mezcla audio: el hijo declara una pista mezclada y archiva la evidencia multipista como procedencia, sin atribuirle varios streams transcritos. Mappings con relojes compuestos, huecos o transformaciones no representables se rechazan.
- Requests/proposals/manifests y dos pasadas de temas con schema/IDs/digests/cobertura/base. La segunda pasada cita el mapa validado de la primera. Capas genéricas user/topics/ai, trims y montaje usan preparación/protecciones comunes. Respuestas V1 sin request_id requieren vinculación local explícita después de comprobar sus dos digests; original y copia vinculada se archivan. Avisos de montaje por duración/motivo/repetición/tolerancia se muestran antes de aplicar.
- Panel documental con validación/diff/aplicación y recibos que distinguen aplicación en sesión de guardado. Versiones obsoletas/manifests divergentes se archivan para revalidación; no se inventa ejecución de pasos.
- Watcher de documentos V1 individuales: capas/trims/autor/bloques/montajes, lectura estable, base guardada, merge de tres vías, resolución explícita y revalidación antes del commit. Pedidos/propuestas se abren en una copia V2. El documento vigilado permanece intacto.
- Conversación/utterances/palabras de solo lectura, búsqueda/tiempo/pista, 64 filas por página e IDs originales. Apariciones en secuencia calculadas en worker, incluidas repeticiones/audio independiente. No duplica el master por frame.
- Save As materializa auxiliares SHA verificados en source-bundles, relocaliza sesión/historia y conserva directorios vacíos. Captura sin rechazo por 20000 entradas/32 MiB textuales: el exceso de caché se referencia con bytes/SHA y streaming. Publicación multidocumento/binaria recuperable sin modificar fuentes.
- Auditoría segmentada con índice transaccional y páginas por digest; recibos fríos indexados más allá de 10000 claves. Replay no ejecuta otra vez. Migración project/1→project/2 al guardar; schemas futuros rechazados antes de deserializar.
- Codec de almacenamiento versionado externaliza masters/capas mayores de 64 KiB a blobs SHA/canónicos. Incluye historia/autosave/intención/comandos auditados/recibos, hidratación y rechazo de corrupción. Preserva campos desconocidos y marcadores literales. Items COW compartidos en proyecto/historia/workers; certificados de validación por snapshot completo y duración evitan repetir capas iguales sin omitir validación global.

## E4 implementado

Crate tv2-control, servicio MCP HTTP loopback con token efímero y schemas cerrados; cliente PowerShell HTTP/stdio real en scripts/mcp-client.ps1. Conectado a sesión/selección/transporte/jobs de la aplicación. Consultas temporales, evidencia, búsqueda y auditoría paginadas. Permisos locales por sesión/proyecto/clase y alcance automático por tipos explícitos. Importación solicita selección local; exportación usa configuración local. Sin shell, SQL, paths arbitrarios ni autoconcesión remota de permisos.

Propuestas con revisión/digest/idempotencia: dry-run → diff/preview exacto → revisión local o alcance automático → apply → verify. IDs conservados; diff precalculado. Review/apply no reserializan el master en GUI; verify calcula hashes en worker y devuelve pending/null hasta completar. El cliente admite WaitForVerification. Se rechazan bases antiguas y cambio de contenido con igual revisión. UI muestra eventos, propuestas y persistencia; un reinicio revoca autorizaciones de revisión. Detalles y aceptación reproducible en ../control.md.

## Límites explícitos

Metadatos project/autosave:64 MiB; history/intención:128 MiB; undo:200 operaciones; índice/segmento auditoría:64 MiB; recibo individual codificado:64 MiB. Masters/capas externos no comparten el tope del JSON principal. Recibos calientes:1024 de hasta64 KiB; claves frías indexadas en RAM. Certificados:256 capas, una versión por ID.

COW opera por capa: editar copia su vector si está compartido. Extra, clips y metadatos no tienen presupuesto global. Abrir/recuperar puede hidratar evidencia/capas y materializar auditoría completa. Algunas consultas/snapshots/validaciones globales son lineales. Sin GC automático de blobs durables; jobs en configuración V2, sin traslado al Save As. Son costes/límites declarados, sin promesa de RAM ilimitada ni certificación PERF-01. No se ha medido fluidez, sincronía A/V o resistencia a pérdida física de energía en este incremento.

## Comprobaciones y aceptación

- application-continuacion-12.log conserva68 unitarios correctos y el fallo inicial de una fixture de bloques que creaba items humanos. Corregida la fixture sin relajar protección, sus3 integraciones pasaron. application-continuacion-12-final.log registra después70 unitarios+3 integraciones correctos tras diff/caché segura. Los resultados finales posteriores se anotan al pie.
- Control:17 tests se ejecutaron correctamente al principio. Un intento posterior con19 quedó bloqueado por Windows4551 antes de ejecutarlos. No se reintenta/evade; los tests posteriores solo se compilan.
- Dominio/desktop/V1compat: tests compilados con all-targets, nunca ejecutados ni reintentados tras4551. Ningún test multimedia se ejecuta.
- RUN contiene14 recorridos de aceptación futura; control.md añade cliente contra servicio de la aplicación. Compilación y unitarios TCP no sustituyen aceptación del ejecutable. E5 permanece fuera del alcance.

Build/publicación y los controles del último árbol se registran al pie. Una prerelease de fuentes no es un paquete binario aceptado.

## Legado y preparación de importación

Los proyectos antiguos sin source_bundle pueden vincular su carpeta original en el mismo proyecto: AttachMaster solo permite añadir el bundle ausente con documento completo/identidad/digest idénticos. El importador emite ese único comando y conserva capas, secuencias, vista y selección. El worker prepara también el batch normal de importación, validación y diff; la GUI confirma el PreparedCommand contra la misma base. No revalida ni reserializa todas las capas recién importadas en el event loop.

El autosave posterior a deshacer una vinculación exige conservar la semántica de ese undo en recuperación: la excepción a retirar el bundle se restringe al checkpoint con historia validada que demuestra esa operación. El merge/reconcile ordinario no obtiene esa excepción. Los logs dirigidos y el control final de application identifican la comprobación exacta.

## Control final del árbol integrado

73 tests unitarios de application correctos (26,76 s) y 3 de integración correctos (0,00 s), en application-continuacion-12-integrated.log. Incluyen recuperación posterior al undo de la vinculación, pruebas negativas de historia ausente/no pura y redo portable tras mover la carpeta. El fallo intermedio de localizadores se conserva en application-legacy-recovery-12.log; se corrigió la normalización de contenido idéntico, sin permitir reemplazar evidencia.

Check workspace/all-targets correcto; su log conserva un error intermedio de tipo en la nueva acción Guardar como, corregido descartando el retorno booleano del handler. La repetición termina correctamente (2,53 s). Clippy workspace/all-targets -D warnings correcto (5,56 s). Formato correcto y parser PowerShell del cliente correcto. Los tests all-targets de dominio/desktop/V1compat/control están solo compilados. No se lanzó FFmpeg ni el ejecutable. Build release y publicación se registran a continuación cuando finalicen.

Build release final correcto: 2 min32 s de Cargo (153,95 s con wrapper), 30656000 bytes. SHA-256:2577a293b879ce2aedd997bfc15e054f43f579ce21632fb22512c3e881367a66. Metadata en build-alpha9.json y stdout en build-continuacion-12.log. Ejecutable NO lanzado; NO se adjunta como asset de release. V1 se observó limpio al terminar las comprobaciones.


Publicación confirmada: [v2.0.0-alpha.9](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.9), código/tag 0fc6ae2184ce642b222ec6764d0edf9796096592; prerelease pública no draft, sin assets binarios. Verificación en publication-alpha9.json. Main incluye después el commit documental de este registro; el tag conserva exactamente el código compilado. La publicación no declara aceptación física ni inicia E5.

