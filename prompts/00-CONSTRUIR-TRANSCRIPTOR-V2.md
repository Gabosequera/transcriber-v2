# Construir Transcriptor V2: instrucciones de ejecución

> **MARCADOR DE CONTINUACIÓN (continuación 7, alpha.4).**
> Estado: **E0/E1 aceptadas históricamente · E2 abierta · E3 parcial · E4 pendiente · parar antes de E5.** Testing físico y revisión general aplazados.
> Fuente de verdad del progreso: `implementation/STATUS.md`, `implementation/requirements-matrix.md`, `implementation/decisions.md` (D-0001…D-0021), `implementation/evidence/e0|e1|e2/`.
> Al reanudar: lee el anexo «Traspaso al siguiente agente» al final de este archivo, luego `STATUS.md`, y continúa por el primer punto de «Qué sigue» sin rehacer lo verificado.

Fecha de preparación: 2026-09-07. Este es el punto de entrada para el agente implementador. Lee también, completos, [la especificación](01-ESPECIFICACION-EDITOR-Y-CAPAS.md) y [las entregas y pruebas](02-ENTREGAS-PRUEBAS-Y-CONTINUIDAD.md), situados en esta misma carpeta.

## Encargo y resultado esperado

Pongámonos manos a la obra. Implementa Transcriptor V2 como aplicación nativa de escritorio en Rust, con un ejecutable Windows utilizable, una interfaz moderna y un editor multimedia funcional. Conserva la semántica editorial, los proyectos JSON y las funciones valiosas de V1. El usuario y un agente externo deben poder controlar las mismas operaciones mediante un dominio común.

Construye primero un recorrido completo de importar, reproducir, editar, guardar, reabrir y exportar. Integra desde ese recorrido las estructuras y los comandos necesarios para las capas editoriales; completa después su paridad con V1 y su control externo. A continuación incorpora los modelos y pipelines Python existentes: transcripción, alineación, heurísticas, risas, intensidad, arousal y las demás capacidades inventariadas. El editor debe funcionar sin instalar ni cargar modelos.

El resultado solicitado incluye implementación, pruebas, corrección de fallos, revisión visual, documentación y distribución. Continúa automáticamente entre entregas cuando sus criterios estén satisfechos. No te detengas al tener un plan, un scaffold, una ventana vacía, capturas bonitas o una demo con datos simulados. Tampoco declares completada una entrega apoyándote únicamente en que compila.

## Autoridad y alcance

- Producto e investigación V2: `G:\TODO\transcriptor-v2`. Puedes implementar aquí. Conserva la documentación y cualquier trabajo de otros agentes.
- V1: `G:\TODO\transcriptor-installer-v0.2.0`, estrictamente de solo lectura. Sus medios, modelos, configuración y proyectos reales también permanecen intactos. Ejecuta comparaciones que puedan escribir exclusivamente contra copias aisladas en V2.
- `references/`: fuentes de consulta. Reutiliza los clones existentes; descarga solo los que falten y comprueba origen, revisión y licencia. No conviertas los clones en el código del producto ni los modifiques como efecto lateral de builds.
- Los permisos anteriores para solo investigar quedan ampliados a implementar V2 por este encargo. No quedan ampliados a modificar V1, publicar servicios, subir datos, comprar recursos o distribuir públicamente el programa.
- Adopta decisiones de implementación rutinarias y reversibles. Solicita una decisión únicamente si falta autoridad o una elección de producto imprescindible que cambie materialmente el encargo; continúa mientras tanto los trabajos independientes.

Lee las instrucciones locales aplicables. Al iniciar registra los estados Git de ambos directorios, si son repositorios, y las modificaciones existentes. V2 era un workspace documental cuando se preparó este encargo: no presupongas que contiene `.git` o `Cargo.toml`. Si ya hay implementación al comenzar, evalúala y continúa desde ella; evita generar un segundo producto en paralelo.

## Lectura inicial y uso del research

Lee estos tres archivos antes de implementar. Después consulta:

