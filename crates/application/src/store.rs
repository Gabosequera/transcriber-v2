//! Persistencia de proyecto: carpeta `<nombre>.transcriptor/` con
//! `project.json` (contrato, escritura atómica: tmp + fsync + rename) y
//! `journal.jsonl` (auditoría append-only). Las cachés van en `cache/`.
//!
//! Política de durabilidad (E1): un solo documento autoritativo → el rename es
//! la frontera atómica. El journal se escribe **después** del rename; si el
//! proceso muere entre ambos, el proyecto es coherente y el journal solo pierde
//! la última línea (se reconstruye la revisión desde `project.json`). Los
//! documentos V1 (E3) usan el mismo escritor atómico por archivo y un manifiesto
//! de transacción para la atomicidad lógica multidocumento.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tv2_domain::error::{DomainError, DomainResult};
use tv2_domain::project::{PROJECT_SCHEMA, Project};

use crate::session::JournalEvent;

pub const PROJECT_FILE: &str = "project.json";
pub const JOURNAL_FILE: &str = "journal.jsonl";
pub const PROJECT_DIR_SUFFIX: &str = ".transcriptor";

#[derive(Clone, Debug)]
pub struct ProjectStore {
    pub root: PathBuf,
}

impl ProjectStore {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        ProjectStore { root: root.into() }
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
        ProjectStore { root: p }
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
        fs::create_dir_all(&self.root).map_err(|e| DomainError::io(format!("no se pudo crear {}: {e}", self.root.display())))?;
        let text = serde_json::to_string_pretty(project)? + "\n";
        atomic_write(&self.project_path(), text.as_bytes())
    }

    pub fn load(&self) -> DomainResult<Project> {
        let path = self.project_path();
        let bytes = fs::read(&path).map_err(|e| {
            DomainError::io(format!("no se pudo leer {}: {e}", path.display())).with_action("comprueba que la carpeta del proyecto existe")
        })?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let project: Project =
            serde_json::from_str(text).map_err(|e| DomainError::invalid(format!("{} no es un proyecto válido: {e}", path.display())))?;
        if project.schema != PROJECT_SCHEMA {
            return Err(DomainError::unsupported(format!("schema de proyecto desconocido: {}", project.schema))
                .with_action(format!("este editor entiende {PROJECT_SCHEMA}")));
        }
        Ok(project)
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
