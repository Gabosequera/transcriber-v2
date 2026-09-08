# Entregas verificables y continuidad de implementación

Lee junto con [el encargo](00-CONSTRUIR-TRANSCRIPTOR-V2.md) y [la especificación](01-ESPECIFICACION-EDITOR-Y-CAPAS.md). La secuencia evita posponer los contratos hasta después de dibujar la UI. Los modelos llegan después del editor y del control externo. Aprobar una entrega por sus pruebas habilita continuar automáticamente a la siguiente.

## E0. Preparación concreta

Registra estados de repositorios, instrucciones locales, toolchain disponible y revisión V1. Reutiliza `research/repositories-lock.json`; comprueba qué clones faltan y descarga únicamente esos. No ejecutes scripts de instalación de repositorios de referencia sin revisar sus efectos.

Crea la matriz de requisitos con los IDs de este archivo y amplíala con el inventario de funciones V1. Captura fixtures pequeños propios o copias permitidas dentro de V2; anonimiza antes de compartirlos. Los medios personales no se suben a servicios externos. Incluye proyectos preanálisis, con master, con multirrango, trims y montaje.

Decide límites mínimos de módulos, tiempo, persistencia y comandos en notas breves. Ejecuta un experimento temprano de ventana nativa, video embebido y dependencias Windows. Los experimentos de accesibilidad/IME y de fidelidad de media se amplían durante las siguientes entregas.

Salida: build nativo inicial, contratos mínimos, fixtures, matriz y primer riesgo multimedia reproducido/resuelto. No detenerse aquí.

## E1. Primer recorrido completo, sin modelos

Implementa una ventana con biblioteca, visor, transporte, timeline e inspector. Importa un video real, reprodúcelo a 1×/2×, divide un clip, mueve una pieza, crea una capa manual con rango, guarda el proyecto, ciérralo, reábrelo y exporta el montaje editado. Todas las mutaciones usan comandos; el primer proyecto ya tiene identidad, revisión y mapping temporal.

Implementa el mínimo de undo y errores necesario para ese recorrido. Incluye un ejecutable release invocable fuera de `cargo run`, con sus dependencias resueltas. El visor muestra la salida del timeline, no un reproductor del archivo fuente ajeno a las ediciones.

Salida: recorrido reproducible y clip exportado cuya edición se verifica. Los mocks solo cubren pruebas unitarias; esta entrega exige video y export reales.

## E2. Editor multimedia completo dentro del alcance

Completa pistas de video/audio, imágenes/overlays, transformación básica, composición y mezcla; operaciones normales, selección múltiple, snapping, ripple explícito, markers, source/sequence y linked A/V. Implementa todas las velocidades V1, loop, frame step, scrub y cachés progresivas. Pulir la UI ocurre durante esta entrega, no después de toda la inferencia.

Implementa presets y exportación de proyecto/rangos/chunks, cola, cancelación y verificación de resultados. Prueba la equivalencia observable de preview/export. El importador de JSON V1 se desarrolla en paralelo a la GUI; una fixture editorial debe aparecer correctamente en el timeline, aunque el inspector avanzado se complete en E3.

Salida: editor utilizable con medios reales y paquete provisional. Los formatos declarados disponibles tienen pruebas y los ausentes explican su estado.

## E3. Paridad editorial V1 y proyectos durables

Completa import/export de contratos V1, aceptación/activación/borrado según tipo, chunks contiguos, jerarquías, multirrango, carriles, comentarios, evidencia, derivación y protección de decisiones humanas. Reproduce las reglas del montaje heredado mediante adaptador probado. Incorpora inventario de botones y gestos de V1 a la UI nueva y termina los atajos configurables.

Implementa autosave/recovery, reconciliación de cambios externos, revisión/digests, migraciones y atomicidad lógica multidocumento. Una persona debe poder editar capas manualmente, revisar recortes y seguir trabajando sin modelos ni agente conectado.

Salida: comparación V1/V2 con pruebas de contratos e interacciones. Si hace falta ejecutar V1, usa copia aislada con configuración y cachés redirigidas a V2; nunca el checkout vivo.

## E4. Control externo por AI, antes de inferencia

Implementa descubrimiento de capabilities, consultas con contexto temporal, comandos/batches, propuestas, dry-run/diff/preview/apply, eventos, auditoría y permisos de sesión. Agrega cliente de prueba y documentación de uso local. Conserva la vía de requests/respuestas JSON y su estado/stale en UI.

Demuestra un cliente externo que consulta clips/capas, propone un cambio, lo aplica dentro de su alcance autorizado y lo ve reflejado en la GUI abierta; undo en GUI revierte su contenido como nueva revisión. Demuestra también rechazo por base antigua y reintento idempotente. No requiere contratar ni conectar un LLM: se prueba el protocolo con un cliente real y luego se verifica la integración disponible.