1. `reports/contradicciones-y-correcciones.md`, `reports/validacion-documentacion-existente.md` y `reports/trazabilidad-afirmaciones.md`.
2. `docs/18-matriz-conservar-refactorizar-migrar.md` a `docs/24-backlog-de-prototipos-tecnicos.md`.
3. Los ADRs de GUI, motor, persistencia, dominio, comandos, IPC y migración.
4. `docs/03-inventario-de-reutilizacion.md`, `docs/14-licencias-y-procedencia.md`, `research/repositories-lock.json` y los mapas de fuentes correspondientes.
5. Los archivos V1 relevantes a la primera entrega y sus tests; amplía la lectura al entrar en cada subsistema.

La documentación anterior es evidencia de investigación, no una garantía de exactitud. En especial, un título como «arquitectura validada» no demuestra que haya un prototipo integrado. Resuelve discrepancias mediante implementación, tests, versiones verificadas y fuentes oficiales. Registra las correcciones en un ADR o nota con archivo y símbolo de origen.

No repitas toda la investigación antes de producir software. Limita cada experimento a una pregunta de integración, una métrica y una decisión. Los 14 prototipos previos se distribuyen entre entregas; no constituyen 14 bloqueos obligatorios antes de abrir una ventana nativa.

## Base técnica y decisiones reversibles

Parte de Rust para dominio, comandos, GUI y supervisión. La hipótesis de GUI es egui con eframe y backend wgpu; eframe puede gestionar winit. No dupliques event loops ni añadas una integración manual de winit sin necesidad demostrada. Fija toolchain y versiones compatibles en los archivos del producto.

La hipótesis multimedia es GStreamer-RS para reproducción/decodificación y FFmpeg/FFprobe externos para probe/export. Valida pronto que el video se presenta dentro de la ventana y que el paquete Windows encuentra sus dependencias. GES es opcional y, si se usa, vive en su hilo propietario; el dominio propio continúa siendo la autoridad. La integración de texturas y el supuesto zero-copy necesitan evidencia real en el backend Windows elegido.

Mantén estas decisiones detrás de interfaces pequeñas. Si una hipótesis falla, reproduce el fallo, documenta la causa y prueba la alternativa más pequeña compatible con el encargo. Una licencia pendiente de un repositorio de referencia no debe detener el producto: utiliza código propio o una dependencia cuya licencia esté clara.

Define una política única de persistencia antes de escribir proyectos. SQLite es candidato para transacciones, índices, revisiones y trabajos; JSON conserva los contratos externos. Implementar simultáneamente un event store completo, una base duplicada y un sistema de sincronización genérico requiere una necesidad demostrada. Elige el mecanismo más sencillo que satisfaga las pruebas de recuperación y edición externa del archivo 01.

## Reutilización con procedencia

Consulta las rutas y commits de `research/repositories-lock.json` como punto de partida. Comprueba también los encabezados y licencias de cada archivo que realmente incorporarás.

| Referencia | Uso previsto | Condición |
|---|---|---|
| Cutlass | Comandos, inversas, validación de AI y componentes multimedia candidatos | Revisar MIT/Apache y dependencias concretas; portar solo lo necesario con atribución |
| Gausian | Diseño de editor, límites de módulos, representación visual y render | Conflicto README/`LICENSE` registrado; no copiar implementación o assets mientras siga sin resolverse |
| OpenCut | Comportamiento de timeline y editor | No copiar sin licencia verificada |
| Kerf | Flujo conceptual de propuestas/diff y control externo | PolyForm Noncommercial registrado; no incorporar código bajo el supuesto de uso comercial permitido |
| GStreamer-RS/GES | Dependencias e integración multimedia | Separar licencia de bindings, runtime y plugins distribuidos |
| MLT | Comparación de motor y semántica NLE | Revisar componentes y opciones; `COPYING` local contiene LGPL-2.1, por lo que «todo MLT es GPL» es una conclusión incorrecta |
| Kdenlive/Shotcut | Referencias de comportamiento y usabilidad | No copiar código GPL al producto bajo una licencia incompatible |

Fusionar significa integrar responsabilidades y comportamientos compatibles. No concatenes aplicaciones enteras, distintos command buses ni sistemas de persistencia competidores. Leer código restringido y reescribirlo de memoria no convierte automáticamente el resultado en una implementación clean-room; no hagas esa afirmación. Implementa a partir de requisitos propios cuando la reutilización no esté autorizada.

Registra cada incorporación en `implementation/reuse-ledger.md`: origen, commit, archivo, licencia, código incorporado, modificaciones y dependencias. Conserva avisos requeridos y genera el inventario de terceros del paquete final. No supongas que ejecutar FFmpeg como subproceso elimina las obligaciones de distribución.

