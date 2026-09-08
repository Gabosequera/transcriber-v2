# Transcriptor V2

Editor multimedia nativo para Windows, implementado en Rust con egui/eframe y wgpu. Versión **2.0.0-alpha.1**: E0 y E1 completadas en el host de desarrollo; E2 en curso y E3 pendiente. Es una prerelease para conservar y probar el avance, no una versión final.

Incluye biblioteca, visor de timeline compuesto, pistas de video/audio, overlays, mezcla, edición enlazada, capas manuales, marcadores, undo/redo, guardado y exportación por cola. Usa FFmpeg/FFprobe externos y un compositor/mezclador propio. El adaptador V1 conserva contratos de capas/recortes y la semántica de flatten del montaje. La importación de medios es asíncrona.

Los modelos de transcripción/inferencia y el cliente externo de AI todavía no están integrados. La paridad editorial V1 completa, recuperación ante crashes, reconciliación externa y algunas pruebas físicas de UI/A-V siguen pendientes.

- [Ejecutar, compilar y reproducir pruebas](implementation/RUN.md)
- [Estado exacto y cómo continuar](implementation/STATUS.md)
- [Publicación y releases](implementation/PUBLISH.md)
- [Matriz de requisitos](implementation/requirements-matrix.md)
- [Evidencia de continuación4](implementation/evidence/e2/continuacion-04.md)
- [Encargo y anexo de traspaso](prompts/00-CONSTRUIR-TRANSCRIPTOR-V2.md)
- [Procedencia y terceros](implementation/reuse-ledger.md)

## Desarrollo

Rust1.98.1, toolchain MSVC/Windows SDK, FFmpeg8.0.1 para las pruebas de referencia. Desde la raíz: `cargo build --release --locked -p transcriptor`, `cargo test --workspace --locked`. Ejecutable `target/release/Transcriptor.exe`.

Los clones de referencia, modelos, cachés de build y medios personales no forman parte del repositorio. Los generadores de fixtures están en tests/fixtures; algunos goldens necesitan la copia V1 indicada en sus instrucciones. Las rutas absolutas de evidence corresponden al host de desarrollo y se documentan para reproducir ese entorno.

## Licencia

Copyright2026 Gabriel Sequera. **Software propietario, todos los derechos reservados.** Consultar [LICENSE.md](LICENSE.md). La visibilidad pública no concede una licencia general de uso, modificación o redistribución. Las dependencias y fuentes de terceros mantienen sus propias licencias.
