//! Persistencia de proyecto: carpeta `<nombre>.transcriptor/` con
//! `project.json` (contrato, escritura atómica: tmp + fsync + rename) y
//! `journal.jsonl` (auditoría append-only). Las cachés van en `cache/`.
//!
//! A durable intent precedes project/journal publication. Reopening finishes
//! either boundary idempotently, provided the disk still matches old/new content.
//! V1 multi-document exports require their own transaction protocol.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tv2_domain::error::{DomainError, DomainResult};
use tv2_domain::project::{PROJECT_SCHEMA, Project};

use crate::session::JournalEvent;

pub const PROJECT_FILE: &str = "project.json";
pub const JOURNAL_FILE: &str = "journal.jsonl";
pub const PROJECT_DIR_SUFFIX: &str = ".transcriptor";
const MAX_PROJECT_BYTES: u64 = 64 * 1024 * 1024;
const PENDING_FILE: &str = ".pending-commit.json";

pub struct RecoveryCheckpoint {
    pub project: Project,
    pub events: Vec<JournalEvent>,
    pub history: Option<crate::session::DurableHistory>,
}

/// Keeps the cooperative disk lock until the GUI confirms the exact prepared command.
pub struct PreparedExternal {
    command: crate::session::PreparedCommand,
    external: Project,
    external_digest: String,
    _lock: fs::File,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitIntent {
    schema: String,
    before_digest: Option<String>,
    project: Project,
    events: Vec<JournalEvent>,
    #[serde(default, with = "crate::history_codec::optional")]
    history: Option<crate::session::DurableHistory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audit: Option<crate::audit::AuditCommit>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AutosaveSnapshot {
    schema: String,
    project: Project,
    events: Vec<JournalEvent>,
    #[serde(default, with = "crate::history_codec::optional")]
    history: Option<crate::session::DurableHistory>,
    #[serde(default)]
    writer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audit: Option<crate::audit::AuditIndex>,
}

#[derive(Clone, Debug)]
pub struct ProjectStore {
    pub root: PathBuf,
    observed: Arc<Mutex<Option<String>>>,
    baseline: Arc<Mutex<Option<Project>>>,
    writer: String,
}

impl ProjectStore {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        ProjectStore {
            root: root.into(),
            observed: Arc::new(Mutex::new(None)),
            baseline: Arc::new(Mutex::new(None)),
            writer: tv2_domain::ids::random_hex12(),
        }
    }

    /// Normaliza una ruta elegida por el usuario a la carpeta del proyecto.
    pub fn from_user_path(path: &Path) -> Self {
        let p = if path.is_file() && path.file_name().is_some_and(|n| n == PROJECT_FILE) {
            path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
        } else if path.extension().is_some_and(|e| e == "transcriptor") || path.to_string_lossy().ends_with(PROJECT_DIR_SUFFIX) {
            path.to_path_buf()
        } else {
            let mut s = path.as_os_str().to_owned();
            s.push(PROJECT_DIR_SUFFIX);
            PathBuf::from(s)
        };
        Self::at(p)
    }

    pub fn project_path(&self) -> PathBuf {
        self.root.join(PROJECT_FILE)
    }

    pub fn journal_path(&self) -> PathBuf {
        self.root.join(JOURNAL_FILE)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn exists(&self) -> bool {
        self.project_path().is_file()
    }

    pub fn save(&self, project: &Project) -> DomainResult<()> {
        self.save_with_journal(project, &[])
    }

    /// Logical commit of snapshot and audit. A retry after a crash finishes the
    /// published intent; a conflicting external writer is never overwritten.
    pub fn save_with_journal(&self, project: &Project, events: &[JournalEvent]) -> DomainResult<()> {
        self.save_checkpoint(project, events, None)
    }

    pub fn save_checkpoint(&self, project: &Project, events: &[JournalEvent], history: Option<&crate::session::DurableHistory>) -> DomainResult<()> {
        project.validate()?;
        if let Some(history) = history {
            history.validate(project)?;
        }
        crate::source_bundles::verify_checkpoint(self, project, history)?;
        fs::create_dir_all(&self.root).map_err(|e| DomainError::io(format!("no se pudo crear {}: {e}", self.root.display())))?;
        // OS lock is released on process exit, including crashes. Cooperating
        // instances serialize compare-and-replace; arbitrary editors must also
        // respect this lock to eliminate the final check/write race.
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
        lock.try_lock().map_err(|e| DomainError::new(tv2_domain::ErrorCode::ExternalConflict, format!("otro escritor usa el proyecto: {e}")))?;
        let recovering = self.root.join(PENDING_FILE).is_file();
        self.finish_pending()?;
        let mut observed = self.observed.lock().map_err(|_| DomainError::io("estado de persistencia no disponible"))?;
        let disk_digest = match Self::read_project(&self.project_path()) {
            Ok(disk) => Some(Self::digest(&disk)?),
            Err(e) if !self.project_path().exists() && e.code == tv2_domain::ErrorCode::Io => None,
            Err(e) => return Err(e),
        };
        // A previous call may have published its intent and then failed at a
        // later boundary. Once recovered, retrying that exact snapshot is safe.
        if disk_digest != *observed && !(recovering && disk_digest.as_ref() == Some(&Self::digest(project)?)) {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el proyecto en disco cambió desde su apertura")
                .with_action("abre la versión externa o guarda tus cambios en otra carpeta"));
        }
        // Validate audit before publishing the intent, including duplicate IDs.
        let audit = if project.schema == PROJECT_SCHEMA {
            Some(crate::audit::prepare(&self.root, events)?)
        } else {
            self.merged_journal(events)?;
            None
        };
        let mut writer = crate::storage_codec::Writer::new(&self.root);
        let shadow_project = writer.project(project)?;
        let shadow_history = history.map(|history| writer.history(history)).transpose()?;
        let storage = writer.used;
        if serde_json::to_vec(&shadow_project)?.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("metadatos editables de proyecto superan 64 MiB"));
        }
        let intent = CommitIntent {
            schema: if storage { "transcriptor-commit/2".into() } else { "transcriptor-commit/1".into() },
            before_digest: disk_digest,
            project: shadow_project,
            events: if audit.is_some() { Vec::new() } else { events.to_vec() },
            history: shadow_history,
            audit,
        };
        let bytes = serde_json::to_vec(&intent)?;
        if bytes.len() as u64 > MAX_PROJECT_BYTES * 2 {
            return Err(DomainError::invalid("transacción supera el límite de 128 MiB"));
        }
        atomic_write(&self.root.join(PENDING_FILE), &bytes)?;
        self.finish_pending()?;
        *observed = Some(Self::digest(project)?);
        *self.baseline.lock().map_err(|_| DomainError::io("baseline no disponible"))? = Some(project.clone());
        Ok(())
    }

    pub fn load(&self) -> DomainResult<Project> {
        let lock = self.read_lock()?;
        self.finish_pending()?;
        let result = self.load_unlocked();
        drop(lock);
        result
    }

    fn read_lock(&self) -> DomainResult<fs::File> {
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
        lock.try_lock().map_err(|e| DomainError::new(tv2_domain::ErrorCode::ExternalConflict, format!("proyecto ocupado: {e}")))?;
        Ok(lock)
    }

    pub fn load_session(&self) -> DomainResult<crate::ProjectSession> {
        let _lock = self.read_lock()?;
        self.finish_pending()?;
        let project = self.load_unlocked()?;
        let history = self.load_history(&project)?;
        let mut session = crate::ProjectSession::new(project);
        let snapshot = crate::audit::index(&self.root)?;
        if session.project().schema == PROJECT_SCHEMA && snapshot.is_none() {
            return Err(DomainError::invalid("proyecto /2 sin índice durable de auditoría"));
        }
        let mut ids = std::collections::HashSet::new();
        crate::audit::visit(&self.root, snapshot.as_ref(), |event| {
            if !ids.insert(event.command_id.clone()) {
                return Err(DomainError::invalid("command_id duplicado en archivo de auditoría"));
            }
            session.restore_receipts(std::slice::from_ref(&event))
        })?;
        if let Some(history) = history {
            session.restore_history(history)?;
        }
        Ok(session)
    }

    fn load_unlocked(&self) -> DomainResult<Project> {
        let path = self.project_path();
        let project = Self::read_project(&path)?;
        crate::source_bundles::verify_checkpoint(self, &project, None)?;
        *self.observed.lock().map_err(|_| DomainError::io("estado de persistencia no disponible"))? = Some(Self::digest(&project)?);
        *self.baseline.lock().map_err(|_| DomainError::io("baseline no disponible"))? = Some(project.clone());
        Ok(project)
    }

    /// Read-only probe. Two equal valid reads are required before proposing;
    /// callers run this on a worker and periodically rescan even without a watcher.
    pub fn external_change(&self, local: &Project) -> DomainResult<Option<crate::reconcile::ExternalChange>> {
        let base = self.baseline.lock().map_err(|_| DomainError::io("baseline no disponible"))?.clone();
        let Some(mut base) = base else {
            return Ok(None);
        };
        let mut external = Self::read_project(&self.project_path())?;
        let disk_digest = Self::digest(&external)?;
        if disk_digest == Self::digest(&base)? {
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        if Self::digest(&Self::read_project(&self.project_path())?)? != disk_digest {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el archivo externo sigue cambiando; se volverá a leer"));
        }
        crate::source_bundles::verify_checkpoint(self, &external, None)?;
        for p in [&mut base, &mut external] {
            crate::source_bundles::map_project_paths(p, |path| self.resolve_path(path).to_string_lossy().replace('\\', "/"));
        }
        let mut change = crate::reconcile::inspect(&base, local, &external)?;
        change.external_digest = disk_digest;
        Ok(Some(change))
    }

    /// Explicit approval, revalidating both sides. Disk stays untouched until
    /// the next save; the external version becomes the CAS baseline only on success.
    pub fn accept_external(&self, session: &mut crate::ProjectSession, change: crate::reconcile::ExternalChange) -> DomainResult<()> {
        let prepared = self.prepare_external(session.project(), change)?;
        self.commit_external(session, prepared)
    }

    pub fn prepare_external(&self, local: &Project, change: crate::reconcile::ExternalChange) -> DomainResult<PreparedExternal> {
        let change = change.resolved()?;
        if local.revision != change.base_revision || Self::digest(local)? != change.local_digest {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el proyecto cambió mientras se revisaba el diff"));
        }
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
        lock.try_lock().map_err(|e| DomainError::io(format!("proyecto ocupado: {e}")))?;
        let external = Self::read_project(&self.project_path())?;
        if Self::digest(&external)? != change.external_digest {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el archivo externo cambió mientras se revisaba el diff"));
        }
        let command = crate::ProjectSession::new(local.clone()).prepare_command(
            crate::CommandEnvelope::human(tv2_domain::Command::ReconcileProject { project: Box::new(change.merged) })
                .with_base(change.base_revision)
                .with_actor(if change.human_review { crate::Actor::Human } else { crate::Actor::External { source: "project.json".into() } }),
        )?;
        Ok(PreparedExternal { command, external, external_digest: change.external_digest, _lock: lock })
    }
    pub fn commit_external(&self, session: &mut crate::ProjectSession, prepared: PreparedExternal) -> DomainResult<()> {
        let mut observed = self.observed.lock().map_err(|_| DomainError::io("estado de persistencia no disponible"))?;
        let mut baseline = self.baseline.lock().map_err(|_| DomainError::io("baseline no disponible"))?;
        session.commit_prepared(prepared.command)?;
        *observed = Some(prepared.external_digest);
        *baseline = Some(prepared.external);
        Ok(())
    }

    /// Called only while holding .write.lock. Removal is last: interrupted
    /// removal simply causes another harmless replay on the next open.
    fn finish_pending(&self) -> DomainResult<()> {
        let path = self.root.join(PENDING_FILE);
        if !path.is_file() {
            return Ok(());
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?.take(MAX_PROJECT_BYTES * 2 + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PROJECT_BYTES * 2 {
            return Err(DomainError::invalid("intent demasiado grande"));
        }
        let mut intent: CommitIntent = serde_json::from_slice(&bytes)?;
        if !matches!(intent.schema.as_str(), "transcriptor-commit/1" | "transcriptor-commit/2") {
            return Err(DomainError::unsupported("schema de transacción desconocido"));
        }
        if intent.schema == "transcriptor-commit/2" {
            let mut reader = crate::storage_codec::Reader::new(&self.root);
            reader.project(&mut intent.project)?;
            if let Some(history) = &mut intent.history {
                reader.history(history)?;
            }
        }
        intent.project.validate()?;
        if let Some(history) = &intent.history {
            history.validate(&intent.project)?;
        }
        crate::source_bundles::verify_checkpoint(self, &intent.project, intent.history.as_ref())?;
        let next = Self::digest(&intent.project)?;
        let disk = if self.project_path().exists() { Some(Self::digest(&Self::read_project(&self.project_path())?)?) } else { None };
        if disk != intent.before_digest && disk.as_ref() != Some(&next) {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el archivo cambió durante una transacción pendiente"));
        }
        let journal = if let Some(audit) = &intent.audit {
            crate::audit::validate_commit(&self.root, audit)?;
            None
        } else if intent.project.schema == PROJECT_SCHEMA {
            crate::audit::additions(&self.root, &intent.events)?;
            None
        } else {
            Some(self.merged_journal(&intent.events)?)
        };
        let mut writer = crate::storage_codec::Writer::new(&self.root);
        let shadow_project = writer.project(&intent.project)?;
        let wire_project = if writer.used {
            serde_json::json!({"schema":crate::storage_codec::PROJECT_STORAGE,"project":shadow_project})
        } else {
            serde_json::to_value(&shadow_project)?
        };
        let project_bytes = serde_json::to_vec_pretty(&wire_project)?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("proyecto demasiado grande"));
        }
        if disk.as_ref() != Some(&next) {
            atomic_write(&self.project_path(), &project_bytes)?;
        }
        if let Some(audit) = &intent.audit {
            crate::audit::commit(&self.root, audit)?;
        } else if let Some(journal) = journal {
            atomic_write(&self.journal_path(), &journal)?;
        } else {
            crate::audit::publish(&self.root, &intent.events)?;
        }
        // Publish under the same recoverable intent as project + audit.
        if let Some(history) = &intent.history {
            let shadow_history = writer.history(history)?;
            let compact = crate::history_codec::encode(&shadow_history)?;
            let wire_history =
                if writer.used { serde_json::json!({"schema":crate::storage_codec::HISTORY_STORAGE,"history":compact}) } else { compact };
            atomic_write(&self.root.join("history.json"), &serde_json::to_vec(&wire_history)?)?;
        } else if self.root.join("history.json").exists() {
            fs::remove_file(self.root.join("history.json"))?;
        }
        fs::remove_file(path)?;
        Ok(())
    }

    pub fn load_history(&self, project: &Project) -> DomainResult<Option<crate::session::DurableHistory>> {
        let path = self.root.join("history.json");
        if !path.exists() {
            return Ok(None);
        } // explicit migration: pre-history projects start empty
        let mut bytes = Vec::new();
        fs::File::open(path)?.take(MAX_PROJECT_BYTES * 2 + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PROJECT_BYTES * 2 {
            return Err(DomainError::invalid("historial supera 128 MiB"));
        }
        let mut raw: serde_json::Value = serde_json::from_slice(&bytes)?;
        let storage = raw["schema"] == crate::storage_codec::HISTORY_STORAGE;
        if storage {
            raw = crate::storage_codec::unwrap_envelope(&mut raw, "history")?;
        }
        let mut history = crate::history_codec::decode(raw)?;
        if storage {
            let mut reader = crate::storage_codec::Reader::new(&self.root);
            reader.seed_project(project);
            reader.history(&mut history)?;
        }
        history.validate(project)?;
        crate::source_bundles::verify_checkpoint(self, project, Some(&history))?;
        Ok(Some(history))
    }

    fn merged_journal(&self, events: &[JournalEvent]) -> DomainResult<Vec<u8>> {
        let mut all = self.read_journal()?;
        let mut by_id: std::collections::HashMap<_, _> = all.iter().enumerate().map(|(i, e)| (e.command_id.clone(), i)).collect();
        for event in events {
            if let Some(&i) = by_id.get(&event.command_id) {
                if &all[i] != event {
                    return Err(DomainError::precondition("command_id de auditoría reutilizado con otro contenido"));
                }
            } else {
                by_id.insert(event.command_id.clone(), all.len());
                all.push(event.clone());
            }
        }
        let mut bytes = Vec::new();
        for event in all {
            serde_json::to_writer(&mut bytes, &event)?;
            bytes.push(b'\n');
        }
        if bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("journal supera 64 MiB; archivar auditoría antes de continuar"));
        }
        Ok(bytes)
    }

    fn digest(project: &Project) -> DomainResult<String> {
        Ok(tv2_domain::digest::digest_json(&serde_json::to_value(project)?))
    }

    fn read_project(path: &Path) -> DomainResult<Project> {
        let mut bytes = Vec::new();
        fs::File::open(path).and_then(|f| f.take(MAX_PROJECT_BYTES + 1).read_to_end(&mut bytes)).map_err(|e| {
            DomainError::io(format!("no se pudo leer {}: {e}", path.display())).with_action("comprueba que la carpeta del proyecto existe")
        })?;
        if bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("proyecto supera el límite de 64 MiB"));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| DomainError::invalid("proyecto no es UTF-8 válido"))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut raw: serde_json::Value =
            serde_json::from_str(text).map_err(|e| DomainError::invalid(format!("{} no es un proyecto válido: {e}", path.display())))?;
        let storage = raw["schema"] == crate::storage_codec::PROJECT_STORAGE;
        if storage {
            raw = crate::storage_codec::unwrap_envelope(&mut raw, "project")?;
        }
        if !matches!(raw["schema"].as_str(), Some(PROJECT_SCHEMA | tv2_domain::project::LEGACY_PROJECT_SCHEMA)) {
            return Err(DomainError::unsupported(format!("schema de proyecto desconocido: {}", raw["schema"]))
                .with_action(format!("este editor entiende {PROJECT_SCHEMA}")));
        }
        let mut project: Project =
            serde_json::from_value(raw).map_err(|e| DomainError::invalid(format!("{} no es un proyecto válido: {e}", path.display())))?;
        if storage {
            crate::storage_codec::Reader::new(path.parent().ok_or_else(|| DomainError::invalid("proyecto sin carpeta"))?).project(&mut project)?;
        }
        project.validate()?;
        Ok(project)
    }

    /// A recovery snapshot is a candidate, never silently replaces user data.
    pub fn recovery_candidate(&self, saved: &Project) -> DomainResult<Option<Project>> {
        Ok(self.recovery_with_audit(saved)?.map(|(project, _)| project))
    }

    pub fn save_autosave(&self, project: &Project, pending: &[JournalEvent]) -> DomainResult<()> {
        self.save_autosave_checkpoint(project, pending, None)
    }

    pub fn save_autosave_checkpoint(
        &self,
        project: &Project,
        pending: &[JournalEvent],
        history: Option<&crate::session::DurableHistory>,
    ) -> DomainResult<()> {
        project.validate()?;
        if let Some(history) = history {
            history.validate(project)?;
        }
        crate::source_bundles::verify_checkpoint(self, project, history)?;
        fs::create_dir_all(&self.root)?;
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
        lock.try_lock().map_err(|e| DomainError::new(tv2_domain::ErrorCode::ExternalConflict, format!("otro escritor usa el proyecto: {e}")))?;
        self.finish_pending()?;
        let saved = Self::read_project(&self.project_path())?;
        if Some(Self::digest(&saved)?) != *self.observed.lock().map_err(|_| DomainError::io("estado de persistencia no disponible"))? {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el proyecto cambió en disco antes del autosave"));
        }
        let autosave_path = self.root.join("autosave.json");
        if autosave_path.exists() {
            let mut previous = Vec::new();
            fs::File::open(&autosave_path)?.take(MAX_PROJECT_BYTES * 2 + 1).read_to_end(&mut previous)?;
            if previous.len() as u64 > MAX_PROJECT_BYTES * 2 {
                return Err(DomainError::invalid("autosave previo demasiado grande"));
            }
            let raw: serde_json::Value = serde_json::from_slice(&previous)?;
            let revision = raw["project"]["revision"].as_u64().or_else(|| raw["revision"].as_u64()).unwrap_or(u64::MAX);
            if revision > saved.revision && raw["writer"].as_str() != Some(&self.writer) {
                return Err(DomainError::new(
                    tv2_domain::ErrorCode::ExternalConflict,
                    "hay un autosave recuperable de otra sesión; recupera o guarda explícitamente antes de sustituirlo",
                ));
            }
        }
        let mut audit = crate::audit::index(&self.root)?;
        let mut events = if audit.is_some() {
            crate::audit::additions(&self.root, pending)?
        } else {
            let bytes = self.merged_journal(pending)?;
            bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()).map(serde_json::from_slice).collect::<Result<Vec<JournalEvent>, _>>()?
        };
        crate::ProjectSession::with_audit(project.clone(), &events)?;
        if audit.is_some() {
            audit = Some(crate::audit::prepare(&self.root, &events)?.after);
            events.clear();
        }
        let mut writer = crate::storage_codec::Writer::new(&self.root);
        let shadow_project = writer.project(project)?;
        let shadow_history = history.map(|history| writer.history(history)).transpose()?;
        let snapshot = AutosaveSnapshot {
            schema: if writer.used { "transcriptor-autosave/2".into() } else { "transcriptor-autosave/1".into() },
            project: shadow_project,
            events,
            history: shadow_history,
            writer: Some(self.writer.clone()),
            audit,
        };
        let bytes = serde_json::to_vec(&snapshot)?;
        if bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("autosave con auditoría supera 64 MiB"));
        }
        atomic_write(&self.root.join("autosave.json"), &bytes)
    }

    /// Legacy bare snapshots remain readable; new autosaves publish content and
    /// audit together so recovery cannot lose successful idempotency receipts.
    pub fn recovery_with_audit(&self, saved: &Project) -> DomainResult<Option<(Project, Vec<JournalEvent>)>> {
        Ok(self.recovery_checkpoint(saved)?.map(|c| (c.project, c.events)))
    }

    pub fn recovery_checkpoint(&self, saved: &Project) -> DomainResult<Option<RecoveryCheckpoint>> {
        let path = self.root.join("autosave.json");
        if !path.is_file() {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?.take(MAX_PROJECT_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("autosave supera 64 MiB"));
        }
        let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
        let (candidate, events, history) = if matches!(raw["schema"].as_str(), Some("transcriptor-autosave/1" | "transcriptor-autosave/2")) {
            let mut snapshot: AutosaveSnapshot = serde_json::from_value(raw)?;
            if snapshot.schema == "transcriptor-autosave/2" {
                let mut reader = crate::storage_codec::Reader::new(&self.root);
                reader.seed_project(saved);
                reader.project(&mut snapshot.project)?;
                if let Some(history) = &mut snapshot.history {
                    reader.history(history)?;
                }
            }
            if let Some(history) = &snapshot.history {
                history.validate(&snapshot.project)?;
            }
            let mut events = Vec::new();
            if let Some(index) = &snapshot.audit {
                crate::audit::visit(&self.root, Some(index), |event| {
                    events.push(event);
                    Ok(())
                })?;
            }
            events.extend(snapshot.events);
            (snapshot.project, events, snapshot.history)
        } else {
            (Self::read_project(&path)?, vec![], None)
        };
        candidate.validate()?;
        crate::source_bundles::verify_checkpoint(self, &candidate, history.as_ref())?;
        crate::ProjectSession::with_audit(candidate.clone(), &events)?;
        if candidate.project_id != saved.project_id || candidate.revision <= saved.revision {
            return Ok(None);
        }
        Ok(Some(RecoveryCheckpoint { project: candidate, events, history }))
    }

    pub fn append_journal(&self, events: &[JournalEvent]) -> DomainResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.root)?;
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
        lock.try_lock().map_err(|e| DomainError::io(format!("proyecto ocupado: {e}")))?;
        self.finish_pending()?;
        if self.project_path().exists() && Self::read_project(&self.project_path())?.schema == PROJECT_SCHEMA {
            crate::audit::publish(&self.root, events)?;
        } else {
            atomic_write(&self.journal_path(), &self.merged_journal(events)?)?;
        }
        Ok(())
    }

    pub fn read_journal(&self) -> DomainResult<Vec<JournalEvent>> {
        if let Some(index) = crate::audit::index(&self.root)? {
            let mut events = Vec::new();
            crate::audit::visit(&self.root, Some(&index), |event| {
                events.push(event);
                Ok(())
            })?;
            return Ok(events);
        }
        let path = self.journal_path();
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?.take(MAX_PROJECT_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("journal supera 64 MiB"));
        }
        let mut out = Vec::new();
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<JournalEvent>(line) {
                Ok(e) => out.push(e),
                Err(_) if !line.ends_with(b"\n") => break, // only an incomplete final record may be discarded
                Err(e) => return Err(DomainError::invalid(format!("journal corrupto antes de su frontera final: {e}"))),
            }
        }
        Ok(out)
    }

    /// Bounded audit queries. Cursors bind to the index digest; appending new
    /// events requires restarting the query instead of silently skipping records.
    pub fn journal_page(&self, cursor: Option<&str>, limit: usize) -> DomainResult<crate::audit::JournalPage> {
        let _lock = self.read_lock()?;
        self.finish_pending()?;
        crate::audit::page(&self.root, cursor, limit)
    }

    /// Save As can transfer archived audit without collecting every past event
    /// in RAM or embedding it again in the project commit intent.
    pub fn copy_audit_from(&self, source: &ProjectStore, project_id: &str, revision: u64) -> DomainResult<()> {
        if self.root == source.root {
            return Ok(());
        }
        let _source_lock = source.read_lock()?;
        source.finish_pending()?;
        let saved = Self::read_project(&source.project_path())?;
        if saved.project_id != project_id || saved.revision > revision {
            return Err(DomainError::precondition("la auditoría de origen pertenece a otro proyecto o revisión futura"));
        }
        if source.observed.lock().map_err(|_| DomainError::io("baseline no disponible"))?.as_ref() != Some(&Self::digest(&saved)?) {
            return Err(DomainError::precondition("el proyecto de origen cambió antes de copiar auditoría"));
        }
        fs::create_dir_all(&self.root)?;
        let _target_lock = self.read_lock()?;
        if self.project_path().exists() || self.root.join(PENDING_FILE).exists() {
            return Err(DomainError::precondition("solo se copia auditoría a un proyecto nuevo"));
        }
        crate::audit::copy_to(&source.root, &self.root)
    }

    /// Ruta portable relativa a la carpeta del proyecto si es posible.
    pub fn portable_path(&self, path: &Path) -> String {
        let rel = self.root.parent().and_then(|base| path.strip_prefix(base).ok());
        let p = rel.map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf());
        p.to_string_lossy().replace('\\', "/")
    }

    /// Resuelve una ruta portable guardada en el proyecto.
    pub fn resolve_path(&self, portable: &str) -> PathBuf {
        if let Some(relative) = portable.strip_prefix("@project/") {
            return self.root.join(relative);
        }
        let p = PathBuf::from(portable);
        if p.is_absolute() { p } else { self.root.parent().map(|b| b.join(&p)).unwrap_or(p) }
    }
}

