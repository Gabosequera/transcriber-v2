# Matriz vigente — integración E2/E3/E4, continuación12

Las siguientes filas describen implementación actual. Las comprobaciones exactas del último árbol están en [continuacion-12](evidence/continuacion-12.md). Ninguna fila declara aceptación GUI/multimedia ni habilita E5.

| Requisitos | Resultado implementado | Verificación / aceptación |
|---|---|---|
| UI-02/03/04, MED-03 | Inspector persistente, paleta del registro, estados/razones de clips, preview preparado, snap/selección/autoscroll y cancelación | All-targets compila; interacción/foco/IME/DPI/gestos aplazados |
| MED-01/02/04/05, PERF-01 | Relink cancelable, índices/composición en workers, seek/player pendientes, grafo idéntico conserva reloj; COW/certificados; waveform adaptativo | Núcleo application verifica COW/diff/protección; sincronía/decoding/cache real y recursos aplazados |
| EXP-01/02/03 | Parámetros validados GUI/MCP, snapshot de composición, export durable, EDL/FCPXML representables | Compilado; no encoder, NLE ni multimedia ejecutados |
| LAY-01/02/03/04, DAT-01 | Conversación, derivación/streams, contratos/passes, avisos y adopción legacy; import/export V1 y watchers individuales | Adaptadores/tests compilados; recorridos RUN sin ejecutar; protección/bloques en application |
| DAT-02/03/04 | Save As portable, migración/codec, auditoría/recibos, recuperación/merge y undo | Fixtures application ejecutadas; crash físico y datos reales aplazados; límites declarados abajo |
| AI-01/02/03/04 | Servicio MCP real, schemas/permisos, consultas/contexto/jobs, preview/apply/verify y eventos/cliente |17 tests iniciales control pasaron; posterior4551 bloqueado, nuevos tests solo compilados. Cliente contra GUI pendiente |
| ML-01..03 | Fuera de alcance E2/E3/E4 | E5 no iniciada |

---

# Incremento vigente — continuación 12, durabilidad

Las filas siguientes distinguen código implementado, evidencia del núcleo y aceptación. No cierran por sí solas una etapa completa ni sustituyen la revisión global de E2/E3/E4. E5 no se inicia. Los pendientes de las tablas inferiores describen checkpoints históricos.

| Requisito | Implementación vigente | Evidencia | Aceptación / límites restantes |
|---|---|---|---|
| DAT-01 / DAT-04 | Save As materializa auxiliares SHA en el destino, conserva directorios vacíos y relocaliza proyecto/undo/redo. Copia archivo de auditoría y blobs referenciados | `source_bundles` y `storage_codec`: reapertura/export/undo con origen ausente o carpeta trasladada; corrupción rechazada | Portabilidad documental implementada y verificada sintéticamente; los medios y jobs de configuración no se vuelven portables automáticamente |
| DAT-01 / DAT-04 | Enriquecer un master legado sin bundle exige documento completo/asset/digest idénticos; añade solo la carpeta original ausente y conserva capas/decisiones humanas | [Siete tests dirigidos](evidence/application-legacy-enrichment-12.log), incluidos dos nuevos: rechazo de reemplazos, recovery del enriquecimiento, undo/redo y Save As sin origen | Ya no exige crear otro proyecto para incorporar la fuente ausente; reimportar análisis diferente sigue prohibido. Recorrido GUI/V1 solo compilado |
| DAT-02 / DAT-03 | `project/2` establece barrera de lector; legacy /1 migra al guardar con bases históricas coherentes. Validación de masters externos y rechazo de schemas/campos de almacenamiento desconocidos | Fixture legacy con historia/recibos, undo/redo/replay; tampering y descriptor forjado rechazados sin reescritura | Migración conocida implementada; no lectura de schemas futuros ni downgrade, crash físico pendiente |
| DAT-02 / DAT-04 | Auditoría por segmentos inmutables e índice publicado bajo la intención; autosave apunta a su índice verificado. Paginación por digest y recibos fríos sin borrar claves | 10002 recibos, reapertura/retry, Save As, cursores obsoletos, corrupción y fronteras sintéticas | Archivado implementado; máximo 64 MiB por segmento/índice y metadato de recibo; índice por clave crece en RAM, sin GC de archivo |
| DAT-02 / PERF-01 | Referencias de almacenamiento explícitas para masters/capas >64 KiB en proyecto, historia, autosave, commit, auditoría y recibos. SHA/tamaño/canonical antes de hidratar | Master y capa de 65 MiB conservan digest al reabrir; metadatos pequeños; colisiones escapadas y commit recuperado en cuatro fronteras | Superado el límite efectivo de esos payloads; metadatos project/autosave 64 MiB, history/intent 128 MiB, undo 200; no tamaño ilimitado del proyecto |
| PERF-01 / DAT-04 | `SharedVec` comparte items entre snapshots, copia al mutar y conserva el JSON. Lectura reutiliza capas verificadas; escritura cachea la última versión por capa | 100000 items: clone/Agent rename/historia/undo/redo compartidos, mutación aislada; reapertura de 1000 items comparte proyecto/historia | Compartición implementada y verificada; COW por capa completa, `extra` sin COW, materialización del journal/recovery y costes JSON pendientes de optimización/medición |
| PERF-01 / AI-03 | Diff precalculado dentro de `PreparedCommand`; preview y commit usan el mismo resumen. Certificados de validación exitosos por capa completa/duración, con igualdad exacta | Diff coincide en dry-run/commit/auditoría/recibo, incluso al subir revisión externa; metadata/base cambiadas e invariantes inválidas se rechazan con caché fría o caliente | Hasta 256 certificados, sin presupuesto en bytes; globals se revalidan siempre. Store/history/recovery y comandos específicos aún validan completamente |

