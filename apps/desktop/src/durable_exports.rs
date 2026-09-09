use crate::app::{Severity, TranscriptorApp};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use tv2_application::jobs::{self, JobRecord, JobState};
use tv2_domain::{AssetId, DomainError, Fingerprint, error::DomainResult};
use tv2_media::export::{ExportRequest, ExportResult};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportPayload {
    pub request: ExportRequest,
    pub fingerprints: HashMap<AssetId, Fingerprint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub editorial_sources: Vec<EditorialSource>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct EditorialSource {
    pub asset: tv2_domain::Asset,
    pub master: tv2_domain::evidence::EvidenceDocument,
    pub layers: Vec<tv2_domain::SemanticLayer>,
}
impl EditorialSource {
    pub fn capture(project: &tv2_domain::Project) -> Vec<Self> {
        project
            .masters
            .iter()
            .filter_map(|master| {
                let asset = project.asset(&master.asset_id)?.clone();
                let layers = project
                    .layers
                    .iter()
                    .filter(|l| {
                        l.asset_id == asset.id
                            && !l.extra.contains_key("master_projection")
                            && matches!(
                                l.kind,
                                tv2_domain::LayerKind::User
                                    | tv2_domain::LayerKind::Topics
                                    | tv2_domain::LayerKind::Ai
                                    | tv2_domain::LayerKind::Other(_)
                            )
                    })
                    .cloned()
                    .collect();
                Some(Self { asset, master: master.document.clone(), layers })
            })
            .collect()
    }
}
pub struct QueuedExport {
    pub payload: ExportPayload,
    pub root: PathBuf,
    pub record: Option<JobRecord<ExportPayload>>,
    saving: Option<crossbeam_channel::Receiver<DomainResult<JobRecord<ExportPayload>>>>,
    pub recovering: bool,
}
impl std::ops::Deref for QueuedExport {
    type Target = ExportRequest;
    fn deref(&self) -> &ExportRequest {
        &self.payload.request
    }
}
impl QueuedExport {
    pub fn new(root: PathBuf, project: String, payload: ExportPayload) -> std::io::Result<Self> {
        let copy = payload.clone();
        let dir = root.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::Builder::new().name("export-enqueue".into()).spawn(move || {
            let _ = tx.send(jobs::enqueue(&dir, project, copy.request.project_revision, copy));
        })?;
        Ok(Self { root, payload, record: None, saving: Some(rx), recovering: false })
    }
    pub fn ready(&mut self) -> Option<DomainResult<()>> {
        let Some(rx) = &self.saving else {
            return Some(Ok(()));
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(crossbeam_channel::TryRecvError::Empty) => return None,
            Err(_) => Err(DomainError::process("No se confirmó la persistencia del job")),
        };
        self.saving = None;
        Some(result.map(|record| self.record = Some(record)))
    }
    pub fn run(
        self,
        tools: &tv2_media::FfmpegTools,
        cancel: &Arc<AtomicBool>,
        progress: impl FnMut(tv2_media::export::ExportProgress),
    ) -> DomainResult<ExportResult> {
        let record = self.record.as_ref().ok_or_else(|| DomainError::precondition("Job sin recibo durable"))?;
        let lease = jobs::claim::<ExportPayload>(&self.root, &record.id, &record.payload_digest, self.recovering)?;
        let result = (|| {
            // On recovery the locator may point to another file. Revalidate full
            // V1 identity before an encoder can consume it.
            if self.recovering {
                for (id, fp) in &self.payload.fingerprints {
                    let source = self.assets.get(id).ok_or_else(|| DomainError::invalid("asset del job ausente"))?;
                    let actual = tools.import_cancellable(&source.path, source.path.to_string_lossy().into(), cancel.clone())?;
                    if !actual.fingerprint.same_identity(fp) {
                        return Err(DomainError::precondition(format!("Cambió el medio del job: {}", source.path.display())));
                    }
                }
            }
            tv2_media::export::ExportJob::run(tools, &self.payload.request, cancel, progress)
        })();
        let state = match &result {
            Ok(_) => JobState::Succeeded,
            Err(e) if e.code == tv2_domain::ErrorCode::Cancelled => JobState::Cancelled,
            Err(_) => JobState::Failed,
        };
        lease
            .finish(state, result.as_ref().ok().map(serde_json::to_value).transpose()?, result.as_ref().err().map(ToString::to_string))
            .map_err(|e| DomainError::io(format!("El worker terminó pero no pudo archivar el recibo: {e}. Revisa el destino antes de reanudar.")))?;
        result
    }
    pub fn cancel(mut self) -> std::io::Result<()> {
        std::thread::Builder::new()
            .name("export-cancel-persist".into())
            .spawn(move || {
                if let Some(rx) = self.saving.take() {
                    match rx.recv() {
                        Ok(Ok(r)) => self.record = Some(r),
                        _ => return,
                    }
                }
                if let Some(r) = self.record
                    && let Err(e) = jobs::cancel::<ExportPayload>(&self.root, &r.id, &r.payload_digest)
                {
                    tracing::error!("No se guardó cancelación {}: {}", r.id, e);
                }
            })
            .map(|_| ())
    }
}
pub type RecoveryScan = crossbeam_channel::Receiver<DomainResult<Vec<JobRecord<ExportPayload>>>>;
#[cfg(not(test))]
pub fn root() -> PathBuf {
    crate::paths::config_dir().join("export-jobs")
}
#[cfg(test)]
pub fn root() -> PathBuf {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| tempfile::tempdir().expect("isolated export jobs")).path().into()
}
impl TranscriptorApp {
    pub fn poll_export_recovery(&mut self) {
        if !self.export_jobs_scanned {
            self.export_jobs_scanned = true;
            let (tx, rx) = crossbeam_channel::bounded(1);
            match std::thread::Builder::new().name("export-jobs-recovery".into()).spawn(move || {
                let _ = tx.send(jobs::discover::<ExportPayload>(&root()));
            }) {
                Ok(_) => self.export_jobs_scan = Some(rx),
                Err(e) => self.report(e.into()),
            }
        }
        if let Some(rx) = &self.export_jobs_scan {
            let result = match rx.try_recv() {
                Ok(r) => r,
                Err(crossbeam_channel::TryRecvError::Empty) => return,
                Err(_) => Err(DomainError::process("Lectura de jobs interrumpida")),
            };
            self.export_jobs_scan = None;
            match result {
                Ok(records) => self.export_jobs_recovery = records,
                Err(e) => self.report(e),
            }
        }
    }
    pub fn recover_export_job(&mut self, index: usize) {
        let record = self.export_jobs_recovery[index].clone();
        if self.export.pending.len() >= 16
            || self.export.pending.iter().any(|q| q.destination == record.payload.request.destination)
            || self.export.running.as_ref().is_some_and(|r| r.destination == record.payload.request.destination)
        {
            self.toast(Severity::Warn, "Destino ya en cola o cola llena");
            return;
        }
        if matches!(record.state, JobState::Running | JobState::Succeeded) {
            self.toast(Severity::Warn, "El job está activo o ya terminó");
            return;
        }
        self.export.pending.push_back(QueuedExport {
            root: root(),
            payload: record.payload.clone(),
            record: Some(record),
            saving: None,
            recovering: true,
        });
        self.export_jobs_recovery.remove(index);
        self.toast(Severity::Info, "Job recuperado; se verificará la identidad de sus medios antes de exportar");
    }
}
pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    if !app.export_jobs_open {
        return;
    }
    let mut open = true;
    let mut resume = None;
    let mut derive = None;
    egui::Window::new("Trabajos de exportación guardados").open(&mut open).show(ctx, |ui| {
        ui.label("Se conservan la revisión, los medios y la solicitud original. Un destino existente requiere inspección y nunca se sobrescribe.");
        egui::ScrollArea::vertical().max_height(450.0).show(ui, |ui| {
            for (i, job) in app.export_jobs_recovery.iter().enumerate() {
                ui.group(|ui| {
                    ui.label(format!("{} · {:?} · proyecto {} rev {}", job.id, job.state, job.project_id, job.revision));
                    ui.label(job.payload.request.destination.display().to_string());
                    if let Some(e) = &job.error {
                        ui.label(e);
                    }
                    if !matches!(job.state, JobState::Running | JobState::Succeeded) && ui.button("Reanudar esta solicitud").clicked() {
                        resume = Some(i);
                    }
                    if job.state == JobState::Succeeded && ui.button("Derivar carpeta hija desde esta exportación…").clicked() {
                        derive = Some(i);
                    }
                    if let Some(result) = &job.result {
                        ui.collapsing("Recibo", |ui| {
                            ui.monospace(result.to_string());
                        });
                    }
                    ui.collapsing(format!("{} intentos", job.attempts.len()), |ui| {
                        for attempt in &job.attempts {
                            ui.label(format!("{} · {:?} · {}", attempt.started_at, attempt.state, attempt.error.as_deref().unwrap_or("")));
                        }
                    });
                });
            }
        });
        if ui.button("Actualizar estados").clicked() {
            app.export_jobs_scanned = false;
        }
    });
    app.export_jobs_open = open;
    if let Some(i) = resume {
        app.recover_export_job(i);
    }
    if let Some(i) = derive {
        app.derive_export_job(i);
    }
}

