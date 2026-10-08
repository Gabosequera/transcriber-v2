//! File-based clients use the same prepared command gate as MCP and GUI edits.
use crate::app::TranscriptorApp;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tv2_application::{Actor, CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{AssetId, DomainError, Ticks, TimeRange, error::DomainResult};
use tv2_pipeline::editorial::{self, BackendStatus, EditorialInput, EditorialJob, EditorialParameters, EditorialResult, EditorialRuntime, TrimMode};
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
    local: Option<(EditorialJob, EditorialResult)>,
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
    Backend(BackendStatus),
    Jobs(Vec<EditorialJob>),
    TopicsConfirmed { root: PathBuf, input: Box<EditorialInput>, documents: BTreeMap<String, String> },
}
#[derive(Clone, Default)]
pub struct LocalEditorialSettings {
    pub enabled: bool,
    pub python: String,
    pub worker: String,
    pub model: String,
    pub manifest: String,
    pub lock: String,
}
impl LocalEditorialSettings {
    fn runtime(&self) -> DomainResult<Option<EditorialRuntime>> {
        if !self.enabled {
            return Ok(None);
        }
        let paths = [&self.python, &self.worker, &self.model, &self.manifest, &self.lock];
        if paths.iter().any(|p| p.trim().is_empty()) {
            return Err(DomainError::invalid("Completa las cinco rutas públicas del backend local"));
        }
        if paths.iter().any(|p| !PathBuf::from(p.trim()).is_absolute()) {
            return Err(DomainError::invalid("Las rutas del backend local deben ser absolutas"));
        }
        Ok(Some(EditorialRuntime {
            python: self.python.trim().into(),
            worker: self.worker.trim().into(),
            model: self.model.trim().into(),
            model_manifest: self.manifest.trim().into(),
            lock: self.lock.trim().into(),
            work_root: local_root(),
        }))
    }
    fn load_local(&mut self) -> DomainResult<()> {
        let root = std::env::current_exe()?
            .ancestors()
            .skip(1)
            .take(6)
            .find(|p| p.join("workers/python/editorial_worker.py").is_file())
            .ok_or_else(|| DomainError::not_available("Selecciona las rutas del runtime local instalado"))?
            .to_owned();
        self.python = root.join(".local/e5-editorial-venv/Scripts/python.exe").to_string_lossy().into_owned();
        self.worker = root.join("workers/python/editorial_worker.py").to_string_lossy().into_owned();
        self.model = root.join(".local/models/qwen2.5-1.5b-instruct").to_string_lossy().into_owned();
        self.manifest = root.join(".local/models/qwen2.5-1.5b-instruct/model-manifest.json").to_string_lossy().into_owned();
        self.lock = root.join("workers/python/requirements-editorial.lock").to_string_lossy().into_owned();
        self.enabled = true;
        Ok(())
    }
}
struct LocalRunning {
    cancel: Arc<AtomicBool>,
    progress: crossbeam_channel::Receiver<Value>,
}
impl Drop for LocalRunning {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
fn local_root() -> PathBuf {
    crate::paths::config_dir().join("editorial-analysis")
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
    pub runtime: LocalEditorialSettings,
    parameters: EditorialParameters,
    scope_enabled: bool,
    scope_start: f64,
    scope_end: f64,
    backend: String,
    local_running: Option<LocalRunning>,
    progress: Option<Value>,
    jobs: Vec<EditorialJob>,
    continuation: Option<EditorialInput>,
    scanned: bool,
}
impl EditorialReview {
    pub fn cancel(&self) {
        if let Some(running) = &self.local_running {
            running.cancel.store(true, Ordering::Release);
        }
    }
    fn launch_local(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(Arc<AtomicBool>, &dyn Fn(Value)) -> DomainResult<Completion> + Send + 'static,
    ) -> DomainResult<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (progress_tx, progress) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        std::thread::Builder::new().name("local-editorial-proposal".into()).spawn(move || {
            let emit = |value| {
                let _ = progress_tx.try_send(value);
                wake.request_repaint();
            };
            let _ = tx.send(work(token, &emit));
            wake.request_repaint();
        })?;
        self.pending = Some(rx);
        self.local_running = Some(LocalRunning { cancel, progress });
        self.progress = None;
        Ok(())
    }
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
fn json_preview(value: &Value) -> String {
    struct PreviewWriter(Vec<u8>);
    impl std::io::Write for PreviewWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let available = (16 * 1024usize).saturating_sub(self.0.len());
            let count = available.min(bytes.len());
            self.0.extend_from_slice(&bytes[..count]);
            if count == 0 && !bytes.is_empty() {
                return Err(std::io::Error::other("preview limit"));
            }
            Ok(count)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = PreviewWriter(Vec::new());
    let truncated = serde_json::to_writer_pretty(&mut writer, value).is_err();
    let mut text = String::from_utf8_lossy(&writer.0).into_owned();
    if truncated {
        text.push_str("\n… Vista previa limitada a 16 KiB; el documento completo se conserva en la carpeta del pedido.");
    }
    text
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
fn local_preview(snapshot: tv2_domain::Project, record: EditorialJob, result: EditorialResult, cancel: &AtomicBool) -> DomainResult<Completion> {
    let prepared = editorial::prepare_result(snapshot, &record, &result, cancel)?;
    let proposal = editorial::read_verified_proposal(&record, &result, cancel)?;
    // Every explicit review gets its own publication folder, preserving earlier
    // human receipts and the confirmed topics map when a result is reopened.
    let root = record.payload.runtime.work_root.join(&record.id).join(format!("review-{}", tv2_domain::ids::random_hex12()));
    tv2_application::documents::create(&root)?;
    let mut archive = record.payload.input.documents.clone();
    archive.insert(format!(".work/reviews/{}-input.json", prepared.review.proposal_digest), serde_json::to_string_pretty(&proposal)?);
    tv2_application::documents::publish(&root, &archive)?;
    let diff = prepared
        .command
        .as_ref()
        .map(|c| c.diff().human())
        .unwrap_or_else(|| "Primera pasada: mapa validado pendiente de confirmación humana; sin edición del proyecto".into());
    Ok(Completion::Preview(Box::new(Preview {
        root,
        request: record.payload.input.request.clone(),
        proposal,
        documents: prepared.review.documents,
        command: prepared.command,
        revision: record.revision,
        digest: prepared.review.proposal_digest,
        diff,
        warnings: prepared.review.warnings,
        local: Some((record, result)),
    })))
}
fn stored_job(record: &EditorialJob) -> DomainResult<EditorialJob> {
    Ok(serde_json::from_value(read_json(&record.payload.runtime.work_root.join("jobs").join(format!("{}.json", record.id)))?)?)
}
fn execute_local(
    snapshot: tv2_domain::Project,
    record: EditorialJob,
    resume: bool,
    cancel: Arc<AtomicBool>,
    emit: &dyn Fn(Value),
) -> DomainResult<Completion> {
    if record.payload.input.parameters.max_tokens > 2048 {
        return Err(DomainError::invalid("Este job supera el límite local de 2048 tokens de respuesta; prepara un pedido nuevo"));
    }
    if snapshot.project_id != record.project_id
        || snapshot.revision != record.revision
        || tv2_domain::digest::digest_json(&serde_json::to_value(&snapshot)?) != record.payload.input.project_digest
    {
        return Err(DomainError::precondition("Abre la revisión original del job o crea un pedido nuevo"));
    }
    let result = editorial::run(&record, resume, cancel.clone(), emit)?;
    local_preview(snapshot, stored_job(&record)?, result, &cancel)
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
    app.editorial_review = EditorialReview {
        open: true,
        project: app.project().project_id.clone(),
        pending: Some(rx),
        runtime: app.editorial_review.runtime.clone(),
        parameters: app.editorial_review.parameters.clone(),
        ..Default::default()
    };
    Ok(())
}

pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.editorial_review);
    if state.project != app.project().project_id {
        state.cancel();
        state = EditorialReview {
            project: app.project().project_id.clone(),
            open: state.open,
            runtime: state.runtime,
            parameters: state.parameters,
            ..Default::default()
        };
    }
    if let Some(running) = &state.local_running {
        while let Ok(progress) = running.progress.try_recv() {
            state.progress = Some(progress);
        }
    }
    if let Some(rx) = &state.pending {
        let result = match rx.try_recv() {
            Ok(v) => Some(v),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(_) => Some(Err(DomainError::process("Worker editorial terminó sin resultado"))),
        };
        if let Some(result) = result {
            let local_operation = state.local_running.is_some();
            state.pending = None;
            state.local_running = None;
            state.progress = None;
            match result {
                Ok(Completion::Request(root, request)) => {
                    state.status = format!("Pedido disponible en {}", root.display());
                    state.root = Some(root);
                    state.request = Some(request);
                    state.preview = None;
                    state.legacy = None;
                    state.adopt_legacy = false;
                    state.continuation = None;
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
                    state.root = state.preview.as_ref().map(|p| p.root.clone());
                    state.continuation = None;
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
                    state.status = if state.continuation.is_some() {
                        format!(
                            "Mapa validado y recibo publicados en {}. La segunda pasada requiere Ejecutar segunda pasada; el proyecto no se ha editado.",
                            root.display()
                        )
                    } else {
                        format!("Documentos/recibo publicados en {}. Guarda el proyecto para conservar una edición aplicada.", root.display())
                    };
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
                Ok(Completion::Backend(status)) => {
                    state.backend = match status {
                        BackendStatus::NotConfigured => "Backend local no configurado; el ciclo JSON manual sigue disponible".into(),
                        BackendStatus::MissingResources { paths } => {
                            format!("Recursos faltantes: {}", paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" · "))
                        }
                        BackendStatus::ConfiguredUnverified { identity } => {
                            format!("Recursos verificados por hash; inferencia no verificada. {} · {}", identity.model_id, identity.model_revision)
                        }
                    };
                    state.status = "Comprobación de recursos finalizada; no se ejecutó inferencia".into();
                }
                Ok(Completion::Jobs(records)) => {
                    state.jobs = records;
                    if state.status.is_empty() || state.status == "Recuperando jobs editoriales guardados" {
                        state.status = "Jobs editoriales recuperados; elige revisar o reanudar explícitamente".into();
                    }
                }
                Ok(Completion::TopicsConfirmed { root, input, documents }) => {
                    state.request = Some(input.request.clone());
                    state.continuation = Some(*input);
                    state.root = Some(root);
                    state.preview = None;
                    let root = state.root.clone().expect("topics review root");
                    state.publication = Some((root.clone(), documents.clone()));
                    match launch(ctx, move || {
                        tv2_application::documents::publish(&root, &documents)?;
                        Ok(Completion::Published(root))
                    }) {
                        Ok(rx) => state.pending = Some(rx),
                        Err(error) => state.status = error.to_string(),
                    }
                    state.status = "Primera pasada confirmada. Publicando mapa validado y recibo; la segunda pasada requiere otro clic".into();
                }
                Err(e) => {
                    state.status = if state.publication.is_some() {
                        format!("{} Reintenta la publicación del recibo sin volver a aplicar la propuesta.", e)
                    } else if local_operation {
                        format!("{} Los jobs conservan su resultado: actualiza la lista y revisa o reanuda explícitamente.", e)
                    } else {
                        e.to_string()
                    }
                }
            }
        }
    }
    if state.open && !state.scanned && state.pending.is_none() {
        match launch(ctx, || Ok(Completion::Jobs(tv2_application::jobs::discover(&local_root().join("jobs"))?))) {
            Ok(rx) => {
                state.pending = Some(rx);
                state.scanned = true;
            }
            Err(error) => state.status = error.to_string(),
        }
    }
    if state.open {
        let mut generate = false;
        let mut load = false;
        let mut proposal = false;
        let mut apply = false;
        let mut retry = false;
        let mut adopt = false;
        let mut check_backend = false;
        let mut scan_jobs = false;
        let mut start_local = false;
        let mut next_local = false;
        let mut saved_action: Option<(EditorialJob, bool)> = None;
        let mut window_open = state.open;
        egui::Window::new("Pedidos y respuestas editoriales JSON")
            .open(&mut window_open)
            .default_width(740.0)
            .default_height(650.0)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label("Pedido → propuesta → validación/diff → aplicar. Puedes intercambiar JSON o ejecutar un modelo local opcional.");
                egui::ComboBox::from_label("Contrato")
                    .selected_text(["Capas AI", "Temas (dos pasadas)", "Recortes AI", "Montaje"][state.kind])
                    .show_ui(ui, |ui| {
                        for (i, label) in ["Capas AI", "Temas (dos pasadas)", "Recortes AI", "Montaje"].into_iter().enumerate() {
                            ui.selectable_value(&mut state.kind, i, label);
                        }
                    });
                ui.add_enabled_ui(state.pending.is_none() && state.publication.is_none(), |ui| {
                    generate = ui.button("Crear pedido en carpeta nueva…").clicked();
                    load = ui.button("Abrir carpeta de pedido existente…").clicked();
                    proposal = ui.add_enabled(state.root.is_some(), egui::Button::new("Cargar y validar respuesta JSON…")).clicked();
                });
                ui.collapsing("Modelo local opcional",|ui| {
                    ui.add_enabled_ui(state.pending.is_none(),|ui| {
                        let mut changed=ui.checkbox(&mut state.runtime.enabled,"Usar backend local para propuestas").changed();
                        if ui.button("Cargar rutas del runtime instalado en V2").clicked() {
                            match state.runtime.load_local() {Ok(())=>changed=true,Err(error)=>state.status=error.to_string()}
                        }
                        egui::Grid::new("editorial-runtime-paths").num_columns(3).show(ui,|ui| {
                            for (label,path,directory) in [("Python",&mut state.runtime.python,false),("Worker",&mut state.runtime.worker,false),
                                ("Modelo",&mut state.runtime.model,true),("Manifest",&mut state.runtime.manifest,false),("Lock",&mut state.runtime.lock,false)] {
                                ui.label(label);changed|=ui.text_edit_singleline(path).changed();
                                if ui.button("Elegir…").clicked() {
                                    let dialog=rfd::FileDialog::new().set_title(label);
                                    let picked=if directory {dialog.pick_folder()}else {dialog.pick_file()};
                                    if let Some(picked)=picked {*path=picked.to_string_lossy().into_owned();changed=true;}
                                }
                                ui.end_row();
                            }
                        });
                        if changed {state.backend=if state.runtime.enabled {"Configurado sin verificar; comprueba los recursos antes de ejecutar"}else {"Backend local no configurado"}.into();}
                        check_backend=ui.button("Comprobar recursos sin cargar el modelo").clicked();
                        ui.horizontal(|ui| {ui.label("Tokens máximos de respuesta");ui.add(egui::DragValue::new(&mut state.parameters.max_tokens).range(1..=2048));});
                        ui.horizontal(|ui| {ui.label("Temperatura");ui.add(egui::DragValue::new(&mut state.parameters.temperature).range(0.0..=2.0).speed(0.05));
                            ui.label("Seed");ui.add(egui::DragValue::new(&mut state.parameters.seed));});
                        ui.horizontal(|ui| {ui.label("Timeout (s)");ui.add(egui::DragValue::new(&mut state.parameters.timeout_seconds).range(1..=86400));});
                        if state.kind==2 {
                            egui::ComboBox::from_label("Recortes").selected_text(if state.parameters.trim_mode==TrimMode::Deep {"Deep · carril ai-deep"}else {"Contenido · carril ai"}).show_ui(ui,|ui| {
                                ui.selectable_value(&mut state.parameters.trim_mode,TrimMode::Content,"Contenido · carril ai");
                                ui.selectable_value(&mut state.parameters.trim_mode,TrimMode::Deep,"Deep · carril ai-deep");
                            });
                        }
                        ui.checkbox(&mut state.scope_enabled,"Limitar ámbito temporal del pedido");
                        if state.scope_enabled {ui.horizontal(|ui| {ui.label("Desde (s)");ui.add(egui::DragValue::new(&mut state.scope_start).range(0.0..=86400.0).speed(0.1));
                            ui.label("Hasta (s)");ui.add(egui::DragValue::new(&mut state.scope_end).range(0.0..=86400.0).speed(0.1));});}
                        ui.label("Contexto del runtime local: 4096 tokens. Reduce el ámbito si el pedido completo no cabe; no se recorta texto automáticamente.");
                        start_local=ui.add_enabled(state.runtime.enabled && state.publication.is_none(),egui::Button::new("Crear pedido y ejecutar propuesta local")).clicked();
                        if state.continuation.is_some() {next_local=ui.add_enabled(state.runtime.enabled && state.publication.is_none(),egui::Button::new("Ejecutar segunda pasada de temas confirmada")).clicked();}
                    });
                    ui.label(if state.backend.is_empty() {"Backend local no configurado"}else {&state.backend});
                    ui.label(format!("Jobs y recibos: {}",local_root().display()));
                });
                if state.local_running.is_some() {
                    if ui.button("Cancelar propuesta local").clicked() {state.cancel();state.status="Cancelación solicitada; el resultado no se aplica".into();}
                    if let Some(progress)=&state.progress {
                        if let Some(fraction)=progress["fraction"].as_f64() {ui.add(egui::ProgressBar::new(fraction.clamp(0.0,1.0) as f32).show_percentage());}
                        ui.label(match progress["stage"].as_str() {
                            Some("load_editorial_model") => "Cargando modelo local…",
                            Some("loaded_editorial_model") => "Modelo cargado",
                            Some("quantize_editorial_linears" | "editorial_remaining_float") => "Preparando modelo para la CPU…",
                            Some("editorial_inference") => "Generando propuesta…",
                            Some("editorial_complete") => "Propuesta generada; verificando…",
                            _ => "Preparando propuesta local…",
                        });
                    }
                }
                ui.collapsing("Jobs editoriales guardados",|ui| {
                    ui.label("Reanudar utiliza las rutas y parámetros capturados por ese job; revisar un resultado no ejecuta otra inferencia.");
                    scan_jobs=ui.add_enabled(state.pending.is_none(),egui::Button::new("Actualizar jobs y recuperar interrumpidos")).clicked();
                    egui::ScrollArea::vertical().id_salt("editorial-local-jobs").max_height(180.0).show(ui,|ui| {
                        for job in &state.jobs {
                            if job.project_id!=app.project().project_id {continue;}
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("{} · {} · pasada {} · {:?} · revisión {}",job.id,job.payload.input.kind.stem(),job.payload.input.request["pass_required"],job.state,job.revision));
                                let current=job.revision==app.session.revision();
                                let resumable=matches!(job.state,tv2_application::jobs::JobState::Queued|tv2_application::jobs::JobState::Interrupted|tv2_application::jobs::JobState::Failed|tv2_application::jobs::JobState::Cancelled);
                                if job.state==tv2_application::jobs::JobState::Succeeded {
                                    if ui.add_enabled(current && state.pending.is_none() && state.publication.is_none(),egui::Button::new("Revisar resultado")).clicked() {saved_action=Some((job.clone(),false));}
                                } else if resumable && ui.add_enabled(current && state.pending.is_none() && state.publication.is_none(),egui::Button::new("Reanudar explícitamente")).clicked() {saved_action=Some((job.clone(),true));}
                            });
                            if let Some(error)=&job.error {ui.label(error);}
                        }
                    });
                });
                if state.pending.is_some() {
                    ui.spinner();
                }
                ui.label(&state.status);
                if state.publication.is_some() {
                    retry = ui
                        .add_enabled(state.pending.is_none(), egui::Button::new("Reintentar publicación del recibo (sin repetir edición)"))
                        .clicked();
                }
                if let Some(request) = &state.request {
                    let stale = request.get("tv2_project_revision").and_then(Value::as_u64).is_some_and(|r| r != app.session.revision());
                    ui.label(if stale {
                        "Pedido obsoleto respecto a la revisión actual"
                    } else {
                        "Pedido de la revisión actual (se revalidan también sus digests)"
                    });
                    ui.collapsing("Request JSON", |ui| {
                        ui.monospace(json_preview(request));
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
                        ui.monospace(json_preview(&preview.proposal));
                    });
                    apply = ui
                        .add_enabled(
                            state.pending.is_none() && state.publication.is_none() && preview.revision == app.session.revision(),
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
                        ui.monospace(json_preview(&legacy.proposal));
                    });
                    ui.add_enabled_ui(state.pending.is_none(), |ui| {
                        ui.checkbox(&mut state.adopt_legacy, "Vincular explícitamente esta copia original al pedido mostrado");
                        adopt = ui.add_enabled(state.adopt_legacy, egui::Button::new("Vincular y validar; revisar diff antes de aplicar")).clicked();
                    });
                }
            });
        state.open = window_open;
        if check_backend {
            match state.runtime.runtime() {
                Ok(runtime) => {
                    match state.launch_local(ctx, move |cancel, _| Ok(Completion::Backend(editorial::backend_status(runtime.as_ref(), &cancel)?))) {
                        Ok(()) => state.status = "Comprobando hashes del backend; no se carga el modelo".into(),
                        Err(error) => state.status = error.to_string(),
                    }
                }
                Err(error) => state.backend = error.to_string(),
            }
        }
        if scan_jobs {
            match launch(ctx, || Ok(Completion::Jobs(tv2_application::jobs::discover(&local_root().join("jobs"))?))) {
                Ok(rx) => {
                    state.pending = Some(rx);
                    state.status = "Recuperando jobs editoriales guardados".into();
                }
                Err(error) => state.status = error.to_string(),
            }
        }
        if start_local {
            let result = (|| {
                let runtime = state.runtime.runtime()?.ok_or_else(|| DomainError::not_available("Configura el backend local antes de ejecutar"))?;
                let asset =
                    app.current_layer_asset().ok_or_else(|| DomainError::not_available("Selecciona el medio editorial con un master incorporado"))?;
                let scope = if state.scope_enabled {
                    if !state.scope_start.is_finite() || !state.scope_end.is_finite() || state.scope_end <= state.scope_start {
                        return Err(DomainError::invalid("El ámbito debe terminar después de su inicio"));
                    }
                    Some(TimeRange::new(Ticks::from_seconds_f64(state.scope_start), Ticks::from_seconds_f64(state.scope_end)))
                } else {
                    None
                };
                let project = app.project().clone();
                let k = kind(state.kind);
                let mut parameters = state.parameters.clone();
                if k != ReviewKind::Trims {
                    parameters.trim_mode = TrimMode::Content;
                }
                if parameters.max_tokens > 2048 {
                    return Err(DomainError::invalid("La interfaz local admite como máximo 2048 tokens de respuesta"));
                }
                state.launch_local(ctx, move |cancel, emit| {
                    let id = format!("request-{}", tv2_domain::ids::random_hex12());
                    let input = editorial::prepare_input(&project, &asset, &id, k, scope, parameters)?;
                    let record = editorial::enqueue(input, runtime, &cancel)?;
                    execute_local(project, record, false, cancel, emit)
                })
            })();
            match result {
                Ok(()) => {
                    state.preview = None;
                    state.continuation = None;
                    state.scanned = false;
                    state.status = "Propuesta local en curso; se revisará antes de aplicar".into();
                }
                Err(error) => state.status = error.to_string(),
            }
        }
        if next_local && let Some(input) = state.continuation.clone() {
            let result = (|| {
                let runtime = state.runtime.runtime()?.ok_or_else(|| DomainError::not_available("Configura el backend local"))?;
                let snapshot = app.project().clone();
                state.launch_local(ctx, move |cancel, emit| {
                    if input.parameters.max_tokens > 2048 {
                        return Err(DomainError::invalid("El pedido confirmado supera 2048 tokens de respuesta; prepara un ciclo nuevo"));
                    }
                    if tv2_domain::digest::digest_json(&serde_json::to_value(&snapshot)?) != input.project_digest {
                        return Err(DomainError::precondition("Proyecto cambiado desde la confirmación del mapa; prepara otro ciclo de temas"));
                    }
                    let record = editorial::enqueue(input, runtime, &cancel)?;
                    execute_local(snapshot, record, false, cancel, emit)
                })
            })();
            match result {
                Ok(()) => {
                    state.preview = None;
                    state.scanned = false;
                    state.status = "Segunda pasada solicitada explícitamente con el mapa validado".into();
                }
                Err(error) => state.status = error.to_string(),
            }
        }
        if let Some((record, resume)) = saved_action {
            let snapshot = app.project().clone();
            match state.launch_local(ctx, move |cancel, emit| {
                if resume {
                    execute_local(snapshot, record, true, cancel, emit)
                } else {
                    let result: EditorialResult =
                        serde_json::from_value(record.result.clone().ok_or_else(|| DomainError::invalid("Job final sin recibo"))?)?;
                    local_preview(snapshot, record, result, &cancel)
                }
            }) {
                Ok(()) => {
                    state.preview = None;
                    state.scanned = false;
                    state.status = if resume { "Reanudación editorial explícita en curso" } else { "Validando recibo y preparando diff" }.into();
                }
                Err(error) => state.status = error.to_string(),
            }
        }
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
            let local_job_id = preview.local.as_ref().map(|(record, _)| record.id.clone());
            if preview.command.is_none()
                && let Some((record, result)) = preview.local.take()
            {
                let snapshot = app.project().clone();
                match state.launch_local(ctx, move |cancel, _| {
                    editorial::prepare_result(snapshot, &record, &result, &cancel)?;
                    let input = editorial::continue_topics(&record, &result, &cancel)?;
                    let mut documents = preview.documents;
                    documents.insert(
                        format!(".work/reviews/{}-receipt.json", preview.digest),
                        serde_json::to_string_pretty(&json!({
                        "schema":"transcriptor-editorial-apply/1","proposal_digest":preview.digest,"base_revision":preview.revision,
                        "receipt":null,"state":"pass-validated","source_job_id":record.id,"human_confirmed":true}))?,
                    );
                    Ok(Completion::TopicsConfirmed { root: preview.root, input: Box::new(input), documents })
                }) {
                    Ok(()) => state.status = "Revalidando mapa de primera pasada confirmado; no se ejecuta otra inferencia".into(),
                    Err(error) => {
                        state.status = format!(
                            "{} Resultado conservado en {}: vuelve a Revisar resultado.",
                            error,
                            local_job_id.as_deref().unwrap_or("el job editorial")
                        )
                    }
                }
            } else {
                let result = if let Some(command) = preview.command.take() { app.session.commit_prepared(command).map(Some) } else { Ok(None) };
                match result {
                    Ok(receipt) => {
                        if receipt.is_some() {
                            app.after_change();
                        }
                        let receipt_doc = json!({"schema":"transcriptor-editorial-apply/1","proposal_digest":preview.digest,"base_revision":preview.revision,"receipt":receipt,"source_job_id":local_job_id,"state":if receipt.is_some(){"applied-in-session"}else{"pass-validated"}});
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
                    Err(e) => {
                        state.status = format!(
                            "{} Propuesta conservada{}. Vuelve a revisarla antes de aplicar.",
                            e,
                            local_job_id.as_ref().map(|id| format!(" en el job {id}")).unwrap_or_else(|| format!(" en {}", preview.root.display()))
                        )
                    }
                }
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
        local: None,
    })))
}
