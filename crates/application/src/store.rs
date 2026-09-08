//! Persistencia de proyecto: carpeta `<nombre>.transcriptor/` con
//! `project.json` (contrato, escritura atómica: tmp + fsync + rename) y
//! `journal.jsonl` (auditoría append-only). Las cachés van en `cache/`.
//!
//! Política de durabilidad (E1): un solo documento autoritativo → el rename es
//! la frontera atómica. El journal se escribe **después** del rename; si el
//! proceso muere entre ambos, el proyecto es coherente y el journal solo pierde
//! la última línea (se reconstruye la revisión desde `project.json`). Los
//! documentos V1 todavía requieren un manifiesto transaccional para completar
//! atomicidad multidocumento en E3. El lock/CAS actual protege escritores V2;
//! no convierte project.json y journal.jsonl en una transacción única.

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

#[derive(Clone, Debug)]
pub struct ProjectStore {
    pub root: PathBuf,
    observed: Arc<Mutex<Option<String>>>,
}

impl ProjectStore {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        ProjectStore { root: root.into(), observed: Arc::new(Mutex::new(None)) }
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
        let mut observed = self.observed.lock().map_err(|_| DomainError::io("estado de persistencia no disponible"))?;
        let disk_digest = match Self::read_project(&self.project_path()) {
            Ok(disk) => Some(Self::digest(&disk)?),
            Err(e) if !self.project_path().exists() && e.code == tv2_domain::ErrorCode::Io => None,
            Err(e) => return Err(e),
        };
        if disk_digest != *observed {
            return Err(DomainError::new(tv2_domain::ErrorCode::ExternalConflict, "el proyecto en disco cambió desde su apertura")
                .with_action("abre la versión externa o guarda tus cambios en otra carpeta"));
        }
        atomic_write(&self.project_path(), text.as_bytes())?;
        *observed = Some(Self::digest(project)?);
        Ok(())
    }

    pub fn load(&self) -> DomainResult<Project> {
        let path = self.project_path();
        let project = Self::read_project(&path)?;
        *self.observed.lock().map_err(|_| DomainError::io("estado de persistencia no disponible"))? = Some(Self::digest(&project)?);
        Ok(project)
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
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.journal_path())?;
        for e in events {
            let line = serde_json::to_string(e)?;
            f.write_all(line.as_bytes())?;
            f.write_all(b"\n")?;
        }
        f.flush()?;
        Ok(())
    }

    pub fn read_journal(&self) -> DomainResult<Vec<JournalEvent>> {
        let path = self.journal_path();
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path)?;
        let mut out = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<JournalEvent>(line) {
                Ok(e) => out.push(e),
                Err(_) => break, // línea final truncada: el resto no es fiable
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
        assert_eq!(store.read_journal().unwrap().len(), 2);
    }
}