impl TranscriptorApp {
    pub fn derive_export_job(&mut self, index: usize) {
        if self.v1_export_job.is_some() {
            self.toast(Severity::Warn, "Hay una publicación documental en curso");
            return;
        }
        let Some(job) = self.export_jobs_recovery.get(index).cloned() else { return };
        if job.state != JobState::Succeeded {
            return;
        }
        let Some(parent_id) = self.current_layer_asset() else {
            self.toast(Severity::Warn, "Selecciona el medio padre en la biblioteca o capa");
            return;
        };
        let source = job.payload.editorial_sources.iter().find(|s| s.asset.id == parent_id).cloned().or_else(|| {
            (job.project_id == self.project().project_id && job.revision == self.session.revision())
                .then(|| EditorialSource::capture(self.project()).into_iter().find(|s| s.asset.id == parent_id))
                .flatten()
        });
        let Some(source) = source else {
            self.toast(Severity::Warn, "Este job no conserva evidencia editorial de ese padre; usa una exportación nueva o su revisión original");
            return;
        };
        let Some(directory) = rfd::FileDialog::new().set_title("Destino de la carpeta hija (incluye copia del medio exportado)").pick_folder() else {
            return;
        };
        let Ok(tools) = self.tools.clone() else {
            self.toast(Severity::Error, "FFprobe no está disponible");
            return;
        };
        let (tx, rx) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("derive-export-child".into()).spawn(move || {
            let work = || -> DomainResult<PathBuf> {
                let receipt: ExportResult = serde_json::from_value(job.result.ok_or_else(|| DomainError::precondition("Export sin recibo"))?)?;
                let req = &job.payload.request;
                if receipt.path != req.destination || receipt.project_revision != job.revision {
                    return Err(DomainError::precondition("Recibo y solicitud no coinciden"));
                }
                let mapping = tv2_v1compat::projects::mapping_from_timeline(&req.timeline, &source.asset, req.range, req.preset.has_video)?;
                let extension = req.destination.extension().and_then(|x| x.to_str()).ok_or_else(|| DomainError::invalid("Salida sin extensión"))?;
                if !extension.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return Err(DomainError::invalid("Extensión no portable"));
                }
                let media_path = format!("media/export.{extension}");
                let child = tools.import(&req.destination, media_path.clone())?;
                let parent = tv2_v1compat::master::V1Master::parse((*source.master).clone())?;
                let layers = source.layers.iter().map(|l| tv2_v1compat::layers::layer_to_v1(l, &source.asset.fingerprint)).collect::<Vec<_>>();
                let audio = match receipt.audio_streams {
                    0 => tv2_v1compat::projects::AudioDisposition::Silent,
                    1 => tv2_v1compat::projects::AudioDisposition::Mixed { output_audio_index: 0 },
                    _ => return Err(DomainError::precondition("El exportador no tiene contrato de preservación multipista para este job")),
                };
                let mut docs = tv2_v1compat::projects::derive_package(&parent, &child, &mapping, &audio, &layers)?;
                docs.insert("archive/export-receipt.json".into(), serde_json::to_string_pretty(&receipt)?);
                let files = std::collections::BTreeMap::from([(
                    media_path,
                    tv2_domain::evidence::SourceFile {
                        source: req.destination.clone(),
                        size: std::fs::metadata(&req.destination)?.len(),
                        sha256: receipt.sha256,
                    },
                )]);
                let root = directory.join(format!("child-{}", tv2_domain::ids::random_hex12()));
                tv2_application::documents::create(&root)?;
                tv2_application::documents::publish_with_files(&root, &docs, &files)?;
                Ok(root)
            };
            let _ = tx.send(work());
        }) {
            Ok(_) => self.v1_export_job = Some(rx),
            Err(e) => self.report(e.into()),
        }
    }
}
