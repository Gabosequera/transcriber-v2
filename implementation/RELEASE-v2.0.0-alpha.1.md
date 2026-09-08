Primera prerelease pública del editor nativo de Transcriptor V2 para Windows x64. Código original propietario, todos los derechos reservados, Copyright2026 Gabriel Sequera; las dependencias conservan sus propias licencias.

Incluye edición de video/audio enlazados, overlays y mezcla, timeline/markers/snapping, capas manuales, undo/redo, guardado y exportación por cola con selección y rangos. Importación asíncrona, cachés con presupuesto de memoria y seek VFR corregido. Adaptador editorial V1 parcial.

**Estado:** E0/E1 completadas en host; E2 en curso y E3 pendiente. No incluye modelos de inferencia ni cliente externo AI. No declara pruebas completas de DPI/IME/sincronía física ni Windows limpio para V2.

### Instalar

1. Descarga y extrae Transcriptor-v2.0.0-alpha.1-windows-x64.zip.
2. Ejecuta `powershell -ExecutionPolicy Bypass -File .\VERIFY.ps1` para comprobar integridad.
3. Ejecuta `powershell -ExecutionPolicy Bypass -File .\Install-FFmpeg.ps1`: obtiene FFmpeg8.0.1 directamente del distribuidor oficial, verifica ambos ejecutables y conserva GPLv3/README. Alternativamente configura TRANSCRIPTOR_FFMPEG_DIR con una instalación compatible. El ZIP del editor no incluye binarios FFmpeg.
4. Abre Transcriptor.exe y Demo.transcriptor/project.json. No requiere Rust ni Python para ejecutarse. Consulta INICIO.md y THIRD-PARTY.md.

### Validación

91 tests pasan y benchmark explícito10k/100kclips; Clippy limpio y build release. VFR/rotación contrastados contra PTS nativos en298cuadros exportados. Paquete probado desde TEMP con espacios/Unicode, checksum575archivos, descarga FFmpeg e instalación idempotente, cachés y export14s420frames comparada contra fuente y visor. Las limitaciones de la entrega y el punto para continuar están en implementation/STATUS.md y el anexo del encargo.

SHA256 ZIP: `6278e97df38597b7f7809bb61c54c9909f9439264ef1a6fe38420ddb8ddab1f5`.

Este artefacto conserva un avance verificable; no presenta E2/E3/E6 como cerradas. Licencia propietaria: publicar la descarga no concede una licencia general de uso/modificación/redistribución. Solicitudes al titular mediante el repositorio.