## Organización del producto

Ubica el workspace Rust en la raíz de V2, separado de `references/`, `docs/` y `research/`. Una organización inicial razonable es `apps/desktop`, `crates/domain`, `crates/application`, `crates/persistence`, `crates/media`, `crates/agent`, `workers/python`, `tests/fixtures`, `packaging` e `implementation`.

Es una guía de responsabilidades, no una orden de crear crates vacíos. Comienza con pocos paquetes cohesionados y extrae límites cuando existan consumidores o necesidades de aislamiento reales. El dominio no importa GUI, GStreamer, procesos Python ni widgets. Las capas de infraestructura dependen de los contratos del dominio. La GUI presenta estado y emite intenciones; los callbacks no escriben directamente JSON ni construyen comandos FFmpeg.

Define primero los tipos que requieren estabilidad: identidad de medios, tiempo, clips, tracks, items semánticos, selección, revisión, comandos, jobs y errores. Usa tipos de dominio y validación en fronteras; conserva los campos JSON desconocidos cuando la compatibilidad lo requiera. Evita un `serde_json::Value` universal, registros globales mutables, strings mágicos dispersos y mutexes que abarquen operaciones lentas.

Un adaptador de compatibilidad V1 es una parte válida de la arquitectura. Dale contrato, pruebas y límites. Corrige las causas de los fallos; no ocultes errores con `catch` amplio, `unwrap` sobre entradas externas, reintentos infinitos, espera arbitraria o fallback silencioso. Evita también abstraer un framework general antes de necesitarlo.

## Bucle de trabajo obligatorio

Para cada requisito o defecto:

1. Identifica su ID en el archivo 02, el comportamiento observable y el estado actual.
2. Lee la implementación y las pruebas relevantes. Si hay una decisión dependiente de versión, consulta código/documentación oficial y registra fuente, fecha y versión.
3. Escribe una nota breve: decisión, alternativas pertinentes, riesgo y prueba que demostrará el resultado. Expón conclusiones y evidencia; no es necesario volcar razonamiento interno paso a paso.
4. Implementa el incremento completo a través de dominio, persistencia, UI y salida multimedia cuando corresponda.
5. Ejecuta las pruebas pertinentes. Inspecciona la aplicación real para interacciones y presentación; comprueba el medio exportado para operaciones de render.
6. Revisa datos, concurrencia, rendimiento y paridad V1. Corrige los fallos encontrados y repite las pruebas afectadas.
7. Actualiza matriz, evidencia y punto de continuación. Pasa al siguiente requisito pendiente.

Haz tres revisiones distintas por entrega: funcional contra el encargo, técnica contra contratos/concurrencia/errores y de experiencia contra el ejecutable. Repite una revisión si encuentra problemas; evita repetir comandos idénticos sin cambio ni incertidumbre nueva. Cada pasada debe terminar en evidencia, correcciones o una limitación explícita.

Si hay agentes auxiliares, asígnales límites de archivos y resultados concretos. Comparte contratos antes de desarrollar consumidores, conserva las ediciones ajenas y reserva la integración y aceptación al responsable principal. No permitas que cada agente invente su timeline, su registro de comandos o su formato de proyecto.

## Continuidad y criterio de finalización

Mantén `implementation/STATUS.md` con entrega activa, build, requisitos verificados, fallos abiertos, decisiones, comandos de reproducción y siguiente acción concreta. Usa `implementation/requirements-matrix.md` para trazabilidad y `implementation/evidence/` para resultados. No marques un requisito como completo si solo existe documentación o una prueba con mocks de su dependencia principal.

Al reanudar, lee estos archivos y continúa desde el primer requisito pendiente. Guarda avances antes de una interrupción. No conviertas la duración de una sesión en criterio de finalización. Si una dependencia externa imprescindible impide continuar, registra qué falta, intentos y trabajos independientes completados; no certifiques lo que no se probó.

Entrega el ejecutable, el paquete y las instrucciones de ejecución con rutas reales; informa pruebas, limitaciones y diferencias de compatibilidad. Windows es el primer destino verificable. Mantén límites portables para Linux y documenta su estado real; no declares soporte multiplataforma sin pruebas en esas plataformas.

