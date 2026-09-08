//! Paneles: menú, biblioteca (izquierda), visor y transporte (centro),
//! inspector (derecha), timeline (abajo), consola plegable, diálogos y toasts.

use crate::app::{Severity, Tool, TranscriptorApp, ViewMode, colors, describe_asset};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, Vec2};
use tv2_domain::asset::AssetKind;
use tv2_domain::ids::AssetId;
use tv2_domain::layers::ItemState;
use tv2_domain::time::{Ticks, TimeRange};
use tv2_domain::{Command, Transform};
use tv2_media::player::{PlayerCommand, SPEEDS, audio_policy};

pub fn draw(app: &mut TranscriptorApp, root: &mut egui::Ui) {
    let ctx = root.ctx().clone();
    menu_bar(app, root);
    egui::Panel::top("toolbar").show(root, |ui| toolbar(app, ui));
    let console_open = app.ui.console_open;
    if console_open {
        egui::Panel::bottom("console").resizable(true).default_size(170.0).min_size(80.0).show(root, |ui| {
            console(app, ui);
        });
    }
    let timeline_max = (root.available_height() * 0.5).max(140.0);
    let side_max = (root.available_width() * 0.24).max(180.0);
    egui::Panel::bottom("timeline").resizable(true).default_size(340.0).min_size(140.0).max_size(timeline_max).show(root, |ui| {
        // el timeline pinta con painter: reservar todo el alto disponible para que el panel no se encoja a su mínimo
        ui.set_min_height(ui.available_height());
        timeline_header(app, ui);
        crate::ui_timeline::draw(app, ui);
    });
    egui::Panel::left("library").resizable(true).default_size(260.0).min_size(160.0).max_size(side_max).show(root, |ui| {
        library(app, ui);
    });
    egui::Panel::right("inspector").resizable(true).default_size(300.0).min_size(180.0).max_size(side_max).show(root, |ui| {
        inspector(app, ui);
    });
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_rgb(14, 14, 16))).show(root, |ui| {
        viewer(app, ui);
    });
    dialogs(app, &ctx);
    crate::ui_markers::draw(app, &ctx);
    toasts(app, &ctx);
}

fn menu_bar(app: &mut TranscriptorApp, root: &mut egui::Ui) {
    let ctx = root.ctx().clone();
    let ctx = &ctx;
    egui::Panel::top("menu").show(root, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            let item = |ui: &mut egui::Ui, app: &TranscriptorApp, id: &str| -> bool {
                let text = app.keymap.label(id).to_string();
                let shortcut = app.keymap.pretty(id);
                let b = egui::Button::new(text).shortcut_text(shortcut);
                ui.add(b).clicked()
            };
            ui.menu_button("Archivo", |ui| {
                if ui.button("Nuevo proyecto").clicked() {
                    app.new_project();
                    ui.close();
                }
                if item(ui, app, "file.open") {
                    app.open_project_dialog();
                    ui.close();
                }
                if item(ui, app, "file.save") {
                    app.save_project(false);
                    ui.close();
                }
                if ui.button("Guardar como…").clicked() {
                    app.save_project(true);
                    ui.close();
                }
                ui.separator();
                if item(ui, app, "file.import") {
                    app.import_dialog();
                    ui.close();
                }
                if item(ui, app, "montage.export") {
                    app.open_export_dialog();
                    ui.close();
                }
                if ui
                    .button("Importar proyecto V1…")
                    .on_hover_text("Carpeta editorial de V1: capas, recortes y montaje (perfil V1 aplanado)")
                    .clicked()
                {
                    app.import_v1_dialog();
                    ui.close();
                }
                ui.separator();
                if ui.button("Salir").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edición", |ui| {
                for id in [
                    "edit.undo",
                    "edit.redo",
                    "edit.copy",
                    "edit.cut",
                    "edit.paste",
                    "edit.duplicate",
                    "edit.split",
                    "edit.delete",
                    "edit.delete_ripple",
                    "sequence.insert_selection",
                    "tools.select_all",
                    "edit.deselect",
                ] {
                    if item(ui, app, id) {
                        app.dispatch(id);
                        ui.close();
                    }
                    if id == "edit.redo" || id == "edit.duplicate" {
                        ui.separator();
                    }
                }
            });
            ui.menu_button("Capas", |ui| {
                for id in ["sequence.marker_add", "sequence.markers"] {
                    if item(ui, app, id) {
                        app.dispatch(id);
                        ui.close();
                    }
                }
                ui.separator();
                for id in
                    ["layers.new_lane", "layers.add_range", "marks.in", "marks.out", "marks.point", "edit.accept", "edit.toggle", "edit.activate"]
                {
                    if item(ui, app, id) {
                        app.dispatch(id);
                        ui.close();
                    }
                }
            });
            ui.menu_button("Vista", |ui| {
                for id in ["view.mode_montage", "view.fit", "view.zoom_sel", "view.zoom_in", "view.zoom_out", "view.center", "view.follow"] {
                    if item(ui, app, id) {
                        app.dispatch(id);
                        ui.close();
                    }
                }
                ui.separator();
                ui.checkbox(&mut app.ui.console_open, "Consola / trabajos");
                if ui.button("Restablecer disposición").clicked() {
                    let keep_project = app.ui.last_project.clone();
                    let keep_media = app.ui.last_media_dir.clone();
                    app.ui = crate::app::UiState { last_project: keep_project, last_media_dir: keep_media, ..Default::default() };
                    ui.ctx().memory_mut(|m| m.data.clear());
                    ui.close();
                }
            });
            ui.menu_button("Ajustes", |ui| {
                if ui.button("Atajos…").clicked() {
                    app.shortcuts_open = true;
                    ui.close();
                }
                ui.checkbox(&mut app.ui.snapping, "Imán (snapping)");
            });
            ui.menu_button("Ayuda", |ui| {
                if ui.button("Acerca de Transcriptor V2").clicked() {
                    app.about_open = true;
                    ui.close();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let rev = app.session.revision();
                let dirty = if app.session.is_dirty() { " · sin guardar" } else { "" };
                ui.label(RichText::new(format!("rev {rev}{dirty}")).size(11.0).color(colors::TEXT_DIM));
                match &app.tools {
                    Ok(t) => ui.label(RichText::new("FFmpeg ✓").size(11.0).color(colors::OK)).on_hover_text(format!("{} · {}", t.version, t.origin)),
                    Err(e) => ui.label(RichText::new("FFmpeg ausente").size(11.0).color(colors::ERR)).on_hover_text(e.clone()),
                };
            });
        });
    });
}

