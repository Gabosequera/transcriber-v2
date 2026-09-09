# Guía vigente — integración E2/E3 posterior a alpha.8

Fuentes revisadas el 2026-09-09. **Estos recorridos GUI, multimedia, NLE y de rendimiento están preparados para aceptación posterior; no se ejecutan en esta continuación.** No ejecutar V1, modelos ni los tests de dominio/V1compat/desktop bloqueados por Windows 4551, ni copiar sus binarios a otra ubicación para eludirlo. Los apartados históricos inferiores no autorizan esas ejecuciones.

Desde la raíz del checkout, los controles de compilación usan el entorno MSVC del wrapper:

```powershell
./scripts/cargo.ps1 check --workspace --all-targets --locked
./scripts/cargo.ps1 clippy --workspace --all-targets --locked '--' -D warnings
./scripts/cargo.ps1 fmt --all --check
```

`--all-targets` compila los tests; no los ejecuta. Los tests permitidos de application y sus resultados se registran por separado en STATUS/evidence. No afirmar que estas órdenes se han ejecutado sobre cambios posteriores al último log. El inventario funcional vigente es [v1-action-inventory.md](v1-action-inventory.md).

## Preparación de la aceptación futura

Usar una sesión PowerShell nueva y una carpeta de trabajo V2 nueva, separada del checkout V1 y de cualquier proyecto personal. Configurar `TRANSCRIPTOR_CONFIG_DIR` dentro de esa carpeta antes de abrir el ejecutable. Preparar fixtures sintéticas con: video CFR, uno y dos streams de audio distinguibles, palabras/utterances en dos pistas, conversación con duplicados marcados, notas con jerarquía/multirrango/tombstones, recortes humanos/AI, bloques contiguos y montaje V1 con clips visibles/cubiertos/desactivados. Registrar los SHA-256 iniciales de todos los archivos de entrada. Los casos de VFR, datos incompletos y rangos fuera de fotograma se guardan como fixtures separadas.

Por recorrido registrar: revisión/commit del ejecutable, OS/herramientas, fixture y hashes, pasos, resultado esperado/observado, errores, revisión/IDs/digests antes/después y evidencia de undo/reapertura. Las mediciones de latencia deben incluir número de palabras/items/clips, duración y tamaño de proyecto; compilar un test grande no es medir el frame time del editor. No ejecutar generación multimedia ni abrir el ejecutable hasta reanudarse explícitamente esa aceptación.

## Recorridos y resultados esperados

1. **Carpeta completa y escala.** Importar la fixture V1, guardar y reabrir; exportar Archivo → Exportar carpeta documental V1 → Capas del medio seleccionado. Incluir un JSON mayor de 64 KiB, auxiliares que superen 32 MiB en total y directorios vacíos. La caché textual de 32 MiB no debe rechazar la carpeta completa; originales grandes quedan referenciados/verificados por streaming. Comparar hashes de fuentes antes/después y de archivos conservados en destino. Editar/mover un auxiliar original entre import y export debe provocar rechazo por identidad/bytes, no adoptar silenciosamente el contenido nuevo. Repetir Save As, retirar la copia fuente de la fixture y reabrir el destino; los objetos/bundles trasladados deben bastar. Una referencia externa cuyo hash o descriptor fue alterado debe fallar en apertura/recuperación.

2. **Conversación.** Abrir Archivo → Conversación y evidencia (también acción `view.conversation` en la paleta). Alternar Conversación limpia / Todas las intervenciones / Palabras; filtrar por texto/ID, pista y límites fuente en segundos. Comprobar orden por tiempo entre pistas, IDs originales y que la selección limpia coincide con el master. Más de 64 resultados debe habilitar Siguiente/Anterior sin omitir ni repetir filas. Un texto de más de 4000 caracteres se abrevia en pantalla, pero una coincidencia posterior a ese límite sigue encontrándose. Cambiar búsqueda/medio/revisión durante la consulta no debe presentar el resultado anterior como actual; cerrar cancela el trabajo.

