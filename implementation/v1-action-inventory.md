# Actualización vigente — continuación 11

| Entrada | Comportamiento conectado | Verificación |
|---|---|---|
| Archivo → Exportar carpeta documental V1 → capas | Snapshot inmutable del medio/capas, adaptadores, vistas, originales/manifests, transacción de carpeta nueva | Test adaptador compilado; GUI aplazada |
| Misma entrada → capas y montaje activo | Añade inversa y comprueba identidad del medio | Inversa compilada; GUI aplazada |
| Archivo → Recuperar exportación V1 interrumpida | Recupera ahora intents /1 textuales y /2 con binarios verificados | Test application de fronteras/origen/destino ejecutado |

No tienen nuevo atajo por defecto. Comparten núcleo de proyecto/adaptadores/publicación; no crean otro estado editorial. El inventario dinámico completo, contratos padre/hijo/passes y E4 siguen pendientes. V1 solo leído, no ejecutado.

---

# Histórico — continuación 10

La revisión dinámica completa sigue abierta. No se ha ejecutado V1 ni su UI.

| Fuente V1 | Entrada V2 / comportamiento | Control actual |
|---|---|---|
| marcas.resolver/store_path/app_paths.MARKS_STORE_DIR | Archivo → Buscar y resolver marcas; import carpeta consulta stores; selector revisión/digest/adopción | author_candidates test escrito/compilado, ejecución 4551 bloqueada; GUI compilada |
| marcas.adoptar / sidecars divergentes | Comparación de colecciones y adopción explícita; preserva tombstones/revisión, undo común | compile; aceptación de interacción pendiente |
| editorial_montaje persist/flatten | Export de montaje editado contiguo, originales invisibles preservados con estado y mapping | prueba inversa nueva compilada, no ejecutada |
| editorial_layers_ui gesto al borde | Autoscroll de EditItems y BoxEdit ajusta origen en pantalla para conservar origen temporal | UI compilada; procedimiento RUN |
| editorial_history / cambios externos | Diff por campo/orden, resolución explícita y worker+undo | tests application; aceptación GUI pendiente |
| export queue / cierre | Persistir antes de encoder, recuperación explícita y recibos por intento | jobs application; multimedia pendiente |

Las tablas históricas de abajo no describen el estado completo de alpha.7. Pedidos/propuestas/passes/manifests/derivación y controles dinámicos restantes siguen pendientes; no declarar paridad por contar entradas.

---

# Auditoría vigente — continuación 8

Registro ACTIONS heredado: 69 V1/81 V2, sin IDs V1 ausentes; no se usa como criterio de cierre. Nuevo inventario AST de **140 constructores/bindings** fuera de ACTIONS: `scripts/audit_v1_ui.py` → `evidence/continuacion-08-ui-inventory.json`, con archivo/línea/función/argumentos. No ejecuta V1. No enumera automáticamente todos los controles creados por bucles/helpers; no declara paridad funcional completa.