Al reanudar, sigue el traspaso vigente al final de este archivo; E0/E1 no se reinician. La prioridad es que una persona pueda editar y exportar desde una ventana nativa y que esa misma operación quede representada en contratos utilizables por la AI.

---

## Traspaso vigente — continuación 6

Alcance: implementar hasta E4 y parar antes de E5. Testing físico, regresión multimedia y revisión general aplazados. Leer STATUS, evidence/continuacion-06.md, requirements-matrix, RUN y decisiones D-0030–D-0034. No rehacer E0/E1.

Checkpoint alpha.3 desde main 7267880: validación de snapshots, master V1 completo y proyecciones protegidas, conservación de aceptación de recortes, protección humana, intención recuperable project/journal, reconciliación con rescaneo estable/merge/diff resumido/aprobación/undo, autosave de proyectos sin carpeta, salto de recortes, montaje de temas/multirrango y navegación de ocurrencias. E2/E3 abiertas, E4 pendiente. Compilar no equivale a aceptación.

Continuar por paridad V1 completa: import editorial asíncrono, inventario/atajos restantes, export GUI/montaje inverso sin pérdida, chunks/autor/bloques/jerarquías/derivación/manifests/requests/passes. Durabilidad: idempotencia/historial/jobs entre aperturas, migraciones y multidocumento V1, auditoría del autosave legado, workers de guardado, watcher SO/diff detallado/conflictos. Optimizar clones/hash de master e índices. Completar distinción de actor al marcar edited/aceptación; la protección actual no es autorización MCP.

Después E4: tools MCP específicas y schemas, capacidades/queries temporales paginadas, permisos por sesión/proyecto/clase, propuestas/digests/dry-run/diff/preview/apply, eventos/auditoría, requests/respuestas JSON y cliente conectado a GUI con núcleo común. Nada de herramienta genérica de shell/SQL/estado.

Entorno: scripts/cargo.ps1 descubre Rust/MSVC; '--' entre comillas al pasar argumentos Clippy. Logs exactos en evidence. Windows bloqueó tests de dominio (4551); tests desktop siguen sin ejecutar, no eludir la política. Tests application/V1 puros y build release local no acreditan GUI ni multimedia. No hay paquete nuevo aceptado. Fuente de publicación: PUBLISH y publication-alpha3.json.

V1 y datos personales estrictamente solo lectura. Preservar evidencia histórica. Si se interrumpe por contexto, dejar commit/push/prerelease y traspaso; no declarar cumplido el objetivo mientras E3/E4 tengan pendientes.


## Traspaso vigente — continuación 7 (sustituye al anterior)

Reanudar desde main, no reiniciar E0/E1. Objetivo hasta E4 incompleto; E2 abierta, E3 parcial, E4 pendiente; parar antes de E5. Aceptación física/revisión general siguen aplazadas. Base de este incremento 9287a02; alpha.4 publicada/verificada sobre 7e050bc, con código subido a main; consultar PUBLISH/evidence.

Leer STATUS, evidence/continuacion-07.md (y 06 histórico), matriz, RUN, decisiones D-0035–D-0038. Implementado: import editorial worker con revisión capturada y batch total; rechazo de docs reconocidos inválidos; editor persistente texto/comentario/rangos/padre con undo; portapapeles de items/árboles y clips preservando propiedades; cortar condicionado a copia, duplicación enlazada y borrado multicapa atómicos; Ctrl+A editorial; import/export keymap; export documental GUI limitado; original íntegro del montaje conservado; recibos idempotentes durables/reapertura/Save As; autosave auditado en worker.

Pendientes de implementación reales: split/bordes/nudge semánticos, ciclo de marca autor y gestión/gestos/carriles; chunks/adaptadores autor/bloques/trims completo/derivación/manifests/requests/passes; export de carpeta e inverso de montajes editados; historial/jobs/migraciones y transacciones multidocumento; guardado/apertura workers, watcher V1 y resolución detallada de conflictos; actor AI/edited/aceptación; E4 completo con MCP específico/GUI/núcleo común. El export actual rechaza montajes editados, autor/bloques/trims y tiempos submilisegundo; no presentar ese rechazo como inverso completo.