Salida: editor controlable externamente con el mismo comportamiento que sus acciones humanas. Una lista de herramientas sin cliente funcionando no satisface la entrega.

## E5. Integración de modelos y flujo existente

Conecta workers Python a los contratos y jobs ya existentes. Integra capacidades en incrementos: extracción/transcripción; alineación; heurísticas y propuestas; risas e intensidad/arousal; capacidades adicionales inventariadas. Respeta dependencias reales en vez de forzar un orden artificial. Instala runtimes y modelos en el espacio V2 correspondiente, reutilizando lecturas seguras de recursos existentes cuando sea viable.

Comprueba toggles, preparación de carpetas, outputs, progreso, cancelación, checkpoints, resume y errores. Abre los resultados en las capas ya funcionales. Mantén reproducción mientras se analiza y prueba presión de recursos. Documenta disponibilidad opcional sin borrar capacidades anteriores del inventario.

Salida: flujo desde medio sin analizar hasta capas revisables y export, con resultados reales de modelos. Una interfaz con casillas y workers simulados no cierra E5. Si falta un recurso externo imprescindible, la capacidad queda bloqueada y el resto del producto sigue verificándose.

## E6. Estabilización y entrega

Resuelve defectos críticos, prueba durabilidad y concurrencia, completa revisión visual y mide rendimiento en release. Produce paquete Windows reproducible con dependencias, licencias, checksums, guía corta, diagnóstico y proyecto demostrativo pequeño. Prueba el paquete desde una ruta con espacios/Unicode y sin depender del directorio de desarrollo.

Valida en Windows limpio sin toolchain ni dependencias globales, si hay entorno disponible. Si no lo hay, distingue «probado en host» de «probado en Windows limpio» y registra la prueba pendiente; no falsifiques portabilidad. No hagas publicación externa como parte implícita del empaquetado.

Salida: rutas reales a ejecutable y paquete, build reproducible, matriz con evidencia, limitaciones y guía de inicio. Conserva la separación del editor básico y los paquetes de modelos.

## Requisitos y evidencia mínima

La matriz de implementación debe incluir estos IDs, además de los descubiertos en V1. Estados: `pendiente`, `implementado-sin-verificar`, `verificado`, `fallo`, `bloqueado`, `opcional-no-incluido`. Un requisito obligatorio no puede convertirse en opcional para cerrar una entrega.

| ID | Requisito | Evidencia de aceptación |
|---|---|---|
| UI-01 | Ventana nativa y paquete ejecutable | Abrir `.exe` release; documentar dependencias; sin navegador/WebView |
| UI-02 | Diseño y estados usables | Capturas y recorrido real: vacío, cargado, error, resize, paneles y 100/150/200% DPI |
| UI-03 | Foco, texto y atajos | Texto español/AltGr/IME, shortcuts no disparados al escribir, overrides persistidos y conflictos visibles |
| UI-04 | Paridad de acciones V1 | Cada acción inventariada enlazada a comando/UI/test o decisión explícita |
| MED-01 | Import/probe/relink | Video/audio/imagen, streams, offsets, rotación, paths Unicode; identidad comprobada al relink |
| MED-02 | Transporte | 1×/2×/3×/4×/8×, pausa, seek, loop, frame step y política de audio observable |
| MED-03 | Timeline y gestos | Mover entre tiempos/pistas, split, trim, copiar/pegar, multi, ripple, cancel drag, undo de un solo gesto |
| MED-04 | Composición y mezcla | Dos videos más imagen transparente y dos audios; coincidencia de capas, orden, mute/solo/gain entre visor y salida |
| MED-05 | Mapping temporal | Fuente/secuencia/repetición, VFR/B-frames, start time, audio offset y bordes sin deriva acumulada |
| LAY-01 | Contratos y estados | Fixtures aceptado/propuesto/desactivado, enabled independiente de accepted, borrados/tombstones y bloques sin aceptación |
| LAY-02 | Rango/jerarquía | Multirrango, puntos permitidos, padres/hijos, límites, IDs y no resurrección tras merge |
| LAY-03 | Montaje V1 | Superposición superior de video/audio, compactación y repetición preservadas; conversión V2 explícita |
| LAY-04 | Antes/después de modelos | Crear capas sin master inferido; importar master real conservando edición y procedencia |
| DAT-01 | Round-trip V1 | Comparación de contratos y canonicalización/digests con fixtures; sin campos ni IDs perdidos |
| DAT-02 | Durabilidad | Cierre/crash en fronteras de escritura, recuperación coherente y proyecto reabierto |
| DAT-03 | Edición externa | Cambio válido/incompleto/stale, diff, reconciliación y ausencia de lost update con GUI abierta |
| DAT-04 | Undo/redo | Gesto/batch/multidocumento; nueva revisión; conflicto detectado; export existente no borrado por undo |
| AI-01 | Comandos comunes | GUI y cliente externo provocan el mismo cambio de dominio con mismos invariantes |
| AI-02 | Dry-run y autorización | No cambia estado; apply valida base actual y permisos; diff coincide con resultado |
| AI-03 | Idempotencia/conflictos | Retry sin duplicación, IDs/rangos inválidos rechazados y stale con error estable |
| AI-04 | Estado en tiempo real | Cliente consulta capabilities/contexto; eventos actualizan UI y ciclo de propuestas |
| EXP-01 | Export exacto | Cortes fuera de keyframes, duración y primer/último frame; A/V dentro de tolerancia |
| EXP-02 | Matriz de formatos | Cada preset soportado produce salida inspeccionable; unsupported devuelve causa |
| EXP-03 | Jobs y fallos | Cancelar export, fallo de encoder/destino, edición concurrente; snapshot fijo y publicación válida |
| ML-01 | Inferencia real | Entradas/outputs/versiones registradas; no mocks; contratos compatibles |
| ML-02 | Etapas/resume | Selección de pasos/dependencias, invalidación selectiva, checkpoints y no repetición innecesaria |
| ML-03 | Supervisión | Error/OOM/cancelación, stderr separado, backpressure y ausencia de procesos huérfanos |
| PERF-01 | Fluidez y recursos | Benchmark release con hardware/dataset, latencias y RAM/VRAM; límites declarados |
| OPS-01 | Diagnóstico | Error estable con causa/acción, correlación, logs acotados, consola limpia y secretos redactados |
| PKG-01 | Distribución | Build desde lock, paquete con dependencias/avisos y prueba fuera del workspace; estado Windows limpio explícito |