fn toolbar(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        let tb = |ui: &mut egui::Ui, app: &mut TranscriptorApp, id: &str, text: &str| {
            if ui.button(text).on_hover_text(app.keymap.tooltip(id)).clicked() {
                app.dispatch(id);
            }
        };
        if ui.selectable_label(app.tool == Tool::Select, "Selección").on_hover_text(app.keymap.tooltip("tools.select")).clicked() {
            app.tool = Tool::Select;
        }
        if ui.selectable_label(app.tool == Tool::Cut, "Corte").on_hover_text(app.keymap.tooltip("tools.cut")).clicked() {
            app.tool = Tool::Cut;
        }
        ui.separator();
        tb(ui, app, "file.import", "Importar");
        tb(ui, app, "edit.split", "Dividir");
        tb(ui, app, "edit.delete", "Eliminar");
        tb(ui, app, "edit.delete_ripple", "Eliminar+ripple");
        tb(ui, app, "edit.undo", "Deshacer");
        tb(ui, app, "edit.redo", "Rehacer");
        ui.separator();
        tb(ui, app, "marks.in", "IN");
        tb(ui, app, "marks.out", "OUT");
        tb(ui, app, "layers.new_lane", "Nueva capa");
        tb(ui, app, "layers.add_range", "Añadir tramo");
        tb(ui, app, "montage.add_selection", "A la secuencia");
        ui.separator();
        let view_label = match app.view {
            ViewMode::Sequence => "Vista: Secuencia",
            ViewMode::Source => "Vista: Fuente",
        };
        if ui.button(view_label).on_hover_text(app.keymap.tooltip("view.mode_montage")).clicked() {
            app.toggle_view();
        }
        tb(ui, app, "montage.export", "Exportar…");
        ui.checkbox(&mut app.ui.snapping, "Imán");
    });
}

fn library(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    if app.imports.busy() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Importando · {} pendientes", app.imports.pending.len()));
            if ui.small_button("Cancelar").clicked() {
                app.imports.cancel();
            }
        });
        if let Some(job) = &app.imports.running {
            ui.add(egui::Label::new(job.path.file_name().unwrap_or_default().to_string_lossy()).truncate())
                .on_hover_text(job.path.display().to_string());
        }
    }
    ui.heading("Biblioteca");
    ui.horizontal(|ui| {
        if ui.button("Importar…").on_hover_text(app.keymap.tooltip("file.import")).clicked() {
            app.import_dialog();
        }
        ui.label(RichText::new("o arrastra archivos").color(colors::TEXT_DIM).size(11.0));
    });
    ui.separator();
    let assets: Vec<(AssetId, String, AssetKind, bool, String)> =
        app.project().assets.iter().map(|a| (a.id.clone(), a.name.clone(), a.kind, a.missing, describe_asset(a))).collect();
    if assets.is_empty() {
        ui.label(RichText::new("Sin medios. Importa video, audio o imágenes.").color(colors::TEXT_DIM));
    }
    egui::ScrollArea::vertical().id_salt("library-scroll").show(ui, |ui| {
        for (id, name, kind, missing, info) in assets {
            let selected = app.selection.asset.as_ref() == Some(&id);
            let icon = match kind {
                AssetKind::Video => "🎬",
                AssetKind::Audio => "🎵",
                AssetKind::Image => "🖼",
            };
            let label = format!("{icon} {name}{}", if missing { "  (ausente)" } else { "" });
            let resp = ui.selectable_label(selected, RichText::new(label).color(if missing { colors::ERR } else { colors::TEXT }));
            let resp = resp.on_hover_text(&info);
            if resp.clicked() {
                app.selection.asset = Some(id.clone());
                app.set_source_asset(id.clone());
            }
            if resp.double_clicked() {
                app.insert_asset_at_playhead(id.clone(), None);
            }
            resp.context_menu(|ui| {
                if ui.button("Insertar en el playhead").clicked() {
                    app.insert_asset_at_playhead(id.clone(), None);
                    ui.close();
                }
                if ui.button("Ver en Fuente").clicked() {
                    app.set_source_asset(id.clone());
                    if app.view != ViewMode::Source {
                        app.toggle_view();
                    }
                    ui.close();
                }
                if missing && ui.button("Reenlazar…").clicked() {
                    relink(app, id.clone());
                    ui.close();
                }
                if ui.button("Quitar de la biblioteca").clicked() {
                    app.exec(Command::RemoveAsset { asset_id: id.clone() });
                    ui.close();
                }
            });
            if selected {
                ui.label(RichText::new(&info).size(10.5).color(colors::TEXT_DIM));
                ui.horizontal(|ui| {
                    if ui.small_button("Insertar en playhead").clicked() {
                        app.insert_asset_at_playhead(id.clone(), None);
                    }
                    if missing && ui.small_button("Reenlazar…").clicked() {
                        relink(app, id.clone());
                    }
                });
            }
        }
    });
    ui.separator();
    ui.heading("Capas");
    let asset = app.current_layer_asset();
    let layers: Vec<(tv2_domain::ids::LayerId, String, bool, bool, usize)> = asset
        .as_ref()
        .map(|a| {
            app.project()
                .ordered_layers()
                .iter()
                .filter(|l| &l.asset_id == a)
                .map(|l| (l.layer_id.clone(), l.name.clone(), l.visible, l.locked, l.items.len()))
                .collect()
        })
        .unwrap_or_default();
    if asset.is_none() {
        ui.label(RichText::new("Las capas pertenecen a un medio. Selecciona uno.").color(colors::TEXT_DIM).size(11.0));
    }
    for (id, name, visible, locked, n) in layers {
        ui.horizontal(|ui| {
            let sel = app.selection.layer.as_ref() == Some(&id);
            if ui.selectable_label(sel, format!("{name} ({n})")).clicked() {
                app.selection.layer = Some(id.clone());
            }
            let mut v = visible;
            if ui.checkbox(&mut v, "").on_hover_text("Visible").changed() {
                app.exec(Command::SetLayerProps { layer_id: id.clone(), name: None, color: None, visible: Some(v), locked: None });
            }
            if ui.small_button(if locked { "🔒" } else { "🔓" }).on_hover_text("Bloquear").clicked() {
                app.exec(Command::SetLayerProps { layer_id: id.clone(), name: None, color: None, visible: None, locked: Some(!locked) });
            }
        });
    }
    if ui.button("Nueva capa…").on_hover_text(app.keymap.tooltip("layers.new_lane")).clicked() {
        app.dispatch("layers.new_lane");
    }
}

