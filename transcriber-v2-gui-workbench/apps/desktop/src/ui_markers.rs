//! Marcadores de secuencia, independientes de las marcas del autor V1.
use crate::app::TranscriptorApp;
use tv2_domain::{
    Command,
    time::{Ticks, TimeRange},
};

pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let mut open = app.markers_open;
    egui::Window::new("Marcadores de secuencia").open(&mut open).default_width(420.0).show(ctx, |ui| {
        if ui.button("Añadir en playhead").clicked() {
            app.dispatch("sequence.marker_add");
        }
        let mut markers = app.sequence().map(|s| s.markers.clone()).unwrap_or_default();
        markers.sort_by_key(|m| m.range.start);
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            if markers.is_empty() {
                ui.label("Sin marcadores. Añade uno para señalar un punto de la secuencia.");
            }
            for marker in markers {
                ui.push_id(&marker.id, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(marker.range.start.timecode_ms()).on_hover_text("Ir a este marcador").clicked() {
                            app.view = crate::app::ViewMode::Sequence;
                            app.resolved_revision = None;
                            app.seek(marker.range.start);
                            app.timeline_view.ensure_visible(marker.range.start);
                        }
                        ui.add(egui::Label::new(&marker.label).truncate()).on_hover_text(&marker.label);
                        if ui.small_button("Editar").clicked() {
                            app.marker_editor = Some(marker.clone());
                        }
                        if ui.small_button("Borrar").clicked() {
                            app.exec(Command::RemoveMarker { marker_id: marker.id.clone() });
                        }
                    });
                });
            }
        });
    });
    app.markers_open = open;
    if let Some(mut draft) = app.marker_editor.take() {
        let mut editor_open = true;
        let mut save = false;
        egui::Window::new("Editar marcador").open(&mut editor_open).show(ctx, |ui| {
            ui.label("Nombre");
            ui.text_edit_singleline(&mut draft.label);
            ui.label("Comentario");
            ui.text_edit_multiline(&mut draft.comment);
            let (mut start, mut end) = (draft.range.start.as_seconds_f64(), draft.range.end.as_seconds_f64());
            ui.horizontal(|ui| {
                ui.label("Inicio (s)");
                ui.add(egui::DragValue::new(&mut start).speed(0.01).range(0.0..=1e7));
                ui.label("Fin (s)");
                ui.add(egui::DragValue::new(&mut end).speed(0.01).range(0.0..=1e7));
            });
            draft.range = TimeRange::new(Ticks::from_seconds_f64(start), Ticks::from_seconds_f64(end));
            if start > end {
                ui.colored_label(egui::Color32::LIGHT_RED, "El fin debe ser igual o posterior al inicio.");
            }
            save = ui.add_enabled(start <= end, egui::Button::new("Guardar marcador")).clicked();
        });
        if save {
            if !app.exec(Command::SetMarker {
                marker_id: draft.id.clone(),
                range: Some(draft.range),
                label: Some(draft.label.clone()),
                comment: Some(draft.comment.clone()),
                color: Some(draft.color.clone()),
            }) {
                app.marker_editor = Some(draft);
            }
        } else if editor_open {
            app.marker_editor = Some(draft);
        }
    }
}