3. **Navegación de evidencia.** Pulsar el intervalo de una palabra/intervención: Fuente debe seleccionar ese medio, pausar, establecer IN/OUT y buscar `t_ini` sin sumar el t0 de nuevo. Colocar el mismo rango tres veces en la secuencia y añadir audio independiente del mismo medio. «En secuencia…» debe mostrar cada mapping independiente; A/V enlazados con intervalos idénticos aparecen una vez. Preparar más de 64 apariciones para comprobar cursor/total. El final exclusivo del clip no pertenece a su ocurrencia. Cambiar la secuencia mientras el worker consulta invalida la lista y requiere volver a abrirla.

4. **Bloques y gestos.** Editar un bloque con F2/Enter: título, comentario, confianza 0..1 y bordes. Primer inicio/último final quedan fijos, vecinos mantienen continuidad y Aplicar prepara el snap seguro contra la evidencia actual. Un borde sin solución segura o una revisión nueva debe rechazar el conjunto. Exportar bloques y comprobar selected/view, master derivado y archivos por bloque, sin mutar el master de sesión. Para clips/items: arrastrar, recortar y caja Corte cerca de bordes del viewport; el preview geométrico y el autoscroll deben conservar la intención temporal. Escape no cambia el proyecto; release produce una sola operación undo. Repetir con índices en construcción y confirmar que una base antigua no aplica un gesto.

5. **Decisiones del montaje.** Importar montaje V1; editar motivo y estado desde inspector y E/P/X; copiar/repetir una pieza enlazada. Revisar que todos sus twins conservan reason/state/edited y que undo restaura ambos. Exportar JSON V1 compatible y reimportar: originales cubiertos/desactivados siguen archivados, piezas editadas tienen nuevos IDs y mapping, y el motivo/aceptación se conserva. Desactivar produce `state: disabled`. Audio y video con decisiones o tiempos independientes deben obtener rechazo explícito de la inversa V1.

6. **Capas editoriales y adopción legacy.** Abrir Pedidos y respuestas editoriales JSON, elegir Capas editoriales y Crear pedido en carpeta nueva. Elaborar una respuesta sintética `editorial-layers-proposal/1` copiando literalmente `request_id`, `source_master_digest` y `source_layers_digest` del request; incluir `layer`/`layers` de tipo `user`, `topics` o `ai`, IDs nuevos, rangos válidos, `state: proposed` y `edited: false`. Cargarla, revisar diff y aplicar. Reintentar el mismo contenido no duplica la edición; guardar/reabrir y undo conservan el historial. Alterar cualquiera de los digests o la revisión del proyecto exige un nuevo ciclo. Proponer cambios sobre items humanos/aceptados/desactivados/tombstones no los sustituye. Capas de evidencia/bloques/recortes/autor y marcadores internos de evidencia/rutas deben rechazarse por esta vía. Para la variante V1 genérica, omitir únicamente request_id: la carga debe pedir vinculación explícita al request mostrado, conservar el original y su digest canónico y guardar por separado la respuesta adoptada. Un request_id diferente nunca se reasocia. Que el adaptador admita estos tipos no concede por sí solo permisos de escritura al control MCP.

7. **Temas en dos pasadas.** Crear pedido Temas. La primera respuesta lleva `pass: 1`, `complete: true` e items; Confirmar primera pasada publica `views/topics-pass1.json` validado y actualiza `views/topics-request.json`, sin cambiar la revisión editorial. Leer el nuevo `previous_pass_digest`; no reutilizar el digest de la respuesta cruda. La segunda respuesta lleva `pass: 2`, ese digest y `source_item_ids`: cada ID de la primera pasada debe aparecer exactamente una vez y sus rangos deben conservarse por unión, incluidos temas recurrentes multirrango. Omitir, duplicar, inventar IDs o cambiar un rango debe fallar antes del commit. Aplicar la segunda crea la capa y permite undo.

