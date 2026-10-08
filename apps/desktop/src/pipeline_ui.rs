//! The UI only starts workers and confirms already prepared common commands.
use crate::app::{Severity, TranscriptorApp};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tv2_application::{
    jobs::{self, JobRecord, JobState},
    session::PreparedCommand,
};
use tv2_domain::{AssetId, DomainError, error::DomainResult};
use tv2_pipeline::{
    AnalysisPayload, AnalysisResult, Parameters, Runtime,
    alignment::AlignmentRuntime,
    arousal::ArousalRuntime,
    generation::GenerationRuntime,
    laughter::LaughterRuntime,
    workflow::{self, AlignmentStep, ArousalStep, LaughterStep, WorkflowPayload, WorkflowPlan, WorkflowResult},
};

#[derive(Clone)]
pub enum SavedAnalysis {
    Asr(Box<JobRecord<AnalysisPayload>>),
    Workflow(Box<JobRecord<WorkflowPayload>>),
}
impl SavedAnalysis {
    fn id(&self) -> &str {
        match self {
            Self::Asr(record) => &record.id,
            Self::Workflow(record) => &record.id,
        }
    }
    fn project_id(&self) -> &str {
        match self {
            Self::Asr(record) => &record.project_id,
            Self::Workflow(record) => &record.project_id,
        }
    }
    fn state(&self) -> JobState {
        match self {
            Self::Asr(record) => record.state,
            Self::Workflow(record) => record.state,
        }
    }
    fn revision(&self) -> u64 {
        match self {
            Self::Asr(record) => record.revision,
            Self::Workflow(record) => record.revision,
        }
    }
    fn error(&self) -> Option<&str> {
        match self {
            Self::Asr(record) => record.error.as_deref(),
            Self::Workflow(record) => record.error.as_deref(),
        }
    }
    fn asset_id(&self) -> &AssetId {
        match self {
            Self::Asr(record) => &record.payload.asset.id,
            Self::Workflow(record) => &record.payload.asr.payload.asset.id,
        }
    }
    fn prepare(&self, snapshot: tv2_domain::Project, cancel: &AtomicBool, rebase: bool) -> DomainResult<PreparedCommand> {
        match self {
            Self::Asr(record) => {
                let result: AnalysisResult =
                    serde_json::from_value(record.result.clone().ok_or_else(|| DomainError::invalid("Job completado sin resultado"))?)?;
                if rebase {
                    tv2_pipeline::prepare_rebased_result(snapshot, record, &result, cancel)
                } else {
                    tv2_pipeline::prepare_result(snapshot, record, &result, cancel)
                }
            }
            Self::Workflow(record) => {
                let result: WorkflowResult =
                    serde_json::from_value(record.result.clone().ok_or_else(|| DomainError::invalid("Workflow completado sin resultado"))?)?;
                workflow::validate_result(record, &result, cancel)?;
                tv2_pipeline::generation::prepare_result(snapshot, &result.generation, &result.result, cancel, rebase)
            }
        }
    }
}
type Completion = (SavedAnalysis, DomainResult<PreparedCommand>);