| Fuente/símbolo V1 | Diferencia encontrada | Implementación actual / prueba | Pendiente |
|---|---|---|---|
| editorial_edits.split_layer_item; editorial_layers_ui.split | S solo dividía clips | SplitItem, árbol/multirrango/evidencia; semantic_tests | Snap/materialización del plan |
| editorial_layers_ui.trim_edge/nudge/nudge_selection | Items sin trim/nudge; multicapa podía aplicar parcial | TrimItem/ShiftItems y batches GUI; semantic_tests | Editor de bloques debe adaptar vecinos también al cambiar varios rangos desde inspector |
| editor_medios._marca_ciclar/_aplicar_decision | X solo desactivaba marcas | CycleAuthorDecision; punto→región; semantic_tests | Lectura/escritura del sidecar autor |
| editorial_layers_ui.accept | Shift+E avanzaba aun fallando aceptar; multicapa no atómica | set_selected_items_state devuelve éxito; batch | Aceptación física |
| editorial_layers_ui.move_lane/new_lane_dialog | Orden no accesible; crear solo User | Biblioteca/cabecera ↑↓; selector de tipos; SetLayerOrder exige totalidad | Borrado/movimiento entre carriles según clase V1 |
| editorial_layers_ui mouse/EDGE_PX/CLICK_PX | Drag sobre item solo seleccionaba | EditItems con base/rango/borde, guía temporal, release/Escape | Caja Corte unión/resta; preview geométrico completo/autoscroll semántico |
| editorial_edits.box_add/box_subtract | Falta crear/estirar/fundir/restar con caja Corte | Inventariado, sin implementación | Requiere conservación de propiedades/procedencia en merge |
| editorial_montaje_ui tool/click | Corte con clic dividía solo una parte A/V | split_at_playhead común a teclado y herramienta | Aceptación física |
| editorial_layers_ui.persist (trims) | Export de carril descartaría cabecera/otros carriles | Export trims completo, header único normalizado; V1compat tests | Coalescencia V1, documentos antiguos sin cabecera |
| editorial_layers_ui.persist (bloques) | Plan no importado; bordes independientes rompen cobertura | Plan elegido, cobertura, SplitItem/TrimItem/ShiftItems con vecinos | Snap seguro/evidencia, materialización/derivación |
| app.py/keymap_ui/toolbar_ui | Controles de ajustes/export/navegación | AST conserva referencias; keymap de alpha.4 conservado | Contrastar recorridos dinámicos, no contar registros como pruebas |
| automatico_ui.py | Controles de pipeline y requests | AST inventariado; no se ejecuta ni instala backend | Requests E4; modelos/pipeline E5 fuera del alcance |

Las tablas siguientes son históricas y no sustituyen los límites anteriores.

---
# Auditoría vigente de UI-04 — continuación 7

Lectura estática real de `../transcriber/keymap.py` (AST, sin ejecutar V1) y registro Rust: **69 acciones V1, 81 V2, ninguna V1 ausente**. `montage.export` añade Ctrl+E; el resto conserva defaults. Detalle por ID en `evidence/continuacion-07-action-audit.json`. Registro no implica handler completo ni aceptación GUI.

Corregidos recorridos reales: copiar/cortar/pegar/duplicar items y descendientes; Ctrl+A de carril; corte condicionado a copia válida; batch de borrado multicapa; pegado/duplicado de clips enlazados preserva nombre/efectos/ganancia/enabled/extras; F2/Enter e inspector abren editor persistente de texto/comentario/rangos/padre; import/export keymap/1 en Ajustes. Símbolos: `PasteItems`, `PasteClips`, `SetItemStructure`, `ItemEditor`, `Keymap::import_json`. Pruebas en application y V1compat; pruebas desktop solo compiladas.

**Pendientes funcionales reales:** `edit.split` sigue siendo de clips, no divide items/bloques; trim/nudge semánticos y ciclo X de autor no completos; carriles/gestos/contextos y los botones fuera de ACTIONS requieren completar inventario. Import editorial es ahora worker/batch; export documental GUI es limitado, no carpeta V1 completa. No declarar UI-04 terminada contando brazos de dispatch.

---

# Inventario de acciones V1 → V2 (UI-04)

Fuente V1: `keymap.py` (`ACTIONS`, 69 registros), `editor_medios.py::_armar_handlers`, `editorial_layers_ui.py::accept`, `editorial_montaje_ui.py`, `toolbar_ui.py` (revisión original `c3677ba`, relectura en `bb7012c`). V2: `apps/desktop/src/keymap.rs::registry` (mismos IDs y acordes por defecto) y `app.rs::dispatch`.

Estados: **impl** = handler V2 funcional; **impl-parcial** = funciona con límites indicados; **aviso** = registrado y con atajo, muestra aviso explícito «pendiente E-n»; **decisión** = comportamiento distinto documentado.

