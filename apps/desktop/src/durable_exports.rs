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
}
