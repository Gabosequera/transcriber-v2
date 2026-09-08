# Transcriptor V2 — 2.0.0-alpha.1 para Windows x64

Prerelease: E0/E1 probadas en host; E2 en curso, E3 pendiente. No incluye modelos de inferencia ni cliente externo de AI. Windows limpio y pruebas físicas completas de DPI/IME/A-V aún pendientes.

1. Extrae todo el ZIP en una carpeta propia. Verifica los archivos con `powershell -ExecutionPolicy Bypass -File .\VERIFY.ps1`.
2. Media requiere FFmpeg/FFprobe. Ejecuta `powershell -ExecutionPolicy Bypass -File .\Install-FFmpeg.ps1`: descarga el build8.0.1 desde el distribuidor oficial y valida los hashes de los ejecutables. Conserva la licencia GPLv3 de esa dependencia. Necesita Internet solo para esa descarga. Si ya lo tienes, puedes definir TRANSCRIPTOR_FFMPEG_DIR apuntando a la carpeta de ambos ejecutables.
3. Abre Transcriptor.exe. Archivo → Abrir: Demo.transcriptor/project.json. El medio es sintético. Las cachés se calculan en segundo plano.
4. Espacio reproduce; L/J cambian velocidad;1–4 fijan velocidad; Shift+L skim8× sin audio. Arrastra regla para buscar. S divide; arrastra clips para mover; Escape cancela; Ctrl+Z deshace.
5. Ctrl+I importa medios (worker cancelable); Ctrl+S guarda; Ctrl+E exporta a un destino nuevo. Puedes elegir todo, IN/OUT, clips o tramos/bloques seleccionados y encolar/cancelar trabajos.

Formatos según FFmpeg/capacidades: H264/HEVC/AV1, ProRes, VP9, MKV, WAV, FLAC, MP3. NVENC depende de la GPU. No se sobrescriben destinos existentes. La compatibilidad editorial V1 es parcial; consulta el estado del repositorio antes de confiarle un flujo definitivo.

El editor no requiere Rust ni Python instalados para ejecutarse. FFmpeg no se redistribuye dentro de este ZIP; es una dependencia separada. Configuración en %APPDATA%\Transcriptor, logs/cachés en %LOCALAPPDATA%\Transcriptor. VERIFY comprueba integridad de los archivos incluidos, no autentica al autor.

Licencia del editor: LICENSE.md (propietaria, todos los derechos reservados). Dependencias y fuentes: THIRD-PARTY.md y third-party/rust. Permisos de uso del editor requieren autorización del titular; publicar este artefacto no concede una licencia general.