| ID V1 | Defaults V1 | Precondición / comportamiento V1 | V2 (comando / UI) | Estado | Prueba |
|---|---|---|---|---|---|
| transport.play_pause | space | play/pausa | `PlayerCommand::TogglePlay`; visor clic; toolbar | impl | guion E1/E2 |
| transport.pause | K | pausa si reproduce | `Pause` | impl | E2 transporte |
| transport.faster | L | ×1→×2→×3→×4→×8; pausado = play ×1 | `rate_step(+1)` | impl | E2 transporte (asserts de rate) |
| transport.slower | J | baja velocidad; en ×1 pausa (no reproducción inversa) | `rate_step(-1)` | impl | — |
| transport.rate_1..4 | 1–4 | velocidad exacta | `SetRate` | impl | E2 transporte |
| transport.skim | Shift+L | ×8 solo keyframes | `SetRate(8)` + `Play`, `-skip_frame nokey`, sin audio | impl | E2 transporte |
| transport.play_from_item | Shift+space | play desde item o IN | `dispatch` | impl | — |
| nav.frame_prev/next(_10) | , . Shift+, Shift+. | paso de fotograma | `StepFrames(±1/±10)` | impl | E2 transporte |
| nav.step_prev/next(_5) | ←/→ (Shift) | 0,5 s / 5 s | `seek` | impl | E1 |
| nav.home/end | Home/End | inicio/fin del medio | `seek` | impl | — |
| nav.prev_edge/next_edge | ↑/↓ | bordes de clips/items | `edges()` | impl | — |
| nav.prev_silence/next_silence | Ctrl+↑/↓ | requiere capa de silencios | `navigate_silence`, sobre silencios importados | impl | compilado; física pendiente |
| nav.goto | Ctrl+G | diálogo ir a tiempo | diálogo `goto_dialog` (`parse_time`) | impl | — |
| nav.sel_start/sel_end | Shift+I/O | ir a inicio/fin de selección | `selected_range` | impl | — |
| edit.split | S | divide en playhead (clip seleccionado o bajo playhead) | `SplitClip` (+ enlazados) | impl | kittest `keyboard_split…`, E1 |
| edit.trim_start/trim_end | [ ] | recorta borde al playhead | `TrimClip` | impl | commands tests |
| edit.nudge_prev/next(_10) | Alt+←/→ (Shift) | empuja fotogramas | `ShiftClips` | impl | — |
| edit.item_prev/next | Shift+Tab / Tab | item anterior/siguiente del carril | `select_neighbor` | impl | — |
| edit.accept | E | acepta; los bloques no admiten; aceptar aceptado no vuelve a propuesto | `SetItemState(Accepted)` con `NotAvailable` en bloques | impl | domain `layers_follow_v1_rules`, E1 |
| edit.accept_next | Shift+E | acepta y avanza | idem + `select_neighbor(1)` | impl | — |
| edit.toggle | X | desactiva; en marca cicla decisión | `SetItemState(Disabled)` / `SetClipEnabled(false)`; ciclo de marcas del autor pendiente E3 | impl-parcial | E1 |
| edit.activate | P | vuelve a propuesto | `SetItemState(Proposed)` / `SetClipEnabled(true)` | impl | — |
| edit.delete | Delete, BackSpace, D | borra (tombstone en capas) | `DeleteItems` / `RemoveClips{ripple:false}` | impl | E2 escenario 1 |
| edit.edit | Return, F2 | editar etiqueta | `ItemEditor` con texto/comentario/rangos/padre y base de revisión | impl | application estructura; GUI pendiente |
| edit.deselect | Escape | deselecciona (y cancela gesto) | `Selection::clear` + `cancel_gesture` | impl | kittest `escape_cancels…` |
| edit.undo / edit.redo | Ctrl+Z / Ctrl+R, Ctrl+Shift+Z, Ctrl+Y | historial multidocumento con revisión | `ProjectSession::undo/redo` | impl | session tests, E1 |
| tools.select / tools.cut | A, V / B | herramienta | `Tool::Select/Cut` (corte = clic divide) | impl | — |
| tools.select_all | Ctrl+A | selecciona todo el carril | `dispatch` (pista del clip seleccionado o todos) | impl | — |
| layers.new_lane | Ctrl+N | añadir capa | diálogo `CreateLayer` | impl | E1 |
| view.zoom_in/out | + = Num+ / − Num− | zoom | `TimelineView::zoom_by` | impl | — |
| view.fit / view.zoom_sel | Shift+Z / Z | ver todo / zoom selección | impl | impl | E1 |
| view.follow / view.center | F / C | seguir/centrar playhead | impl | impl | — |
| view.skip_trims | Shift+T | saltar recortes al reproducir | `SetSkipTrims` + player/skip_ranges | impl | continuación 6; física pendiente |
| loop.set_in/out/clear | (regla: clic der., Ctrl, Shift) | loop | `SetLoop` + regla | impl | E2 transporte |
| view.mode_montage | Ctrl+M | Fuente/Montaje | `toggle_view` (Fuente/Secuencia) | impl | — |
| montage.add_selection | Ctrl+Shift+A | añade rango/item al montaje | `InsertAssetLinked{source}` al final | impl | — |
| montage.add_topic | Ctrl+Shift+T | añade tema completo | `add_topic_to_sequence`, batch multirrango | impl | continuación 6; física pendiente |
| montage.reveal_source | Ctrl+Shift+R (doble clic clip) | ver clip en fuente | `reveal_in_source` (posición fuente exacta; en Fuente marca IN/OUT del clip) | impl | — |
| montage.move_up/down | Ctrl+Shift+↑/↓ | subir/bajar pista | `ShiftClips{track_delta}` | impl | — |
| montage.export | — | exportar montaje | diálogo export (Ctrl+E nuevo) | impl | E1/E2 |
| marks.point / in / out | M / I / O | marca puntual / IN / OUT | `AddItem` en capa «Marcas del autor» / `in_point` `out_point` | impl | E1 (IN/OUT) |