struct WorkflowOptions {
    alignment_enabled: bool,
    arousal_enabled: bool,
    laughter_enabled: bool,
    alignment: AlignmentStep,
    arousal: ArousalStep,
    laughter: LaughterStep,
    generation: GenerationRuntime,
    work_root: PathBuf,
}
impl WorkflowOptions {
    fn new(root: &std::path::Path, analysis: &std::path::Path) -> Self {
        Self {
            alignment_enabled: false,
            arousal_enabled: false,
            laughter_enabled: false,
            alignment: AlignmentStep {
                runtime: AlignmentRuntime {
                    python: root.join(".local/e5-alignment-venv/Scripts/python.exe"),
                    worker: root.join("workers/python/alignment_worker.py"),
                    model: root.join(".local/models/mms-fa/model.pt"),
                    model_manifest: root.join(".local/models/mms-fa/model-manifest.json"),
                    lock: root.join("workers/python/requirements-alignment.lock"),
                    work_root: analysis.join("alignment"),
                },
                parameters: Default::default(),
            },
            arousal: ArousalStep {
                runtime: ArousalRuntime {
                    python: root.join(".local/e5-arousal-venv/Scripts/python.exe"),
                    worker: root.join("workers/python/arousal_worker.py"),
                    model: root.join(".local/models/arousal-audeering"),
                    model_manifest: root.join(".local/models/arousal-audeering/model-manifest.json"),
                    lock: root.join("workers/python/requirements-arousal.lock"),
                    work_root: analysis.join("arousal"),
                },
                parameters: Default::default(),
            },
            laughter: LaughterStep {
                runtime: LaughterRuntime {
                    python: root.join(".local/e5-laughter-venv/Scripts/python.exe"),
                    worker: root.join("workers/python/laughter_worker.py"),
                    model: root.join(".local/models/laughter-omine/model.safetensors"),
                    model_manifest: root.join(".local/models/laughter-omine/model-manifest.json"),
                    config: root.join(".local/models/laughter-omine/config.json"),
                    lock: root.join("workers/python/requirements-laughter.lock"),
                    work_root: analysis.join("laughter"),
                },
                parameters: Default::default(),
            },
            generation: GenerationRuntime {
                python: root.join(".local/e5-venv/Scripts/python.exe"),
                worker: root.join("workers/python/finalize_worker.py"),
                assembler: root.join("workers/python/finalize.py"),
                derivation: root.join("workers/python/worker.py"),
                work_root: analysis.join("generation"),
            },
            work_root: analysis.join("workflows"),
        }
    }
    fn plan(&self) -> WorkflowPlan {
        WorkflowPlan {
            alignment: self.alignment_enabled.then(|| self.alignment.clone()),
            arousal: self.arousal_enabled.then(|| self.arousal.clone()),
            laughter: self.laughter_enabled.then(|| self.laughter.clone()),
            generation: self.generation.clone(),
        }
    }
    fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.generation.python.is_file()
            || !self.generation.worker.is_file()
            || !self.generation.assembler.is_file()
            || !self.generation.derivation.is_file()
        {
            missing.push("Preparación editorial");
        }
        let alignment = &self.alignment.runtime;
        if self.alignment_enabled
            && (!alignment.python.is_file()
                || !alignment.worker.is_file()
                || !alignment.model.is_file()
                || !alignment.model_manifest.is_file()
                || !alignment.lock.is_file())
        {
            missing.push("Alineación MMS");
        }
        let arousal = &self.arousal.runtime;
        if self.arousal_enabled
            && (!arousal.python.is_file()
                || !arousal.worker.is_file()
                || !arousal.model.join("model.safetensors").is_file()
                || !arousal.model_manifest.is_file()
                || !arousal.lock.is_file())
        {
            missing.push("Arousal");
        }
        let laughter = &self.laughter.runtime;
        if self.laughter_enabled
            && (!laughter.python.is_file()
                || !laughter.worker.is_file()
                || !laughter.model.is_file()
                || !laughter.model_manifest.is_file()
                || !laughter.config.is_file()
                || !laughter.lock.is_file())
        {
            missing.push("Risas");
        }
        missing
    }
}
struct Running {
    cancel: Arc<AtomicBool>,
    result: crossbeam_channel::Receiver<DomainResult<Completion>>,
    progress: crossbeam_channel::Receiver<serde_json::Value>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
pub struct AnalysisUi {
    pub open: bool,
    pub runtime: Runtime,
    pub parameters: Parameters,
    pub asset: Option<AssetId>,
    workflow: WorkflowOptions,
    running: Option<Running>,
    scan: Option<crossbeam_channel::Receiver<DomainResult<Vec<SavedAnalysis>>>>,
    records: Vec<SavedAnalysis>,
    ready: Option<Completion>,
    progress: Option<serde_json::Value>,
    pub error: Option<String>,
    pub last_applied: Option<String>,
}
impl Default for AnalysisUi {
    fn default() -> Self {
        let root = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.ancestors().skip(1).take(5).find(|dir| dir.join("workers/python/worker.py").is_file()).map(PathBuf::from))
            .unwrap_or_default();
        Self {
            open: false,
            runtime: Runtime {
                python: std::env::var_os("TRANSCRIPTOR_PYTHON").map(PathBuf::from).unwrap_or_else(|| root.join(".local/e5-venv/Scripts/python.exe")),
                worker: std::env::var_os("TRANSCRIPTOR_WORKER").map(PathBuf::from).unwrap_or_else(|| root.join("workers/python/worker.py")),
                model: std::env::var_os("TRANSCRIPTOR_WHISPER_MODEL").map(PathBuf::from).unwrap_or_else(|| root.join(".local/models/whisper-tiny")),
                work_root: crate::paths::config_dir().join("analysis"),
            },
            parameters: Default::default(),
            workflow: WorkflowOptions::new(&root, &crate::paths::config_dir().join("analysis")),
            asset: None,
            running: None,
            scan: None,
            records: Vec::new(),
            ready: None,
            progress: None,
            error: None,
            last_applied: None,
        }
    }
}
impl AnalysisUi {
    pub fn busy(&self) -> bool {
        self.running.is_some() || self.scan.is_some()
    }
    pub fn cancel(&self) {
        if let Some(running) = &self.running {
            running.cancel.store(true, Ordering::Release);
        }
    }
}
impl TranscriptorApp {
    pub fn start_analysis(&mut self, asset_id: AssetId, resume: Option<SavedAnalysis>, ctx: &egui::Context) {
        if self.analysis.busy() {
            self.toast(Severity::Warn, "Espera o cancela el análisis activo");
            return;
        }
        let Some(asset) = self.project().asset(&asset_id).cloned() else {
            self.report(DomainError::not_found("Medio", asset_id));
            return;
        };
        let tools = match &self.tools {
            Ok(tools) => tools.clone(),
            Err(error) => {
                self.toast(Severity::Error, error.clone());
                return;
            }
        };
        let source = self.resolve_asset_path(&asset);
        let snapshot = self.project().clone();
        if resume.as_ref().is_some_and(|record| record.project_id() != snapshot.project_id) {
            self.report(DomainError::precondition("Abre el proyecto al que pertenece este análisis"));
            return;
        }
        let runtime = self.analysis.runtime.clone();
        let parameters = self.analysis.parameters.clone();
        let plan = self.analysis.workflow.plan();
        let workflow_root = self.analysis.workflow.work_root.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        let (progress_tx, progress) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("local-analysis".into()).spawn(move || {
            let value = (|| {
                let record = match resume {
                    Some(record) => record,
                    None => {
                        // A workflow may fail/cancel while capturing resources after
                        // ASR enqueue. Keep these queued parents outside the legacy
                        // nonrecursive discovery root so they cannot be incorporated
                        // as a complete analysis without their selected stages.
                        let mut workflow_asr_runtime = runtime;
                        workflow_asr_runtime.work_root = workflow_asr_runtime.work_root.join("workflow-asr");
                        let asr = tv2_pipeline::enqueue(
                            &tv2_application::ProjectSession::new(snapshot.clone()),
                            asset,
                            source,
                            workflow_asr_runtime,
                            parameters,
                            &tools,
                            &token,
                        )?;
                        SavedAnalysis::Workflow(Box::new(workflow::enqueue(asr, plan, workflow_root, &token)?))
                    }
                };
                let progress = |event| {
                    let _ = progress_tx.try_send(event);
                    wake.request_repaint();
                };
                let completed = match record {
                    SavedAnalysis::Asr(record) => {
                        tv2_pipeline::run(&record, record.state != JobState::Queued, token.clone(), progress)?;
                        SavedAnalysis::Asr(Box::new(
                            jobs::discover::<AnalysisPayload>(&record.payload.runtime.work_root.join("jobs"))?
                                .into_iter()
                                .find(|saved| saved.id == record.id)
                                .ok_or_else(|| DomainError::invalid("Falta recibo de transcripción final"))?,
                        ))
                    }
                    SavedAnalysis::Workflow(record) => {
                        workflow::run(&record, record.state != JobState::Queued, token.clone(), progress)?;
                        SavedAnalysis::Workflow(Box::new(
                            jobs::discover::<WorkflowPayload>(&record.payload.work_root.join("jobs"))?
                                .into_iter()
                                .find(|saved| saved.id == record.id)
                                .ok_or_else(|| DomainError::invalid("Falta recibo de análisis final"))?,
                        ))
                    }
                };
                let command = completed.prepare(snapshot, &token, false);
                Ok((completed, command))
            })();
            let _ = tx.send(value);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => {
                self.analysis.running = Some(Running { cancel, result, progress });
                self.analysis.ready = None;
                self.analysis.error = None;
                self.analysis.progress = None;
                self.analysis.last_applied = None;
            }
            Err(error) => self.report(error.into()),
        }
    }
    pub fn scan_analysis(&mut self, ctx: &egui::Context) {
        if self.analysis.scan.is_some() {
            return;
        }
        let root = self.analysis.runtime.work_root.join("jobs");
        let workflow_root = self.analysis.workflow.work_root.join("jobs");
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        match std::thread::Builder::new().name("analysis-recovery".into()).spawn(move || {
            let value = (|| {
                let workflows = jobs::discover::<WorkflowPayload>(&workflow_root)?;
                let owned_asr = workflows.iter().map(|record| record.payload.asr.id.as_str()).collect::<std::collections::BTreeSet<_>>();
                let mut records = jobs::discover::<AnalysisPayload>(&root)?
                    .into_iter()
                    .filter(|record| !owned_asr.contains(record.id.as_str()))
                    .map(|record| SavedAnalysis::Asr(Box::new(record)))
                    .collect::<Vec<_>>();
                records.extend(workflows.into_iter().map(|record| SavedAnalysis::Workflow(Box::new(record))));
                Ok(records)
            })();
            let _ = tx.send(value);
            wake.request_repaint();
        }) {
            Ok(_) => self.analysis.scan = Some(rx),
            Err(error) => self.report(error.into()),
        }
    }
    pub fn review_saved_analysis(&mut self, record: SavedAnalysis, ctx: &egui::Context) {
        if self.analysis.busy() {
            return;
        }
        self.analysis.progress = None;
        let snapshot = self.project().clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        let (_, progress) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        match std::thread::Builder::new().name("analysis-review".into()).spawn(move || {
            let value = {
                let prepared = record.prepare(snapshot, &token, true);
                Ok((record, prepared))
            };
            let _ = tx.send(value);
            wake.request_repaint();
        }) {
            Ok(_) => {
                self.analysis.running = Some(Running { cancel, result, progress });
                self.analysis.ready = None;
                self.analysis.error = None;
            }
            Err(error) => self.report(error.into()),
        }
    }
    pub fn poll_analysis(&mut self, ctx: &egui::Context) {
        if let Some(scan) = &self.analysis.scan {
            match scan.try_recv() {
                Ok(result) => {
                    self.analysis.scan = None;
                    match result {
                        Ok(records) => self.analysis.records = records,
                        Err(error) => self.analysis.error = Some(error.to_string()),
                    }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.analysis.scan = None;
                    self.analysis.error = Some("Lector de jobs terminó sin resultado".into());
                }
                Err(_) => {}
            }
        }
        if let Some(running) = &self.analysis.running {
            for progress in running.progress.try_iter() {
                self.analysis.progress = Some(progress);
            }
            let result = match running.result.try_recv() {
                Ok(value) => Some(value),
                Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(DomainError::process("Worker de análisis terminó sin resultado"))),
                Err(_) => None,
            };
            if let Some(result) = result {
                self.analysis.running = None;
                self.analysis.progress = None;
                match result {
                    Ok(ready) => {
                        if let Err(error) = &ready.1 {
                            self.analysis.error = Some(error.to_string());
                        }
                        self.analysis.ready = Some(ready);
                    }
                    Err(error) => {
                        self.analysis.error = Some(error.to_string());
                        self.report(error);
                    }
                }
                self.scan_analysis(ctx);
            }
        }
    }
    pub fn apply_analysis(&mut self) -> DomainResult<()> {
        let ready = self.analysis.ready.as_ref().ok_or_else(|| DomainError::precondition("No hay análisis preparado"))?;
        let prepared = ready.1.as_ref().map_err(Clone::clone)?;
        if !prepared.matches_base(self.project()) {
            return Err(DomainError::precondition("Cambió el proyecto; revisa el resultado en la revisión actual"));
        }
        let (record, prepared) = self.analysis.ready.take().unwrap();
        // Commit rechecks the exact snapshot, including local human decisions.
        if let Err(error) = self.session.commit_prepared(prepared?) {
            self.analysis.ready = Some((record, Err(error.clone())));
            return Err(error);
        }
        self.analysis.last_applied = Some(record.id().to_owned());
        self.after_change();
        Ok(())
    }
}

pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    if !app.analysis.open {
        return;
    }
    let mut open = true;
    let mut start = false;
    let mut scan = false;
    let mut apply = false;
    let mut resume = None;
    let mut review = None;
    egui::Window::new("Análisis local de audio").open(&mut open).default_width(650.0).default_height(600.0).vscroll(true).show(ctx, |ui| {
        ui.label("Transcripción y análisis de audio. Los resultados quedan como evidencia revisable y no cambian el proyecto hasta incorporarlos.");
        let selected = app
            .analysis
            .asset
            .as_ref()
            .or(app.source_asset.as_ref())
            .and_then(|id| app.project().asset(id))
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Selecciona un medio".into());
        egui::ComboBox::from_id_salt("analysis_asset").selected_text(selected).show_ui(ui, |ui| {
            for asset in &app.session.project().assets {
                if !asset.probe.audio.is_empty() {
                    ui.selectable_value(&mut app.analysis.asset, Some(asset.id.clone()), &asset.name);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Idioma");
            ui.text_edit_singleline(&mut app.analysis.parameters.language);
            ui.label("Hilos CPU");
            ui.add(egui::DragValue::new(&mut app.analysis.parameters.threads).range(1..=4));
        });
        ui.label("Transcripción Whisper local en CPU. Se verifican el entorno y el modelo antes de procesar.");
        ui.add_enabled_ui(!app.analysis.busy(), |ui| {
            ui.add_enabled(!app.analysis.workflow.arousal_enabled, egui::Checkbox::new(&mut app.analysis.workflow.alignment_enabled, "Alinear las palabras con el audio (MMS)"));
            ui.checkbox(&mut app.analysis.workflow.arousal_enabled, "Medir activación acústica de la voz (arousal, incluye alineación)");
            if app.analysis.workflow.arousal_enabled { app.analysis.workflow.alignment_enabled = true; }
            ui.checkbox(&mut app.analysis.workflow.laughter_enabled, "Detectar risas");
        });
        ui.label("La intensidad y las pausas se calculan con los tiempos finales. Las etapas seleccionadas se ejecutan antes de preparar la incorporación.");
        ui.collapsing("Instalación de análisis", |ui| {
            for (label, value, folder) in [
                ("Python aislado", &mut app.analysis.runtime.python, false),
                ("Worker", &mut app.analysis.runtime.worker, false),
                ("Modelo Whisper local", &mut app.analysis.runtime.model, true),
            ] {
                choose_path(ui, label, value, folder);
            }
            ui.collapsing("Alineación MMS", |ui| {
                let runtime = &mut app.analysis.workflow.alignment.runtime;
                for (label,path) in [("Python MMS",&mut runtime.python),("Worker MMS",&mut runtime.worker),("Modelo MMS",&mut runtime.model),
                    ("Manifest MMS",&mut runtime.model_manifest),("Versiones MMS",&mut runtime.lock)] { choose_path(ui,label,path,false); }
            });
            ui.collapsing("Activación acústica", |ui| {
                let runtime = &mut app.analysis.workflow.arousal.runtime;
                choose_path(ui,"Modelo arousal",&mut runtime.model,true);
                for (label,path) in [("Python arousal",&mut runtime.python),("Worker arousal",&mut runtime.worker),
                    ("Manifest arousal",&mut runtime.model_manifest),("Versiones arousal",&mut runtime.lock)] { choose_path(ui,label,path,false); }
            });
            ui.collapsing("Risas", |ui| {
                let runtime = &mut app.analysis.workflow.laughter.runtime;
                for (label,path) in [("Python risas",&mut runtime.python),("Worker risas",&mut runtime.worker),("Modelo risas",&mut runtime.model),
                    ("Configuración del modelo",&mut runtime.config),("Manifest risas",&mut runtime.model_manifest),("Versiones risas",&mut runtime.lock)] { choose_path(ui,label,path,false); }
            });
            ui.collapsing("Preparación editorial", |ui| {
                let runtime = &mut app.analysis.workflow.generation;
                for (label,path) in [("Python de preparación",&mut runtime.python),("Worker de preparación",&mut runtime.worker),
                    ("Ensamblador",&mut runtime.assembler),("Derivación",&mut runtime.derivation)] { choose_path(ui,label,path,false); }
            });
        });
        let missing = app.analysis.workflow.missing();
        if !missing.is_empty() { ui.label(format!("Faltan archivos de: {}. Revisa Instalación de análisis.",missing.join(", "))); }
        let enabled = !app.analysis.busy()
            && app.analysis.asset.as_ref().or(app.source_asset.as_ref())
                .and_then(|id| app.project().asset(id)).is_some_and(|asset| !asset.probe.audio.is_empty())
            && missing.is_empty()
            && app.analysis.runtime.python.is_file()
            && app.analysis.runtime.worker.is_file()
            && app.analysis.runtime.model.join("model.bin").is_file();
        ui.horizontal_wrapped(|ui| {
            start = ui.add_enabled(enabled, egui::Button::new("Iniciar análisis")).clicked();
            scan = ui.add_enabled(!app.analysis.busy(), egui::Button::new("Recuperar trabajos guardados")).clicked();
            if ui.add_enabled(app.analysis.running.is_some(), egui::Button::new("Cancelar")).clicked() {
                app.analysis.cancel();
            }
        });
        if let Some(event) = &app.analysis.progress {
            let fraction = event.get("fraction").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let step = event.get("step").or_else(|| event.get("stage")).and_then(|v| v.as_str()).unwrap_or("");
            let label = match step.split('/').next().unwrap_or("") {
                "asr" | "transcribe" => "Transcribiendo audio",
                "extract" => "Preparando audio",
                "alignment" => "Alineando palabras con el audio",
                "arousal" => "Midiendo activación acústica",
                "laughter" => "Detectando risas",
                "generation" | "finalize" => "Preparando capas y comprobando resultados",
                _ => "Analizando",
            };
            ui.add(egui::ProgressBar::new(fraction).text(label));
        }
        if let Some(error) = &app.analysis.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some((record, command)) = &app.analysis.ready {
            ui.label(format!("Resultado conservado: {}", record.id()));
            if let Ok(prepared) = command {
                let diff = prepared.diff();
                ui.label(format!("{} elementos nuevos, {} modificados y {} eliminados en {} capas.",diff.items_added,diff.items_changed,diff.items_removed,diff.layers_changed));
                ui.collapsing("Vista previa de las capas del medio", |ui| {
                    ui.label("Muestra de hasta 20 capas y 12 elementos por capa.");
                    egui::ScrollArea::vertical().id_salt("analysis_preview").max_height(200.0).show(ui, |ui| {
                        for layer in prepared.preview().layers.iter().filter(|layer| &layer.asset_id == record.asset_id()).take(20) {
                            ui.push_id(layer.layer_id.as_str(),|ui| {
                                ui.collapsing(format!("{} · {} elementos",layer.name,layer.items.len()),|ui| {
                                    for item in layer.items.iter().take(12) {
                                        let label = item.label.chars().take(240).collect::<String>();
                                        ui.label(format!("{:.3}–{:.3} s · {}{}",item.start().as_seconds_f64(),item.end().as_seconds_f64(),label,
                                            if label.len() < item.label.len() { "…" } else { "" }));
                                    }
                                    if layer.items.len() > 12 { ui.label("Se muestran los primeros 12 elementos de esta capa."); }
                                });
                            });
                        }
                    });
                });
                let current = prepared.matches_base(app.project());
                if !current { ui.label("El proyecto cambió. Revisa este resultado en la revisión actual antes de incorporarlo."); }
                apply = ui.add_enabled(current,egui::Button::new("Incorporar análisis")).clicked();
            }
        }
        if app.analysis.last_applied.is_some() {
            ui.label("Análisis incorporado. Puedes revisarlo, deshacerlo o guardar el proyecto.");
        }
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for record in &app.analysis.records {
                if record.project_id() != app.project().project_id { continue; }
                ui.horizontal_wrapped(|ui| {
                    let label = if matches!(record, SavedAnalysis::Workflow(_)) { "Análisis por etapas" } else { "Transcripción anterior" };
                    ui.label(format!("{} · {} · {:?} · revisión {}", label, record.id(), record.state(), record.revision()));
                    if record.state() == JobState::Succeeded
                        && ui.add_enabled(!app.analysis.busy(), egui::Button::new("Revisar en revisión actual")).clicked()
                    {
                        review = Some(record.clone());
                    }
                    if matches!(record.state(), JobState::Queued | JobState::Interrupted | JobState::Failed | JobState::Cancelled)
                        && ui.add_enabled(!app.analysis.busy(), egui::Button::new("Reanudar solicitud")).clicked()
                    {
                        resume = Some(record.clone());
                    }
                });
                if let Some(error) = record.error() { ui.colored_label(egui::Color32::LIGHT_RED,error); }
            }
        });
    });
    app.analysis.open = open;
    if scan {
        app.scan_analysis(ctx);
    }
    if apply && let Err(error) = app.apply_analysis() {
        app.analysis.error = Some(error.to_string());
        app.report(error);
    }
    if start && let Some(asset) = app.analysis.asset.clone().or_else(|| app.source_asset.clone()) {
        app.start_analysis(asset, None, ctx);
    }
    if let Some(record) = resume {
        app.start_analysis(record.asset_id().clone(), Some(record), ctx);
    }
    if let Some(record) = review {
        app.review_saved_analysis(record, ctx);
    }
}

fn choose_path(ui: &mut egui::Ui, label: &str, value: &mut PathBuf, folder: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        ui.label(value.to_string_lossy());
        if ui.button("Elegir…").clicked()
            && let Some(path) = if folder { rfd::FileDialog::new().pick_folder() } else { rfd::FileDialog::new().pick_file() }
        {
            *value = path;
        }
    });
}