Evidencia final ejecutada: [application-continuacion-12-final.log](evidence/application-continuacion-12-final.log), **70 unitarios correctos (28,38 s) y tres tests de integración de bloques correctos (0,00 s)**, tras los cambios de diff/validación. El [log previo](evidence/application-continuacion-12.log) conserva los 68 anteriores y el fallo/corrección de fixture. No acredita ejecución de dominio/desktop/V1compat, GUI, multimedia, modelos ni pérdida real de energía. PERF-01 y la aceptación física permanecen abiertos; no presentar archivado, migración conocida o portabilidad de auxiliares como código todavía pendiente.

---

# Histórico — continuación 11, alpha.8

| Requisito | Incremento implementado | Estado y límite |
|---|---|---|
| DAT-01 / LAY-01 | Snapshot V1, export por adaptadores/vistas, archivo de originales/manifests | Parcial: derivación/requests/passes y portabilidad/límites pendientes |
| DAT-02 / DAT-03 | Intención /2, copia binaria SHA por streaming, recuperación y recibos | Integrado al menú; tests sintéticos; aceptación física pendiente |
| PERF / DAT | Bundle compartido, master con digest completo cacheado; binarios fuera del historial | No completo: doble recorrido/hash, serialización textual y límites |

47 application tests ejecutados/pasan; resto compilado sin reintentar Windows4551. E2/E3 abiertas, E4 pendiente, E5 fuera de alcance. Evidencia vigente: evidence/continuacion-11.md.

---

# Histórico — continuación 10, alpha.7

**E2/E3 abiertas; E4 pendiente; E5 fuera de alcance.** Evidencia exacta: `evidence/continuacion-10.md`. Aceptación física aplazada.

| ID | Incremento implementado | Verificación y límites |
|---|---|---|
| UI-04/LAY-01/DAT-01 | Candidatos globales de autor, selector/adopción por digest y revisión, conservación de borrados; autoscroll semántico | GUI/all-targets compilados; V1compat bloqueado 4551 antes de ejecución. Inventario dinámico completo pendiente |
| LAY-03/DAT-01 | Inversa de montajes editados contiguos con A/V enlazado, archivo de originales invisibles, mapping y flatten de verificación | Tests compilados, no ejecutados; efectos/mezcla/overlays/huecos no representables se rechazan. Carpeta V1 completa pendiente |
| DAT-03/DAT-04 | Diff por campo, conflictos múltiples/orden, elección explícita, prepared commit con lock, undo y watcher Windows | Tests application; GUI y watcher compilados, físico pendiente; documentos V1 individuales sin watcher/reconcile aún |
| EXP-03/DAT-02 | Jobs de export durables, estados de recuperación, bloqueo de worker, fingerprints e historial de intentos/recibos | Tests de registro/locks/recuperación sintética en application; integración multimedia compilada, no ejecutada; jobs locales a configuración V2 |
| DAT-02/DAT-04/PERF-01 | Historial/2 por deltas integrado en autosave/commit/reapertura; migration lectura /1 | Test roundtrip/corrupción/undo/redo y tamaño sintético; RAM, clones/serialización y límites 200/64/128 MiB siguen pendientes |