8. **Recortes AI.** Crear el pedido correspondiente y copiar además `trims_digest`, modo/carril del contrato. Proponer cortes nuevos junto a cortes humanos/aceptados/desactivados ya existentes. Validar ajuste de bordes de palabras/risas, mínimo temporal y rechazo fuera del medio; solo los nuevos AI se coalescen. La publicación conserva todos los carriles del documento. Una propuesta para otro carril/digest se rechaza sin edición parcial.

9. **Propuesta de montaje.** Crear pedido Montaje con la secuencia compatible activa. Responder con pass/montage_digest correctos, clips ordenados, referencias reales y keep de un clip existente. Una repetición superpuesta de fuente requiere repeat:true y motivo. Comprobar las advertencias editoriales de clips cortos/largos, falta de motivo y duración fuera de target/tolerancia; no confundirlas con un fallo de integridad ni con garantía de duración renderizada. Aplicar crea una secuencia nueva conservando la antecesora y los clips protegidos. Cambiar el montage_digest o inventar una referencia provoca rechazo.

10. **Watchers de documentos individuales.** Vincular copias aisladas de layer/trims/bloques/autor/montaje. La primera lectura muestra la adopción inicial; aceptar guarda la base por path en configuración V2. Hacer una edición local y otra externa independiente: el diff debe conservar ambas. En conflicto elegir explícitamente por campo; autorizar decisiones humanas no permite perder master ni tombstones. Hacer retroceder revisión, escribir JSON parcial o cambiar el archivo tras revisar: no aplicar. Otro documento válido debe seguir detectándose. Posponer afecta a path+digest; guardar/reabrir conserva la última base aceptada. Para montaje, la secuencia queda vinculada al iniciar la vigilancia y los IDs se mantienen en lecturas posteriores. No se usa para sustituir una secuencia con otros medios.

11. **Watcher de contratos.** Vincular un request/proposal de cada tipo. Abrir desde el aviso debe cargar su contrato real, tipo y preview en la revisión editorial, copiando a una carpeta nueva `config/editorial-reviews/review-*`. Para propuesta, preparar junto a ella `<tipo>-request.json` y, en segunda pasada de temas, `topics-pass1.json`. Modificar alguno durante captura obliga a repetir. Verificar que ningún recibo/pass se escribe en la carpeta fuente vigilada. Una respuesta genérica legacy requiere la adopción explícita del recorrido 6.

12. **Intercambio EDL/FCPXML.** Usar Archivo → Exportar intercambio de montaje. Una secuencia CFR contigua de un solo medio con A/V completos debe generar `montage.edl` o `montage.fcpxml` en carpeta nueva, con fuente original resuelta. En CMX3600 comprobar NDF, ≤999 eventos, <24 h y máximo un stream mono/estéreo; para varios streams usar FCPXML 1.9. Comparar mapping de rangos/repeticiones antes de abrir en un NLE. Fuente VFR, bordes fuera de frame, rate distinto, gain/efectos, audio independiente, huecos/overlays no representables o streams omitidos deben fallar sin publicar un documento parcial. La apertura en NLE y su conformado quedan dentro de la aceptación aplazada, no se deducen de XML bien formado.

13. **Derivación padre/hijo desde export real.** En una fixture autorizada, exportar una selección con recortes, reordenación y repetición. Esperar job Succeeded y recibo; cambiar luego la edición local para comprobar que Trabajos de exportación guardados → Derivar carpeta hija usa timeline/rango/evidencia congelados del job. Seleccionar el padre y destino nuevo. Verificar copia `media/export.*`, SHA del recibo, master hijo, `archive/export-receipt.json`, master parental intacto y evidencia parental proyectada. El export actual mezcla: el hijo debe declarar un solo stream `mix` con análisis propios vacíos, o Silent si no hubo audio; nunca declarar conservados los dos originales porque existan en el padre. Capas trasladadas mantienen IDs, decisiones, jerarquía/tombstones y procedencia por segmento. Un job antiguo sin snapshot editorial solo puede usar el proyecto si coinciden exactamente identidad/revisión; de otro modo exige export nueva. Fuente múltiple, gaps/mappings simultáneos incompatibles, tiempos V1 submilisegundo o recibo/SHA alterados se rechazan.