fn relink(app: &mut TranscriptorApp, id: AssetId) {
    let Some(p) = rfd::FileDialog::new().set_title("Reenlazar medio").pick_file() else { return };
    let Ok(tools) = app.tools.clone() else { return };
    let portable = match &app.store {
        Some(s) => s.portable_path(&p),
        None => p.to_string_lossy().replace('\\', "/"),
    };
    match tools.import(&p, portable.clone()) {
        Ok(asset) => {
            app.exec(Command::RelinkAsset { asset_id: id, path: portable, asset: Some(asset) });
        }
        Err(e) => app.report(e),
    }
}

fn viewer(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    let total = ui.available_rect_before_wrap();
    let transport_h: f32 = if total.width() < 540.0 {
        116.0
    } else if total.width() < 850.0 {
        92.0
    } else {
        64.0
    };
    let transport_h = transport_h.min((total.height() - 24.0).max(32.0));
    let video_rect = Rect::from_min_max(total.min, Pos2::new(total.right(), total.bottom() - transport_h));
    // tamaño de decodificación = tamaño del visor (par, acotado)
    let scale = ui.ctx().pixels_per_point();
    let (vw, vh) = (((video_rect.width() * scale) as u32).clamp(64, 3840), ((video_rect.height() * scale) as u32).clamp(36, 2160));
    let seq_aspect = app.resolved.width as f32 / app.resolved.height.max(1) as f32;
    let (dw, dh) =
        if (vw as f32 / vh as f32) > seq_aspect { (((vh as f32) * seq_aspect) as u32, vh) } else { (vw, ((vw as f32) / seq_aspect) as u32) };
    let (dw, dh) = (dw.max(64) & !1, dh.max(36) & !1);
    if (dw, dh) != app.viewer_size {
        app.viewer_size = (dw, dh);
        app.player_send(PlayerCommand::SetViewportSize { width: dw, height: dh });
    }
    let painter = ui.painter_at(video_rect);
    painter.rect_filled(video_rect, 0.0, Color32::from_rgb(10, 10, 12));
    let has_content = app.resolved.duration.0 > 0;
    if let (Some(tex), true) = (&app.texture, has_content) {
        let size = tex.size_vec2();
        let s = (video_rect.width() / size.x).min(video_rect.height() / size.y);
        let draw_size = size * s;
        let r = Rect::from_center_size(video_rect.center(), draw_size);
        painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        if app.player_snapshot.buffering {
            painter.text(Pos2::new(r.left() + 8.0, r.top() + 8.0), Align2::LEFT_TOP, "cargando…", FontId::proportional(12.0), colors::WARN);
        }
    } else {
        let msg = if app.project().assets.is_empty() {
            "Sin medios. Importa un video para empezar."
        } else if !has_content {
            match app.view {
                ViewMode::Sequence => "La secuencia está vacía: inserta un medio desde la biblioteca (doble clic).",
                ViewMode::Source => "Selecciona un medio en la biblioteca para verlo en Fuente.",
            }
        } else if app.tools.is_err() {
            "FFmpeg no disponible: no se puede decodificar."
        } else {
            "cargando…"
        };
        painter.text(video_rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(14.0), colors::TEXT_DIM);
    }
    // clic en el visor = play/pausa
    let resp = ui.interact(video_rect, ui.id().with("viewer"), egui::Sense::click());
    if resp.clicked() {
        app.dispatch("transport.play_pause");
    }
    // transporte
    let transport_rect = Rect::from_min_max(Pos2::new(total.left(), total.bottom() - transport_h), total.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(transport_rect.shrink(6.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    child.horizontal_wrapped(|ui| {
        let snap = app.player_snapshot.clone();
        let tc = app.playhead.timecode_frames(app.frame_rate());
        ui.label(RichText::new(tc).monospace().size(18.0).color(colors::TEXT));
        ui.label(RichText::new(format!("/ {}", app.duration().timecode_frames(app.frame_rate()))).monospace().size(12.0).color(colors::TEXT_DIM));
        ui.separator();
        let btn = |ui: &mut egui::Ui, app: &mut TranscriptorApp, id: &str, text: &str| {
            if ui.button(RichText::new(text).size(14.0)).on_hover_text(app.keymap.tooltip(id)).clicked() {
                app.dispatch(id);
            }
        };
        btn(ui, app, "nav.home", "⏮");
        btn(ui, app, "nav.frame_prev", "◀");
        if ui
            .button(RichText::new(if snap.playing { "⏸" } else { "▶" }).size(16.0))
            .on_hover_text(app.keymap.tooltip("transport.play_pause"))
            .clicked()
        {
            app.dispatch("transport.play_pause");
        }
        btn(ui, app, "nav.frame_next", "▶|");
        btn(ui, app, "nav.end", "⏭");
        ui.separator();
        for s in SPEEDS {
            let on = (snap.rate - s).abs() < 1e-6;
            let label = format!("×{}", s as i32);
            if ui.selectable_label(on, label).on_hover_text(audio_policy(s)).clicked() {
                app.set_rate(s);
                if s >= 8.0 && !snap.playing {
                    app.player_send(PlayerCommand::Play);
                }
            }
        }
        ui.label(RichText::new(audio_policy(snap.rate)).size(10.5).color(colors::TEXT_DIM));
        ui.separator();
        let mut muted = snap.muted;
        if ui.checkbox(&mut muted, "Silencio").changed() {
            app.player_send(PlayerCommand::SetMuted(muted));
        }
        if let Some(l) = app.loop_range
            && ui
                .button(format!("Loop {}–{} ✕", l.start.clock(), l.end.clock()))
                .on_hover_text("Quitar loop (Shift+clic derecho en la regla)")
                .clicked()
        {
            app.dispatch("loop.clear");
        }
        if !snap.audio_available {
            ui.label(RichText::new("sin dispositivo de audio").size(10.5).color(colors::WARN));
        }
    });
    child.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!(
                "{} · decodificación {:.0} ms",
                match app.view {
                    ViewMode::Sequence => "Secuencia",
                    ViewMode::Source => "Fuente",
                },
                app.player_snapshot.decode_ms
            ))
            .size(10.5)
            .color(colors::TEXT_DIM),
        );
        if let (Some(i), Some(o)) = (app.in_point, app.out_point) {
            ui.label(
                RichText::new(format!("IN {} · OUT {} · {:.2} s", i.clock(), o.clock(), (o - i).as_seconds_f64())).size(10.5).color(colors::INOUT),
            );
        }
    });
}