No reclasificar las filas históricas como aceptación del build actual.

---

# Incremento histórico — continuación 9, alpha.6

E2 abierta/E3 parcial/E4 pendiente. Evidencia: `evidence/continuacion-09.md`. Las secciones inferiores son históricas.

| ID | Incremento real | Verificación / límite |
|---|---|---|
| UI-03/UI-04/LAY-02 | Caja Corte add/subtract, selección/edición de pedido, menú de carriles y movimientos | Tests application; GUI compilada, interacción pendiente; caja solo un rango como V1 |
| DAT-01/LAY-01 | Coalescencia strict overlap por enabled, actor/metadata/tombstones; carriles de fábrica protegidos | Tests box/coalesce/move/undo; falta inventario dinámico completo |
| DAT-01/LAY-01/LAY-04 | Sidecar autor schema 1, revisión, identidad, cuarentena, export; import explícito sin master | Test roundtrip/conflictos/decisiones; store global automático/diálogo especializado pendiente |
| LAY-01/LAY-02/DAT-01 | Inspector adapta vecinos; snap global-safe/2/evidencia; materialización de archivos por bloque | Tests chunks; no aceptación física ni carpeta V1 completa |
| DAT-02/DAT-04 | Recuperación de historia completa de autosave y reapertura con undo/redo | Test recovery_restores_full_undo_and_redo; crash real aplazado |
| DAT-02/DAT-03 | Publicación documental recuperable en carpeta V2 y recibo; CAS, rutas y todos los checkpoints | Tests documents, sin escritura en V1; no sustituye jobs/archivado/migraciones generales |
| PERF-01/AI-02/AI-03 | Master compartido inmutable/digest cacheado; prepared command conserva IDs y valida base al commit; workers | Tests shared/prepared/stale; GUI compilada; rendimiento físico y E4 abiertos |


---

# Incremento vigente — continuación 8, alpha.5

E2/E3 abiertas, E4 pendiente; aceptación física aplazada. Evidencia: `evidence/continuacion-08.md`. Las secciones inferiores son históricas.

| ID | Incremento | Estado / comprobación |
|---|---|---|
| UI-03/UI-04/LAY-02 | S/trim/nudge de items, jerarquía, menú/corte/arrastre, estados multicapa atómicos, ciclo autor, orden/selección/tipos de carril | Núcleo verificado por semantic_tests; GUI compilada, sin aceptación física |
| UI-04 | 140 constructores/bindings fuera del registro ACTIONS en siete fuentes V1 | Inventario AST trazable; handlers dinámicos/recorridos restantes pendientes |
| LAY-01/LAY-02/DAT-01 | Bloques contiguos, split/bordes/nudge con vecinos; selected-plan antes que view | Parcial; test chunks; snap/evidencia/materialización multidocumento pendientes |
| DAT-01/LAY-01 | Export completo del documento trims del medio desde un carril; cabecera/metadata/aceptación/IDs; crear pre-master | Tests V1compat; no equivale a export carpeta ni coalescencia V1 completa |
| DAT-02/DAT-04 | Historial/1 durable junto con proyecto/journal; reabrir undo/redo, continuidad/base; cuatro fronteras recuperables | Tests semantic_tests/store ejecutados. Recuperación de pila autosave y jobs siguen pendientes |
| DAT-02/DAT-03 | Autosave lock/CAS/propietario de recovery; dos instancias no sobrescriben recovery ajeno | Unitarios sintéticos ejecutados; crash/cierre físico pendientes |
| PERF-01 | Apertura/guardado/Save As en worker único acotado; GUI espera guardar y salir | Compilado; validación/hash/clones/history grandes y descubrimiento inicial siguen pendientes |
| AI-01/AI-02 | Actor Agent no produce edited/aceptación humana; comandos semánticos pasan por la misma sesión | Unitarios; permisos/MCP/propuestas/cliente E4 pendientes |