14. **Recuperación documental.** Interrumpir únicamente una publicación sobre fixture propiedad de V2 y usar Archivo → Recuperar exportación V1 interrumpida. Las intenciones /1, /2 y /3 conservan compatibilidad; /3 incluye directorios vacíos. Base o destino esperados permiten completar; un tercer contenido se conserva y produce conflicto. Para propuestas, un fallo de publicación después del commit debe ofrecer reintentar recibo sin ejecutar otra vez el comando. El recibo diferencia `applied-in-session`/`pass-validated` de un proyecto posteriormente guardado. Verificar manifests por tamaño/SHA de bytes y digest canónico del manifest; cambiar solo espacios/saltos de línea en un output invalida su hash de bytes. No eliminar manualmente intenciones ni sustituir fuentes para hacer pasar recuperación.

No resalvar proyectos con referencias de evidencia/capas ni bundles nuevos mediante lectores antiguos: sus esquemas de almacenamiento requieren la versión que los creó o una posterior compatible. Las referencias grandes y SharedVec reducen duplicación; el master JSON residente, los índices, los clips y la serialización de documentos derivados conservan costes proporcionales a sus datos. Mantener DAT/PERF y la aceptación física abiertos hasta medir/verificar sus criterios, aunque las implementaciones y pruebas permitidas compilen.

---

# Histórico — continuación 11

**No ejecutar ahora GUI, medios reales ni revisión general.** Usar scripts/cargo.ps1; application tests permitidos; V1compat/dominio/desktop bloqueados 4551 no se reintentan ni se reubican.

Para aceptación posterior en fixtures/copias aisladas V2:

1. Importar carpeta editorial con master único, layers/trims/bloques/montaje, documentos desconocidos, manifests y auxiliar binario. Guardar/reabrir; comprobar que la fuente no cambia y el bundle persiste.
2. Editar notas/bloques, Archivo → Exportar carpeta documental V1 → capas. Revisar nueva carpeta, notas moments existentes, vistas regeneradas, originales bajo .work/tv2-original y reporte .work/tv2-exports. No se copia el medio principal, sí auxiliares. Comprobar bytes/SHA de originales y binarios.
3. Repetir con montaje activo: mismo medio, perfil compatible, original desactivado y mapping editado. Reimportar la carpeta en proyecto nuevo, conservando archivo original sin anidar copias.
4. Cambiar/mover un auxiliar después de importar: exportación debe rechazarlo antes de publicar datos. No sustituirlo por el nuevo archivo bajo el mismo nombre.
5. Interrumpir publicación /2 y usar Archivo → Recuperar exportación V1 interrumpida. Con origen disponible termina; con destino ya completo no necesita recopiado. Con una tercera edición externa rechaza sin sobrescribirla.
6. Confirmar manifest válido por SHA y tamaño y archivo de los obsoletos; requests/passes preservados no equivalen a validados/aplicados.

Límites actuales en evidence/continuacion-11.md. No resalvar nuevos bundles con alpha.7 o anteriores; no recuperan intents /2. Proyectos anteriores sin snapshot no tienen exportación completa de carpeta. Binarios dependen del origen hasta exportarlos; traslado por Save As pendiente.

---

# Histórico — continuación 10

Fuentes alpha.7. Controles permitidos: wrapper `scripts/cargo.ps1`; application tests, check, Clippy/all-targets y formato. **No ejecutar ni reintentar tests V1compat, dominio o desktop bloqueados por Windows 4551.** Las instrucciones históricas de tests desktop debajo no son autorización vigente para ejecutarlos.