fn timeline_header(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(match app.view {
                ViewMode::Sequence => "Secuencia",
                ViewMode::Source => "Fuente",
            })
            .strong(),
        );
        if app.view == ViewMode::Sequence
            && let Some(seq) = app.sequence()
        {
            ui.label(
                RichText::new(format!(
                    "{}×{} · {} fps · {} pistas · {} clips",
                    seq.width,
                    seq.height,
                    seq.frame_rate,
                    seq.tracks.len(),
                    seq.clips.len()
                ))
                .size(11.0)
                .color(colors::TEXT_DIM),
            );
            if ui.small_button("+ pista video").clicked() {
                let n = seq.tracks_of_kind(tv2_domain::TrackKind::Video).len() + 1;
                app.exec(Command::AddTrack { sequence_id: None, kind: tv2_domain::TrackKind::Video, name: format!("V{n}"), index: None });
            }
            if ui.small_button("+ pista audio").clicked() {
                let n = app.sequence().map(|s| s.tracks_of_kind(tv2_domain::TrackKind::Audio).len() + 1).unwrap_or(1);
                app.exec(Command::AddTrack { sequence_id: None, kind: tv2_domain::TrackKind::Audio, name: format!("A{n}"), index: None });
            }
        } else if let Some(a) = app.source_asset.as_ref().and_then(|a| app.project().asset(a)) {
            ui.label(RichText::new(describe_asset(a)).size(11.0).color(colors::TEXT_DIM));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Ver todo").on_hover_text(app.keymap.tooltip("view.fit")).clicked() {
                app.dispatch("view.fit");
            }
            if ui.small_button("−").on_hover_text(app.keymap.tooltip("view.zoom_out")).clicked() {
                app.dispatch("view.zoom_out");
            }
            if ui.small_button("+").on_hover_text(app.keymap.tooltip("view.zoom_in")).clicked() {
                app.dispatch("view.zoom_in");
            }
            ui.label(RichText::new(format!("{:.0} px/s", app.timeline_view.px_per_s)).size(10.5).color(colors::TEXT_DIM));
            let n_sel = app.selection.clips.len() + app.selection.items.len();
            if n_sel > 0 {
                ui.label(RichText::new(format!("{n_sel} seleccionado(s)")).size(10.5).color(colors::ACCENT));
            }
        });
    });
}

