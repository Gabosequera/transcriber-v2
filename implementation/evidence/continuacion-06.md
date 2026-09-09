# Continuación 6 — evidencia protegida y reconciliación

Fecha: 2026-09-08. Base `7267880` en main, árbol inicial limpio y sincronizado con origin/main. V1 hermano observado limpio; solo se consultó código, sin ejecutar módulos, modelos ni medios. No se encontró AGENTS.md aplicable en raíz/ancestros ni dentro del checkout.

Encargo: implementar hasta E4, sin E5; aceptación física y revisión general aplazadas. Este checkpoint parcial por contexto no certifica la implementación global ni aceptación de fases.

## Cambios

Validación de snapshots; master V1 completo protegido y proyecciones de evidencia; preservación de aceptación de recortes; restricciones sobre análisis/capas bloqueadas; protección humana frente a import/reconcile/actores externos. Guardado lógico proyecto+journal recuperable, rescaneo/merge/aprobación con undo, autosave de proyectos nuevos y recuperación de su auditoría. Salto de recortes con límite de alimentación de audio, montaje de temas/multirrango, navegación de silencios existentes y ocurrencias en inspector. Símbolos y límites en STATUS y decisiones D-0030–D-0034.

Proyecciones actuales: tracks.words, utterances, laughter, arousal, emotions. El original completo conserva campos desconocidos. No es compatibilidad V1 completa: faltan chunks, documentos/adaptadores/export y otros pendientes de STATUS. No se regeneraron goldens mediante V2.

## Controles

- `./scripts/cargo.ps1 check --workspace --locked`: pasó al iniciar y tras integrar GUI.
- `./scripts/cargo.ps1 test -p tv2-application -p tv2-v1compat --lib --locked`: **19 application + 13 v1compat pasan en alpha.3**, log final `unit-continuacion-06.log`.
- `./scripts/cargo.ps1 clippy --workspace --all-targets --locked '--' -D warnings`: resultado final en `clippy-continuacion-06.log`; controles intermedios correctos. Incluye compilación de tests, no su ejecución.
- `./scripts/cargo.ps1 fmt --all --check`: `fmt-continuacion-06.log`.
- `./scripts/cargo.ps1 build --release --locked -p transcriptor`: **OK, 6m30s**, `build-continuacion-06.log`. Exe 23 111 168 bytes, SHA256 en `build-alpha3.json`. No se ejecutó; compilar no acredita playback ni paquete.

Application prueba protección humana ante edición/borrado/reemplazo/batch, merge independiente/conflicto de campos y orden, revalidación al cambiar cualquiera de las bases, revisión/undo/guardado tras reconcile, recuperación de tres fronteras de commit y rechazo de un tercer contenido externo, journal idempotente/cola truncada/corrupción interna y recuperación inválida. V1 prueba recorte aceptado desactivado/reactivación, proyecciones estables con registro original y conservación del master al importar.

**Ejecución bloqueada:** el intento `test -p tv2-domain -p tv2-application -p tv2-v1compat --lib --locked` ejecutó application pero Windows no permitió iniciar `target/debug/deps/tv2_domain-7e35141021bf74e1.exe`: «Una directiva de Control de aplicaciones bloqueó este archivo» (4551). No se alteraron políticas ni se relocalizó el binario. Tests puros de dominio/validación/review se compilan; no constan ejecutados. Desktop mantiene bloqueo histórico y no se volvió a ejecutar. Resumen transcrito del resultado de herramienta, no log nativo: `domain-control-continuacion-06.txt`.

Fallos intermedios corregidos: préstamo doble en test de validación; test de protección que detectó borrado de capa conservando items en su tombstone; poll externo insertado en método sin ctx. Un intento Clippy con -- sin comillas fue interpretado por PowerShell; repetido con '--' conforme a RUN. No presentar esos intentos como controles exitosos.

## Límites

Sin aceptación física/GUI/A-V, comparación integrada con V1 ni benchmark. Fronteras de escritura manipuladas en tempdir sintético: no prueba de pérdida de energía ni terminación de procesos. Intención cubre project.json + journal.jsonl; JSON V1, idempotencia entre sesiones, historial/jobs durables y migraciones pendientes. Autosave legado de proyectos guardados no incorpora journal. Rescaneo de 2 s es respaldo, no watcher SO. GUI muestra resumen de diff y rechaza conflictos; sin resolución por campo. Guardado/autosave/import editorial/descubrimiento inicial de recovery aún hacen IO síncrono. No certificar PERF-01.

Protección actual no equivale a autorización MCP: edited/aceptación necesitan distinción completa de actor en E4. E3 parcial, E4 pendiente. No se instalaron dependencias nuevas ni modelos. V1 solo lectura, evidencia histórica conservada.

## Publicación

Checkpoint `v2.0.0-alpha.3` publicado y verificado, tag/target `a6728e2a361fd517d0b959609f8904f614d52977`, push a main completado. Prerelease no draft, sin assets binarios. Resultado remoto en `publication-alpha3.json` y PUBLISH. Sin medios personales, runtimes o paquete multimedia sin validar. V1 observado limpio al cierre. Parada por contexto con implementación E3 parcial/E4 pendiente; siguiente trabajo en STATUS/traspaso vigente.
