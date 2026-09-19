//! Marcadores de secuencia, independientes de las marcas del autor V1.
use crate::app::TranscriptorApp;
use tv2_application::CommandEnvelope;
use tv2_domain::{Command, Project, SequenceId, time::Ticks, timeline::Marker};

pub struct MarkerEditor {
    project: String,
    revision: u64,
    sequence: Option<SequenceId>,
    draft: Marker,
}

impl MarkerEditor {
    fn new(project: &Project, draft: Marker) -> Self {
        Self { project: project.project_id.clone(), revision: project.revision, sequence: project.active_sequence.clone(), draft }
    }

    fn is_stale(&self, project: &Project) -> bool {
        self.project != project.project_id || self.revision != project.revision || self.sequence != project.active_sequence
    }
}

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
                            app.marker_editor = Some(MarkerEditor::new(app.project(), marker.clone()));
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
    if let Some(mut editor) = app.marker_editor.take() {
        let mut editor_open = true;
        let mut save = false;
        let stale = editor.is_stale(app.project());
        let draft = &mut editor.draft;
        egui::Window::new("Editar marcador").open(&mut editor_open).show(ctx, |ui| {
            if stale {
                ui.colored_label(egui::Color32::YELLOW, "La revisión cambió. Cierra y vuelve a abrir el marcador antes de guardar.");
            }
            ui.label("Nombre");
            ui.text_edit_singleline(&mut draft.label);
            ui.label("Comentario");
            ui.text_edit_multiline(&mut draft.comment);
            let (mut start, mut end) = (draft.range.start.as_seconds_f64(), draft.range.end.as_seconds_f64());
            ui.horizontal(|ui| {
                ui.label("Inicio (s)");
                if ui.add(egui::DragValue::new(&mut start).speed(0.01).range(0.0..=1e7)).changed() {
                    draft.range.start = Ticks::from_seconds_f64(start);
                }
                ui.label("Fin (s)");
                if ui.add(egui::DragValue::new(&mut end).speed(0.01).range(0.0..=1e7)).changed() {
                    draft.range.end = Ticks::from_seconds_f64(end);
                }
            });
            if start > end {
                ui.colored_label(egui::Color32::LIGHT_RED, "El fin debe ser igual o posterior al inicio.");
            }
            save = ui.add_enabled(!stale && draft.range.start <= draft.range.end, egui::Button::new("Guardar marcador")).clicked();
        });
        if save {
            let command = Command::SetMarker {
                marker_id: draft.id.clone(),
                range: Some(draft.range),
                label: Some(draft.label.clone()),
                comment: Some(draft.comment.clone()),
                color: Some(draft.color.clone()),
            };
            match app.session.execute(CommandEnvelope::human(command).with_base(editor.revision)) {
                Ok(result) => app.accept_command_result(&result, None),
                Err(error) => {
                    app.report(error);
                    app.marker_editor = Some(editor);
                }
            }
        } else if editor_open {
            app.marker_editor = Some(editor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::{MarkerId, TimeRange};

    #[test]
    fn marker_draft_rejects_changed_project_revision_and_sequence() {
        let project = Project::new("prueba");
        let marker = Marker {
            id: MarkerId::new("marker"),
            range: TimeRange::new(Ticks(123456789), Ticks(987654321)),
            label: "Marca".into(),
            color: "#ffffff".into(),
            comment: String::new(),
        };
        let editor = MarkerEditor::new(&project, marker);
        assert!(!editor.is_stale(&project));
        let mut changed = project.clone();
        changed.revision += 1;
        assert!(editor.is_stale(&changed));
        changed = project.clone();
        changed.project_id = "otro".into();
        assert!(editor.is_stale(&changed));
        changed = project.clone();
        changed.active_sequence = Some(SequenceId::new("otra"));
        assert!(editor.is_stale(&changed));
    }
}
