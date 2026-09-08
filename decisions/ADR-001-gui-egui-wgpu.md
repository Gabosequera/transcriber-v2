# ADR-001 — GUI primaria egui + wgpu

- Estado: provisional
- Fecha: 2026-09-07
- Nivel de confianza: medio

## Contexto

Se necesita GUI nativa Windows/Linux, canvas de timeline de alta densidad, frames WGPU y una ruta de test accesible. HTML/WebView/Electron/Tauri quedan fuera de la interfaz principal.

## Alternativas

egui+wgpu; GPUI; Slint; GTK4; Qt/QML; winit+wgpu propio. egui tiene integración nativa, wgpu y AccessKit, pero declara API en desarrollo. Slint tiene desktop Windows/Linux y backends winit/Qt/software, pero su licencia debe elegirse y su integración de video/timeline requiere spike. GPUI es atractivo para layouts de editor pero su soporte multiplataforma y madurez de producto no son suficientes para aprobarlo. GTK/Qt maximizan widgets/accesibilidad pero suben dependencia, packaging y complejidad de integración con canvas/GStreamer.

## Decisión

Prototipar egui+eframe/egui-wgpu con canvas custom, AccessKit, `rfd`/winit y un panel de timeline virtualizado. No depender de widgets web. Mantener Slint como fallback si falla una puerta de accesibilidad/IME/docking.

## Red-team

Si 200% DPI, IME CJK, lector de pantalla, multimonitor o 120Hz fallan, congelar el dominio/command bus y repetir el vertical slice con Slint o GTK. El motor de video se integra por textura/frame bridge y no debe bloquear el event loop.

## Fuentes

`research/sources.md`; `references/rust-editors/gausian-native-editor/apps/desktop`; documentación oficial de egui/AccessKit y Slint.