Cada fila enlaza implementación/símbolo, test automatizado o procedimiento manual, resultado y artefacto. «Test pasa» sin comando, build y caso identificables no es evidencia suficiente.

## Escenarios integrados obligatorios

1. **Editor sin ML:** iniciar paquete básico, importar video y audio más PNG transparente, componer, cortar fuera de keyframe, mover, undo/redo, guardar/reabrir y exportar dos resoluciones. Comparar visor y salida.
2. **Revisión editorial:** importar proyecto V1 con recortes propuestos, aceptados y desactivados. E marca revisión; X desactiva; P activa. Verificar que export corta todos los habilitados, independientemente de aceptación. Intentar aceptar un bloque: acción no disponible, sin mutación.
3. **Montaje heredado:** dos pistas solapadas con audios distintos, un hueco y un fragmento repetido. Comparar resultado V1 de referencia con V2, revelar en fuente y volver a cada ocurrencia.
4. **Edición por AI y persona:** agente lee revisión R, persona edita a R+1, propuesta R falla con diff/conflicto. Nueva propuesta sobre R+1 aplica; repetir idempotency key no duplica; undo GUI funciona.
5. **Archivo externo:** modificar una copia JSON mientras la UI está abierta, incluyendo una escritura temporal incompleta. Validar reconciliación y conservar decisiones humanas. Un tombstone impide resurrección por merge.
6. **Carga y transporte:** timeline denso, scrub alternando destinos, reproducción 2× y cambio a skim 8×. No aparece frame de un seek viejo tras confirmar uno nuevo. Pausar y editar debe responder bajo análisis/export en background.
7. **Pipeline completo:** medio sin resultados, etapas elegidas, outputs reales, generación de capas, revisión humana, propuesta AI, export. Interrumpir una etapa y comprobar resume por digest sin repetir las ya válidas.
8. **Durabilidad y paquete:** simular error de escritura mediante inyección controlada o volumen de prueba limitado, terminar proceso durante publicación y reabrir. No llenes el disco real del usuario ni mates procesos ajenos. Abrir el paquete desde otra carpeta con configuración limpia aislada.

## Estrategia de pruebas

Prueba reglas puras con tests unitarios y de propiedades: mapping, intervalos, colisiones, jerarquía, operaciones inversas, revisión e idempotencia. Los tests de integración comprueban JSON/SQLite, IPC, procesos y recuperación. La UI se inspecciona con herramientas nativas disponibles o ejecución manual documentada; una captura sola no demuestra drag, foco o playback.

Usa fixtures sintéticas con números de frame/timecode, tonos/clicks y colores conocidos para medir render y sync. Añade medios representativos VFR, B-frames, audio separado y offsets. Los golden tests deben contrastar contratos/salidas esperadas independientemente de la implementación nueva. No generes los «esperados» llamando a la misma función que estás probando.