- Marcas: importar un medio de fixture aislada, Archivo → Buscar y resolver marcas del autor. Descubrimiento junto al medio y stores V1. `TRANSCRIPTOR_V1_MARKS_STORE` elige el store directamente; `TRANSCRIPTOR_ROOT`/`TRANSCRIPTOR_SHARED_DIR` replican instalación administrada V1; `TRANSCRIPTOR_V1_DIR` sirve para un checkout V1 de solo lectura. Sin override se busca un hermano `transcriber/marcas_store` relativo al ejecutable. Para aceptación, usar siempre un store sintético dentro de V2. Preparar dos documentos con la misma identidad, revisión igual/contenido distinto; comprobar elección requerida, datos actuales/candidato, adopción/undo. Cambiar el archivo durante la revisión y comprobar rechazo por digest. No reasociar identidad ajena automáticamente.
- Caja/arrastre semántico: en Fuente y Secuencia, acercar cursor a ambos bordes mientras B+arrastre o Ctrl+arrastre; verificar que el origen temporal permanece fijo, Escape cancela y release genera un solo undo. En Secuencia caja sigue limitada a una ocurrencia; usar Fuente al cruzar saltos.
- Montaje: importar fixture V1, recortar/mover/repetir piezas con A/V enlazado manteniendo secuencia contigua; Archivo → Exportar montaje a JSON V1. Verificar clips originales desactivados con estado anterior, nuevos IDs/procedencia y mapping visible. Volver a importar y comparar tiempos; comparar medios solo cuando se autorice. Efectos, audio independiente, overlays, gaps y submilisegundo deben mostrar rechazo explícito.
- Reconciliación: trabajar sobre `.transcriptor` sintético. Editar project.json desde otro editor con cambios independientes y dos campos divergentes. El watcher debe despertar lectura estable; comprobar diff de campos y una elección por conflicto. Sin autorizar decisiones humanas, sus modificaciones se rechazan. Autorizarlas no permite perder tombstones/master. Modificar la base local o disco durante preparación debe conservar ambas versiones. Aplicar y deshacer, después guardar/reabrir.
- Jobs: con `TRANSCRIPTOR_CONFIG_DIR` aislado, encolar varias exportaciones, cerrar y reabrir; Archivo → Trabajos de exportación guardados. Queued/Interrupted requieren «Reanudar esta solicitud»; no se lanzan automáticamente. Comprobar fingerprint cambiado, worker vivo en otra instancia, cancelación, fallo y recibo con intentos. Si la salida ya existe se conserva: no borrar ni sobrescribir para simular recuperación. Los jobs viven en `config/export-jobs`, no acompañan todavía Save As.
- Historial: guardar varias ediciones, deshacer dos, autosave, cerrar/reabrir/recuperar. Debe conservar ambas pilas con nueva revisión y digests. `history/2` se lee por alpha.7; alpha.6 solo entiende /1. No intentar downgrade sobre la misma carpeta como procedimiento de aceptación.

Estos procedimientos no se ejecutaron. Evidencia automatizada: `evidence/continuacion-10.md`. E4 pendiente; no hay cliente MCP que probar todavía.

---

# Continuación 9 — nuevas operaciones (aceptación física aplazada)

- B y arrastrar en carril semántico: crear/estirar/unir. Shift resta; Ctrl mueve un item. Handles recortan, S divide. Escape cancela. Caja solo items de un rango, como V1; para cajas que cruzan clips usa Fuente.
- Cabecera de recortes → Unir solapados / Mover selección a… / Quitar carril… (conservando en destino o borrando con undo). main/ai no se quitan.
- Archivo → Importar marcas del autor V1 permite elegir un sidecar del medio seleccionado, aun sin master. Valida identidad; empate de candidatos de carpeta con distinta información requiere elegir archivo. Exportar su capa genera autor.marcas.json. El original V1 no se escribe.
- Inspector de bloques ajusta los vecinos de ambos bordes. Cabecera → Ajustar bordes seguros (radio 15 s), en worker. Si cambia la base no aplica. Exportar bloques materializa un conjunto V1 nuevo (master derivado, selected/view, Markdown, transcript y señales por bloque).
- Archivo → Recuperar exportación V1 interrumpida reanuda una carpeta creada por V2 con intención pendiente; rechaza terceros contenidos/rutas ajenas. No borres .pending-documents.json manualmente.
- Recuperar autosave conserva sus undo/redo completos. Con historia disponible, undo recorre ediciones originales del candidato; legacy sin historia tiene un paso hacia lo guardado.

