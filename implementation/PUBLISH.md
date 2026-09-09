# Checkpoint alpha.6 — continuación 9

Base main `87d9a26`, versión de fuentes 2.0.0-alpha.6. 60 tests dirigidos (40 application +20 V1compat), check, Clippy/all-targets y formato correctos. Build release local correcto en 2m04s; `evidence/build-alpha6.json` con tamaño/SHA256. Exe no ejecutado. Pruebas físicas/multimedia/revisión general aplazadas; no se eludió Windows4551.

Publicación de checkpoint autorizada por el encargo vigente. Prerelease de fuentes sin assets binarios nuevos; estado exacto de la publicación se registra tras confirmación. **E2 abierta, E3 parcial y E4 pendiente; objetivo hasta E4 incompleto. E5 no iniciada.** Alcance y próximos pendientes en STATUS y evidence/continuacion-09.md.

---

# Publicación vigente — continuación 8, alpha.5

Publicada y verificada [v2.0.0-alpha.5](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.5) sobre `83987b68a8dae139a8f70a5dba71c407b6fd7de8`, código subido a main. `isPrerelease=true`, `isDraft=false`, `assets=[]`; tag remoto coincide con el SHA completo. Registro: `evidence/publication-alpha5.json`, publicado 2026-09-09 01:32:59 UTC.

51 tests dirigidos (33 application +18 V1compat), Clippy all-targets y formato OK. Build release final OK en 2m13s, tamaño/SHA256 en `evidence/build-alpha5.json`. Exe no ejecutado; sin paquete nuevo ni aceptación GUI/multimedia/general. E2/E3 siguen abiertas; E4 pendiente; E5 no iniciada. V1 observado limpio y tratado como solo lectura.

Este commit posterior solo registra publicación/evidencia. Continuar desde main para conservar el traspaso vigente, no desde un checkpoint anterior. El objetivo global hasta completar E4 sigue **incompleto**.

---
# Publicación de continuación 7 — alpha.4

La prerelease [v2.0.0-alpha.4](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.4) está publicada y verificada sobre `7e050bcee3777924389703db79e87b0ecf381841`, código subido a main. `isPrerelease=true`, `isDraft=false`, `assets=[]`; tag remoto coincide con ese commit. Build release OK (2m51s), 39 tests dirigidos, Clippy y formato. No se ejecutó el exe ni se preparó un paquete nuevo. E2/E3 abiertas y E4 pendiente. Registro remoto: `evidence/publication-alpha4.json`; tamaño/SHA256 local: `evidence/build-alpha4.json`. Main incluye después el commit documental de este resultado.

V1 observado limpio, estrictamente solo lectura. El primer intento de release con SHA corto fue rechazado por GitHub (422, target_commitish inválido); se repitió con SHA completo y se verificaron release y tag remoto. No se creó ninguna release fallida. Checkpoint por contexto autorizado, objetivo global hasta E4 aún incompleto.

---

# Publicación de continuación 6 — alpha.3

Checkpoint autorizado de fuentes, con E2/E3 abiertas y E4 pendiente. Build local Windows completado con `cargo build --release --locked -p transcriptor` en 6m30s; ejecutable no lanzado ni empaquetado. Registro de tamaño/hash en `evidence/build-alpha3.json`. Unitarios alpha.3: 19 application + 13 V1compat pasan. Clippy/all-targets y formato en logs de continuación 6. Tests dominio bloqueados por Windows 4551; desktop/GUI/multimedia/revisión general siguen pendientes.

Publicada y verificada [v2.0.0-alpha.3](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.3) sobre `a6728e2a361fd517d0b959609f8904f614d52977`, código/contexto subido a main. `isDraft=false`, `isPrerelease=true`, sin assets binarios; source archives de GitHub. Registro `evidence/publication-alpha3.json`, notas `evidence/release-notes-alpha3.md`. El commit posterior solo registra la publicación y el checkpoint; continuar desde main para incluirlo. V1 observado limpio al terminar. Objetivo global hasta E4 aún incompleto.

## Histórico — continuación 5, alpha.2

El usuario autorizó commit/push/prerelease al cortar por contexto. Se publicó `v2.0.0-alpha.2` como **checkpoint de fuentes**, con avances parciales E3 y rutas portables. E4 pendiente. No incluye nuevo paquete Windows ni ejecutable probado; los source archives son los de GitHub. Clippy/formato y pruebas unitarias dirigidas no sustituyen testing real, aplazado expresamente. Publicación verificada: [v2.0.0-alpha.2](https://github.com/Gabosequera/transcriber-v2/releases/tag/v2.0.0-alpha.2), `draft=false`, `prerelease=true`, sin assets binarios adjuntos. Tag y target apuntan a `31d18437788fcd1da0155b478b538d1753d1b93d`, commit de código/contexto subido a origin/main. Registro: `evidence/publication-alpha2.json`. El commit posterior solo registra este resultado; continuar desde main para incluirlo.

El contenido restante describe la publicación **histórica alpha.1**. Sus hashes/binarios no corresponden a alpha.2 y su orden de parar fue reemplazada por el encargo actual hasta E4.

---

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