## Acciones nuevas en V2 (sin equivalente V1)

| ID | Acorde | Motivo |
|---|---|---|
| edit.delete_ripple | Shift+Delete | ripple explícito (spec §2) |
| edit.copy / edit.cut / edit.paste / edit.duplicate | Ctrl+C/X/V/D | spec §2, respetan contexto (clip/tramo) |
| layers.add_range | Ctrl+Return | crear tramo IN/OUT en la capa seleccionada (V1 lo hacía con marcas) |
| file.open / file.save / file.import | Ctrl+O / Ctrl+S / Ctrl+I | proyecto V2 |
| montage.export | Ctrl+E | V1 no tenía acorde |

## Gestos y botones V1 pendientes de inventariar (E3)

`editorial_layers_ui.py` (arrastre de items, redimensionar rangos, menú contextual de capa, administración de capas), `editorial_montaje_ui.py` (arrastrar clips entre pistas, doble clic = revelar), `automatico_ui.py` (pipeline, pedidos/propuestas) y `app.py` (ajustes, wizard). Se completan al importar contratos V1 (E3) y el ciclo de propuestas (E4).


## Continuación 9 — ampliaciones vigentes

- `editorial_layers_ui.apply_box`, `editorial_edits.box_add/box_subtract` → Command::BoxEdit, B+arrastre/Shift, control semántico de timeline; tests box_union_subtract. Ctrl mueve, handles recortan, S divide. Caja solo un rango (V1); bases y cancelación compiladas en GUI, sin ejecutar.
- `editorial_trims.coalesce/remove_lane` → CoalesceTrims/MoveTrimItems/RemoveTrimLane; menú de cabecera, controles de misma identidad y carriles de fábrica; tests coalesce/lane/undo.
- `marcas.validar_marca/_doc_valido` → v1compat::author; Archivo → Importar marcas del autor V1 / export capa; tests identidad/revisión/cuarentena/punto-región. X ya cicla nota/incluir/excluir desde alpha.5; la fila histórica que dice pendiente E3 no describe este incremento.
- `editorial_chunks.snap_plan_to_safe_boundaries/materialize` → SnapBlockBoundaries en worker y materialize::blocks + documents::publish; cabecera / export bloques. Tests snap/archivos/recuperación, sin validación física.

Esto no termina el inventario dinámico V1 completo ni certifica paridad física.