Para V1 usa módulos puros en una copia aislada con `PYTHONDONTWRITEBYTECODE=1`, configuración y temporales redirigidos. Revisa efectos antes de importar módulos. No des por vigente la cifra histórica de tests ni su estado anterior; registra comando/runtime/resultados actuales. Ejecuta `cargo fmt --check`, `cargo check`, tests y clippy apropiados al workspace implementado, distinguiendo advertencias previas de las nuevas.

La igualdad preview/export se evalúa por resolución temporal y contenido con tolerancias fijadas antes de medir: fronteras de corte dentro del fotograma de salida que corresponda, audio considerando sample rate/delay y color según conversiones soportadas. No exijas hashes idénticos entre decoders y encoders con pérdida. Documenta medición de drift y ausencia de frames/sonidos ajenos al rango.

## Presupuestos de rendimiento y medición

Registra CPU, GPU, driver, RAM/VRAM, almacenamiento, resolución/DPI, build release y codecs. Separa caché fría/caliente y usa suficientes repeticiones para informar percentiles con sentido; muestra tamaño de muestra. No compares un build Rust release con Python cargando modelos por primera vez y atribuyas la diferencia al lenguaje.

Objetivos iniciales de producto, sujetos a baseline y hardware documentados:

| Medición | Objetivo inicial y condición |
|---|---|
| UI durante navegación | 60 Hz; presupuesto de frame P95 ≤16,7 ms en escenario de referencia, medir CPU y presentación |
| Respuesta visible a input | P95 ≤50 ms sin bloqueo prolongado por media/IO |
| Timeline virtualizado | 10 000 items como caso obligatorio; 100 000 como estrés con LOD, sin crecimiento de coste por todos los items fuera de vista |
| Apertura del editor básico | Objetivo ≤3 s hasta interacción en host de referencia, sin carga de modelos |
| Seek caliente en 1080p/proxy | Objetivo P95 ≤250 ms; medir seek exacto frío por separado y comunicar latencia mayor |
| Memoria y estabilidad | RAM/VRAM/colas acotadas; sesiones repetidas de abrir/cerrar/seek no muestran crecimiento sostenido sin causa |
| Reproducción bajo carga | 2× con sync medido; export/inferencia se regulan antes de bloquear interacción |
| Reposo | Repintado por demanda y uso observado bajo; sin bucle activo continuo innecesario |

Estos números no son promesas universales ni resultados ya medidos. Antes de cerrar PERF-01, fija perfil de referencia y tolerancias que puedan reproducirse. Si un objetivo falla, registra causa y corrige; cualquier ajuste requiere justificación visible, sin ocultar la cifra original. Registra tamaños de ejecutable, editor empaquetado, modelos, runtime y cachés por separado; establece presupuesto de tamaño después del primer bundle real.

## Revisión visual y de uso

Comprueba a 1280×720 y una resolución de laptop mayor, con DPI 100/150/200%: texto legible, toolbar accesible, paneles redimensionables, foco visible, estado de selección claro, handles usables y ausencia de solapamientos de controles. Prueba teclado, mouse y textos largos en español. Conserva screenshots del ejecutable con fixture conocida, no mockups presentados como producto.

Recorre tareas habituales sin consola de desarrollador: importar, encontrar tramo, crear rango, aceptar/desactivar, mover clip, configurar atajo, comparar propuesta, exportar y recuperar un error. Comprueba navegación por teclado, IME y accesibilidad disponible. Registra pruebas que requieren entorno adicional; no equipares una dependencia AccessKit instalada con accesibilidad comprobada.

## Registro de decisiones y continuación

En `implementation/STATUS.md` mantén: fecha/build; E0–E6; requisito activo; verificado en qué entorno; errores abiertos; cambios propios/ajenos; siguiente comando o acción; dependencias pendientes. En `implementation/requirements-matrix.md` conserva estado y evidencia por ID. En `implementation/decisions.md` registra decisiones pequeñas y enlaza ADRs para cambios de GUI/motor/persistencia/compatibilidad.

Cada cierre de entrega resume qué funciona, qué se comprobó y qué sigue. No solicites permiso de nuevo para continuar dentro de esta secuencia autorizada. Si se interrumpe la sesión, deja un checkpoint recuperable; una interrupción no convierte requisitos incompletos en completados.

## Informe de entrega final

Incluye rutas al `.exe`, paquete, guía, proyecto demo y evidencia. Indica etapas completas, pruebas reales, métricas, compatibilidad V1, formatos soportados, modelos disponibles y limitaciones. Enumera cualquier requisito obligatorio bloqueado: en ese caso la entrega global sigue incompleta aunque exista una versión utilizable.

Confirma el alcance de archivos modificados por ti y el estado observado de V1, sin atribuirte ni revertir cambios concurrentes de terceros. No incluyas clones, cachés de build o modelos innecesarios en el paquete básico. El usuario debe poder abrir el programa y reproducir por sí mismo el recorrido editorial documentado.
