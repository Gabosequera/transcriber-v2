# Publicación solicitada — 2026-09-08

El usuario pidió detener implementación y guardar el traspaso; después autorizó commit/push/release de ambas versiones. Confirmó: V2 todos los derechos reservados; V1 conserva PolyForm Noncommercial. Versiones previstas: V1 v0.4.0 (VERSION existente), V2 v2.0.0-alpha.1 (Cargo existente). Repositorio V1 https://github.com/Gabosequera/transcriber; nuevo V2 público https://github.com/Gabosequera/transcriber-v2.

**Ambas versiones publicadas y verificadas. Trabajo terminado; desarrollo pausado por petición del usuario.** La implementación E0–E3 sigue pausada por petición del usuario; la release alpha no cierra E2/E3/E6.

V1 se encontraba limpio, diez commits por delante de origin/main local; HEAD de referencia c3677ba568cfc7d5947ec4695c42fffef6d79e03. No había Git en V2. La autorización de publicación constituye excepción expresa a V1 solo lectura para preparar su release; no autoriza cambiar sus medios/modelos/configuración personales.

La release pública V2 debe preservar licencias de terceros. El paquete local histórico incluía FFmpeg GPLv3 y avisos incompletos en24crates; se prepara un paquete público del editor con avisos completados y FFmpeg como dependencia descargada por separado desde su distribuidor oficial. No redistribuir el ZIP histórico como paquete final bajo licencia propietaria.

Fuentes oficiales revisadas: https://ffmpeg.org/legal.html y https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository. Licencia propia no sustituye términos de dependencias ni permisos de visualización/fork de la plataforma.

## V1 terminada

Commit bb7012c (documentación de release;11commits subidos contando los10previos), tag v0.4.0. Release: https://github.com/Gabosequera/transcriber/releases/tag/v0.4.0. Workflow https://github.com/Gabosequera/transcriber/actions/runs/34284363498 terminó success: tests Linux/Windows e instalación limpia/idempotente en Windows. Local150tests,148pasan/2omitidos. ZIP público descargado y contrastado contra update-manifest/release-manifest y82hashes; evidence/e2/publication-v1.json. Licencia V1 intacta.

## V2 preparada

Repositorio público creado https://github.com/Gabosequera/transcriber-v2, rama main. Licencia propietaria en LICENSE.md y Cargo/Acerca de; derechos de terceros preservados. Se completaron24paquetes de avisos desde commits oficiales, incluyendo licencias de fonts. Lista conservadora de293crates y575archivos verificados por VERIFY.

Paquete público final: dist/Transcriptor-v2.0.0-alpha.1-windows-x64-20260908-221703.zip,12.041.211bytes; SHA2566278e97df38597b7f7809bb61c54c9909f9439264ef1a6fe38420ddb8ddab1f5. Asset estable copiado a dist/public-release/Transcriptor-v2.0.0-alpha.1-windows-x64.zip, con SHA256SUMS.txt. No incluye FFmpeg ejecutable; Install-FFmpeg.ps1 obtiene el build oficial8.0.1 y verifica los dos hashes, mantiene LICENSE/README. Sin Rust/Python necesarios para ejecutar el editor.

Intentos conservados: package-smoke-20260908-181225 falló comparación visor (posición cambió durante espera, causa no determinada);181348 falló por tipo de assertion del guion (float en lugar de [min,max]). Se corrigió el procedimiento a pausa/seek después de esperar cachés y assertion explícita de posición/playing.181515 pasa con el primer paquete público. Se repite para el paquete final cuyo único cambio de Rust es el texto de licencia en Acerca de. No se ocultaron los intentos fallidos.

La regresión completa E1/E2 de215404 sigue preparada y pendiente; la smoke del paquete y el recorrido VFR tienen sus propios casos acotados. La publicación alpha no cierra E2/E3/E6. No se cambiaron funcionalidades editoriales durante este encargo de publicación.

## Aceptación del paquete final V2

Build41,63s; exe SHA2562eb8464f6800dd84cb111c942115a6658df85dbb92a11a5aad64864030fdaabc. `publication-build-final.log` y `package-smoke-20260908-181823/`:575hashes antes/después, instalación FFmpeg desde upstream e idempotencia, assertion pausa5,200s, cachés presentes, screenshot inspeccionada, export14s420frames. Verificador fuente42,6–42,8dB/visor30,6dB/hueco0, RESULTADO OK. Esto valida el artefacto que se adjunta a la prerelease, con las limitaciones de E2 anotadas.

## Resultado final

- V1: commit bb7012c, tag v0.4.0, https://github.com/Gabosequera/transcriber/releases/tag/v0.4.0. Tres assets publicados por CI correcto. PolyForm NC intacta.
- V2: commit de release5cd55a7, tag v2.0.0-alpha.1, https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.1. Repo público, prerelease publicada (draft=false), ZIP y SHA256SUMS.txt adjuntos. Digest del ZIP confirmado por GitHub:6278e97df38597b7f7809bb61c54c9909f9439264ef1a6fe38420ddb8ddab1f5. Licencia propietaria/todos los derechos reservados para código propio; terceros separados. Registro API en evidence/e2/publication-v2.json.
- Commit posterior únicamente actualiza este resultado y STATUS; el tag conserva el código/paquete probado. No se continúa desarrollo. Al reanudar, E2 sigue abierta y E3 pendiente; primera regresión preparada215404 según STATUS.