fn inspector(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    ui.heading("Inspector");
    egui::ScrollArea::vertical().id_salt("inspector-scroll").show(ui, |ui| {
        if let Some((layer_id, item_id)) = app.selection.items.first().cloned() {
            let Some(layer) = app.project().layer(&layer_id).cloned() else { return };
            let Some(item) = layer.item(&item_id).cloned() else { return };
            ui.label(RichText::new("Tramo semántico").strong());
            ui.label(format!("Capa: {} ({})", layer.name, layer.kind.v1_name()));
            ui.label(format!("ID: {}", item.item_id));
            let mut label = item.label.clone();
            if ui.text_edit_singleline(&mut label).lost_focus() && label != item.label {
                app.exec(Command::SetItemProps {
                    layer_id: layer_id.clone(),
                    item_id: item_id.clone(),
                    label: Some(label),
                    comment: None,
                    ranges: None,
                });
            }
            let mut comment = item.comment.clone();
            ui.label("Comentario:");
            if ui.text_edit_multiline(&mut comment).lost_focus() && comment != item.comment {
                app.exec(Command::SetItemProps {
                    layer_id: layer_id.clone(),
                    item_id: item_id.clone(),
                    label: None,
                    comment: Some(comment),
                    ranges: None,
                });
            }
            let ranges: Vec<String> = item
                .ranges
                .iter()
                .map(|r| {
                    if r.is_point() {
                        format!("{} · punto", r.start.clock())
                    } else {
                        format!("{} – {} · {:.1} s", r.start.clock(), r.end.clock(), r.duration().as_seconds_f64())
                    }
                })
                .collect();
            ui.label(format!("Rangos (tiempo fuente): {}", ranges.join(" | ")));
            ui.horizontal(|ui| {
                ui.label("Estado:");
                let state_label = match item.state {
                    ItemState::Proposed => "propuesto (se aplica)",
                    ItemState::Accepted => "aceptado ✓ (revisado)",
                    ItemState::Disabled => "desactivado (no se aplica)",
                };
                ui.label(RichText::new(state_label).color(match item.state {
                    ItemState::Accepted => colors::OK,
                    ItemState::Disabled => colors::TEXT_DIM,
                    _ => colors::WARN,
                }));
            });
            ui.horizontal(|ui| {
                let can_accept = layer.kind.accepts_acceptance();
                let b = ui.add_enabled(can_accept, egui::Button::new("Aceptar (E)"));
                if b.clicked() {
                    app.dispatch("edit.accept");
                }
                if !can_accept {
                    b.on_hover_text("Los bloques no admiten aceptación (regla V1)");
                }
                if ui.button("Desactivar (X)").clicked() {
                    app.dispatch("edit.toggle");
                }
                if ui.button("Activar (P)").clicked() {
                    app.dispatch("edit.activate");
                }
            });
            ui.label(
                RichText::new(format!("edited: {} · origen: {}", item.edited, item.origin.clone().unwrap_or_default()))
                    .size(10.5)
                    .color(colors::TEXT_DIM),
            );
            if let Some(p) = &item.parent_id {
                ui.label(format!("Padre: {p}"));
            }
            if ui.button("Borrar tramo (tombstone)").clicked() {
                app.dispatch("edit.delete");
            }
            return;
        }
        if let Some(clip_id) = app.selection.clips.first().cloned() {
            let Some(clip) = app.sequence().and_then(|s| s.clip(&clip_id)).cloned() else { return };
            let asset = app.project().asset(&clip.asset_id).cloned();
            ui.label(RichText::new("Clip").strong());
            let mut name = clip.name.clone();
            if ui.text_edit_singleline(&mut name).lost_focus() && name != clip.name {
                app.exec(Command::SetClipProps { clip_id: clip_id.clone(), name: Some(name), gain_db: None, transform: None });
            }
            ui.label(format!("ID: {}", clip.id));
            if let Some(a) = &asset {
                ui.label(format!("Medio: {}", a.name));
            }
            ui.label(format!(
                "Secuencia: {} – {} ({:.2} s)",
                clip.position.timecode_ms(),
                clip.end().timecode_ms(),
                clip.duration().as_seconds_f64()
            ));
            ui.label(format!("Fuente: {} – {}", clip.source.start.timecode_ms(), clip.source.end.timecode_ms()));
            ui.label(format!("Pista: {}", app.sequence().and_then(|s| s.track(&clip.track_id)).map(|t| t.name.clone()).unwrap_or_default()));
            if let Some(g) = &clip.link_group {
                ui.label(RichText::new(format!("Vinculado: {g}")).size(10.5).color(colors::TEXT_DIM));
                if ui.small_button("Desvincular").clicked() {
                    let ids = app.linked_selection();
                    app.exec(Command::UnlinkClips { clip_ids: ids });
                }
            } else if app.selection.clips.len() >= 2 && ui.small_button("Vincular selección").clicked() {
                let ids = app.selection.clips.clone();
                app.exec(Command::LinkClips { clip_ids: ids });
            }
            let mut enabled = clip.enabled;
            if ui.checkbox(&mut enabled, "Activo").changed() {
                app.exec(Command::SetClipEnabled { clip_ids: vec![clip_id.clone()], enabled });
            }
            let is_audio = app.sequence().and_then(|s| s.track(&clip.track_id)).map(|t| t.kind == tv2_domain::TrackKind::Audio).unwrap_or(false);
            if is_audio {
                let mut gain = clip.gain_db;
                if ui.add(egui::Slider::new(&mut gain, -48.0..=12.0).text("ganancia dB")).drag_stopped() {
                    app.exec(Command::SetClipProps { clip_id: clip_id.clone(), name: None, gain_db: Some(gain), transform: None });
                }
            } else {
                ui.label(RichText::new("Transformación").strong());
                let mut t: Transform = clip.transform;
                let mut changed = false;
                changed |= ui.add(egui::Slider::new(&mut t.scale, 0.1..=4.0).text("escala")).drag_stopped();
                changed |= ui.add(egui::Slider::new(&mut t.x, -1.0..=1.0).text("x")).drag_stopped();
                changed |= ui.add(egui::Slider::new(&mut t.y, -1.0..=1.0).text("y")).drag_stopped();
                changed |= ui.add(egui::Slider::new(&mut t.opacity, 0.0..=1.0).text("opacidad")).drag_stopped();
                egui::ComboBox::from_label("ajuste").selected_text(format!("{:?}", t.fit)).show_ui(ui, |ui| {
                    for (v, l) in [
                        (tv2_domain::timeline::FitMode::Fit, "Ajustar"),
                        (tv2_domain::timeline::FitMode::Fill, "Rellenar"),
                        (tv2_domain::timeline::FitMode::Stretch, "Estirar"),
                        (tv2_domain::timeline::FitMode::Native, "Nativo"),
                    ] {
                        if ui.selectable_value(&mut t.fit, v, l).clicked() {
                            changed = true;
                        }
                    }
                });
                if changed {
                    app.exec(Command::SetClipProps { clip_id: clip_id.clone(), name: None, gain_db: None, transform: Some(t) });
                }
                if asset.as_ref().is_some_and(|a| a.kind == AssetKind::Image) {
                    let mut secs = clip.duration().as_seconds_f64();
                    if ui.add(egui::Slider::new(&mut secs, 0.1..=60.0).text("duración (s)")).drag_stopped() {
                        let new_end = clip.position + Ticks::from_seconds_f64(secs);
                        app.exec(Command::TrimClip { clip_id: clip_id.clone(), edge: tv2_domain::ClipEdge::End, new_time: new_end });
                    }
                }
            }
            ui.label(
                RichText::new(format!(
                    "origen: {}{}",
                    clip.provenance.origin,
                    clip.provenance.parent_clip.as_ref().map(|p| format!(" ← {p}")).unwrap_or_default()
                ))
                .size(10.5)
                .color(colors::TEXT_DIM),
            );
            return;
        }
        if let Some(layer_id) = app.selection.layer.clone()
            && let Some(layer) = app.project().layer(&layer_id).cloned()
        {
            ui.label(RichText::new("Capa").strong());
            let mut name = layer.name.clone();
            if ui.text_edit_singleline(&mut name).lost_focus() && name != layer.name {
                app.exec(Command::SetLayerProps { layer_id: layer_id.clone(), name: Some(name), color: None, visible: None, locked: None });
            }
            ui.label(format!("Tipo: {} · {} tramos · revisión {}", layer.kind.v1_name(), layer.items.len(), layer.revision));
            let mut color = layer.color.clone();
            if ui.text_edit_singleline(&mut color).lost_focus() && color != layer.color {
                app.exec(Command::SetLayerProps { layer_id: layer_id.clone(), name: None, color: Some(color), visible: None, locked: None });
            }
            if !layer.deleted_item_ids.is_empty() {
                ui.label(RichText::new(format!("{} tombstones", layer.deleted_item_ids.len())).size(10.5).color(colors::TEXT_DIM));
            }
            ui.label(RichText::new("Marca IN/OUT (I/O) y pulsa «Añadir tramo» (Ctrl+Enter).").size(11.0).color(colors::TEXT_DIM));
            if ui.button("Borrar capa").clicked() {
                app.exec(Command::DeleteLayer { layer_id: layer_id.clone() });
                app.selection.layer = None;
            }
            return;
        }
        if let Some(a) = app.selection.asset.as_ref().and_then(|a| app.project().asset(a)).cloned() {
            ui.label(RichText::new("Medio").strong());
            ui.label(&a.name);
            ui.label(RichText::new(describe_asset(&a)).size(11.0));
            ui.label(RichText::new(format!("Ruta: {}", a.path)).size(10.5).color(colors::TEXT_DIM));
            ui.label(
                RichText::new(format!(
                    "Identidad: size {} · hash muestreado {}… · inventario {}…",
                    a.fingerprint.size,
                    &a.fingerprint.hash_muestreado[..12],
                    &a.fingerprint.inventario_sha256[..12]
                ))
                .size(10.5)
                .color(colors::TEXT_DIM),
            );
            if a.kind == AssetKind::Image {
                let mut secs = a.duration().as_seconds_f64();
                if ui.add(egui::Slider::new(&mut secs, 0.1..=60.0).text("duración por defecto (s)")).drag_stopped() {
                    app.exec(Command::SetImageDuration { asset_id: a.id.clone(), duration: Ticks::from_seconds_f64(secs) });
                }
            }
            return;
        }
        ui.label(RichText::new("Nada seleccionado.").color(colors::TEXT_DIM));
        ui.label(RichText::new("Clic en un clip, un tramo o una capa para ver sus propiedades.").size(11.0).color(colors::TEXT_DIM));
        ui.separator();
        ui.label(RichText::new("Proyecto").strong());
        let mut name = app.project().name.clone();
        if ui.text_edit_singleline(&mut name).lost_focus() && name != app.project().name {
            app.exec(Command::RenameProject { name });
        }
        ui.label(format!(
            "Revisión {} · {} medios · {} capas",
            app.session.revision(),
            app.project().assets.len(),
            app.project().layers.iter().filter(|l| !l.deleted).count()
        ));
        if let Some(s) = &app.store {
            ui.label(RichText::new(format!("{}", s.root.display())).size(10.5).color(colors::TEXT_DIM));
        } else {
            ui.label(RichText::new("Sin guardar (Ctrl+S)").size(10.5).color(colors::WARN));
        }
        ui.horizontal(|ui| {
            let u = app.session.undo_label().map(|s| s.to_string());
            if ui.add_enabled(u.is_some(), egui::Button::new(format!("Deshacer {}", u.clone().unwrap_or_default()))).clicked() {
                app.undo();
            }
        });
    });
}