39 tests application/V1 pasan; Clippy/all-targets y fmt; build/publicación exactos en evidence/PUBLISH. Tests nuevos desktop compilados, no ejecutados; no reintentar moviendo binarios bloqueados ni cambiar política Windows. V1 bb7012c observado limpio, solo lectura de fuentes; datos personales intactos. Ningún medio/modelo nuevo ni instalación de dependencias. Al cortar por contexto: commit/push/prerelease/traspaso, sin declarar E2/E3/E4 cerradas.

## Traspaso vigente — continuación 8 (sustituye al anterior)

Reanudar desde main con alpha.5 de checkpoint; no reiniciar E0/E1. Objetivo hasta completar E4 sigue incompleto, E2/E3 abiertas y E4 pendiente; parar antes de E5. Aceptación física/multimedia/revisión general siguen aplazadas. Base de incremento acecb37. Estado remoto final en PUBLISH/evidence/publication-alpha5.json.

Leer STATUS, evidence/continuacion-08.md, matriz, RUN, inventario UI y D-0039–D-0042. Nuevo: SplitItem/TrimItem/ShiftItems jerárquicos, ciclo autor y punto→región, batches por contexto, arrastre/corte semántico, carriles/tipos; 140 controles/bindings V1 por AST. Trims exporta documento multicarril con header/metadata/IDs, admite crear antes de master; bloques importa plan elegido, mantiene partición y exporta con límites de evidencia. History/1 durable en intención proyecto+journal+history; IO abrir/guardar/Save As en workers; ACK conserva ediciones nuevas; cerrar espera escritura; autosave lock/CAS/writer; atribución Agent corregida.

Pendientes: caja Corte unión/resta, paridad de gestos/control dinámico; sidecar autor; coalescencia/admin carriles trims; snap seguro/utterances/materialización chunks y edición inspector de sus bordes; derivación/manifests/requests/proposals/passes; carpeta V1 e inverso montaje editado; transacciones multidocumento V1. Durabilidad: recuperar pila autosave completa, jobs, archivado, migraciones generales, watcher SO/V1/diff y resolver conflictos por campo, eliminar clones/hash en UI. E4 entero: MCP específico/schemas/capabilities/queries/selection/transport/context/jobs/permisos/propuestas/eventos/requests/cliente GUI. No presentar el actor interno como E4 ni las negativas de export como adaptación completa.

51 tests dirigidos (33 application +18 V1compat) pasan; controles finales exactos en evidence. Tests desktop/dominio solo compilados; no reintentar ni eludir Windows4551. V1 bb7012c observado limpio, AST/texto solo lectura, datos/modelos intactos. Al cortar por contexto: actualizar evidencia, commit/push/prerelease y traspaso sin inflar el cierre.

Publicación verificada: [v2.0.0-alpha.5](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.5), código/tag `83987b68a8dae139a8f70a5dba71c407b6fd7de8`, main subido. Prerelease no draft, sin assets binarios; `evidence/publication-alpha5.json` y PUBLISH. 51 tests, Clippy/formato/build release final OK; sin ejecución física ni cierre global de E2/E3/E4.


## Traspaso vigente — continuación 9 (sustituye al anterior)

Continuar desde main; base de este incremento 87d9a26, fuentes alpha.6. Leer STATUS, evidence/continuacion-09.md, matriz, RUN, inventario y D-0043–D-0046. No reiniciar E0/E1. Alcance hasta completar E4 sigue autorizado e incompleto; parar antes de E5. Pruebas físicas/multimedia/revisión general aplazadas.

Nuevo: BoxEdit creación/unión/resta por gesto, selección/pedido; coalescencia strict/actor/metadata/tombstones y movimiento/eliminación de carriles trims (main/ai protegidos); adaptador author sidecar schema 1 con identidad/revisión/cuarentena/import explícito/export, sin master editable duplicado. Inspector de bloques ajusta vecinos; SnapBlockBoundaries global-safe/2/evidencia en worker; materializa master derivado/selected/view y archivos por bloque. Transacción documental recuperable en carpetas propias V2, recibos y recuperación desde menú. Autosave restaura pila undo/redo completa. EvidenceDocument inmutable compartido con digest cacheado; PreparedCommand ligado a base conserva IDs al commit.

