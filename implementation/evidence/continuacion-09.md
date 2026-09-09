# Continuación 9 — Caja Corte, autor, bloques seguros y recuperación

Base main limpia/sincronizada `87d9a26bdf54191d6514451640faa577cbbd0258`; V1 hermano `bb7012c5bc22403908b6dfb48fe721c67045b6b5` limpio, exclusivamente lecturas de fuentes. No AGENTS.md aplicable encontrado en ancestros/checkout. Se conservaron E0/E1 y la implementación alpha.5. Fuentes nuevas 2.0.0-alpha.6.

Alcance implementado y límites: STATUS, D-0043–D-0046, matriz, RUN y traspaso final. **E2 abierta, E3 parcial, E4 pendiente, E5 no iniciada.** Checkpoint de continuidad, no finalización del objetivo ni aceptación general.

## Controles ejecutados

- Inicial `./scripts/cargo.ps1 check --workspace --locked`: exit 0, alpha.5, 6.46 s (salida de la sesión).
- Final alpha.6 `./scripts/cargo.ps1 test -p tv2-application -p tv2-v1compat --lib --locked`: **40 application + 20 V1compat = 60 tests pasan**, 37.73 s compilación, 0.51/0.01 s tests. `unit-continuacion-09.log`.
- `./scripts/cargo.ps1 clippy --workspace --all-targets --locked '--' -D warnings`: exit 0, **7.80 s**; `clippy-continuacion-09.log`. Compila targets/tests dominio/desktop; no ejecuta sus binarios.
- `./scripts/cargo.ps1 fmt --all --check`: exit 0; `fmt-continuacion-09.log` (sin salida).
- `./scripts/cargo.ps1 check --workspace --locked`: exit 0, **6.29 s**; `check-continuacion-09.log`.
- Build release: `build-continuacion-09.log`, metadatos exactos al terminar en `build-alpha6.json`. No ejecutar el exe.

## Evidencia de comportamiento

Los nuevos tests cubren creación/unión/resta de caja, conservación de estado/evidencia/extras, tombstones/retry/undo-redo; coalescencia strict overlap (bordes adyacentes separados), actor/estado enabled y movement/eliminación de carril/fábrica protegida; edición de bloque por ambos caminos de inspector con vecinos y rollback. Sidecar autor prueba roundtrip de puntos/regiones, null/prompt/label/extras, identidad errónea, revisión/candidatos y cuarentena con IDs reservados. Snap recalcula evidencia de bordes y materializa conjunto sin mutar el master. PreparedCommand conserva IDs de preview, rechaza base cambiada y comparte almacenamiento de master; un master deserializado alterado no pasa digest. Recovery autosave restaura ambas pilas, guarda/reabre y recorre historia original. Publicación multidocumento prueba tres archivos con todas las fronteras, retry, tercer contenido, rutas inseguras/alias/carpeta ajena.

Pruebas filesystem exclusivamente tempdirs sintéticos dentro del host; no equivalen a terminación real de proceso, pérdida de energía o aceptación UI. Los tests desktop/dominio siguen sin ejecutarse por bloqueo Windows4551 histórico, no se reintentaron ni reubicaron binarios ni se cambió política. No ejecución de V1, modelos, medios, GUI ni regresión multimedia. No regeneración de goldens con V2.

## Límites de implementación que siguen abiertos

- Caja sigue la restricción V1 de items de un rango; conflictos jerárquicos no se resuelven por pérdida silenciosa. Caja de Secuencia exige una misma ocurrencia; para cruzar divisiones usar Fuente. Inventario de gestos/controles dinámicos completo pendiente.
- Autor: import explícito y candidatos locales de carpeta; el store global de V1 no se inspecciona automáticamente. No interfaz específica de conciliación entre sidecar/store ni edición de snapshot del master. Mantiene las protecciones de decisiones humanas comunes.
- Bloques: la exportación produce un paquete derivado NUEVO; no vuelve a escribir fuentes V1. No exporta aún todos los contratos de una carpeta V1. La recuperación de export está conectada mediante menú explícito, no por watcher. Recibo de publicación no equivale al archivado general de auditoría/recibos del proyecto.
- Master compartido reduce clones/hash de análisis, pero proyecciones, snapshots históricos, serialización/digests del proyecto y otros costos GUI permanecen. Snap en worker captura un clon de proyecto; no acredita PERF-01 ni presupuesto de memoria ilimitado.
- Derivación/mapping/manifests/requests/proposals/passes, export carpeta completa e inversa de montaje editado siguen pendientes. Jobs durables, archivado general, migraciones, watcher SO/V1 y resolución detallada de conflictos siguen pendientes. E4 íntegro pendiente; PreparedCommand no es MCP.

Fallos intermedios de compilación: se corrigió conversión String→LayerId del adaptador autor usando constructor tipado. Los controles finales reemplazan esos intentos. No se eludió ningún bloqueo de ejecución.


Build release final alpha.6: **OK en 2m04s**, exe compilado y no ejecutado; tamaño/SHA256/tiempo del wrapper en `evidence/build-alpha6.json` (desde evidence: `build-alpha6.json`). Sin paquete binario nuevo aceptado.
