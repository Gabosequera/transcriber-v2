# Construir Transcriptor V2: instrucciones de ejecución

> **MARCADOR DE CONTINUACIÓN (actualizado 2026-09-08, continuación 3).**
> Estado: **E0 cerrada · E1 cerrada (host) · E2 avanzada y abierta · E3–E6 pendientes.**
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

## Traspaso vigente — continuación 5

### 1. Alcance

El usuario reanudó el desarrollo desde otro equipo y autorizó completar **implementación hasta E4**, parando antes de E5. Testing real y revisión general quedan para otra iteración. Los antiguos requisitos de aceptación física siguen pendientes; no bloquear implementación en ellos ni declarar que pasaron. Al aproximarse al límite de contexto, preparar commit/push/prerelease y traspaso breve.

### 2. Estado

E0/E1 aceptadas históricamente. E2 funcional implementada con validación integrada pendiente. E3 parcial: esta continuación añade portabilidad de guiones, preparación MSVC, atajos editables, continuidad de undo/redo, identidad de reintentos/digest, guardado con control de cambios externos y recuperación ofrecida en GUI. **E4 aún pendiente.** No se integraron modelos.

### 3. Fuentes autoritativas

Leer `implementation/STATUS.md`, `implementation/evidence/continuacion-05.md`, `implementation/requirements-matrix.md` y `implementation/RUN.md`. El traspaso anterior está archivado en `implementation/evidence/traspaso-continuacion-04.md`; sus rutas G:/TODO son históricas.

### 4. Entorno

Descubrir raíz desde scripts; no usar rutas del equipo anterior. `scripts/cargo.ps1` descubre Rust/Visual Studio y configura MSVC x64. Los JSON de tests son plantillas que se materializan con `prepare_script.py` o `prepare_regression.py`. FFmpeg por variable explícita, copia local o PATH. Los goldens V1 históricos requieren bytes originales o regeneración explícita mediante V1 aislado.

### 5. Próximo trabajo

Completar E3: validación integral al cargar/recuperar; auditoría y atomicidad multidocumento ante fallos; autosave de proyectos nuevos; reconciliación externa con diff y protección humana; master y contratos V1 completos/export GUI/montaje inverso; chunks/jerarquías/evidencia/derivación/ocurrencias y acciones V1 restantes. Después E4: herramientas MCP específicas, catálogo/schema, consultas paginadas, permisos, propuestas/dry-run/apply/eventos y cliente local conectado a GUI. Nunca una herramienta genérica de shell/SQL/estado.

### 6. Restricciones

V1 y datos personales solo lectura. Preservar evidencia y licencias. El commit de checkpoint no certifica E3/E4 ni una release binaria validada. Guardar contexto antes de parar; no entrar en E5/E6.