---
# Incremento vigente — continuación 7, alpha.4

E2/E3 abiertas; E4 pendiente. Aceptación física aplazada no equivale a cierre de implementación. Evidencia exacta: `evidence/continuacion-07.md`.

| ID | Incremento | Estado |
|---|---|---|
| UI-03/UI-04 | Import/export keymap/1; auditoría 69 V1/81 V2; copy/cut/paste/duplicate por contexto; Ctrl+A editorial | Parcial; keymap compila, GUI pendiente; aplicación prueba pegado y rollback |
| LAY-02 | Editor persistente de comentario/rangos/padre; copias con jerarquía y procedencia | Implementado sin aceptación GUI; tests application pasan |
| DAT-01/LAY-03 | Import worker/batch estricto; export de capa user/topics/ai y montaje original sin cambios | Parcial; tests V1 pasan; inverso editado/carpeta completa pendientes |
| DAT-02/DAT-04 | Recibos durables, conservación al Save As, autosave con auditoría y worker | Parcial; tests reopen/recuperación pasan; historial/jobs/migraciones/multidocumento pendientes |
| PERF-01 | IO/probe editorial y escritura autosave/export documental fuera de GUI | Parcial; clones/validación master, apertura/guardar manual siguen costosos |
| AI-01/02 | Nuevos comandos comparten sesión/dry-run/protección/undo | Base interna; E4 pendiente |

---

## Registro anterior (continuación 6 e histórico)

# Matriz de requisitos

## Incremento vigente — continuación 6, alpha.3

E2/E3 siguen abiertas y E4 pendiente. Las filas históricas siguientes conservan su evidencia de otro equipo; no implican aceptación de este build. Evidencia del incremento: `evidence/continuacion-06.md` y logs enlazados allí.

| ID | Implementación añadida | Estado / control |
|---|---|---|
| DAT-01 / LAY-04 | Master original completo con digest y proyecciones de palabras/intervenciones/señales protegidas | Parcial; tests master/import V1. Export completo, chunks y versionado de master pendientes |
| LAY-01 / LAY-02 | Aceptación de trims separada de enabled; capas de análisis/bloqueadas y decisiones humanas protegidas; tombstones de descendientes | Parcial; tests V1/protection. Edición completa de jerarquías/chunks/derivación pendiente |
| MED-02 / UI-04 | Shift+T salta trims; navegación de silencios importados; añadir tema y multirrango sin sus huecos | Implementado sin aceptación física; Clippy/all-targets. Tests review de dominio compilados, no ejecutados por bloqueo 4551 |
| LAY-03 | Inspector muestra y permite elegir cada ocurrencia por clip/rango | Implementado sin aceptación GUI; montaje inverso/export V1 pendientes |
| DAT-02 | Validación de snapshots; intención recuperable proyecto+journal; recuperación de autosave de proyectos sin carpeta | Parcial; tests store de tres fronteras y corrupción. Multidocumento V1/migraciones/idempotencia durable pendientes |
| DAT-03 / DAT-04 | Rescaneo estable en worker, merge de tres vías, resumen de diff/aprobación en GUI, revalidación de ambas bases y undo | Parcial; tests reconcile/store pasan. Watcher SO/documentos V1/editor de conflictos y flujo físico pendientes |
| AI-01 / AI-02 | Protección humana/evidencia común para cambios externos, imports y batches | Base interna; permisos/MCP/propuestas/capabilities/cliente E4 aún pendientes |
| OPS-01 / PKG-01 | Build release local y prerelease de fuentes | Control exacto en build-continuacion-06.log/PUBLISH. No es paquete binario aceptado |

## Registro histórico — continuación 5

**Actualización continuación 5:** implementación hasta E4 autorizada; testing real y revisión general aplazados por el usuario. E3 parcial/E4 pendiente. Se conservan las filas/evidencia histórica debajo. Nuevos alcances:

| ID | Incremento alpha.2 | Estado y evidencia |
|---|---|---|
| UI-03/UI-04 | Captura/edición/restauración de atajos; rechazo de conflictos; persistencia atómica | Implementado; keymap compilado, ejecución bloqueada por Windows 4551; interacción real pendiente |
| DAT-02 | Recuperación explícita con undo; guardado CAS+lock | Parcial; store/session unitarios; crash/multidocumento/journal/migraciones pendientes |
| DAT-03 | Detectar escritura externa al guardar y rechazar sin pisarla | Parcial; dos stores/JSON incompleto/lock en unitarios; watcher/diff/reconcile pendientes |
| DAT-04 | Historial multistep y rutas estables tras Save As | Parcial; session unitarios; flujo real pendiente |
| AI-01/02/03 | Validación protocolo/proyecto/digest, identidad de retry, diff ampliado, límites | Base interna implementada; permisos/MCP/propuestas/cliente E4 pendientes |
| OPS-01/PKG-01 | Wrapper MSVC portable y templates de guiones | Compilación/expansión controladas; release de fuentes, sin nuevo paquete probado |

Detalles y logs exactos: `evidence/continuacion-05.md`. No reclasificar las pruebas físicas históricamente pendientes como completadas.


Estados: `pendiente`, `implementado-sin-verificar`, `verificado`, `fallo`, `bloqueado`, `opcional-no-incluido`. «Verificado» exige comando/build/caso identificables y artefacto en `implementation/evidence/`.

Build de referencia: `cargo build --release --locked -p transcriptor` (Rust1.98.1, egui0.36.1), 2026-09-08. Workspace inicialmente sin Git; repositorio público inicializado por autorización posterior (ver PUBLISH). Host: `evidence/e0/initial-state.md`. Checkpoint continuación4: evidencia `evidence/e2/continuacion-04.md`; E2 abierta. latest-regression.json apunta a JSON215404 preparados, NO ejecutados; última regresión aceptada160410.

