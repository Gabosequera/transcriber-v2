//! CPU-heavy editorial commands validate on one worker and commit the exact preview.
use crate::app::{Severity, TranscriptorApp};
use tv2_application::{CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{Command, DomainError, error::DomainResult};
pub struct SemanticJob {
    pub result: crossbeam_channel::Receiver<DomainResult<PreparedCommand>>,
}
impl TranscriptorApp {
    pub fn export_interchange_dialog(&mut self, xml: bool) {
        if self.v1_export_job.is_some() {
            self.toast(Severity::Warn, "Hay una exportación documental en curso");
            return;
        }
        let Some(sequence) = self.sequence().map(|s| s.id.clone()) else { return };
        let Some(directory) = rfd::FileDialog::new().set_title("Destino de intercambio de montaje").pick_folder() else { return };
        let mut project = self.project().clone();
        for asset in &mut project.assets {
            asset.path = self.resolve_asset_path(asset).to_string_lossy().replace('\\', "/");
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("interchange-export".into()).spawn(move || {
            let work = || -> DomainResult<std::path::PathBuf> {
                let text = if xml {
                    tv2_v1compat::interchange::export_fcpxml(&project, &sequence)?
                } else {
                    tv2_v1compat::interchange::export_edl(&project, &sequence)?
                };
                let ext = if xml { "fcpxml" } else { "edl" };
                let docs = std::collections::BTreeMap::from([(format!("montage.{ext}"), text)]);
                let root = directory.join(format!("{ext}-r{}-{}", project.revision, tv2_domain::ids::random_hex12()));
                tv2_application::documents::create(&root)?;
                tv2_application::documents::publish(&root, &docs)?;
                Ok(root)
            };
            let _ = tx.send(work());
        }) {
            Ok(_) => self.v1_export_job = Some(rx),
            Err(e) => self.report(e.into()),
        }
    }
    pub fn export_v1_folder_dialog(&mut self, include_montage: bool) {
        if self.v1_export_job.is_some() {
            self.toast(Severity::Warn, "Hay una exportación en curso");
            return;
        }
        let Some(asset) = self.current_layer_asset() else {
            self.toast(Severity::Warn, "Selecciona el medio cuya carpeta V1 quieres exportar");
            return;
        };
        let Some(directory) = rfd::FileDialog::new().set_title("Destino para una nueva carpeta documental V1").pick_folder() else {
            return;
        };
        let project = self.project().clone();
        let sequence = if include_montage { self.sequence().map(|s| s.id.clone()) } else { None };
        if include_montage && sequence.is_none() {
            self.toast(Severity::Warn, "Selecciona un montaje");
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("v1-folder-export".into()).spawn(move || {
            let work = || -> DomainResult<std::path::PathBuf> {
                let docs = tv2_v1compat::folder::export(&project, &asset, sequence.as_ref())?;
                let root = directory.join(format!("v1-folder-r{}-{}", project.revision, tv2_domain::ids::random_hex12()));
                tv2_application::documents::create(&root)?;
                tv2_application::documents::publish_with_directories(&root, &docs.documents, &docs.files, &docs.directories)?;
                Ok(root)
            };
            let _ = tx.send(work());
        }) {
            Ok(_) => self.v1_export_job = Some(rx),
            Err(e) => self.report(e.into()),
        }
    }
    pub fn import_author_dialog(&mut self) {
        self.scan_author(true);
    }

    pub fn recover_export_dialog(&mut self) {
        if self.v1_export_job.is_some() {
            self.toast(Severity::Warn, "Hay una exportación en curso");
            return;
        }
        let Some(root) = rfd::FileDialog::new().set_title("Recuperar carpeta de exportación creada por V2").pick_folder() else {
            return;
        };
        let (tx, rx) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("export-recovery".into()).spawn(move || {
            let result = tv2_application::documents::recover(&root).map(|_| root);
            let _ = tx.send(result);
        }) {
            Ok(_) => self.v1_export_job = Some(rx),
            Err(e) => self.report(e.into()),
        }
    }
    pub fn prepare_semantic(&mut self, command: Command) {
        if self.semantic_job.is_some() {
            self.toast(Severity::Warn, "Hay una edición en preparación");
            return;
        }
        let project = self.project().clone();
        let request = CommandEnvelope::human(command).with_base(project.revision);
        let (tx, result) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("semantic-prepare".into()).spawn(move || {
            let _ = tx.send(ProjectSession::new(project).prepare_command(request));
        }) {
            Ok(_) => {
                self.semantic_job = Some(SemanticJob { result });
                self.toast(Severity::Info, "Calculando edición editorial…");
            }
            Err(e) => self.report(DomainError::io(e.to_string())),
        }
    }
    pub fn poll_semantic(&mut self) {
        let Some(job) = &self.semantic_job else {
            return;
        };
        let result = match job.result.try_recv() {
            Ok(r) => r,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(_) => Err(DomainError::process("worker editorial terminó sin resultado")),
        };
        self.semantic_job = None;
        match result.and_then(|p| self.session.commit_prepared(p)) {
            Ok(_) => self.after_change(),
            Err(e) => self.report(e),
        }
    }
}
