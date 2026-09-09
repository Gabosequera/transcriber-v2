# Continuación 8 — edición semántica y persistencia en workers

Base `acecb37` en main limpio y sincronizado. Host Windows, 2026-09-08/09. No AGENTS.md aplicable encontrado en ancestros/checkout. V1 hermano `bb7012c` observado limpio; lecturas de fuentes y AST sin importar/ejecutar módulos, sin medios/modelos ni cambios de configuración.

Implementación y límites vigentes: STATUS, D-0039–D-0042 y matriz. E2/E3 abiertas, E4 pendiente. Checkpoint por contexto; no aceptación general ni física.

## Controles

- Primer `./scripts/cargo.ps1 check --workspace --locked`: OK; `check-continuacion-08-initial.log`, alpha.4 recibida.
- Tests finales alpha.5: `./scripts/cargo.ps1 test -p tv2-application -p tv2-v1compat --lib --locked`: **33 + 18 = 51 pasan**, `unit-continuacion-08.log`.
- Clippy alpha.5: `./scripts/cargo.ps1 clippy --workspace --all-targets --locked '--' -D warnings`: **OK**, `clippy-continuacion-08.log` (4,43 s); compila tests desktop/dominio, no los ejecuta.
- Formato: `./scripts/cargo.ps1 fmt --all --check`: **exit 0**, `fmt-continuacion-08.log`.
- Build release: `build-continuacion-08.log`, metadatos en `build-alpha5.json` al completar.
- Auditoría UI: `python scripts/audit_v1_ui.py`, **140 constructores/bindings** (33 app, 38 automático, 30 editor, 19 capas, 8 montaje, 8 toolbar, 4 keymap). `continuacion-08-ui-inventory.json`. Esto es cobertura estática, no aceptación funcional; controles generados dinámicamente necesitan contraste adicional.

Los nuevos tests prueban partición jerárquica multirrango con nietos y evidencia; retries del split; nudge de descendientes una vez/clamp; rollback de trim/batch; ciclo de autor; atribución Agent; orden completo; historial undo/redo tras reapertura y rechazo de corrupción; reconocimiento de guardado sin limpiar ediciones posteriores; dos instancias autosave; recuperación de cuatro fronteras de intención (antes/después de proyecto/journal/history); export de trims con cabecera/lanes desconocidos/aceptación/ID nuevo y creación pre-master; bloques divididos/bordes/partición/digest.

No se ejecutaron tests desktop/dominio por el bloqueo Windows 4551 histórico. No se relocalizaron binarios ni se modificaron políticas. Las verificaciones filesystem usan tempdirs sintéticos; no son pruebas de crash real ni cierre de GUI. No se ejecutó el exe release.

Fallos intermedios: Clippy detectó enum de resultados IO demasiado grande; corregido con Open boxed. Los logs finales sustituyen controles anteriores. No se regeneraron goldens V1 mediante V2.

## Límites relevantes

History tiene profundidad 200 y snapshots completos; intención máxima 128 MiB/autosave 64 MiB, sin archivado ni deltas. Recuperar autosave valida la pila archivada pero todavía crea un único paso de recuperación. Jobs no durables. Los workers siguen recibiendo clones congelados desde GUI; validación/hash master por comando aún pendiente de reducir. Apertura rechaza resultado si hubo edición concurrente; no descarta esa edición. Dos instancias cooperan con lock; editores externos que ignoran locks siguen sujetos a la ventana final de CAS descrita antes.

Bloques importan plan elegido antes que view; cabecera/items forman una representación normalizada única. Inversa de bloques con metadata/snap de bordes editados se rechaza explícitamente, pendiente recalcular/materializar; no es compatibilidad completa. Trims conserva ahora cabecera y documento multicarril, sin coalescencia completa V1. Original author sidecar no conectado todavía. Montaje inverso editado y carpeta V1 completa siguen pendientes. No confundir el actor Agent interno con implementación de E4.

Control añadido antes de publicar: test de mapping de borde exclusivo en corte/final de secuencia (33 application). El primer build release de 2m13s fue anterior a esa corrección; el log/build metadata final lo sustituye. Save As valida también identidad/revisión de auditoría leída del origen para rechazar una sustitución concurrente incompatible.

Build final alpha.5 después de la corrección de borde: **OK, 2m13s**, comando ./scripts/cargo.ps1 build --release --locked -p transcriptor; tamaño/SHA256 en build-alpha5.json. Exe compilado, no ejecutado. 51 tests finales, Clippy (4,43 s) y formato OK.
