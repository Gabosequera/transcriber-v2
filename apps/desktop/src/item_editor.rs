use crate::app::TranscriptorApp;
use tv2_application::{CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{Command, ItemId, LayerId, LayerKind, SemanticItem, Ticks, TimeRange};

pub struct ItemEditor {
    project: String,
    revision: u64,
    layer: LayerId,
    item: ItemId,
    label: String,
    comment: String,
    parent: String,
    ranges: Vec<(String, String)>,
    confidence: Option<String>,
    pending: Option<crossbeam_channel::Receiver<Result<PreparedCommand, String>>>,
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
            confidence: item.extra.get("confidence").filter(|value| !value.is_null()).map(ToString::to_string),
            pending: None,
            error: None,
        }
    }

    fn command(&self, block: bool) -> Result<Command, String> {
        let ranges = self
            .ranges
            .iter()
            .map(|(a, b)| {
                let start = crate::ui_panels::parse_time(a).ok_or_else(|| format!("Inicio inválido: {a}"))?;
                let end = crate::ui_panels::parse_time(b).ok_or_else(|| format!("Fin inválido: {b}"))?;
                Ok(TimeRange::new(start, end))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut commands = vec![
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
        ];
        if block {
            if let Some(value) = &self.confidence {
                let confidence = value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                    .ok_or("Confianza inválida: introduce un número entre 0 y 1.")?;
                commands.push(Command::SetBlockConfidence { layer_id: self.layer.clone(), item_id: self.item.clone(), confidence });
            }
            // Same default as V1 ChunkReviewWindow / snap_plan_to_safe_boundaries.
            // The atomic prepare rejects missing evidence or an unsafe boundary.
            commands.push(Command::SnapBlockBoundaries { layer_id: self.layer.clone(), radius: Ticks::from_seconds(15) });
        }
        Ok(Command::Batch { label: if block { "Revisar bloque y bordes seguros" } else { "Editar item editorial" }.into(), commands })
    }
}

pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let Some(mut editor) = app.item_editor.take() else { return };
    let mut open = true;
    let mut completed = false;
    let block =
        app.project().layer(&editor.layer).is_some_and(|layer| layer.kind == LayerKind::Blocks && layer.extra.contains_key("v1_chunks_header"));
    let fixed_edges = app
        .project()
        .layer(&editor.layer)
        .and_then(|layer| {
            let item = layer.item(&editor.item)?;
            let duration = app.project().asset(&layer.asset_id)?.duration();
            Some((block && item.start() == Ticks::ZERO, block && item.end() == duration))
        })
        .unwrap_or_default();
    let stale = editor.project != app.project().project_id || editor.revision != app.session.revision();
    let ready = editor.pending.as_ref().and_then(|receiver| match receiver.try_recv() {
        Ok(result) => Some(result),
        Err(crossbeam_channel::TryRecvError::Empty) => None,
        Err(_) => Some(Err("El worker de revisión terminó sin resultado.".into())),
    });
    if ready.is_some() {
        editor.pending = None;
    }
    let busy = editor.pending.is_some() || ready.is_some();
    egui::Window::new("Editar item: texto, rangos y jerarquía").open(&mut open).default_width(480.0).show(ctx, |ui| {
        if stale {
            ui.colored_label(egui::Color32::YELLOW, "La revisión cambió. Cierra y vuelve a abrir el editor para usar la base actual.");
        }
        ui.add_enabled_ui(!busy && !stale, |ui| {
            ui.label("Etiqueta");
            ui.text_edit_singleline(&mut editor.label);
            ui.label("Comentario");
            ui.text_edit_multiline(&mut editor.comment);
            if block {
                ui.label("Bloque contiguo. Los límites vecinos se ajustarán y el plan recalculará bordes seguros en un radio de 15 s.");
                if let Some(confidence) = &mut editor.confidence {
                    ui.horizontal(|ui| {
                        ui.label("Confianza (0–1)");
                        ui.text_edit_singleline(confidence);
                    });
                } else {
                    ui.label("Confianza no especificada en el original.");
                    if ui.button("Especificar confianza").clicked() {
                        editor.confidence = Some(String::new());
                    }
                }
            } else {
                ui.label("ID del padre (vacío: raíz)");
                ui.text_edit_singleline(&mut editor.parent);
            }
            ui.label("Rangos fuente: segundos o HH:MM:SS.mmm. Deben estar ordenados y dentro del padre.");
            let mut remove = None;
            for (n, (a, b)) in editor.ranges.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_enabled(!fixed_edges.0, egui::TextEdit::singleline(a).desired_width(145.0));
                    ui.add_enabled(!fixed_edges.1, egui::TextEdit::singleline(b).desired_width(145.0));
                    if !block && ui.small_button("Quitar").clicked() {
                        remove = Some(n);
                    }
                });
            }
            if let Some(n) = remove {
                editor.ranges.remove(n);
            }
            if !block && ui.button("Añadir rango").clicked() {
                editor.ranges.push((String::new(), String::new()));
            }
            if ui.button("Aplicar como un cambio").clicked() {
                match editor.command(block) {
                    Ok(command) => {
                        let snapshot = app.project().clone();
                        let revision = editor.revision;
                        let wake = ctx.clone();
                        let (sender, receiver) = crossbeam_channel::bounded(1);
                        match std::thread::Builder::new().name("item-editor-preview".into()).spawn(move || {
                            let result = ProjectSession::new(snapshot)
                                .prepare_command(CommandEnvelope::human(command).with_base(revision))
                                .map_err(|error| error.to_string());
                            let _ = sender.send(result);
                            wake.request_repaint();
                        }) {
                            Ok(_) => {
                                editor.pending = Some(receiver);
                                editor.error = None;
                            }
                            Err(error) => editor.error = Some(error.to_string()),
                        }
                    }
                    Err(error) => editor.error = Some(error),
                }
            }
        });
        if busy {
            ui.spinner();
            ui.label("Calculando y validando el cambio… Cerrar cancela su aplicación.");
        }
        if let Some(error) = &editor.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    });
    if open && let Some(result) = ready {
        let result = if stale {
            Err("La revisión cambió durante la preparación; no se aplicó ningún cambio.".into())
        } else {
            result.and_then(|prepared| app.session.commit_prepared(prepared).map_err(|error| error.to_string()))
        };
        match result {
            Ok(_) => {
                app.after_change();
                completed = true;
            }
            Err(error) => editor.error = Some(error),
        }
    }
    if open && !completed {
        app.item_editor = Some(editor);
    }
}