Controles exactos: evidence/continuacion-09.md. No ejecutar GUI/multimedia/aceptación física aplazada ni eludir Windows 4551.


---

# Continuación 8 — recorridos añadidos

- Selecciona un item y usa S, [ / ], Alt+flechas; el playhead se convierte a tiempo fuente. División jerárquica conserva los demás rangos. Trim que deje hijos fuera falla entero.
- Arrastra un item o su borde: guía de tiempo, una confirmación al soltar; Escape cancela. Una revisión concurrente obliga a repetir el gesto. La herramienta Corte también divide items y clips enlazados.
- X sobre marca cicla nota/incluir/excluir; una decisión convierte un punto a región ±2 s. E acepta; P vuelve a nota. Regiones con decisión necesitan 200 ms.
- Biblioteca/cabecera: subir/bajar carril; seleccionar limpia la selección previa. Nueva capa permite notas/pedidos, recortes, temas y marcas. Bloquear/visibilidad conservados.
- Archivo → exportar capa V1: sobre recortes exporta **todos los carriles del documento** en `trims.json`; sobre bloques exporta `chunks.selected.json`. Se crea carpeta de salida nueva; no escribe V1. Planes editados con evidencia/snap antiguo se rechazan hasta recalcular. No es export carpeta completa ni inverso de montaje editado.
- Abrir y guardar trabajan en segundo plano con un slot. Se puede seguir editando al guardar: solo la revisión capturada queda guardada. Guardar y salir espera escritura exitosa. El guion espera el resultado del worker.
- Reapertura recupera undo/redo desde `history.json`. Save As relocaliza todas las rutas históricas. Versiones antiguas sin history inician pila vacía. Si history existe pero es incoherente se informa error, no se restaura ciegamente.
- Autosave ajeno más nuevo provoca conflicto; conserva ambos estados. Recuperar o guardar manualmente establece la nueva base. Recovery sigue ofreciendo undo a la versión guardada, pero no repone todavía toda la pila anterior del candidato.

No ejecutar pruebas físicas/GUI/multimedia aplazadas. No eludir Windows 4551. Controles finales y límites en STATUS/evidence/continuacion-08.md.

---
# Continuación 7 — nuevas operaciones

- Archivo → Importar V1: lectura/probe en worker; Biblioteca permite cancelar. Cambiar proyecto o revisión impide aplicar un resultado viejo. Una carpeta reconocida inválida no modifica el proyecto.
- Inspector o F2/Enter sobre item → editar texto, comentario, rangos fuente y padre. Borrador persistente; Aplicar confirma un batch. Rango por fila; padre vacío convierte en raíz. Error de base antigua conserva borrador para consulta, exige reabrirlo.
- Copiar/cortar/pegar/duplicar items incluye descendientes y conserva multirrango. Raíces copiadas no conservan padres externos al grupo. Pegado exige capa del mismo medio. Clips conservan efectos, ganancia y enlace con IDs nuevos. Colisión rechaza toda la operación.
- Ajustes → Atajos → Importar/Exportar keymap/1/Restaurar todos. Un archivo inválido no cambia ni guarda ajustes parciales.
- Archivo → Exportar capa seleccionada a JSON V1 / Exportar montaje a JSON V1. Crea una carpeta nueva dentro del destino elegido, con **un documento**. Solo user/topics/ai y montaje importado intacto; otros casos muestran error explícito. Esto no exporta una carpeta de proyecto V1 completa.
- Autosave/1 de proyectos guardados incluye auditoría y recibos; legacy sigue legible. Escritura en worker y recuperación explícita con undo. Guardar manual sigue síncrono.

Controles y límites exactos en STATUS y evidence/continuacion-07.md. Pruebas físicas y generales aplazadas. No eludir Windows 4551.

---

## Instrucciones base e histórico

# Desarrollo y ejecución portable

Ejecutar los comandos desde la raíz del checkout, sea cual sea su nombre o unidad. Las rutas `G:/TODO` de `implementation/evidence/` describen el equipo anterior y no deben sustituirse masivamente: son evidencia histórica.

