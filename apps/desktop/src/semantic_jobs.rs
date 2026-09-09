//! CPU-heavy editorial commands validate on one worker and commit the exact preview.
use crate::app::{Severity, TranscriptorApp};
use tv2_application::{CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{Command, DomainError, error::DomainResult};
pub struct SemanticJob {
    result: crossbeam_channel::Receiver<DomainResult<PreparedCommand>>,
}
impl TranscriptorApp {
    pub fn import_author_dialog(&mut self) {
        if self.semantic_job.is_some() {
            self.toast(Severity::Warn, "Hay una edición en preparación");
            return;
        }
        let Some(asset) = self.current_layer_asset().and_then(|id| self.project().asset(&id)).cloned() else {
            self.toast(Severity::Warn, "Selecciona el medio de las marcas");
            return;
        };
        let Some(path) = rfd::FileDialog::new().set_title("Sidecar autoritativo de marcas V1").add_filter("Marcas", &["json"]).pick_file() else {
            return;
        };
        let project = self.project().clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        let launch = std::thread::Builder::new().name("author-import".into()).spawn(move || {
            let work = || -> DomainResult<PreparedCommand> {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(&path)?.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err(DomainError::invalid("sidecar de marcas supera 16 MiB"));
                }
                let raw = serde_json::from_slice(&bytes)?;
                let mut layer = tv2_v1compat::author::from_v1(&raw, &asset)?;
                if let Some(existing) =
                    project.layers.iter().find(|l| l.asset_id == asset.id && l.kind == tv2_domain::LayerKind::Author && !l.deleted)
                {
                    layer.layer_id = existing.layer_id.clone();
                }
                let request = CommandEnvelope::human(Command::ReplaceLayer { layer })
                    .with_base(project.revision)
                    .with_actor(tv2_application::Actor::External { source: "marcas V1".into() });
                ProjectSession::new(project).prepare_command(request)
            };
            let _ = tx.send(work());
        });
        match launch {
            Ok(_) => self.semantic_job = Some(SemanticJob { result }),
            Err(e) => self.report(e.into()),
        }
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
