# Ejecutar y probar Transcriptor V2 (E2 abierta)

**Corte 2026-09-08, continuación4:** E0/E1 cerradas en host, E2 abierta, E3 pendiente. Exe actual y evidencia en STATUS.md. El ZIP160510 fue probado, pero es anterior a continuación4; F-008/F-009 están corregidos. La publicación solicitada después del checkpoint se registra por separado en `implementation/PUBLISH.md` cuando se prepare.

## Requisitos en el host de desarrollo

- Rust 1.98.1 (`rust-toolchain.toml`; instalado con rustup en `%USERPROFILE%\.cargo\bin`, no está en PATH del sistema).
- Visual Studio Build Tools 2022 (MSVC + Windows SDK).
- FFmpeg/FFprobe: incluidos en `packaging/third-party/ffmpeg/` (no hace falta instalación global).

## Compilar

```powershell
Set-Location G:/TODO/transcriptor-v2
& C:/Users/gabri/.cargo/bin/cargo.exe build --release --locked -p transcriptor
```

Ejecutable: `G:\TODO\transcriptor-v2\target\release\Transcriptor.exe` (release) o `target\debug\Transcriptor.exe`.

Resolución de FFmpeg (en orden): `TRANSCRIPTOR_FFMPEG_DIR` → `third-party\ffmpeg` junto al exe → `ffmpeg` junto al exe → `packaging\third-party\ffmpeg` del workspace (desarrollo) → PATH. El menú «Ayuda → Acerca de» y la barra superior muestran qué FFmpeg se usa.

## Abrir el editor

```bash
G:\TODO\transcriptor-v2\target\release\Transcriptor.exe
```

Opcional: `Transcriptor.exe <carpeta>.transcriptor` o `Transcriptor.exe <ruta>\project.json` abre un proyecto.

## Recorrido E1 manual (sin consola de desarrollador)