fn console(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Consola").strong());
        for (lvl, name) in
            [(tracing::Level::DEBUG, "detalle"), (tracing::Level::INFO, "info"), (tracing::Level::WARN, "avisos"), (tracing::Level::ERROR, "errores")]
        {
            if ui.selectable_label(app.console_filter == lvl, name).clicked() {
                app.console_filter = lvl;
            }
        }
        if ui.small_button("Limpiar").clicked() {
            app.console_lines.clear();
        }
        if let Some(r) = &app.export.running {
            let p = r.progress.lock().clone();
            ui.separator();
            ui.label(format!(
                "Export: {} {:.0}% {}",
                p.stage,
                p.fraction * 100.0,
                if p.fps > 0.0 { format!("{:.0} fps", p.fps) } else { String::new() }
            ));
            ui.add(egui::ProgressBar::new(p.fraction).desired_width(160.0));
            if ui.small_button("Cancelar").clicked() {
                r.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("log: {}", crate::paths::logs_dir().display())).size(10.0).color(colors::TEXT_DIM));
        });
    });
    egui::ScrollArea::vertical().id_salt("console-scroll").stick_to_bottom(true).show(ui, |ui| {
        for line in app.console_lines.iter().filter(|l| l.level <= app.console_filter) {
            let color = match line.level {
                tracing::Level::ERROR => colors::ERR,
                tracing::Level::WARN => colors::WARN,
                tracing::Level::INFO => colors::TEXT,
                _ => colors::TEXT_DIM,
            };
            ui.label(RichText::new(format!("[{}] {}", line.level, crate::console::redact(&line.message))).size(11.0).color(color).monospace());
        }
    });
}

