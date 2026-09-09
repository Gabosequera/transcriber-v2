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

#[derive(serde::Serialize, serde::Deserialize)]
struct CommitIntent {
    schema: String,
    before_digest: Option<String>,
    project: Project,
    events: Vec<JournalEvent>,
}

#[derive(Clone, Debug)]
pub struct ProjectStore {
    pub root: PathBuf,
    observed: Arc<Mutex<Option<String>>>,
    baseline: Arc<Mutex<Option<Project>>>,
}

impl ProjectStore {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        ProjectStore { root: root.into(), observed: Arc::new(Mutex::new(None)), baseline: Arc::new(Mutex::new(None)) }
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
        project.validate()?;
        let text = serde_json::to_string_pretty(project)? + "\n";
        if text.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("proyecto supera el límite de 64 MiB"));
        }
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
        self.merged_journal(events)?;
        let intent =
            CommitIntent { schema: "transcriptor-commit/1".into(), before_digest: disk_digest, project: project.clone(), events: events.to_vec() };
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
        if self.root.join(PENDING_FILE).exists() {
            let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
            lock.try_lock().map_err(|e| DomainError::io(format!("proyecto ocupado: {e}")))?;
            self.finish_pending()?;
        }
        let path = self.project_path();
        let project = Self::read_project(&path)?;
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
        for p in [&mut base, &mut external] {
            for a in &mut p.assets {
                a.path = self.resolve_path(&a.path).to_string_lossy().replace('\\', "/");
            }
        }
        let mut change = crate::reconcile::prepare(&base, local, &external)?;
        change.external_digest = disk_digest;
        Ok(Some(change))
    }

    /// Explicit approval, revalidating both sides. Disk stays untouched until
    /// the next save; the external version becomes the CAS baseline only on success.
    pub fn accept_external(&self, session: &mut crate::ProjectSession, change: crate::reconcile::ExternalChange) -> DomainResult<()> {
        if session.revision() != change.base_revision || session.digest()? != change.local_digest {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el proyecto cambió mientras se revisaba el diff"));
        }
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
        lock.try_lock().map_err(|e| DomainError::io(format!("proyecto ocupado: {e}")))?;
        let external = Self::read_project(&self.project_path())?;
        if Self::digest(&external)? != change.external_digest {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el archivo externo cambió mientras se revisaba el diff"));
        }
        let mut observed = self.observed.lock().map_err(|_| DomainError::io("estado de persistencia no disponible"))?;
        let mut baseline = self.baseline.lock().map_err(|_| DomainError::io("baseline no disponible"))?;
        session.execute(
            crate::CommandEnvelope::human(tv2_domain::Command::ReconcileProject { project: Box::new(change.merged) })
                .with_base(change.base_revision)
                .with_actor(crate::Actor::External { source: "project.json".into() }),
        )?;
        *observed = Some(change.external_digest);
        *baseline = Some(external);
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
        let intent: CommitIntent = serde_json::from_slice(&bytes)?;
        if intent.schema != "transcriptor-commit/1" {
            return Err(DomainError::unsupported("schema de transacción desconocido"));
        }
        intent.project.validate()?;
        let next = Self::digest(&intent.project)?;
        let disk = if self.project_path().exists() { Some(Self::digest(&Self::read_project(&self.project_path())?)?) } else { None };
        if disk != intent.before_digest && disk.as_ref() != Some(&next) {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el archivo cambió durante una transacción pendiente"));
        }
        let journal = self.merged_journal(&intent.events)?;
        let project_bytes = serde_json::to_vec_pretty(&intent.project)?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(DomainError::invalid("proyecto demasiado grande"));
        }
        if disk.as_ref() != Some(&next) {
            atomic_write(&self.project_path(), &project_bytes)?;
        }
        atomic_write(&self.journal_path(), &journal)?;
        fs::remove_file(path)?;
        Ok(())
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
        let project: Project =
            serde_json::from_str(text).map_err(|e| DomainError::invalid(format!("{} no es un proyecto válido: {e}", path.display())))?;
        if project.schema != PROJECT_SCHEMA {
            return Err(DomainError::unsupported(format!("schema de proyecto desconocido: {}", project.schema))
                .with_action(format!("este editor entiende {PROJECT_SCHEMA}")));
        }
        project.validate()?;
        Ok(project)
    }

    /// A recovery snapshot is a candidate, never silently replaces user data.
    pub fn recovery_candidate(&self, saved: &Project) -> DomainResult<Option<Project>> {
        let path = self.root.join("autosave.json");
        if !path.is_file() {
            return Ok(None);
        }
        let candidate = Self::read_project(&path)?;
        if candidate.project_id != saved.project_id || candidate.revision <= saved.revision {
            return Ok(None);
        }
        Ok(Some(candidate))
    }

    pub fn append_journal(&self, events: &[JournalEvent]) -> DomainResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.root)?;
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.root.join(".write.lock"))?;
        lock.try_lock().map_err(|e| DomainError::io(format!("proyecto ocupado: {e}")))?;
        self.finish_pending()?;
        atomic_write(&self.journal_path(), &self.merged_journal(events)?)?;
        Ok(())
    }

    pub fn read_journal(&self) -> DomainResult<Vec<JournalEvent>> {
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

    /// Ruta portable relativa a la carpeta del proyecto si es posible.
    pub fn portable_path(&self, path: &Path) -> String {
        let rel = self.root.parent().and_then(|base| path.strip_prefix(base).ok());
        let p = rel.map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf());
        p.to_string_lossy().replace('\\', "/")
    }

    /// Resuelve una ruta portable guardada en el proyecto.
    pub fn resolve_path(&self, portable: &str) -> PathBuf {
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
        for boundary in 0..3 {
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
            };
            atomic_write(&store.root.join(PENDING_FILE), &serde_json::to_vec(&intent).unwrap()).unwrap();
            if boundary >= 1 {
                atomic_write(&store.project_path(), &serde_json::to_vec(&next).unwrap()).unwrap();
            }
            if boundary >= 2 {
                atomic_write(&store.journal_path(), &store.merged_journal(&intent.events).unwrap()).unwrap();
            }
            let reopened = ProjectStore::at(dir.path());
            assert_eq!(reopened.load().unwrap(), next);
            assert_eq!(reopened.read_journal().unwrap().len(), 1);
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