1. **Importar**: `Ctrl+I`, botón «Importar…» o arrastrar archivos a la ventana. Fixtures sintéticas en `tests\fixtures\media\` (`fixture-a.mp4` 12 s con contador de fotogramas, `overlay-alpha.png` con alfa, `tone-220.wav`).
2. **Insertar en la secuencia**: doble clic en el medio de la biblioteca, o «Insertar en playhead». El video y su audio quedan vinculados.
3. **Reproducir**: `Espacio` (o clic en el visor); `L`/`J` cambian velocidad; `1`–`4` fijan ×1–×4; `Shift+L` skim ×8 (solo keyframes, sin audio); `,`/`.` fotograma a fotograma; `←`/`→` 0,5 s; regla: clic/arrastre = scrub.
4. **Dividir**: colocar el playhead sobre el clip y `S` (o toolbar «Dividir»). Divide el clip seleccionado y sus vinculados; sin selección, todos los clips bajo el playhead.
5. **Mover**: arrastrar el clip en el timeline (previsualización; se confirma al soltar; `Esc` cancela; el imán engancha a bordes/playhead/IN-OUT). Colisión en la misma pista → aviso y sin cambio. `Alt+←/→` empuja un fotograma; `Ctrl+Shift+↑/↓` cambia de pista.
6. **Capa manual con rango**: `Ctrl+N` («Nueva capa…»), marcar `I` y `O` sobre el clip, seleccionar la capa (clic en su nombre) y `Ctrl+Enter` («Añadir tramo»). `E` acepta, `X` desactiva, `P` reactiva, `Supr` borra (tombstone).
7. **Guardar**: `Ctrl+S` → carpeta `<nombre>.transcriptor\project.json` (+ `journal.jsonl`).
8. **Reabrir**: `Ctrl+O` y elegir `project.json`.
9. **Exportar**: `Ctrl+E`, preset (720p/1080p/2160p/vertical/WAV), destino nuevo (nunca se sobrescribe), «Exportar». Progreso en el diálogo y en Vista → Consola; el resultado incluye duración verificada y SHA-256.

Vista **Fuente/Secuencia**: `Ctrl+M`. En Fuente se ve el medio completo con sus capas; IN/OUT + `Ctrl+Shift+A` añade ese rango a la secuencia. `Ctrl+Shift+R` revela en fuente el clip seleccionado.

## Reanudar la regresión preparada (aún no ejecutada al checkpoint)

`implementation/evidence/e2/latest-regression.json` apunta a `regression-20260908-215404/`. La última regresión aceptada está en `regression-20260908-160410/`. Los logs se escriben junto al JSON.

```powershell
Set-Location G:/TODO/transcriptor-v2
$runs = Get-Content implementation/evidence/e2/latest-regression.json -Raw | ConvertFrom-Json
$env:TRANSCRIPTOR_CONFIG_DIR = 'G:/TODO/transcriptor-v2/implementation/evidence/e2/config-regression-215404'
$env:TRANSCRIPTOR_CACHE_DIR = 'G:/TODO/transcriptor-v2/implementation/evidence/e2/cache-regression-215404'
$env:TRANSCRIPTOR_LOGS_DIR = 'G:/TODO/transcriptor-v2/implementation/evidence/e2/logs-regression-215404'
# Ejecutar una vez por cada nombre: e1-recorrido, e2-escenario1
$name = 'e1-recorrido'
$p = Start-Process -FilePath 'G:/TODO/transcriptor-v2/target/release/Transcriptor.exe' -ArgumentList @('--script', ('"' + $runs.$name + '"')) -WorkingDirectory $env:TEMP -WindowStyle Hidden -PassThru
$p.WaitForExit()
$folder = Split-Path $runs.'e1-recorrido'
python -X utf8 tests/scripts/verify_export.py (Join-Path $folder 'export-e1-720p.mp4') $runs.'e1-expect'
# Después de ejecutar e2-escenario1:
$folder = Split-Path $runs.'e2-escenario1'
python -X utf8 tests/scripts/verify_export.py (Join-Path $folder 'escenario1-720p.mp4') $runs.'e2-expect-escenario1'
python -X utf8 tests/scripts/verify_export.py (Join-Path $folder 'escenario1-1080p.mp4') $runs.'e2-expect-escenario1'
```

Exigir fallos=false y RESULTADO OK; inspeccionar capturas. `prepare_regression.py` crea otros destinos si se necesita repetir. No sobrescribir exports existentes. --script ahora espera la importación asíncrona antes del siguiente paso.

VFR/rotación ya ejecutados:
```powershell
$run = (Get-Content implementation/evidence/e2/latest-vfr-rotation.txt -Raw).Trim()
python -X utf8 tests/scripts/verify_vfr_rotation.py $run
```

Para repetir el recorrido VFR, copiar `$run/run.json` a una carpeta nueva y reemplazar **todos** los destinos que contienen la carpeta anterior. Añadir wait de5500ms antes de cada screenshot para expirar toasts; no cambiar fuentes. No existe todavía prepare_vfr_rotation.py. Las fixtures tienen5,933333s VFR y4s rotación. El verificador compara todos los cuadros con PTS nativos sin seek/fps en el esperado, mínimo28dB.

Cierre: `./tests/scripts/shutdown_smoke.ps1`. Benchmark CPU UI: `& C:/Users/gabri/.cargo/bin/cargo.exe test -p transcriptor --release dense_ui_benchmark --locked -- --ignored --nocapture`. No acredita GPU/latencia física.

Otros guiones conservados: `e2-transporte.json`, `e2-import-v1.json`, `prepare_selection.py` (selection/caches con destinos nuevos). Config/cache/logs se aíslan con las variables anteriores; la memoria de eframe app.ron usa almacenamiento propio y no queda aislada solo por TRANSCRIPTOR_CONFIG_DIR. Zoom no es DPI OS.

## Importar un proyecto V1

Archivo → «Importar proyecto V1…» y elegir la carpeta que contiene `<nombre>.editorial.master.json` (o su padre). Si el video original ya está en la biblioteca se enlaza por identidad (`size + hash_muestreado + inventario_sha256`); si no, se intenta `media.path` del master y las carpetas vecinas. Se importan `layers/*.json`, `views/trims.json` (un carril por `lane`) y `views/montaje.json` (perfil V1 aplanado: la pista superior sustituye video y audio, sin huecos; ver D-0011). Fixture de ejemplo: `tests/fixtures/v1/demo-a` (generada con los módulos de V1: `python tests\fixtures\v1\make_v1_fixture.py`).

## Tests automatizados

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets
```

Los tests de `tv2-media` usan el FFmpeg vendorizado y las fixtures reales.

## Continuación 3: paquete y exportación de selección

Paquete anterior a continuación4, probado en host: `G:/TODO/transcriptor-v2/dist/Transcriptor V2 prueba ñ 20260908-160510.zip`. Extraer y abrir `Transcriptor.exe`; demo en `Demo.transcriptor/project.json`, guía `INICIO.md` y verificador `VERIFY.ps1` dentro. E2 sigue abierta; Windows limpio no probado.

En Exportar se puede elegir secuencia, IN/OUT, rangos de clips seleccionados o tramos/bloques seleccionados. Las selecciones se unen y concatenan en orden temporal, con todas las pistas visibles/audibles. Un tramo fuente incluye todas sus apariciones en el montaje. No cambia el proyecto. Un medio requerido ausente o una decodificación fallida impide publicar la salida y muestra una acción de recuperación.

Para repetir pruebas sin pisar exports anteriores, usar `python tests/scripts/prepare_regression.py` y `python tests/scripts/prepare_selection.py` desde V2. Las rutas nuevas quedan en `implementation/evidence/e2/latest-regression.json` y `latest-selection.txt`. Lanzar el exe con `--script` y cada JSON generado (selection.json y caches.json para selección/cachés), exigir `fallos=false` y ejecutar `verify_export.py <salida.mp4> <expect.json>`. Paquete: `python packaging/build_windows.py`, después `./tests/scripts/package_smoke.ps1` (incluye comparación de medios y hashes).

## Datos de usuario (rutas)

- Configuración: `%APPDATA%\Transcriptor\` (`ui-state.json`, `keymap.json` con schema `keymap/1`).
- Logs: `%LOCALAPPDATA%\Transcriptor\logs\transcriptor.log` (rotación a 5 MB).
- Memoria de egui (tamaños de paneles): almacenamiento de eframe para el app_id `transcriptor-v2`.