fn dialogs(app: &mut TranscriptorApp, ctx: &egui::Context) {
    // nueva capa
    if let Some(mut name) = app.new_layer_dialog.clone() {
        let mut open = true;
        let mut done = false;
        egui::Window::new("Nueva capa").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(
            ctx,
            |ui| {
                ui.label("Nombre de la capa (manual, antes de transcribir):");
                let r = ui.text_edit_singleline(&mut name);
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("Crear").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        done = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        app.new_layer_dialog = None;
                    }
                });
            },
        );
        if done {
            app.create_layer(name);
            app.new_layer_dialog = None;
        } else if !open {
            app.new_layer_dialog = None;
        } else if app.new_layer_dialog.is_some() {
            app.new_layer_dialog = Some(name);
        }
    }
    // renombrar
    if let Some((mut text, target)) = app.rename_dialog.clone() {
        let mut open = true;
        let mut done = false;
        egui::Window::new("Editar").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            let r = ui.text_edit_singleline(&mut text);
            r.request_focus();
            ui.horizontal(|ui| {
                if ui.button("Aplicar").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    done = true;
                }
                if ui.button("Cancelar").clicked() {
                    app.rename_dialog = None;
                }
            });
        });
        if done {
            app.apply_rename(text, target);
            app.rename_dialog = None;
        } else if !open {
            app.rename_dialog = None;
        } else if app.rename_dialog.is_some() {
            app.rename_dialog = Some((text, target));
        }
    }
    // ir a tiempo
    if let Some(mut text) = app.goto_dialog.clone() {
        let mut open = true;
        let mut done = false;
        egui::Window::new("Ir a tiempo").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(
            ctx,
            |ui| {
                ui.label("hh:mm:ss.mmm, mm:ss o segundos");
                let r = ui.text_edit_singleline(&mut text);
                r.request_focus();
                if ui.button("Ir").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    done = true;
                }
            },
        );
        if done {
            match parse_time(&text) {
                Some(t) => app.seek(t),
                None => app.toast(Severity::Warn, "Tiempo inválido"),
            }
            app.goto_dialog = None;
        } else if !open {
            app.goto_dialog = None;
        } else if app.goto_dialog.is_some() {
            app.goto_dialog = Some(text);
        }
    }
    // exportar
    if app.export.open {
        export_dialog(app, ctx);
    }
    // atajos
    if app.shortcuts_open {
        let mut open = true;
        egui::Window::new("Atajos de teclado").open(&mut open).default_size([560.0, 480.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Buscar:");
                ui.text_edit_singleline(&mut app.shortcut_search);
                ui.label(RichText::new(format!("Personalización: {}", crate::paths::keymap_file().display())).size(10.5).color(colors::TEXT_DIM));
            });
            if !app.keymap.conflicts.is_empty() {
                ui.label(RichText::new(format!("{} conflicto(s): gana el primero registrado", app.keymap.conflicts.len())).color(colors::WARN));
            }
            let q = app.shortcut_search.to_lowercase();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for group in crate::keymap::GROUPS {
                    let rows: Vec<(String, String, String)> = app
                        .keymap
                        .actions
                        .iter()
                        .filter(|a| a.group == group)
                        .filter(|a| q.is_empty() || a.label.to_lowercase().contains(&q) || a.id.contains(&q))
                        .map(|a| (a.id.to_string(), a.label.to_string(), app.keymap.pretty(a.id)))
                        .collect();
                    if rows.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(group).strong());
                    egui::Grid::new(format!("km-{group}")).num_columns(4).striped(true).show(ui, |ui| {
                        for (id, label, chords) in rows {
                            ui.label(RichText::new(&id).size(10.5).color(colors::TEXT_DIM));
                            ui.label(label);
                            ui.label(RichText::new(if chords.is_empty() { "—".into() } else { chords }).monospace());
                            if ui.button("Editar").clicked() {
                                app.shortcut_editor =
                                    Some(crate::keymap::ShortcutEditor { text: app.keymap.chords(&id).join("; "), action: id, ..Default::default() });
                            }
                            ui.end_row();
                        }
                    });
                }
            });
            if let Some(mut editor) = app.shortcut_editor.take() {
                ui.separator();
                ui.label(app.keymap.label(&editor.action));
                ui.label("Separa combinaciones con ;. Vacío deja la acción sin atajo.");
                ui.text_edit_singleline(&mut editor.text);
                let mut close = false;
                let mut candidate = None;
                ui.horizontal(|ui| {
                    if ui.button(if editor.capturing { "Pulsa una combinación…" } else { "Capturar tecla" }).clicked() {
                        editor.capturing = !editor.capturing;
                    }
                    if ui.button("Guardar").clicked() {
                        candidate = Some(app.keymap.edited(&editor.action, Some(&editor.text)));
                    }
                    if ui.button("Restaurar predeterminado").clicked() {
                        candidate = Some(app.keymap.edited(&editor.action, None));
                    }
                    if ui.button("Cancelar").clicked() {
                        close = true;
                    }
                });
                if let Some(result) = candidate {
                    match result {
                        Ok(keymap) => match crate::keymap::save_overrides(&keymap.overrides) {
                            Ok(()) => {
                                app.keymap = keymap;
                                close = true;
                            }
                            Err(error) => editor.error = Some(error.to_string()),
                        },
                        Err(error) => editor.error = Some(error),
                    }
                }
                if let Some(error) = &editor.error {
                    ui.colored_label(colors::WARN, error);
                }
                if !close {
                    app.shortcut_editor = Some(editor);
                }
            }
        });
        app.shortcuts_open = open;
        if !open {
            app.shortcut_editor = None;
        }
    }
    if app.about_open {
        let mut open = true;
        egui::Window::new("Acerca de").open(&mut open).collapsible(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.label(format!("Transcriptor V2 {}", env!("CARGO_PKG_VERSION")));
            ui.label("Editor multimedia y editorial nativo (Rust · egui · wgpu · FFmpeg).");
            match &app.tools {
                Ok(t) => ui.label(format!("{}\n{}", t.version, t.ffmpeg.display())),
                Err(e) => ui.label(format!("FFmpeg: {e}")),
            };
            ui.label(
                RichText::new("Licencia del editor: propietaria, todos los derechos reservados. Dependencias con licencias propias; FFmpeg externo (GPLv3 en el build de referencia).")
                    .size(10.5)
                    .color(colors::TEXT_DIM),
            );
        });
        app.about_open = open;
    }
    if let Some(candidate) = &app.recovery {
        let revision = candidate.revision;
        egui::Window::new("Recuperación disponible").collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("Hay un autosave más reciente (revisión {revision})."));
            ui.label("Recuperarlo conserva la versión guardada como un paso de Deshacer.");
            ui.horizontal(|ui| {
                if ui.button("Recuperar cambios").clicked()
                    && let Some(mut candidate) = app.recovery.take()
                {
                    if let Some(store) = &app.store {
                        for asset in &mut candidate.assets {
                            let path = store.resolve_path(&asset.path);
                            asset.missing = !path.exists();
                            asset.path = path.to_string_lossy().to_string();
                        }
                    }
                    match app.session.recover(candidate) {
                        Ok(()) => {
                            app.resolved_revision = None;
                            app.selection.clear();
                        }
                        Err(e) => app.report(e),
                    }
                }
                if ui.button("Usar versión guardada").clicked() {
                    app.recovery = None;
                }
            });
        });
    }
    // cierre con cambios sin guardar
    if app.pending_close {
        egui::Window::new("Cambios sin guardar").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.label("El proyecto tiene cambios sin guardar.");
            ui.horizontal(|ui| {
                if ui.button("Guardar y salir").clicked() && app.save_project(false) {
                    app.pending_close = false;
                    app.session.mark_clean();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Salir sin guardar").clicked() {
                    app.pending_close = false;
                    app.session.mark_clean();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Cancelar").clicked() {
                    app.pending_close = false;
                }
            });
        });
    }
}

