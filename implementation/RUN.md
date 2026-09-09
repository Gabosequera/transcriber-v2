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
