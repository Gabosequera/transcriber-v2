# Inventario de acciones V1 → V2 (UI-04)

Fuente V1: `keymap.py` (`ACTIONS`, 70 registros), `editor_medios.py::_armar_handlers`, `editorial_layers_ui.py::accept`, `editorial_montaje_ui.py`, `toolbar_ui.py` (revisión `c3677ba`). V2: `apps/desktop/src/keymap.rs::registry` (mismos IDs y acordes por defecto) y `app.rs::dispatch`.

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
| nav.prev_silence/next_silence | Ctrl+↑/↓ | requiere capa de silencios | aviso «E5» | aviso | — |
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
| edit.edit | Return, F2 | editar etiqueta | diálogo renombrar | impl | — |
| edit.deselect | Escape | deselecciona (y cancela gesto) | `Selection::clear` + `cancel_gesture` | impl | kittest `escape_cancels…` |
| edit.undo / edit.redo | Ctrl+Z / Ctrl+R, Ctrl+Shift+Z, Ctrl+Y | historial multidocumento con revisión | `ProjectSession::undo/redo` | impl | session tests, E1 |
| tools.select / tools.cut | A, V / B | herramienta | `Tool::Select/Cut` (corte = clic divide) | impl | — |
| tools.select_all | Ctrl+A | selecciona todo el carril | `dispatch` (pista del clip seleccionado o todos) | impl | — |
| layers.new_lane | Ctrl+N | añadir capa | diálogo `CreateLayer` | impl | E1 |
| view.zoom_in/out | + = Num+ / − Num− | zoom | `TimelineView::zoom_by` | impl | — |
| view.fit / view.zoom_sel | Shift+Z / Z | ver todo / zoom selección | impl | impl | E1 |
| view.follow / view.center | F / C | seguir/centrar playhead | impl | impl | — |
| view.skip_trims | Shift+T | saltar recortes al reproducir | aviso «E3» (requiere capa de recortes) | aviso | — |
| loop.set_in/out/clear | (regla: clic der., Ctrl, Shift) | loop | `SetLoop` + regla | impl | E2 transporte |
| view.mode_montage | Ctrl+M | Fuente/Montaje | `toggle_view` (Fuente/Secuencia) | impl | — |
| montage.add_selection | Ctrl+Shift+A | añade rango/item al montaje | `InsertAssetLinked{source}` al final | impl | — |
| montage.add_topic | Ctrl+Shift+T | añade tema completo | aviso «E3» (capas de temas) | aviso | — |
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