fn export_dialog(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let presets = tv2_media::export::presets();
    let mut open = true;
    let mut start: Option<usize> = None;
    egui::Window::new("Exportar")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(520.0)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(
                RichText::new("La exportación congela la revisión y el preset al iniciar y usa el mismo compositor y mezcla que el visor.")
                    .size(11.0)
                    .color(colors::TEXT_DIM),
            );
            egui::ComboBox::from_label("Preset").selected_text(presets[app.export.preset_idx.min(presets.len() - 1)].label.clone()).show_ui(
                ui,
                |ui| {
                    for (i, p) in presets.iter().enumerate() {
                        ui.selectable_value(&mut app.export.preset_idx, i, &p.label).on_hover_text(&p.notes);
                    }
                },
            );
            let preset = &presets[app.export.preset_idx.min(presets.len() - 1)];
            ui.label(RichText::new(&preset.notes).size(10.5).color(colors::TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut app.export.range_mode, 0, "Secuencia completa");
                let has_io = matches!((app.in_point, app.out_point), (Some(i), Some(o)) if i < o);
                ui.add_enabled(has_io, egui::RadioButton::new(app.export.range_mode == 1, "Rango IN/OUT"))
                    .clicked()
                    .then(|| app.export.range_mode = 1);
                ui.add_enabled(!app.selection.clips.is_empty(), egui::RadioButton::new(app.export.range_mode == 2, "Rangos de clips seleccionados"))
                    .clicked().then(|| app.export.range_mode = 2);
                ui.add_enabled(!app.selection.items.is_empty(), egui::RadioButton::new(app.export.range_mode == 3, "Tramos / bloques seleccionados"))
                    .clicked().then(|| app.export.range_mode = 3);
            });
            if app.export.range_mode >= 2 {
                ui.label("Une los rangos sin repetir solapes, en orden de secuencia, con todas las pistas visibles y audibles. Los tramos incluyen cada aparición en el montaje.");
            }
            ui.horizontal(|ui| {
                ui.label("Destino:");
                ui.text_edit_singleline(&mut app.export.destination);
                if ui.button("…").clicked() {
                    let ext = preset.container.clone();
                    let mut dlg = rfd::FileDialog::new().set_title("Destino de exportación").add_filter(&ext, &[ext.as_str()]);
                    if let Some(p) = std::path::Path::new(&app.export.destination).parent() {
                        dlg = dlg.set_directory(p);
                    }
                    if let Some(p) = dlg.save_file() {
                        app.export.destination = p.to_string_lossy().to_string();
                    }
                }
            });
            // extensión coherente con el preset
            let wanted = format!(".{}", preset.container);
            if !app.export.destination.to_lowercase().ends_with(&wanted) {
                let base = std::path::Path::new(&app.export.destination).with_extension("");
                app.export.destination = format!("{}{}", base.to_string_lossy(), wanted);
            }
            if std::path::Path::new(&app.export.destination).exists() {
                ui.label(RichText::new("Ya existe un archivo con ese nombre: nunca se sobrescribe. Elige otro nombre.").color(colors::WARN));
            }
            ui.separator();
            if let Some(r) = &app.export.running {
                ui.label(format!("{} · revisión {}", r.destination.display(), r.revision));
                let p = r.progress.lock().clone();
                ui.label(format!(
                    "{} · {:.0}% · {}/{} fotogramas · {:.0} fps · {:.0} s",
                    p.stage,
                    p.fraction * 100.0,
                    p.frames_done,
                    p.frames_total,
                    p.fps,
                    r.started.elapsed().as_secs_f32()
                ));
                ui.add(egui::ProgressBar::new(p.fraction));
                if ui.button("Cancelar").clicked() {
                    r.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if ui
                    .add_enabled(app.export.pending.len() < 16 && !app.export.destination.is_empty(), egui::Button::new("Añadir a la cola"))
                    .clicked()
                {
                    start = Some(app.export.preset_idx);
                }
            } else {
                match &app.export.last_result {
                    Some(Ok(r)) => {
                        ui.label(RichText::new(format!("Listo: {}", r.path.display())).color(colors::OK));
                        ui.label(
                            RichText::new(format!(
                                "duración {} (esperada {}) · {} fotogramas · video {} · audio {} · sha256 {}… · rev {}",
                                r.duration.timecode_ms(),
                                r.expected_duration.timecode_ms(),
                                r.frames_written,
                                r.video_streams,
                                r.audio_streams,
                                &r.sha256[..12],
                                r.project_revision
                            ))
                            .size(10.5)
                            .color(colors::TEXT_DIM),
                        );
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(format!("Falló: {}", e.message)).color(colors::ERR));
                    }
                    None => {}
                }
                let can = app.tools.is_ok()
                    && app.view == ViewMode::Sequence
                    && !app.export.destination.is_empty()
                    && !std::path::Path::new(&app.export.destination).exists();
                let b = ui.add_enabled(can, egui::Button::new("Exportar"));
                if b.clicked() {
                    start = Some(app.export.preset_idx);
                }
                if !can {
                    b.on_hover_text(if app.tools.is_err() {
                        "FFmpeg ausente"
                    } else if app.view != ViewMode::Sequence {
                        "Cambia a la vista Secuencia"
                    } else {
                        "Destino vacío o ya existente"
                    });
                }
            }
            let mut cancel_pending = None;
            for (i, job) in app.export.pending.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(format!("En cola · rev {} · {}", job.project_revision, job.destination.display())).truncate());
                    if ui.small_button(format!("Cancelar #{}", i + 1)).clicked() {
                        cancel_pending = Some(i);
                    }
                });
            }
            if let Some(i) = cancel_pending {
                app.cancel_queued_export(i);
            }
            ui.collapsing("Historial de trabajos", |ui| {
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    for (path, revision, result) in app.export.history.iter().rev() {
                        let status = match result {
                            Ok(_) => "Completado".to_owned(),
                            Err(e) => format!("{:?}: {}", e.code, e.message),
                        };
                        ui.label(format!("{status} · rev {revision} · {}", path.display()));
                    }
                });
            });
        });
    if let Some(i) = start {
        let preset = presets[i.min(presets.len() - 1)].clone();
        app.start_export(preset);
    }
    app.export.open = open;
}

fn toasts(app: &mut TranscriptorApp, ctx: &egui::Context) {
    if app.toasts.is_empty() {
        return;
    }
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("toasts")));
    let mut y = screen.bottom() - 90.0;
    for t in app.toasts.iter().rev().take(5) {
        let color = match t.severity {
            Severity::Info => colors::HEADER_BG,
            Severity::Warn => colors::WARN.gamma_multiply(0.35),
            Severity::Error => colors::ERR.gamma_multiply(0.4),
        };
        let galley = painter.layout(t.text.clone(), FontId::proportional(13.0), colors::TEXT, 460.0);
        let size = galley.size() + Vec2::new(20.0, 12.0);
        let rect = Rect::from_min_size(Pos2::new(screen.right() - size.x - 16.0, y - size.y), size);
        painter.rect_filled(rect, 6.0, color);
        painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_white_alpha(40)), egui::StrokeKind::Inside);
        painter.galley(rect.min + Vec2::new(10.0, 6.0), galley, colors::TEXT);
        y -= size.y + 6.0;
    }
}

pub fn parse_time(text: &str) -> Option<Ticks> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if !text.contains(':') {
        return text.replace(',', ".").parse::<f64>().ok().filter(|f| f.is_finite() && *f >= 0.0).map(Ticks::from_seconds_f64);
    }
    let parts: Vec<&str> = text.split(':').collect();
    let (h, m, s) = match parts.len() {
        2 => (0i64, parts[0].parse::<i64>().ok()?, parts[1].replace(',', ".").parse::<f64>().ok()?),
        3 => (parts[0].parse::<i64>().ok()?, parts[1].parse::<i64>().ok()?, parts[2].replace(',', ".").parse::<f64>().ok()?),
        _ => return None,
    };
    if h < 0 || !(0..60).contains(&m) || !(0.0..60.0).contains(&s) {
        return None;
    }
    Some(Ticks::from_seconds_f64(h as f64 * 3600.0 + m as f64 * 60.0 + s))
}

#[allow(dead_code)]
fn _unused(_: TimeRange) {}
