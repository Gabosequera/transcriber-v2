//! File-based clients use the same prepared command gate as MCP and GUI edits.
use crate::app::TranscriptorApp;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
use tv2_application::{Actor, CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{AssetId, DomainError, error::DomainResult};
use tv2_v1compat::contracts::ReviewKind;

struct Preview {
    root: PathBuf,
    request: Value,
    proposal: Value,
    documents: BTreeMap<String, String>,
    command: Option<PreparedCommand>,
    revision: u64,
    digest: String,
    diff: String,
    warnings: Vec<String>,
}
struct LegacyProposal {
    root: PathBuf,
    asset: AssetId,
    request: Value,
    proposal: Value,
    digest: String,
}
enum Completion {
    Request(PathBuf, Value),
    Preview(Box<Preview>),
    Published(PathBuf),
    Legacy(Box<LegacyProposal>),
    Watched { root: PathBuf, index: usize, request: Value, preview: Option<Box<Preview>> },
}
#[derive(Default)]
pub struct EditorialReview {
    pub open: bool,
    kind: usize,
    root: Option<PathBuf>,
    request: Option<Value>,
    preview: Option<Preview>,
    legacy: Option<LegacyProposal>,
    adopt_legacy: bool,
    pending: Option<crossbeam_channel::Receiver<DomainResult<Completion>>>,
    project: String,
    status: String,
    publication: Option<(PathBuf, BTreeMap<String, String>)>,
}
fn launch(
    ctx: &egui::Context,
    work: impl FnOnce() -> DomainResult<Completion> + Send + 'static,
) -> DomainResult<crossbeam_channel::Receiver<DomainResult<Completion>>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let wake = ctx.clone();
    std::thread::Builder::new().name("editorial-review".into()).spawn(move || {
        let _ = tx.send(work());
        wake.request_repaint();
    })?;
    Ok(rx)
}
fn read_json(path: &std::path::Path) -> DomainResult<Value> {
    use std::io::Read;
    let mut bytes = vec![];
    std::fs::File::open(path)?.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(DomainError::invalid("Documento supera 128 MiB"));
    }
    Ok(serde_json::from_slice(bytes.strip_prefix(&[239, 187, 191]).unwrap_or(&bytes))?)
}
fn kind(index: usize) -> ReviewKind {
    match index {
        1 => ReviewKind::Topics,
        2 => ReviewKind::Trims,
        3 => ReviewKind::Montage,
        _ => ReviewKind::Layers,
    }
}
fn stem(index: usize) -> &'static str {
    match index {
        1 => "topics",
        2 => "trims",
        3 => "montage",
        _ => "layers",
    }
}
fn request_path(root: &std::path::Path, index: usize) -> PathBuf {
    root.join(format!("views/{}-request.json", stem(index)))
}

/// Watcher inputs are copied into a new V2 transaction root; the watched source is read-only.
pub fn open_watched(app: &mut TranscriptorApp, ctx: &egui::Context, path: PathBuf, asset: AssetId) -> DomainResult<()> {
    if app.editorial_review.pending.is_some() || app.editorial_review.publication.is_some() {
        return Err(DomainError::invalid("Termina la revisión/publicación editorial activa antes de abrir otro documento"));
    }
    let project = app.project().clone();
    let rx = launch(ctx, move || {
        let input = read_json(&path)?;
        let schema = input["schema"].as_str().ok_or_else(|| DomainError::invalid("Documento editorial sin schema"))?;
        let index = (0..4)
            .find(|&i| schema == format!("editorial-{}-request/1", stem(i)) || schema == format!("editorial-{}-proposal/1", stem(i)))
            .ok_or_else(|| DomainError::invalid("Versión de pedido/propuesta no soportada"))?;
        let is_proposal = schema.ends_with("-proposal/1");
        let parent = path.parent().ok_or_else(|| DomainError::invalid("Documento sin carpeta"))?;
        let request_file = if is_proposal { parent.join(format!("{}-request.json", stem(index))) } else { path.clone() };
        let request = if is_proposal { read_json(&request_file)? } else { input.clone() };
        let mut docs = BTreeMap::from([(format!("views/{}-request.json", stem(index)), serde_json::to_string_pretty(&request)?)]);
        let previous_path = parent.join("topics-pass1.json");
        let previous = if index == 1 && request["pass_required"] == 2 { Some(read_json(&previous_path)?) } else { None };
        if let Some(previous) = &previous {
            docs.insert("views/topics-pass1.json".into(), serde_json::to_string_pretty(previous)?);
        }
        if read_json(&path)? != input
            || read_json(&request_file)? != request
            || previous.as_ref().is_some_and(|value| read_json(&previous_path).ok().as_ref() != Some(value))
        {
            return Err(DomainError::invalid("Documento cambiado durante captura; vuelve a revisar el aviso"));
        }
        let root = crate::paths::config_dir().join("editorial-reviews").join(format!("review-{}", tv2_domain::ids::random_hex12()));
        std::fs::create_dir_all(root.parent().expect("review root has parent"))?;
        let proposal_path = root.join("views/watched-proposal.json");
        if is_proposal {
            docs.insert("views/watched-proposal.json".into(), serde_json::to_string_pretty(&input)?);
        }
        tv2_application::documents::create(&root)?;
        tv2_application::documents::publish(&root, &docs)?;
        let preview = if is_proposal {
            match prepare(project, asset, root.clone(), index, proposal_path)? {
                Completion::Preview(value) => Some(value),
                Completion::Legacy(value) => return Ok(Completion::Legacy(value)),
                _ => unreachable!(),
            }
        } else {
            None
        };
        Ok(Completion::Watched { root, index, request, preview })
    })?;
    app.editorial_review = EditorialReview { open: true, project: app.project().project_id.clone(), pending: Some(rx), ..Default::default() };
    Ok(())
}

pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.editorial_review);
    if state.project != app.project().project_id {
        state = EditorialReview { project: app.project().project_id.clone(), open: state.open, ..Default::default() };
    }
    if let Some(rx) = &state.pending {
        let result = match rx.try_recv() {
            Ok(v) => Some(v),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(_) => Some(Err(DomainError::process("Worker editorial terminó sin resultado"))),
        };
        if let Some(result) = result {
            state.pending = None;
            match result {
                Ok(Completion::Request(root, request)) => {
                    state.status = format!("Pedido disponible en {}", root.display());
                    state.root = Some(root);
                    state.request = Some(request);
                    state.preview = None;
                    state.legacy = None;
                    state.adopt_legacy = false;
                }
                Ok(Completion::Preview(preview)) => {
                    state.status = "Propuesta validada; revisa el diff antes de aplicar".into();
                    if let Some(index) = (0..4).find(|&index| preview.request["schema"] == format!("editorial-{}-request/1", stem(index))) {
                        state.kind = index;
                    }
                    state.request = Some(preview.request.clone());
                    state.preview = Some(*preview);
                    state.legacy = None;
                    state.adopt_legacy = false;
                }
                Ok(Completion::Legacy(legacy)) => {
                    state.status = "Respuesta V1 sin request_id: requiere vinculación local explícita antes de validar.".into();
                    state.root = Some(legacy.root.clone());
                    state.kind = 0;
                    state.request = Some(legacy.request.clone());
                    state.preview = None;
                    state.adopt_legacy = false;
                    state.legacy = Some(*legacy);
                }
                Ok(Completion::Published(root)) => {
                    state.status =
                        format!("Documentos/recibo publicados en {}. Guarda el proyecto para conservar la revisión aplicada.", root.display());
                    state.preview = None;
                    state.publication = None;
                }
                Ok(Completion::Watched { root, index, request, preview }) => {
                    state.status = format!("Copia de revisión en {}", root.display());
                    state.root = Some(root);
                    state.kind = index;
                    state.request = Some(request);
                    state.preview = preview.map(|value| *value);
                    state.legacy = None;
                    state.adopt_legacy = false;
                }
                Err(e) => state.status = e.to_string(),
            }
        }
    }
    if state.open {
        let mut generate = false;
        let mut load = false;
        let mut proposal = false;
        let mut apply = false;
        let mut retry = false;
        let mut adopt = false;
        egui::Window::new("Pedidos y respuestas editoriales JSON").open(&mut state.open).default_width(740.0).show(ctx, |ui| {
            ui.label("Ciclo V1: pedido → propuesta → validación/diff → aplicar. Los modelos siguen fuera de esta etapa.");
            egui::ComboBox::from_label("Contrato").selected_text(["Capas AI", "Temas (dos pasadas)", "Recortes AI", "Montaje"][state.kind]).show_ui(
                ui,
                |ui| {
                    for (i, label) in ["Capas AI", "Temas (dos pasadas)", "Recortes AI", "Montaje"].into_iter().enumerate() {
                        ui.selectable_value(&mut state.kind, i, label);
                    }
                },
            );
            ui.add_enabled_ui(state.pending.is_none(), |ui| {
                generate = ui.button("Crear pedido en carpeta nueva…").clicked();
                load = ui.button("Abrir carpeta de pedido existente…").clicked();
                proposal = ui.add_enabled(state.root.is_some(), egui::Button::new("Cargar y validar respuesta JSON…")).clicked();
            });
            if state.pending.is_some() {
                ui.spinner();
            }
            ui.label(&state.status);
            if state.publication.is_some() {
                retry =
                    ui.add_enabled(state.pending.is_none(), egui::Button::new("Reintentar publicación del recibo (sin repetir edición)")).clicked();
            }
            if let Some(request) = &state.request {
                let stale = request.get("tv2_project_revision").and_then(Value::as_u64).is_some_and(|r| r != app.session.revision());
                ui.label(if stale {
                    "Pedido obsoleto respecto a la revisión actual"
                } else {
                    "Pedido de la revisión actual (se revalidan también sus digests)"
                });
                ui.collapsing("Request JSON", |ui| {
                    ui.monospace(serde_json::to_string_pretty(request).unwrap_or_default());
                });
            }
            if let Some(preview) = &state.preview {
                ui.label(&preview.diff);
                ui.label(format!("Digest de propuesta: {}", preview.digest));
                if !preview.warnings.is_empty() {
                    ui.colored_label(egui::Color32::YELLOW, format!("{} avisos de validación: revisa antes de aplicar", preview.warnings.len()));
                    egui::ScrollArea::vertical().id_salt("editorial-warnings").max_height(150.0).show(ui, |ui| {
                        for warning in &preview.warnings {
                            ui.label(warning);
                        }
                    });
                }
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    ui.monospace(serde_json::to_string_pretty(&preview.proposal).unwrap_or_default());
                });
                apply = ui
                    .add_enabled(
                        state.pending.is_none() && preview.revision == app.session.revision(),
                        egui::Button::new(if preview.command.is_some() {
                            "Aplicar propuesta revisada"
                        } else {
                            "Confirmar primera pasada y pedir segunda"
                        }),
                    )
                    .clicked();
            }
            if let Some(legacy) = &state.legacy {
                ui.colored_label(egui::Color32::YELLOW, "La respuesta original no identifica el pedido que la produjo.");
                ui.label(format!("Original: {} · destino: {}", legacy.digest, legacy.request["request_id"].as_str().unwrap_or("sin ID")));
                ui.collapsing("Respuesta V1 original", |ui| {
                    ui.monospace(serde_json::to_string_pretty(&legacy.proposal).unwrap_or_default());
                });
                ui.add_enabled_ui(state.pending.is_none(), |ui| {
                    ui.checkbox(&mut state.adopt_legacy, "Vincular explícitamente esta copia original al pedido mostrado");
                    adopt = ui.add_enabled(state.adopt_legacy, egui::Button::new("Vincular y validar; revisar diff antes de aplicar")).clicked();
                });
            }
        });
        if adopt && let Some(legacy) = state.legacy.take() {
            let project = app.project().clone();
            match launch(ctx, move || {
                let bound = tv2_v1compat::contracts::adopt_legacy_layers_proposal(&project, &legacy.asset, &legacy.request, &legacy.proposal)?;
                let archive = BTreeMap::from([
                    (format!(".work/reviews/{}-legacy-original.json", legacy.digest), serde_json::to_string_pretty(&legacy.proposal)?),
                    (format!(".work/reviews/{}-adopted.json", tv2_domain::digest::digest_json(&bound)), serde_json::to_string_pretty(&bound)?),
                ]);
                tv2_application::documents::publish(&legacy.root, &archive)?;
                prepare_values(project, legacy.asset, legacy.root, 0, legacy.request, bound)
            }) {
                Ok(rx) => {
                    state.pending = Some(rx);
                    state.adopt_legacy = false;
                }
                Err(error) => state.status = error.to_string(),
            }
        }
        if generate
            && let Some(asset) = app.current_layer_asset()
            && let Some(directory) = rfd::FileDialog::new().set_title("Destino del pedido editorial").pick_folder()
        {
            let project = app.project().clone();
            let k = kind(state.kind);
            match launch(ctx, move || {
                let id = format!("request-{}", tv2_domain::ids::random_hex12());
                let request = tv2_v1compat::contracts::prepare_request(&project, &asset, &id, k, None)?;
                let root = directory.join(&id);
                tv2_application::documents::create(&root)?;
                tv2_application::documents::publish(&root, &request.documents)?;
                Ok(Completion::Request(root, request.request))
            }) {
                Ok(rx) => state.pending = Some(rx),
                Err(e) => state.status = e.to_string(),
            }
        }
        if load && let Some(root) = rfd::FileDialog::new().set_title("Carpeta del pedido").pick_folder() {
            let index = state.kind;
            match launch(ctx, move || {
                let request = read_json(&request_path(&root, index))?;
                Ok(Completion::Request(root, request))
            }) {
                Ok(rx) => state.pending = Some(rx),
                Err(e) => state.status = e.to_string(),
            }
        }
        if proposal
            && let (Some(root), Some(asset)) = (state.root.clone(), app.current_layer_asset())
            && let Some(path) = rfd::FileDialog::new().set_title("Respuesta editorial JSON").add_filter("JSON", &["json"]).pick_file()
        {
            let project = app.project().clone();
            let index = state.kind;
            match launch(ctx, move || prepare(project, asset, root, index, path)) {
                Ok(rx) => {
                    state.pending = Some(rx);
                    state.preview = None;
                    state.legacy = None;
                    state.adopt_legacy = false;
                }
                Err(e) => state.status = e.to_string(),
            }
        }
        if apply && let Some(mut preview) = state.preview.take() {
            let result = if let Some(command) = preview.command.take() { app.session.commit_prepared(command).map(Some) } else { Ok(None) };
            match result {
                Ok(receipt) => {
                    if receipt.is_some() {
                        app.after_change();
                    }
                    let receipt_doc = json!({"schema":"transcriptor-editorial-apply/1","proposal_digest":preview.digest,"base_revision":preview.revision,"receipt":receipt,"state":if receipt.is_some(){"applied-in-session"}else{"pass-validated"}});
                    preview.documents.insert(
                        format!(".work/reviews/{}-receipt.json", preview.digest),
                        serde_json::to_string_pretty(&receipt_doc).unwrap_or_default(),
                    );
                    // Files may outlive undo. Their receipt records exactly the committed
                    // result; it never asserts that a later project save already happened.
                    let root = preview.root;
                    let documents = preview.documents;
                    state.publication = Some((root.clone(), documents.clone()));
                    match launch(ctx, move || {
                        tv2_application::documents::publish(&root, &documents)?;
                        Ok(Completion::Published(root))
                    }) {
                        Ok(rx) => state.pending = Some(rx),
                        Err(e) => state.status = e.to_string(),
                    }
                }
                Err(e) => state.status = e.to_string(),
            }
        }
        if retry && let Some((root, documents)) = state.publication.clone() {
            match launch(ctx, move || {
                tv2_application::documents::recover(&root)?;
                tv2_application::documents::publish(&root, &documents)?;
                Ok(Completion::Published(root))
            }) {
                Ok(rx) => state.pending = Some(rx),
                Err(e) => state.status = e.to_string(),
            }
        }
    }
    app.editorial_review = state;
}
fn prepare(project: tv2_domain::Project, asset: AssetId, root: PathBuf, index: usize, path: PathBuf) -> DomainResult<Completion> {
    let request = read_json(&request_path(&root, index))?;
    let proposal = read_json(&path)?;
    if index == 0 && proposal["schema"] == "editorial-layers-proposal/1" && proposal.get("request_id").is_none_or(Value::is_null) {
        let digest = tv2_domain::digest::digest_json(&proposal);
        // Both manual and watched paths stop here. No request ID is added and
        // no command is prepared before the separate local confirmation.
        return Ok(Completion::Legacy(Box::new(LegacyProposal { root, asset, request, proposal, digest })));
    }
    prepare_values(project, asset, root, index, request, proposal)
}
fn prepare_values(
    project: tv2_domain::Project,
    asset: AssetId,
    root: PathBuf,
    index: usize,
    request: Value,
    proposal: Value,
) -> DomainResult<Completion> {
    let previous = if index == 1 && request["pass_required"] == 2 { Some(read_json(&root.join("views/topics-pass1.json"))?) } else { None };
    let prepared = tv2_v1compat::contracts::prepare_proposal(&project, &asset, &request, &proposal, previous.as_ref())?;
    let revision = project.revision;
    let command = prepared
        .command
        .map(|command| {
            let request = CommandEnvelope::human(command)
                .with_base(revision)
                .with_actor(Actor::Agent { name: "editorial-json".into() })
                .with_idempotency(format!("editorial:{}:{}", prepared.request_id, prepared.proposal_digest));
            ProjectSession::new(project).prepare_command(request)
        })
        .transpose()?;
    let diff = command.as_ref().map(|c| c.diff().human()).unwrap_or_else(|| "Primera pasada: evidencia documental, sin edición del proyecto".into());
    // Persist the reviewed proposal before offering Apply, allowing explicit recovery
    // by reopening this request and validating the response against current content.
    let archive = BTreeMap::from([(format!(".work/reviews/{}-input.json", prepared.proposal_digest), serde_json::to_string_pretty(&proposal)?)]);
    tv2_application::documents::publish(&root, &archive)?;
    Ok(Completion::Preview(Box::new(Preview {
        root,
        request,
        proposal,
        documents: prepared.documents,
        command,
        revision,
        digest: prepared.proposal_digest,
        diff,
        warnings: prepared.warnings,
    })))
}