| ID | Requisito | Estado | Implementación / símbolo | Prueba | Evidencia |
|---|---|---|---|---|---|
| UI-01 | Ventana nativa y paquete ejecutable | verificado exe y paquete provisional en host; Windows limpio pendiente | main.rs, build_windows.py | E1 y smoke en TEMP Unicode/PATH aislado | evidence/e2/regression-20260908-160410/, package-smoke-20260908-120555/ (ZIP anterior a continuación4) |
| UI-02 | Diseño y estados usables | implementado-sin-verificar global; capturas reales con controles responsivos y toasts expirados, DPI OS pendiente | ui_panels.rs, ui_timeline.rs | selection-20260908-160410/caches.json y capturas inspeccionadas | evidence/e2/continuacion-03.md; dimensiones reales en logs, no DPI OS |
| UI-03 | Foco, texto y atajos | implementado-sin-verificar | `keymap.rs` (`keymap/1`, AltGr, conflictos), `app.rs::handle_keys` (`egui_wants_keyboard_input`) | `cargo test -p transcriptor` (4 tests) ; pendiente prueba IME/layout ES manual | tests |
| UI-04 | Paridad de acciones V1 | pendiente (inventario iniciado: 71 IDs registrados con defaults V1; handlers para 60; 6 devuelven aviso «pendiente E2/E3») | `keymap.rs::registry`, `app.rs::dispatch` | `keymap::tests::defaults_have_no_conflicts_and_keep_v1_ids` | `v1-action-inventory.md` (pendiente) |
| MED-01 | Import/probe/relink | verificado parcialmente en host: video/audio/imagen/Unicode/relink, rotación90 y worker import asíncrono; cancelación en IO bloqueado pendiente | ffmpeg.rs, import_jobs.rs, app.rs::poll_import, RelinkAsset | tests reales + kittest duplicados/edición concurrente/proyecto cambiado | evidence/e2/async-import.log, media-vfr-rotation.log, vfr-rotation-20260908-215336/ |
| MED-02 | Transporte | verificado (host): 1×/2×/3×/4×, skim 8× sin audio, pausa, seek, frame step exacto, loop IN/OUT, parada al final; política de audio visible. Pendiente: medir sincronía A/V (F-001) | `tv2-media::player`, `audio_policy` | `tests/scripts/e2-transporte.json` (asserts de posición/velocidad) | `evidence/e2/e2-transporte.log`, `transporte-*.json/png` |
| MED-03 | Timeline y gestos | verificado host: drag/cancel/trim/scrub, markers/snap/autoscroll, insert de grupo enlazado y undo; prueba manual pendiente | ui_timeline.rs, ui_markers.rs, timeline_index.rs, commands.rs | gesture_tests.rs, domain tests, guiones E1/E2 | evidence/e2/workspace-final.log, latest-regression.json |
| MED-04 | Composición y mezcla | verificado en host: dos videos+alfa+audio/PiP y mezcla/mute/solo/gain contra fuente, preview y WAV; revisión final integrada E2 pendiente | compositor.rs, AudioMixer, resolve.rs | PSNR 720/1080; mute_solo_and_gain_match_preview_and_pcm_export | evidence/e2/mix-equivalence.log, regression-20260908-160410/ |
| MED-05 | Mapping temporal | verificado en fixtures: flicks, fuera keyframe, TS inicio≈5s, audio+250ms, VFR y rotación; sincronía física pendiente F-001 | decoder/render/audio/time | 40 cuadros exactos contra PTS nativos; 298 exportados con PSNR mínimo28dB | evidence/e2/media-vfr-rotation.log, vfr-rotation-20260908-215336/verify.log, sync-export-measurement.txt |
| LAY-01 | Contratos y estados | verificado con fixture V1 auténtica (aceptado/propuesto/desactivado, `enabled` independiente de `accepted` en recortes, tombstone `item-borrado`, bloques sin aceptación) | `layers.rs`, `tv2-v1compat::{layers,trims}` | `crates/v1compat/tests/golden_v1.rs`, `trims::tests`, `commands::tests::layers_follow_v1_rules` | `tests/fixtures/v1/demo-a`, `evidence/e2/import-v1-*` |
| LAY-02 | Rango/jerarquía | verificado (multirrango + jerarquía en fixture V1; puntos solo en marcas del autor, D-0012; 32 niveles) | `layers::validate_items`, `tv2-v1compat::layers` | `layers::tests`, `golden_v1.rs` | `evidence/e2/import-v1-02-fuente-capas.png` |
| LAY-03 | Montaje V1 | implementado-sin-verificar → verificado parcialmente (aplanado idéntico al calculado por V1 en la fixture: tapado, compactación, repetición; conversión explícita D-0011; export del montaje 7,000 s). Pendiente: revelar en fuente por ocurrencia desde la GUI y comparación de píxeles V1↔V2 (V1 no ejecutado) | `tv2-v1compat::montaje` | `montaje::tests` (porta `test_montaje.py`), `golden_v1.rs` | `evidence/e2/import-v1-01-secuencia.png`, `import-v1-montaje-720p.mp4` |
| LAY-04 | Antes/después de modelos | implementado-sin-verificar (capa manual antes de transcribir verificada; master real pendiente E3) | `Command::CreateLayer`, `app.rs::add_range_to_layer` | guion E1 (`08-estado-capa.json`) | `evidence/e1/08-capa-manual-aceptada.png` |
| DAT-01 | Round-trip V1 | verificado parcialmente: fingerprint V2 == V1 sobre el mismo archivo real, `source_master_digest` igual, round-trip canónico idéntico de `editorial-layer/1` y `editorial-trims/1`; pendiente escritura de vuelta desde la GUI y montaje→V1 (E3) | `digest.rs`, `ffmpeg::v1_inventory_json`, `tv2-v1compat` | `golden_v1.rs`, `layers::tests::round_trip_*`, `trims::tests::round_trip_*` | `tests/fixtures/v1/demo-a/expected-v1.json` |
| DAT-02 | Durabilidad | implementado-sin-verificar (escritura atómica, parcial rechazado; crash en frontera pendiente) | `store.rs::atomic_write` | `store::tests` | tests |
| DAT-03 | Edición externa | pendiente | — | — | — |
| DAT-04 | Undo/redo | implementado-sin-verificar (gesto/batch/nueva revisión/stale verificados en tests y guion; export no borrado por undo por diseño: no hay inversa) | `session.rs` | `session::tests`, guion E1 (undo/redo Aceptar) | `evidence/e1/08-estado-capa.json` |
| AI-01 | Comandos comunes | implementado-sin-verificar (mismo `ProjectSession::execute` para GUI y guion; cliente externo E4) | `session.rs::CommandEnvelope` | guion E1 | — |
| AI-02 | Dry-run y autorización | implementado-sin-verificar (dry-run existe; permisos E4) | `session.rs::dry_run` | `session::tests::failed_batch_leaves_project_untouched_and_dry_run_does_not_commit` | tests |
| AI-03 | Idempotencia/conflictos | implementado-sin-verificar | `session.rs::execute` | `session::tests::stale_base_is_rejected_and_idempotent_retry_replays` | tests |
| AI-04 | Estado en tiempo real | pendiente | — | — | — |
| EXP-01 | Export exacto | verificado parcialmente: cortes/IN-OUT/selección/repetición/VFR/rotación/fronteras; A/V export medido; regresión completa de exe actual pendiente | ExportJob, extract_ranges, decoder | todos178cuadros VFR y120rotados contra PTS nativos; primero/último incluidos | evidence/e2/vfr-rotation-20260908-215336/verify.log, latest-selection.txt; latest-regression.json preparado no ejecutado |
| EXP-02 | Matriz de formatos | verificado host:13 presets reales H264720/1080/2160/vertical,HEVC,AV1,ProRes,VP9WebM,MKVH264,NVENC,WAV,FLAC,MP3; NVENC depende del hardware | export.rs::presets, ffmpeg::Capabilities | export tests reales/ffprobe | evidence/e2/format-matrix.txt, workspace-final.log |
| EXP-03 | Jobs y fallos | verificado parcialmente: cola/snapshot/cancel/fallos; cierre activo sin huérfanos/parciales; repetir smoke tras import worker | CancellableChild, CleanupPaths, shutdown_workers | FFmpeg bloqueado esperando stdin + cierre exe durante4K/cola/cache | evidence/e2/shutdown-smoke.log, shutdown-20260908-173501/, process-cancel-test.log, workspace-import-vfr.log |
| ML-01..03 | Inferencia/etapas/supervisión | pendiente (E5) | — | — | — |
| PERF-01 | Fluidez y recursos | implementado-sin-verificar global: P95CPU UI10k/100k0,424/0,433ms; reservas cache256MiB/cola64/2workers/import asíncrono; RAM total/VRAM/presentación/input/seek/reposo pendientes | resolve.rs, cache.rs, import_jobs.rs, timeline_index.rs | n200/caso release kittest, oráculo resolver anterior; admisión/liberación/reintento cache | evidence/e2/dense-ui-benchmark.log, cache-memory-admission.log, async-import.log |
| OPS-01 | Diagnóstico | implementado-sin-verificar (errores con código/acción, consola con filtro, log rotado, redacción de secretos) | `error.rs`, `console.rs` | — | `%LOCALAPPDATA%\Transcriptor\logs` |
| PKG-01 | Distribución | verificado paquete alpha en host, descarga FFmpeg separada; E6/licencias finales/Windows limpio V2 pendientes | build_windows.py --public-release, Install-FFmpeg.ps1, VERIFY | 575hashes, instalación idempotente, PSNRfuente42,6–42,8/visor30,6/hueco0 | evidence/e2/package-smoke-20260908-181823/, publication-build-final.log, PUBLISH.md |

## Requisitos descubiertos en V1 (se amplían en E2/E3)

| ID | Requisito | Estado |
|---|---|---|
| V1-KM-01 | 67 acciones `keymap.py` con IDs y defaults | implementado-sin-verificar (registro + 4 acciones nuevas: copiar/cortar/pegar/duplicar, ripple, `layers.add_range`, `file.*`) |
| V1-MT-01 | `editorial-montaje/1` insert/overwrite/ripple/split/trim semántica | implementado-sin-verificar sobre el modelo V2 (`commands::tests` reproduce `test_montaje.py` para add/split/trim/move/remove) |
| V1-IO-01 | `digest_json` y `fingerprint` compatibles | verificado (goldens Python 3.14) |