## Herramientas Windows

- Rust 1.98.1, fijado en `rust-toolchain.toml`, con rustfmt y clippy.
- Visual Studio con herramientas C++ x64 y Windows SDK.
- Python para preparar guiones/fixtures y empaquetar, no para ejecutar el editor.
- FFmpeg y FFprobe: `TRANSCRIPTOR_FFMPEG_DIR` → copia local de desarrollo → PATH para herramientas Python. La aplicación también busca junto a su ejecutable.

El nuevo host dispone de Rust 1.98.1, Visual Studio 18 Community y Windows SDK 10.0.26100. Rust no descubrió MSVC automáticamente. Usar el wrapper, que encuentra Visual Studio con `vswhere` y activa su entorno x64:

```powershell
./scripts/cargo.ps1 check --workspace --locked
./scripts/cargo.ps1 test -p tv2-application --locked
./scripts/cargo.ps1 test -p transcriptor keymap::tests --locked
./scripts/cargo.ps1 fmt --all --check
./scripts/cargo.ps1 clippy --workspace --all-targets --locked '--' -D warnings
./scripts/cargo.ps1 build --release --locked -p transcriptor
```

El wrapper usa `CARGO_HOME` si existe, o la instalación Rust del usuario. No cambia la configuración global del compilador. Para empaquetar, abrir una Developer PowerShell x64 y ejecutar `python packaging/build_windows.py --public-release`; el builder requiere exe y fixtures, y no es parte del checkpoint de fuentes.

## Abrir el editor

```powershell
& ./target/release/Transcriptor.exe
& ./target/release/Transcriptor.exe '<carpeta>.transcriptor/project.json'
```

Atajos: Ctrl+I importar, espacio reproducir/pausar, S dividir, Ctrl+N capa, I/O rango, Ctrl+Enter añadir tramo, Ctrl+S guardar, Ctrl+O abrir, Ctrl+E exportar. Ajustes/Atajos permite editar combinaciones separadas por `;`, capturar una tecla, dejar una acción sin atajo y restaurar defaults; los conflictos se rechazan antes de guardar.

Al abrir un proyecto con autosave más reciente se ofrece recuperar con Deshacer. Los proyectos aún sin carpeta se guardan automáticamente en configuración/recovery; al arrancar se ofrecen candidatos y al recuperar se pide destino normal en el primer Guardar. No se borran originales ni archivos V1.

Un lector de respaldo comprueba project.json cada 2 s en segundo plano. Cuando hay una versión válida estable, combina cambios independientes y muestra resumen del diff para Aplicar o Posponer. Rechaza conflictos de campos/orden, decisiones humanas y evidencia protegida; guardar no pisa la versión externa. Aplicar revalida ambos lados y admite Deshacer. Todavía faltan watcher del SO y reconciliación de documentos V1 individuales.

Guardar confirma snapshot/auditoría mediante .pending-commit.json. Si se interrumpe, abrir completa la intención si el disco sigue en la base o destino esperado. Si hay un tercer contenido, se informa conflicto y se conserva todo. No borrar manualmente una intención pendiente. Atomicidad multidocumento V1 y archivado de auditoría siguen pendientes.

Shift+T activa el salto de recortes durante revisión; no modifica export. Ctrl+Shift+A añade los rangos de un item/IN-OUT, sin incluir huecos de un multirrango. La acción «Añadir tema completo» utiliza el tema raíz. Inspector → Ocurrencias permite ir a cada repetición. Masters importados y sus proyecciones son evidencia de solo lectura.

## Guiones y fixtures (ejecución real aplazada)

Los JSON bajo `tests/scripts/` son **plantillas**, no se pasan directamente al ejecutable. `${WORKSPACE}` se resuelve a este checkout; `${OUTPUT}` a una carpeta nueva. La preparación no ejecuta la aplicación y no equivale a una prueba aceptada.

