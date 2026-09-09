use crate::app::TranscriptorApp;
use tv2_application::CommandEnvelope;
use tv2_domain::{Command, ItemId, LayerId, SemanticItem, TimeRange};

pub struct ItemEditor {
    project: String,
    revision: u64,
    layer: LayerId,
    item: ItemId,
    label: String,
    comment: String,
    parent: String,
    ranges: Vec<(String, String)>,
    error: Option<String>,
}

impl ItemEditor {
    pub fn new(project: String, revision: u64, layer: LayerId, item: &SemanticItem) -> Self {
        Self {
            project,
            revision,
            layer,
            item: item.item_id.clone(),
            label: item.label.clone(),
            comment: item.comment.clone(),
            parent: item.parent_id.as_ref().map(ToString::to_string).unwrap_or_default(),
            ranges: item.ranges.iter().map(|r| (r.start.timecode_ms(), r.end.timecode_ms())).collect(),
            error: None,
        }
    }

    fn command(&self) -> Result<Command, String> {
        let ranges = self
            .ranges
            .iter()
            .map(|(a, b)| {
                let start = crate::ui_panels::parse_time(a).ok_or_else(|| format!("Inicio inválido: {a}"))?;
                let end = crate::ui_panels::parse_time(b).ok_or_else(|| format!("Fin inválido: {b}"))?;
                Ok(TimeRange::new(start, end))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Command::Batch {
            label: "Editar item editorial".into(),
            commands: vec![
                Command::SetItemStructure {
                    layer_id: self.layer.clone(),
                    item_id: self.item.clone(),
                    parent_id: (!self.parent.trim().is_empty()).then(|| ItemId::new(self.parent.trim())),
                    ranges,
                },
                Command::SetItemProps {
                    layer_id: self.layer.clone(),
                    item_id: self.item.clone(),
                    label: Some(self.label.clone()),
                    comment: Some(self.comment.clone()),
                    ranges: None,
                },
            ],
        })
    }
}

pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let Some(mut editor) = app.item_editor.take() else { return };
    let mut open = true;
    let mut completed = false;
    egui::Window::new("Editar item: texto, rangos y jerarquía").open(&mut open).default_width(480.0).show(ctx, |ui| {
        ui.label("Etiqueta");
        ui.text_edit_singleline(&mut editor.label);
        ui.label("Comentario");
        ui.text_edit_multiline(&mut editor.comment);
        ui.label("ID del padre (vacío: raíz)");
        ui.text_edit_singleline(&mut editor.parent);
        ui.label("Rangos fuente: segundos o HH:MM:SS.mmm. Deben estar ordenados y dentro del padre.");
        let mut remove = None;
        for (n, (a, b)) in editor.ranges.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(a).desired_width(145.0));
                ui.add(egui::TextEdit::singleline(b).desired_width(145.0));
                if ui.small_button("Quitar").clicked() {
                    remove = Some(n);
                }
            });
        }
        if let Some(n) = remove {
            editor.ranges.remove(n);
        }
        if ui.button("Añadir rango").clicked() {
            editor.ranges.push((String::new(), String::new()));
        }
        if let Some(error) = &editor.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if ui.button("Aplicar como un cambio").clicked() {
            let result = if editor.project != app.project().project_id {
                Err("El proyecto cambió. Cierra este editor y vuelve a seleccionar el item.".into())
            } else {
                editor
                    .command()
                    .and_then(|command| app.session.execute(CommandEnvelope::human(command).with_base(editor.revision)).map_err(|e| e.to_string()))
            };
            match result {
                Ok(_) => {
                    app.after_change();
                    completed = true;
                }
                Err(e) => editor.error = Some(e),
            }
        }
    });
    if open && !completed {
        app.item_editor = Some(editor);
    }
}
