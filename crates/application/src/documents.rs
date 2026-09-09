//! Recoverable multi-document publication, restricted to V2-owned export folders.
//! Every base is checked before any publication; recovery never overwrites a third value.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Component, Path},
};
use tv2_domain::{DomainError, error::DomainResult};
const LIMIT: u64 = 128 * 1024 * 1024;
const OWNER: &str = ".transcriptor-export.json";
const INTENT: &str = ".pending-documents.json";
#[derive(Serialize, Deserialize)]
struct Entry {
    path: String,
    before: Option<String>,
    after: String,
}
#[derive(Serialize, Deserialize)]
struct Intent {
    schema: String,
    id: String,
    entries: Vec<Entry>,
}

fn safe_path(root: &Path, path: &str) -> DomainResult<std::path::PathBuf> {
    if path.is_empty() || path.contains('\\') || path.len() > 240 || path.split('/').any(|part| matches!(part, "" | "." | "..")) {
        return Err(DomainError::invalid("ruta de documento inválida"));
    }
    let mut out = root.to_path_buf();
    for part in Path::new(path).components() {
        let Component::Normal(name) = part else {
            return Err(DomainError::invalid("ruta fuera de la carpeta"));
        };
        let text = name.to_string_lossy();
        let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
        if text.ends_with(['.', ' '])
            || text.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c))
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.starts_with("COM") || stem.starts_with("LPT")) && stem[3..].parse::<u32>().is_ok_and(|n| (1..=9).contains(&n))
        {
            return Err(DomainError::invalid("nombre no portable"));
        }
        out.push(name);
        if let Ok(meta) = fs::symlink_metadata(&out)
            && meta.file_type().is_symlink()
        {
            return Err(DomainError::invalid("enlace no permitido en publicación"));
        }
    }
    if [OWNER, INTENT, ".write.lock", "receipts"]
        .iter()
        .any(|reserved| path.eq_ignore_ascii_case(reserved) || path.to_ascii_lowercase().starts_with(&format!("{reserved}/")))
    {
        return Err(DomainError::invalid("ruta reservada"));
    }
    Ok(out)
}
fn read(path: &Path) -> DomainResult<Option<String>> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(LIMIT + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > LIMIT {
                return Err(DomainError::invalid("documento excede 128 MiB"));
            }
            Ok(Some(String::from_utf8(bytes).map_err(|_| DomainError::invalid("documento no es UTF-8"))?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn digest(text: &str) -> String {
    tv2_domain::digest::digest_json(&serde_json::json!(text))
}
fn lock(root: &Path) -> DomainResult<fs::File> {
    let meta = fs::symlink_metadata(root)?;
    if meta.file_type().is_symlink() {
        return Err(DomainError::invalid("carpeta de exportación enlazada"));
    }
    for internal in [OWNER, INTENT, ".write.lock", "receipts"] {
        if fs::symlink_metadata(root.join(internal)).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(DomainError::invalid("enlace en metadatos de publicación"));
        }
    }
    let owner: serde_json::Value =
        serde_json::from_str(&read(&root.join(OWNER))?.ok_or_else(|| DomainError::precondition("solo se publican carpetas creadas por V2"))?)?;
    if owner["schema"] != "transcriptor-export/1" {
        return Err(DomainError::precondition("carpeta sin propietario V2 válido"));
    }
    let file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join(".write.lock"))?;
    file.try_lock().map_err(|e| DomainError::io(format!("exportación ocupada: {e}")))?;
    Ok(file)
}
pub fn create(root: &Path) -> DomainResult<()> {
    fs::create_dir(root)?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(root.join(OWNER))?;
    file.write_all(b"{\"schema\":\"transcriptor-export/1\"}")?;
    file.sync_all()?;
    Ok(())
}
pub fn publish(root: &Path, documents: &BTreeMap<String, String>) -> DomainResult<()> {
    let _lock = lock(root)?;
    finish(root, None)?;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for (path, after) in documents {
        if !seen.insert(path.to_ascii_lowercase()) {
            return Err(DomainError::invalid("rutas duplicadas en Windows"));
        }
        let target = safe_path(root, path)?;
        entries.push(Entry { path: path.clone(), before: read(&target)?.map(|text| digest(&text)), after: after.clone() });
    }
    let intent = Intent { schema: "transcriptor-documents/1".into(), id: tv2_domain::ids::random_hex12(), entries };
    let bytes = serde_json::to_vec(&intent)?;
    if bytes.len() as u64 > LIMIT {
        return Err(DomainError::invalid("transacción excede 128 MiB"));
    }
    crate::store::atomic_write(&root.join(INTENT), &bytes)?;
    finish(root, None)
}
pub fn recover(root: &Path) -> DomainResult<()> {
    let _lock = lock(root)?;
    finish(root, None)
}
fn finish(root: &Path, stop_after: Option<usize>) -> DomainResult<()> {
    let Some(raw) = read(&root.join(INTENT))? else {
        return Ok(());
    };
    let intent: Intent = serde_json::from_str(&raw)?;
    if intent.schema != "transcriptor-documents/1" || !intent.id.bytes().all(|c| c.is_ascii_hexdigit()) || intent.id.len() != 12 {
        return Err(DomainError::invalid("intención inválida"));
    }
    let mut seen = HashSet::new();
    for entry in &intent.entries {
        if !seen.insert(entry.path.to_ascii_lowercase()) {
            return Err(DomainError::invalid("rutas repetidas en intención"));
        }
        let target = safe_path(root, &entry.path)?;
        let disk = read(&target)?.map(|text| digest(&text));
        if disk != entry.before && disk.as_deref() != Some(digest(&entry.after).as_str()) {
            return Err(DomainError::precondition(format!("edición externa durante publicación: {}", entry.path)));
        }
    }
    for (n, entry) in intent.entries.iter().enumerate() {
        if stop_after == Some(n) {
            return Err(DomainError::io("interrupción sintética"));
        }
        let path = safe_path(root, &entry.path)?;
        let disk = read(&path)?.map(|text| digest(&text));
        let after = digest(&entry.after);
        if disk != entry.before && disk.as_deref() != Some(&after) {
            return Err(DomainError::precondition("el documento cambió antes de publicar"));
        }
        if disk.as_deref() != Some(&after) {
            crate::store::atomic_write(&path, entry.after.as_bytes())?;
        }
    }
    let receipt = serde_json::json!({"schema":"transcriptor-document-receipt/1","id":intent.id,"documents":intent.entries.iter().map(|e|serde_json::json!({"path":e.path,"digest":digest(&e.after)})).collect::<Vec<_>>()});
    crate::store::atomic_write(&root.join("receipts").join(format!("{}.json", intent.id)), &serde_json::to_vec(&receipt)?)?;
    fs::remove_file(root.join(INTENT))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multi_document_recovery_all_boundaries_and_external_conflict() {
        for stop in 0..=3 {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("output");
            create(&root).unwrap();
            let entries = (0..3).map(|n| Entry { path: format!("views/{n}.json"), before: None, after: format!("{{\"n\":{n}}}") }).collect();
            let intent = Intent { schema: "transcriptor-documents/1".into(), id: "0123456789ab".into(), entries };
            crate::store::atomic_write(&root.join(INTENT), &serde_json::to_vec(&intent).unwrap()).unwrap();
            let _ = finish(&root, Some(stop));
            recover(&root).unwrap();
            recover(&root).unwrap();
            assert!(!root.join(INTENT).exists());
            for n in 0..3 {
                assert_eq!(read(&root.join(format!("views/{n}.json"))).unwrap().unwrap(), format!("{{\"n\":{n}}}"));
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("output");
        create(&root).unwrap();
        let intent = Intent {
            schema: "transcriptor-documents/1".into(),
            id: "0123456789ab".into(),
            entries: vec![
                Entry { path: "a.json".into(), before: None, after: "new".into() },
                Entry { path: "b.json".into(), before: None, after: "other".into() },
            ],
        };
        crate::store::atomic_write(&root.join(INTENT), &serde_json::to_vec(&intent).unwrap()).unwrap();
        fs::write(root.join("b.json"), "external").unwrap();
        assert!(recover(&root).is_err());
        assert!(!root.join("a.json").exists());
        assert!(root.join(INTENT).exists());
    }
    #[test]
    fn refuses_v1_directories_traversal_reserved_names_and_case_aliases() {
        let temp = tempfile::tempdir().unwrap();
        assert!(publish(temp.path(), &BTreeMap::new()).is_err());
        let root = temp.path().join("owned");
        create(&root).unwrap();
        for path in ["../a", "C:/a", "a:stream", "CON.json", "a./b", ".pending-documents.json", "views/./a.json", "views//a.json"] {
            assert!(publish(&root, &BTreeMap::from([(path.into(), "x".into())])).is_err(), "{path}");
        }
        assert!(publish(&root, &BTreeMap::from([("views/A.json".into(), "a".into()), ("views/a.json".into(), "b".into())])).is_err());
        assert!(!root.join(INTENT).exists());
    }
}
