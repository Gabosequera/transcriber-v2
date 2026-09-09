//! CPU-heavy editorial commands validate on one worker and commit the exact preview.
use crate::app::{Severity, TranscriptorApp};
use tv2_application::{CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{Command, DomainError, error::DomainResult};
pub struct SemanticJob {
    pub result: crossbeam_channel::Receiver<DomainResult<PreparedCommand>>,
}
impl TranscriptorApp {
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
