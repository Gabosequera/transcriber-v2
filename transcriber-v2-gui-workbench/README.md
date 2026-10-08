# Transcriptor V2 — GUI Workbench

Esta carpeta es una copia aislada para iterar sobre el aspecto de la interfaz de Transcriptor V2 sin modificar `../transcriber-v2`.

## Qué contiene

- `apps/desktop/src/`: código de la aplicación de escritorio y la GUI egui.
  - `ui_panels.rs`: composición principal, barra de menús, biblioteca, visor, inspector, consola y diálogos.
  - `ui_timeline.rs`: timeline, regla, clips, capas y gestos.
  - `ui_media.rs`: visor/media y cachés visuales.
  - `ui_markers.rs`: marcadores.
  - `app.rs`: estado de aplicación, estilo global y ciclo de UI.
  - `main.rs`, `keymap.rs`, `console.rs`, `paths.rs`, `import_jobs.rs`, `scripting.rs`, `timeline_index.rs`: arranque y soporte necesario para compilar y ejecutar la GUI.
- `crates/`: copias de las dependencias locales necesarias para que el workbench compile de forma autónoma.
- Los manifests y el toolchain de Rust de la versión original.

No se copiaron `target`, `.git`, evidencias, logs, documentación ni fixtures pesadas.

## Compilar y ejecutar

Desde esta carpeta:

```powershell
& C:/Users/gabri/.cargo/bin/cargo.exe build --release --locked -p transcriptor
& C:/Users/gabri/.cargo/bin/cargo.exe run --release -p transcriptor
```

La salida queda en `target/` dentro de este workbench. Para trabajar en el look, edita primero `apps/desktop/src/ui_panels.rs`, `ui_timeline.rs`, `ui_media.rs`, `ui_markers.rs` y el bloque `configure_style` de `app.rs`.

Este directorio es un snapshot; no se sincroniza automáticamente con `transcriber-v2`.
