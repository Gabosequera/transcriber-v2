//! One bounded IO operation per GUI. Frozen inputs; results are checked against
//! session identity/revision before replacing or acknowledging any state.
use crate::app::{Selection, Severity, TranscriptorApp};
use tv2_application::{
    ProjectSession, ProjectStore,
    session::{DurableHistory, JournalEvent},
};
use tv2_domain::{DomainError, Project, error::DomainResult};

pub struct PersistenceJob {
    pub project_id: String,
    pub revision: u64,
    pub result: crossbeam_channel::Receiver<DomainResult<Completion>>,
}
pub enum Completion {
    Open(Box<LoadedProject>),
    Save { store: ProjectStore, journal_count: usize, warning: Option<DomainError> },
}
pub struct LoadedProject {
    store: ProjectStore,
    session: ProjectSession,
    recovery: Option<tv2_application::store::RecoveryCheckpoint>,
    warning: Option<DomainError>,
}

pub fn load(store: ProjectStore) -> DomainResult<Completion> {
    let mut session = store.load_session()?;
    let (recovery, warning) = match store.recovery_checkpoint(session.project()) {
        Ok(value) => (value, None),
        Err(e) => (None, Some(e)),
    };
    session.resolve_paths(|path| store.resolve_path(path).to_string_lossy().replace('\\', "/"));
    let mut availability = std::collections::HashMap::new();
    session.refresh_asset_availability(|path| *availability.entry(path.to_string()).or_insert_with(|| std::path::Path::new(path).exists()));
    let recovery = recovery.map(|mut checkpoint| {
        for asset in &mut checkpoint.project.assets {
            asset.path = store.resolve_path(&asset.path).to_string_lossy().replace('\\', "/");
        }
        if let Some(history) = &mut checkpoint.history {
            history.map_paths(|path| store.resolve_path(path).to_string_lossy().replace('\\', "/"));
        }
        checkpoint
    });
    Ok(Completion::Open(Box::new(LoadedProject { store, session, recovery, warning })))
}

pub fn save(
    store: ProjectStore,
    source: Option<ProjectStore>,
    mut project: Project,
    mut history: DurableHistory,
    pending: Vec<JournalEvent>,
    recovery_root: Option<std::path::PathBuf>,
) -> DomainResult<Completion> {
    let journal_count = pending.len();
    let mut events = if let Some(source) = source.filter(|s| s.root != store.root) { source.read_journal()? } else { vec![] };
    events.extend(pending);
    // A concurrent replacement of the source store must not mix foreign/future audit into Save As.
    ProjectSession::with_audit(project.clone(), &events)?;
    for asset in &mut project.assets {
        asset.path = store.portable_path(std::path::Path::new(&asset.path));
    }
    history.map_paths(|path| store.portable_path(std::path::Path::new(path)));
    store.save_checkpoint(&project, &events, Some(&history))?;
    let warning = recovery_root.and_then(|root| {
        let marker = serde_json::json!({"saved_to":store.root,"revision":project.revision});
        tv2_application::store::atomic_write(&root.join("resolved.json"), marker.to_string().as_bytes()).err()
    });
    Ok(Completion::Save { store, journal_count, warning })
}

impl TranscriptorApp {
    pub fn poll_persistence(&mut self) {
        let Some(job) = &self.persistence_job else { return };
        let result = match job.result.try_recv() {
            Ok(value) => value,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(_) => Err(DomainError::process("worker de proyecto terminó sin resultado")),
        };
        let job = self.persistence_job.take().unwrap();
        if job.project_id != self.project().project_id {
            self.report(DomainError::precondition("el proyecto cambió durante la operación de archivos"));
            return;
        }
        match result {
            Ok(Completion::Open(loaded)) => {
                let LoadedProject { store, session, recovery, warning } = *loaded;
                if job.revision != self.session.revision() {
                    self.report(DomainError::precondition("se editó durante la apertura; se conserva la sesión actual, vuelve a abrir"));
                    return;
                }
                if let Some(warning) = warning {
                    self.report(warning);
                }
                self.session = session;
                self.recovery = None;
                self.recovery_audit.clear();
                self.recovery_history = None;
                if let Some(checkpoint) = recovery {
                    self.recovery = Some(checkpoint.project);
                    self.recovery_audit = checkpoint.events;
                    self.recovery_history = checkpoint.history;
                }
                self.external = Default::default();
                self.unsaved_store = None;
                self.marker_editor = None;
                self.item_editor = None;
                if let Some(media) = &mut self.media_view {
                    media.clear();
                }
                self.ui.last_project = Some(store.root.to_string_lossy().to_string());
                self.store = Some(store);
                self.selection = Selection::default();
                self.resolved_revision = None;
                self.source_asset = self.project().assets.first().map(|a| a.id.clone());
                self.seek(tv2_domain::Ticks::ZERO);
                self.toast(Severity::Info, format!("Proyecto «{}» abierto (revisión {})", self.project().name, self.session.revision()));
            }
            Ok(Completion::Save { store, journal_count, warning }) => {
                if let Some(warning) = warning {
                    self.report(warning.with_action("Guardado completo; el autosave antiguo seguirá apareciendo como recuperable"));
                }
                self.unsaved_store = None;
                self.session.acknowledge_save(job.revision, journal_count);
                self.ui.last_project = Some(store.root.to_string_lossy().to_string());
                self.toast(Severity::Info, format!("Revisión {} guardada en {}", job.revision, store.root.display()));
                self.store = Some(store);
                self.dirty_title = true;
            }
            Err(e) => {
                self.close_after_save = false;
                if let Some(script) = &mut self.script {
                    script.failed = true;
                }
                self.report(e);
            }
        }
    }
}
