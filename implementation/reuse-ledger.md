# Registro de reutilización y terceros

Cada incorporación al producto: origen, revisión, archivo, licencia, qué se incorporó, modificaciones y dependencias. Los clones de `references/` **no** se copian al producto; se registran aquí solo cuando influyen en una decisión.

## Binarios distribuidos

| Componente | Origen | Versión / revisión | Licencia | Archivos en V2 | Notas |
|---|---|---|---|---|---|
| FFmpeg + FFprobe (estáticos, Windows x64) | gyan.dev «release-essentials» (copiado desde `G:\TODO\editingTools\ffmpeg-8.0.1-essentials_build`, no descargado) | 8.0.1, commit FFmpeg `894da5ca7d` (según `README-gyan.txt`) | **GPLv3** (`--enable-gpl --enable-version3`, incluye libx264/libx265/libaom/libvpx/librubberband) | `packaging/third-party/ffmpeg/{ffmpeg.exe,ffprobe.exe,LICENSE.txt,README-gyan.txt}` | SHA-256 `ffmpeg.exe` `5AF82A0D4FE2B9EAE211B967332EA97EDFC51C6B328CA35B827E73EAC560DC0D`; `ffprobe.exe` `192A1D6899059765AC8C39764FC3148D4E6049955956DC2029F81F4BD6A8972D`. Se ejecuta como proceso separado; **esto no elimina las obligaciones de distribución**: el paquete incluye `LICENSE.txt` y la referencia al código fuente (README). V1 es PolyForm Noncommercial y ya distribuía un build GPL de FFmpeg (`shared/tools/ffmpeg`, BtbN). Si se requiriera un build LGPL, habría que renunciar a x264/x265 (H.264 solo por NVENC/AMF/openh264). |

## Crates Rust (dependencias directas)

| Crate | Versión | Licencia | Uso |
|---|---|---|---|
| eframe / egui / egui_extras / egui-wgpu / wgpu | 0.36.1 / 30.x | MIT OR Apache-2.0 | GUI nativa, backend wgpu |
| serde, serde_json | 1.x | MIT OR Apache-2.0 | contratos JSON |
| thiserror, anyhow | 2.x / 1.x | MIT OR Apache-2.0 | errores |
| tracing, tracing-subscriber | 0.1 / 0.3 | MIT | observabilidad |
| sha2, hex | 0.11 / 0.4 | MIT OR Apache-2.0 | digests V1 y fingerprints |
| rand | 0.10 | MIT OR Apache-2.0 | IDs |
| tempfile | 3.x | MIT OR Apache-2.0 | escritura atómica |
| crossbeam-channel, parking_lot | 0.5 / 0.12 | MIT OR Apache-2.0 | concurrencia |
| cpal | 0.18 | Apache-2.0 | salida de audio (WASAPI) |
| image | 0.25 (png, jpeg) | MIT OR Apache-2.0 | imágenes fijas con alfa |
| rfd | 0.17 | MIT | diálogos nativos de archivo |

El inventario completo con transitivas se generará con `cargo tree`/`cargo about` en E6 (`packaging/THIRD-PARTY.md`).

## Referencias consultadas (sin copiar código)

| Referencia | Commit (lock) | Licencia | Qué se tomó |
|---|---|---|---|
| Cutlass `crates/cutlass-commands/src/command.rs` | `22437e2` | MIT/Apache-2.0 | Forma del contrato: comandos serializables discriminados por `type`, comentarios de semántica por comando. Implementación propia en `crates/domain/src/commands.rs`; no se copió texto. |
| V1 `editorial_montaje.py`, `editorial_layers.py`, `editorial_io.py`, `editorial_history.py`, `keymap.py`, `medios.py` | `c3677ba` | PolyForm NC (mismo titular) | Semántica y contratos reimplementados en Rust con tests que los distinguen (digest, fingerprint, estados, jerarquía, insert/overwrite, keymap). |
| Gausian, OpenCut, Kerf, MLT, Kdenlive, Shotcut | ver `research/repositories-lock.json` | bloqueadas/GPL/PolyForm | Solo lectura previa; nada incorporado. |

Ficheros de fixtures en `tests/fixtures/media/` son sintéticos (generados con FFmpeg `testsrc2`/`smptebars`/`sine`), sin medios personales.

## Continuación 2, 2026-09-08

- Trabajo existente de Fable conservado e integrado: base `crates/media/src/cache.rs`, ampliación de presets/capacidades FFmpeg, pruebas de audio/export y `ShiftClips.policy`. Cambios de esta continuación: workers/cache validada y UI, índice visible, marcadores, integración insert enlazado, cola, normalización temporal y pruebas. No se incorporó código de `references/`.
- `LICENSE.md` copiado de V1 `LICENSE.md`, commit `c3677ba568cfc7d5947ec4695c42fffef6d79e03`, sin modificar V1: texto PolyForm Noncommercial 1.0.0 y titular Gabriel, 2026. No se presenta como autorización de otra licencia.
- `packaging/build_windows.py` inventaría 293 crates resueltos para Windows desde `Cargo.lock`/metadata y copia sus LICENSE/COPYING/NOTICE disponibles en el registro local. Incluye dependencias de test/opcionales, no es SBOM mínimo del binario. `PACKAGE.json::dependency_notices_missing` declara los avisos que no venían en la raíz del crate; su auditoría y obtención de fuentes oficiales sigue pendiente E6. El paquete es provisional local, sin publicación.

## Publicación autorizada, 2026-09-08

El titular pidió V2 propietario/todos los derechos reservados y mantener PolyForm NC en V1. Se reemplaza LICENSE.md y metadatos Cargo/Acerca de para V2; licencias de terceros excluidas de esa restricción. Las entradas anteriores son historia de implementación, no licencia vigente del código propio V2.

Se completaron los avisos de24crates que faltaban en la raíz del registro mediante sus repositorios oficiales y commits exactos de .cargo_vcs_info.json. Cada carpeta packaging/third-party/rust-notices incluye PROVENANCE.json con URLs/hashes. Se incluyen OFL/UFL de epaint_default_fonts. El paquete público contiene293paquetes inventariados con avisos, sin FFmpeg binario. Install-FFmpeg.ps1 descarga opcionalmente8.0.1 desde GyanD/codexffmpeg, valida los hashes registrados y conserva GPLv3/README. No se copiaron clones de references al producto ni al repositorio público.
## Continuación 6, 2026-09-08

Incremento propio en dominio/aplicación/GUI y adaptador existente; sin incorporación de código de references ni nuevas dependencias. Se consultaron en solo lectura editorial_layers_ui.py y los contratos/fixtures V1 existentes para contrastar estados y persistencia. La política explícita de preservar aceptación de trims desactivados corrige pérdida en round-trip y mantiene campos independientes. Fixtures sintéticas Rust existentes ampliadas; no se ejecutó V1 ni se regeneraron goldens. Licencias vigentes intactas.