Pendientes reales: paridad dinámica restante e integración de candidatos globales de autor; derivación/mapping/manifests/requests/proposals/passes, carpeta V1 completa y montaje inverso editado (todavía rechazado), transacciones para contratos restantes. Jobs durables, archivado de auditoría/recibos de sesión, migraciones, watcher SO/V1/diff detallado/resolución; clones de proyecciones/historial serializado y otros costos GUI. E4 entero: MCP real/schemas/capabilities/paginación/selección/transporte/contexto/evidencia/jobs/permisos/propuestas/digests/dry-run/diff/preview/apply/verificación/eventos/cliente GUI. No confundir PreparedCommand con MCP ni materialización de bloques con export carpeta completa.

Controles finales y publicación exactos en evidence/continuacion-09.md y PUBLISH. Solo application/V1compat ejecutados; dominio/desktop compilados por all-targets, no ejecutar ni eludir Windows4551. V1/datos personales intactos. Al necesitar checkpoint: evidencia/matriz/decisiones/traspaso, commit/push/prerelease sin declarar E2/E3/E4 cerradas.


Publicación verificada: [v2.0.0-alpha.6](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.6), código/tag `48784e6ef9a6cfb1c13fcee1e82a1f0989f98a7b`. Prerelease no draft, sin assets binarios; tag y código main confirmados en remoto. Evidencia en `implementation/evidence/publication-alpha6.json`. 60 tests dirigidos, check/Clippy/formato/build release correctos; sin ejecución física ni cierre de E2/E3/E4.

## Traspaso vigente — continuación 10 (sustituye al anterior)

Continuar desde main y conservar avances; base de esta sesión 6997455, fuentes alpha.7. **Objetivo hasta E4 todavía incompleto. E2 abierta/E3 parcial/E4 pendiente/E5 no iniciada.** No reiniciar E0/E1 ni pedir nueva autorización. Aceptación física/multimedia/revisión general aplazadas.

Leer STATUS, evidence/continuacion-10.md, matriz, RUN e inventario, D-0047–D-0051. Implementado: candidatos globales de marcas y selector/adopción por revisión/digest/actor, autoscroll semántico; inversa de montaje editado contiguo/A-V enlazado preservando originales desactivados, estado anterior y mapping; diff por campo/conflictos/orden/elecciones/protección y prepared external worker+lock; watcher Windows para carpeta de proyecto V2; jobs de export durables antes de encoder con recuperación explícita/fingerprint/locks/intentos/recibos; history/2 con ancla+deltas dentro de autosave/commit, lectura /1 y pila completa.

Pendientes reales: derivación padre/hijo/mapping/manifests/requests/proposals/passes; import/export integral de carpeta V1 y transacciones de esos contratos; watcher/reconcile V1 individual; auditoría/recibos generales de sesión, migraciones restantes, coste de clones/proyecciones/serialización, 200 snapshots y límites 64/128 MiB; inventario dinámico y geometría de preview semántico completos. Jobs están en configuración V2, aún sin traslado por Save As ni paginación de archivo/cobertura documental. La inversa rechaza semántica que V1 no representa (gaps/overlays/efectos/audio independiente); no confundirlo con el rechazo anterior de toda edición.

**Primera acción concreta:** seguir las lecturas de editorial_projects.py (time_map/map_range/derive_master/derive_layers/publish_child), editorial_pipeline manifests, editorial_topics y editorial_montaje requests/passes ya iniciadas y completar implementación en dominio/adaptador/worker/UI/publicación aislada. No hay código nuevo de esos contratos en alpha.7. Cuando E3 consuma contratos consolidados, avanzar a E4 MCP específico completo con schemas/capabilities/contexto/paginación/selección/transporte/jobs/permisos/propuestas/digests/preview/apply/verificación/eventos/cliente conectado. No usar E4 para aplazar E3 ni iniciar E5.

Controles: 45 application ejecutados/pasan; Clippy all-targets compila el resto. Windows 4551 **también bloqueó V1compat** en esta sesión, antes de ejecutar su binario; no reintentar ni reubicar/renombrar o cambiar política. Dominio/desktop tampoco se ejecutan. Métrica sintética history: 199436 bytes /2 frente a 7622632 /1 (20 entradas, 190000 caracteres constantes); no prueba rendimiento global. Build/publicación exactos en evidence y PUBLISH. V1 bb7012c solo leído, datos personales intactos.

Un nuevo checkpoint requiere actualizar evidencia/matriz/decisiones/traspaso y commit/push/prerelease autorizado. Esto no equivale a completar el encargo; continuar mientras sea posible dentro del alcance hasta E4.
