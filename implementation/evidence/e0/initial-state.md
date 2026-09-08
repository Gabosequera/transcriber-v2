# E0 · Estado inicial registrado

Fecha: 2026-09-07. Host: Windows 11 Pro 10.0.26200.

## Repositorios

| Directorio | Git | Estado |
|---|---|---|
| `G:\TODO\transcriptor-v2` | no es repositorio | workspace documental: `README.md`, `decisions/` (17 ADR), `docs/` (00–24), `prompts/`, `references/` (8 clones), `reports/`, `research/`. Sin `Cargo.toml`, sin `implementation/`. |
| `G:\TODO\transcriptor-installer-v0.2.0` (V1) | `main`, HEAD `c3677ba568cfc7d5947ec4695c42fffef6d79e03`, «ahead of origin/main by 10» | `git status --short` vacío (working tree limpio). Solo lectura durante todo el encargo. |

Clones de referencia (`research/repositories-lock.json`): los 8 existen localmente (`cutlass`, `gausian-native-editor`, `kerf`, `opencut`, `gstreamer`, `mlt`, `kdenlive`, `shotcut`). No se descargó nada nuevo.

Instrucciones locales: no existe `CLAUDE.md` en `G:\TODO` ni en `G:\TODO\transcriptor-v2`.

## Toolchain al iniciar

| Herramienta | Estado inicial | Acción |
|---|---|---|
| Rust (`cargo`, `rustc`, `rustup`) | ausentes | Instalado `rustup-init` (https://win.rustup.rs/x86_64) con `-y --default-toolchain stable --profile default --no-modify-path`. Resultado: `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1`, host `x86_64-pc-windows-msvc`. PATH del usuario **no** modificado; los comandos del proyecto usan `%USERPROFILE%\.cargo\bin`. |
| MSVC | Visual Studio Build Tools 2022 17.14.36930.0 con `VC\Tools\MSVC` y Windows SDK 10.0.26100 | reutilizado para el enlazador |
| FFmpeg/FFprobe | no en PATH. Existen `G:\TODO\editingTools\ffmpeg-8.0.1-essentials_build` (gyan.dev, GPLv3, estático) y `G:\TODO\transcriptor-installer-v0.2.0\shared\tools\ffmpeg` (BtbN N-126416, GPL) | Copiado el build 8.0.1 essentials a `packaging/third-party/ffmpeg/` con `LICENSE.txt` y `README-gyan.txt`. SHA-256 en `reuse-ledger.md`. |
| GStreamer | ausente (ni runtime ni devel) | no instalado; ver decisión D-0003 en `implementation/decisions.md` |
| Python | 3.14.3 (`C:\Python314`) | V1 usa runtime propio `runtimes/win-py313-*`; V2 crea el suyo en E5 |
| git | 2.x (`C:\Program Files\Git`) | — |

## Hardware del host de referencia

- CPU AMD Ryzen 9 7940HX (16C/32T); RAM 64 GB.
- GPU NVIDIA GeForce RTX 4070 Laptop (driver 32.0.15.9186) + AMD Radeon 610M integrada; también «Meta Virtual Monitor».
- Pantalla principal 2560×1440.
- Discos: C: 284 GB libres; G: 1673 GB libres.

## Revisión V1 y hechos revalidados por lectura

- Licencia V1: PolyForm Noncommercial 1.0.0 (`LICENSE.md`), titular Gabriel Sequera; `COMMERCIAL-LICENSE.md` exige acuerdo separado para uso comercial.
- `editorial_montaje.SCHEMA == "editorial-montaje/1"`, `REQUEST_SCHEMA == "editorial-montage-request/1"`, `PROPOSAL_SCHEMA == "editorial-montage-proposal/1"` (confirmado en código).
- `editorial_layers.STATES == ("proposed","accepted","disabled")`; jerarquía máx. 32; borrado de capa por `deleted: true`; items borrados por `deleted_item_ids`.
- `editorial_io.digest_json`: SHA-256 sobre `json.JSONEncoder(ensure_ascii=False, sort_keys=True, separators=(",",":")).iterencode`.
- `medios.fingerprint`: `{size, mtime_ns, hash_muestreado (3 bloques de 8 MiB + size), inventario_sha256}`; identidad = `size + hash_muestreado + inventario_sha256` (`editorial_trims.IDENTITY_KEYS`).
- `keymap.ACTIONS`: 67 acciones registradas (transporte, navegación, edición, herramientas, vista, montaje, marcas). Ver `implementation/v1-action-inventory.md` (se completa en E2/E3).
- V1 tests: 150 funciones `def test_` en 17 archivos (conteo por `grep`, no ejecución). El número histórico «129» no se reutiliza.
