# Transcriptor V2 — paquete provisional E2

Abre `Transcriptor.exe`. No necesita Python, Rust ni FFmpeg globales para editar. Windows x64: probado en el host de desarrollo; pendiente prueba en Windows limpio.

1. Archivo → Abrir (`Ctrl+O`): elige `Demo.transcriptor/project.json`. Incluye un medio sintético, un corte, un hueco y un marcador. Las miniaturas y waveforms se calculan en segundo plano.
2. Espacio reproduce; L/J cambian velocidad; 1–4 fijan velocidad; Shift+L activa skim 8× sin audio. Arrastra la regla para buscar un instante.
3. S divide. Arrastra un clip para moverlo; Escape cancela. El imán ajusta a bordes y marcadores. Edición → Insertar selección en playhead cierra el origen y desplaza los clips de destino, conservando el enlace A/V. Ctrl+Z deshace.
4. Capas → Añadir marcador de secuencia y Lista de marcadores permiten navegar, editar y borrar marcadores. Las marcas del autor V1 son independientes.
5. Ctrl+S guarda. Ctrl+E exporta: elige preset y un destino nuevo. Puedes encolar más trabajos, cancelar el activo o los pendientes. Cada trabajo conserva la revisión que tenía al encolarlo.

El editor conserva una ventana Fuente y otra Secuencia (`Ctrl+M`). Las capas editoriales importadas y los contratos V1 tienen compatibilidad parcial documentada en el proyecto; E3–E6 siguen abiertas. Los modelos y el control externo aún no forman parte de este paquete. Color SDR, sin garantía HDR. Las exportaciones se recodifican para cortes arbitrarios.

Los presets prueban MP4 H.264/HEVC/AV1, ProRes MOV, VP9 WebM, H.264 MKV, WAV, FLAC y MP3. NVENC depende del hardware y se valida al lanzar el trabajo. No se sobrescriben originales ni destinos existentes.

Verifica los archivos con PowerShell: `powershell -ExecutionPolicy Bypass -File .\VERIFY.ps1`. El hash comprueba integridad, no es una firma de autoría.

Configuración: `%APPDATA%\Transcriptor`; logs: `%LOCALAPPDATA%\Transcriptor\logs`; cachés regenerables: `%LOCALAPPDATA%\Transcriptor\cache`. Los originales se conservan intactos. Este paquete es una entrega local de prueba, sin publicación externa.

Licencia del editor: `LICENSE.md`. FFmpeg tiene su propia licencia y procedencia en `third-party/ffmpeg`. El inventario y avisos de crates están en `third-party/rust`. `PACKAGE.json` indica cualquier aviso de dependencia aún pendiente; la revisión final de distribución corresponde a E6.
