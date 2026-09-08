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

Al abrir un proyecto con autosave más reciente se ofrece recuperar. La recuperación es explícita y reversible con Deshacer. Si otra instancia o un editor externo cambió el archivo, guardar falla con conflicto: abrir la versión externa o guardar en otra carpeta. Todavía no hay watcher/reconciliación con diff ni durabilidad multidocumento completa.

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