```powershell
python tests/scripts/prepare_regression.py
python tests/scripts/prepare_selection.py
python tests/scripts/prepare_script.py tests/scripts/e2-transporte.json
```

Regresión E1/E2: el primer comando escribe destinos nuevos y actualiza `implementation/evidence/e2/latest-regression.json`. No ejecutar el puntero histórico heredado: apunta a otro equipo. Para ejecutar una preparación nueva en la siguiente iteración:

```powershell
$runs = Get-Content implementation/evidence/e2/latest-regression.json -Raw | ConvertFrom-Json
$runPath = $runs.'e1-recorrido'
$runRoot = Split-Path $runPath
$env:TRANSCRIPTOR_CONFIG_DIR = Join-Path $runRoot 'config'
$env:TRANSCRIPTOR_CACHE_DIR = Join-Path $runRoot 'cache'
$env:TRANSCRIPTOR_LOGS_DIR = Join-Path $runRoot 'logs'
$exe = (Resolve-Path ./target/release/Transcriptor.exe).Path
$p = Start-Process -FilePath $exe -ArgumentList @('--script', ('"' + $runPath + '"')) -WorkingDirectory $env:TEMP -WindowStyle Hidden -PassThru
$p.WaitForExit()
python tests/scripts/verify_export.py (Join-Path $runRoot 'export-e1-720p.mp4') $runs.'e1-expect'
```

Repetir con `e2-escenario1` y verificar sus exports 720/1080p. Exigir `fallos=false`, verificador OK y revisión real de las capturas. Las pruebas de DPI/foco/audio físico siguen pendientes; --script no las sustituye.

Los medios no están en Git. Para generar los que faltan:

```powershell
python tests/fixtures/media/make_fixtures.py
python tests/fixtures/v1/make_v1_fixture.py ../transcriber
```

El primer script conserva archivos existentes, admite `TRANSCRIPTOR_TEST_FONT` y genera fixtures base además de VFR/rotación/offsets. Sus nuevas codificaciones pueden cambiar el fingerprint respecto al golden histórico. Por eso la regeneración de goldens es un paso **explícito** con código V1 aislado: nunca ajustar expectativas usando V2 ni presentar esa regeneración como comparación con los bytes originales. `TRANSCRIPTOR_V1_DIR` permite elegir otro checkout V1. No se ejecutaron estos generadores de medios en continuación 5.

## Datos y límites

Configuración: `%APPDATA%/Transcriptor`; logs: `%LOCALAPPDATA%/Transcriptor/logs`. Variables de pruebas: `TRANSCRIPTOR_CONFIG_DIR`, `TRANSCRIPTOR_CACHE_DIR`, `TRANSCRIPTOR_LOGS_DIR`. La memoria de eframe es independiente. Sesión/historial conservan rutas absolutas; los snapshots guardados relativizan medios respecto al padre de `.transcriptor`.

Ver `STATUS.md` para el alcance implementado y `evidence/continuacion-05.md` para controles exactos. Testing real, suite multimedia y revisión general se aplazaron por instrucción del usuario.

## Ampliación de aceptación: composición y legado

Sin ejecutar ahora: abrir la paleta Ctrl+Shift+P, buscar por grupo/etiqueta/ID y contrastar sus shortcuts con Ajustes y tv2_context.ui_actions. Editar mientras se preparan índices/composición; el visor indica que mantiene el último resultado, descarta un resultado de la vista/proyecto anterior, y aplica únicamente el último seek solicitado. Exportar durante la preparación debe explicar que hay que esperar. Verificar también selección/seek/play por MCP y pending_composition.

Para un proyecto legado sin source_bundle, importar su carpeta V1 original (copia aislada): solo debe añadirse el bundle, con mensaje de vinculación; capas humanas, secuencia, vista y selección permanecen. Una modificación incluso de generated_at del master debe impedir la vinculación. Guardar como, retirar únicamente la copia aislada original, reabrir y exportar carpeta; comparar auxiliares y directorios vacíos. Incluir undo/redo y recuperación de autosave tras undo. Ninguno de estos pasos se ha ejecutado físicamente en este incremento.