/// Escritura atómica: temporal en la misma carpeta, fsync, rename con reintentos
/// (Windows puede devolver `PermissionError` transitorio si un watcher tiene el
/// archivo abierto), como `editorial_io.atomic_write_text`.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> DomainResult<()> {
    let dir = path.parent().ok_or_else(|| DomainError::io("ruta sin carpeta"))?;
    fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut tmp = tempfile::Builder::new().prefix(&format!(".{name}.")).suffix(".tmp").tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    tmp.as_file().sync_all()?;
    let tmp_path = tmp.into_temp_path();
    let mut delay = std::time::Duration::from_millis(20);
    for attempt in 0..6 {
        match fs::rename(&tmp_path, path) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < 5 && e.kind() == std::io::ErrorKind::PermissionDenied => {
                std::thread::sleep(delay);
                delay *= 2;
            }
            Err(e) => {
                let _ = tmp_path.close();
                return Err(DomainError::io(format!("no se pudo reemplazar {}: {e}", path.display())));
            }
        }
    }
    Err(DomainError::io(format!("no se pudo reemplazar {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_audit_commit_recovers_project_index_and_history_boundaries() {
        for boundary in 0..=3 {
            let temp = tempfile::tempdir().unwrap();
            let store = ProjectStore::at(temp.path());
            let before = Project::new("before");
            store.save(&before).unwrap();
            let mut live = crate::ProjectSession::new(before.clone());
            live.execute(crate::CommandEnvelope::human(tv2_domain::Command::RenameProject { name: "after".into() }).with_idempotency("once"))
                .unwrap();
            let plan = crate::audit::prepare(&store.root, live.pending_journal()).unwrap();
            let intent = CommitIntent {
                schema: "transcriptor-commit/1".into(),
                before_digest: Some(ProjectStore::digest(&before).unwrap()),
                project: live.project().clone(),
                events: Vec::new(),
                history: Some(live.history_snapshot()),
                audit: Some(plan),
            };
            atomic_write(&store.root.join(PENDING_FILE), &serde_json::to_vec(&intent).unwrap()).unwrap();
            if boundary >= 1 {
                atomic_write(&store.project_path(), &serde_json::to_vec(&intent.project).unwrap()).unwrap();
            }
            if boundary >= 2 {
                crate::audit::commit(&store.root, intent.audit.as_ref().unwrap()).unwrap();
            }
            if boundary >= 3 {
                atomic_write(
                    &store.root.join("history.json"),
                    &serde_json::to_vec(&crate::history_codec::encode(intent.history.as_ref().unwrap()).unwrap()).unwrap(),
                )
                .unwrap();
            }
            let mut restored = ProjectStore::at(temp.path()).load_session().unwrap();
            assert_eq!(restored.project().name, "after");
            assert!(restored.command_receipt("once").unwrap().is_some());
            assert_eq!(store.read_journal().unwrap().len(), 1);
            restored.undo(crate::Actor::Human).unwrap();
            assert_eq!(restored.project().name, "before");
            assert!(!store.root.join(PENDING_FILE).exists());
        }
    }

    #[test]
    fn autosave_publishes_audit_and_receipts_with_content_and_recovers_once() {
        use crate::{Actor, CommandEnvelope, ProjectSession};
        use tv2_domain::Command;
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(dir.path());
        let original = Project::new("saved");
        store.save(&original).unwrap();
        let mut live = ProjectSession::new(original.clone());
        let request = CommandEnvelope::human(Command::RenameProject { name: "autosaved".into() }).with_idempotency("recovery-key");
        live.execute(request.clone()).unwrap();
        store.save_autosave(live.project(), live.pending_journal()).unwrap();
        assert_eq!(store.load().unwrap(), original);
        let (project, events) = store.recovery_with_audit(&original).unwrap().unwrap();
        let mut restored = ProjectSession::new(original);
        restored.recover_with_audit(project, events).unwrap();
        assert!(restored.execute(request.clone()).unwrap().replayed);
        restored.undo(Actor::Human).unwrap();
        assert_eq!(restored.project().name, "saved");
        store.save_with_journal(restored.project(), restored.pending_journal()).unwrap();
        let mut opened = ProjectSession::with_audit(store.load().unwrap(), &store.read_journal().unwrap()).unwrap();
        assert!(opened.execute(request).unwrap().replayed);
        assert_eq!(opened.project().name, "saved");
    }

    #[test]
    fn compare_and_save_rejects_second_instance_and_preserves_external_content() {
        let dir = tempfile::tempdir().unwrap();
        let first = ProjectStore::at(dir.path().join("proyecto ñ.transcriptor"));
        let mut project = Project::new("original");
        first.save(&project).unwrap();
        let second = ProjectStore::at(first.root.clone());
        let stale = second.load().unwrap();
        project.name = "first writer".into();
        project.revision += 1;
        first.save(&project).unwrap();
        assert_eq!(second.save(&stale).unwrap_err().code, tv2_domain::ErrorCode::ExternalConflict);
        assert_eq!(second.load().unwrap().name, "first writer");
        assert!(ProjectStore::at(first.root.clone()).save(&stale).is_err(), "unopened destination must not be overwritten");
    }

    #[test]
    fn invalid_external_json_is_never_overwritten_and_lock_is_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(dir.path());
        let project = Project::new("original");
        store.save(&project).unwrap();
        let lock = fs::OpenOptions::new().read(true).write(true).open(store.root.join(".write.lock")).unwrap();
        lock.lock().unwrap();
        assert_eq!(store.save(&project).unwrap_err().code, tv2_domain::ErrorCode::ExternalConflict);
        drop(lock);
        fs::write(store.project_path(), "{partial").unwrap();
        assert!(store.save(&project).is_err());
        assert_eq!(fs::read_to_string(store.project_path()).unwrap(), "{partial");
    }

    #[test]
    fn recovery_requires_matching_identity_and_newer_revision() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(dir.path());
        let saved = Project::new("saved");
        store.save(&saved).unwrap();
        let mut candidate = saved.clone();
        candidate.revision = 3;
        candidate.name = "recovered".into();
        atomic_write(&store.root.join("autosave.json"), &serde_json::to_vec(&candidate).unwrap()).unwrap();
        assert_eq!(store.recovery_candidate(&saved).unwrap(), Some(candidate.clone()));
        candidate.project_id = "another".into();
        atomic_write(&store.root.join("autosave.json"), &serde_json::to_vec(&candidate).unwrap()).unwrap();
        assert!(store.recovery_candidate(&saved).unwrap().is_none());
    }

    #[test]
    fn save_load_round_trip_and_atomic_replace() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::from_user_path(&dir.path().join("demo"));
        assert!(store.root.to_string_lossy().ends_with("demo.transcriptor"));
        let mut p = Project::new("demo");
        p.revision = 7;
        store.save(&p).unwrap();
        let back = store.load().unwrap();
        assert_eq!(back, p);
        // ningún temporal residual
        let leftovers: Vec<_> =
            fs::read_dir(&store.root).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());
        // un archivo parcialmente escrito no se lee como proyecto
        fs::write(store.project_path(), b"{\"schema\": \"transcriptor-pro").unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn journal_is_append_only_and_tolerates_truncated_tail() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(dir.path().join("x.transcriptor"));
        let ev = JournalEvent {
            at: "t".into(),
            command_id: "c1".into(),
            actor: crate::session::Actor::Human,
            kind: "command".into(),
            label: "l".into(),
            base_revision: 0,
            new_revision: 1,
            idempotency_key: None,
            diff: Default::default(),
            command: None,
            receipt: None,
        };
        store.append_journal(std::slice::from_ref(&ev)).unwrap();
        store.append_journal(std::slice::from_ref(&ev)).unwrap();
        let mut f = fs::OpenOptions::new().append(true).open(store.journal_path()).unwrap();
        f.write_all(b"{\"at\":\"trunc").unwrap();
        assert_eq!(store.read_journal().unwrap().len(), 1);
        drop(f);
        store.append_journal(std::slice::from_ref(&ev)).unwrap();
        assert!(fs::read(store.journal_path()).unwrap().ends_with(b"\n"));
        fs::write(store.journal_path(), b"{bad}\n").unwrap();
        assert!(store.read_journal().is_err());
    }

    #[test]
    fn recovery_finishes_each_commit_boundary_once_and_rejects_external_writes() {
        for boundary in 0..4 {
            let dir = tempfile::tempdir().unwrap();
            let store = ProjectStore::at(dir.path());
            let old = Project::new("old");
            store.save(&old).unwrap();
            let mut session = crate::ProjectSession::new(old.clone());
            session.execute(crate::CommandEnvelope::human(tv2_domain::Command::RenameProject { name: "new".into() })).unwrap();
            let next = session.project().clone();
            let intent = CommitIntent {
                schema: "transcriptor-commit/1".into(),
                before_digest: Some(ProjectStore::digest(&old).unwrap()),
                project: next.clone(),
                events: session.pending_journal().to_vec(),
                history: Some(session.history_snapshot()),
                audit: None,
            };
            atomic_write(&store.root.join(PENDING_FILE), &serde_json::to_vec(&intent).unwrap()).unwrap();
            if boundary >= 1 {
                atomic_write(&store.project_path(), &serde_json::to_vec(&next).unwrap()).unwrap();
            }
            if boundary >= 2 {
                atomic_write(&store.journal_path(), &store.merged_journal(&intent.events).unwrap()).unwrap();
            }
            if boundary >= 3 {
                atomic_write(&store.root.join("history.json"), &serde_json::to_vec(intent.history.as_ref().unwrap()).unwrap()).unwrap();
            }
            let reopened = ProjectStore::at(dir.path());
            assert_eq!(reopened.load().unwrap(), next);
            assert_eq!(reopened.read_journal().unwrap().len(), 1);
            let mut restored = reopened.load_session().unwrap();
            restored.undo(crate::Actor::Human).unwrap();
            assert_eq!(restored.project().name, "old");
            assert_eq!(reopened.load().unwrap(), next);
            assert!(!store.root.join(PENDING_FILE).exists());
            // A writer ignoring our lock must be detected, never rolled back.
            atomic_write(&store.root.join(PENDING_FILE), &serde_json::to_vec(&intent).unwrap()).unwrap();
            let mut external = next;
            external.name = "external".into();
            atomic_write(&store.project_path(), &serde_json::to_vec(&external).unwrap()).unwrap();
            assert_eq!(reopened.load().unwrap_err().code, tv2_domain::ErrorCode::ExternalConflict);
            assert_eq!(ProjectStore::read_project(&store.project_path()).unwrap(), external);
        }
    }

    #[test]
    fn storage_commit_recovers_shadow_history_and_audit_at_every_boundary() {
        for boundary in 0..4 {
            let dir = tempfile::tempdir().unwrap();
            let store = ProjectStore::at(dir.path());
            let mut old = Project::new("old");
            let asset = tv2_domain::commands::tests_support::fake_video("a", 30);
            old.assets.push(asset.clone());
            store.save(&old).unwrap();
            let document: tv2_domain::evidence::EvidenceDocument = serde_json::json!({"schema":"editorial-master/1","media":{"fingerprint":asset.fingerprint,"duration":30.0},"tracks":{},"opaque_analysis":"x".repeat(100*1024)}).into();
            let master = tv2_domain::evidence::MasterEvidence {
                asset_id: asset.id,
                source_digest: document.source_digest().into(),
                document,
                source_bundle: None,
            };
            let request = crate::CommandEnvelope::human(tv2_domain::Command::AttachMaster { master }).with_idempotency("pending-storage");
            let mut session = crate::ProjectSession::new(old.clone());
            session.execute(request.clone()).unwrap();
            let audit = crate::audit::prepare(&store.root, session.pending_journal()).unwrap();
            let mut writer = crate::storage_codec::Writer::new(&store.root);
            let project = writer.project(session.project()).unwrap();
            let history = writer.history(&session.history_snapshot()).unwrap();
            let intent = CommitIntent {
                schema: "transcriptor-commit/2".into(),
                before_digest: Some(ProjectStore::digest(&old).unwrap()),
                project,
                events: Vec::new(),
                history: Some(history),
                audit: Some(audit),
            };
            atomic_write(&store.root.join(PENDING_FILE), &serde_json::to_vec(&intent).unwrap()).unwrap();
            if boundary >= 1 {
                atomic_write(
                    &store.project_path(),
                    &serde_json::to_vec(&serde_json::json!({"schema":crate::storage_codec::PROJECT_STORAGE,"project":intent.project})).unwrap(),
                )
                .unwrap();
            }
            if boundary >= 2 {
                crate::audit::commit(&store.root, intent.audit.as_ref().unwrap()).unwrap();
            }
            if boundary >= 3 {
                let history = crate::history_codec::encode(intent.history.as_ref().unwrap()).unwrap();
                atomic_write(
                    &store.root.join("history.json"),
                    &serde_json::to_vec(&serde_json::json!({"schema":crate::storage_codec::HISTORY_STORAGE,"history":history})).unwrap(),
                )
                .unwrap();
            }
            let reopened = ProjectStore::at(dir.path());
            let mut restored = reopened.load_session().unwrap();
            assert_eq!(restored.project(), session.project());
            assert!(restored.execute(request).unwrap().replayed);
            restored.undo(crate::Actor::Human).unwrap();
            assert!(restored.project().masters.is_empty());
            assert_eq!(reopened.read_journal().unwrap().len(), 1);
            assert!(!store.root.join(PENDING_FILE).exists());
        }
    }

    #[test]
    fn invalid_recovery_does_not_change_observed_base_or_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(dir.path());
        let p = Project::new("valid");
        store.save(&p).unwrap();
        let mut bad = p.clone();
        bad.revision = 7;
        bad.sequences[0].frame_rate.num = 0;
        atomic_write(&store.root.join("autosave.json"), &serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(store.recovery_candidate(&p).is_err());
        let mut session = crate::ProjectSession::new(p.clone());
        assert!(session.recover(bad).is_err());
        assert_eq!(session.project(), &p);
        assert!(session.pending_journal().is_empty());
        store.save(&p).unwrap();
    }

    #[test]
    fn external_diff_revalidates_both_bases_and_is_undoable_then_saveable() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(dir.path());
        let p = Project::new("base");
        store.save(&p).unwrap();
        let mut session = crate::ProjectSession::new(p.clone());
        session.execute(crate::CommandEnvelope::human(tv2_domain::Command::RenameProject { name: "local".into() })).unwrap();
        let mut ext = p;
        ext.settings.snapping = false;
        ext.revision = 7;
        atomic_write(&store.project_path(), &serde_json::to_vec(&ext).unwrap()).unwrap();
        let change = store.external_change(session.project()).unwrap().unwrap();
        session.execute(crate::CommandEnvelope::human(tv2_domain::Command::RenameProject { name: "changed again".into() })).unwrap();
        assert!(store.accept_external(&mut session, change).is_err());
        let change = store.external_change(session.project()).unwrap().unwrap();
        ext.revision = 8;
        atomic_write(&store.project_path(), &serde_json::to_vec(&ext).unwrap()).unwrap();
        assert!(store.accept_external(&mut session, change).is_err());
        let change = store.external_change(session.project()).unwrap().unwrap();
        store.accept_external(&mut session, change).unwrap();
        assert_eq!(session.revision(), 9);
        assert_eq!(session.project().name, "changed again");
        assert!(!session.project().settings.snapping);
        session.undo(crate::Actor::Human).unwrap();
        assert!(session.project().settings.snapping);
        store.save_with_journal(session.project(), session.pending_journal()).unwrap();
        assert_eq!(store.load().unwrap(), *session.project());
    }
}
