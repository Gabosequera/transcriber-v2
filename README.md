# Transcriptor V2

Editor multimedia nativo para Windows, implementado en Rust con egui/eframe y wgpu. Versión **2.0.0-alpha.2**: checkpoint de implementación en un nuevo equipo; E0/E1 históricas, E2 con aceptación pendiente, E3 parcial y E4 pendiente. Es una prerelease para conservar y probar el avance, no una versión final.

Incluye biblioteca, visor de timeline compuesto, pistas de video/audio, overlays, mezcla, edición enlazada, capas manuales, marcadores, undo/redo, guardado y exportación por cola. Usa FFmpeg/FFprobe externos y un compositor/mezclador propio. El adaptador V1 conserva contratos de capas/recortes y la semántica de flatten del montaje. La importación de medios es asíncrona.

Los modelos de transcripción/inferencia y el cliente externo de AI todavía no están integrados. La paridad editorial V1 completa, recuperación ante crashes, reconciliación externa y algunas pruebas físicas de UI/A-V siguen pendientes.

- [Ejecutar, compilar y reproducir pruebas](implementation/RUN.md)
- [Estado exacto y cómo continuar](implementation/STATUS.md)
- [Publicación y releases](implementation/PUBLISH.md)
- [Matriz de requisitos](implementation/requirements-matrix.md)
- [Avances de continuación5](implementation/evidence/continuacion-05.md)
- [Encargo y anexo de traspaso](prompts/00-CONSTRUIR-TRANSCRIPTOR-V2.md)
- [Procedencia y terceros](implementation/reuse-ledger.md)

## Desarrollo

Rust 1.98.1, MSVC y Windows SDK. Desde cualquier ruta de checkout en Windows: `./scripts/cargo.ps1 build --release --locked -p transcriptor`. Los guiones usan plantillas portables; consultar RUN.md. Testing real y revisión general se dejan para la siguiente iteración. Ejecutable `target/release/Transcriptor.exe`.

Los clones de referencia, modelos, cachés de build y medios personales no forman parte del repositorio. Los generadores de fixtures están en tests/fixtures; algunos goldens necesitan la copia V1 indicada en sus instrucciones. Las rutas absolutas de evidence corresponden al host de desarrollo y se documentan para reproducir ese entorno.

## Licencia

Copyright2026 Gabriel Sequera. **Software propietario, todos los derechos reservados.** Consultar [LICENSE.md](LICENSE.md). La visibilidad pública no concede una licencia general de uso, modificación o redistribución. Las dependencias y fuentes de terceros mantienen sus propias licencias.
