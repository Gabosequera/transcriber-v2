//! Recoverable multi-document publication, restricted to V2-owned export folders.
//! Every base is checked before any publication; recovery never overwrites a third value.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    io::{Read, Write},
    path::{Component, Path},
};
use tv2_domain::evidence::SourceFile;
use tv2_domain::{DomainError, error::DomainResult};
const LIMIT: u64 = 128 * 1024 * 1024;
const OWNER: &str = ".transcriptor-export.json";
const INTENT: &str = ".pending-documents.json";
#[derive(Serialize, Deserialize)]
struct Entry {
    path: String,
    before: Option<String>,
    after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file: Option<SourceFile>,
}
#[derive(Serialize, Deserialize)]
struct Intent {
    schema: String,
    id: String,
    entries: Vec<Entry>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    directories: BTreeSet<String>,
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
    publish_with_files(root, documents, &BTreeMap::new())
}
pub fn publish_with_files(root: &Path, documents: &BTreeMap<String, String>, files: &BTreeMap<String, SourceFile>) -> DomainResult<()> {
    publish_with_directories(root, documents, files, &BTreeSet::new())
}
pub fn publish_with_directories(
    root: &Path,
    documents: &BTreeMap<String, String>,
    files: &BTreeMap<String, SourceFile>,
    directories: &BTreeSet<String>,
) -> DomainResult<()> {
    let _lock = lock(root)?;
    finish(root, None)?;
    validate_path_tree(documents.keys().chain(files.keys()).map(String::as_str), directories)?;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for directory in directories {
        let target = safe_path(root, directory)?;
        if !seen.insert(directory.to_ascii_lowercase()) || target.exists() && !target.is_dir() {
            return Err(DomainError::invalid("directorio duplicado o ocupado por un archivo"));
        }
    }
    for (path, after) in documents {
        if !seen.insert(path.to_ascii_lowercase()) {
            return Err(DomainError::invalid("rutas duplicadas en Windows"));
        }
        let target = safe_path(root, path)?;
        entries.push(Entry { path: path.clone(), before: read(&target)?.map(|text| digest(&text)), after: after.clone(), file: None });
    }
    for (path, file) in files {
        if !seen.insert(path.to_ascii_lowercase()) {
            return Err(DomainError::invalid("rutas duplicadas en publicación"));
        }
        let target = safe_path(root, path)?;
        verify_source(file)?;
        entries.push(Entry { path: path.clone(), before: file_digest(&target)?, after: String::new(), file: Some(file.clone()) });
    }
    let schema = if !directories.is_empty() {
        "transcriptor-documents/3"
    } else if files.is_empty() {
        "transcriptor-documents/1"
    } else {
        "transcriptor-documents/2"
    };
    let intent = Intent { schema: schema.into(), id: tv2_domain::ids::random_hex12(), entries, directories: directories.clone() };
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
// Validate the complete namespace before persisting an intent or writing any
// recovered entry. Checking individual paths does not detect a file used as a
// directory by a different entry, including Windows case aliases.
fn validate_path_tree<'a>(files: impl Iterator<Item = &'a str>, directories: &BTreeSet<String>) -> DomainResult<()> {
    let files: HashSet<_> = files.map(str::to_ascii_lowercase).collect();
    for path in files.iter().cloned().chain(directories.iter().map(|path| path.to_ascii_lowercase())) {
        for (index, _) in path.match_indices('/') {
            if files.contains(&path[..index]) {
                return Err(DomainError::invalid("un archivo de publicación no puede ser directorio de otra entrada"));
            }
        }
    }
    Ok(())
}
fn finish(root: &Path, stop_after: Option<usize>) -> DomainResult<()> {
    let Some(raw) = read(&root.join(INTENT))? else {
        return Ok(());
    };
    let intent: Intent = serde_json::from_str(&raw)?;
    if !matches!(intent.schema.as_str(), "transcriptor-documents/1" | "transcriptor-documents/2" | "transcriptor-documents/3")
        || !intent.id.bytes().all(|c| c.is_ascii_hexdigit())
        || intent.id.len() != 12
    {
        return Err(DomainError::invalid("intención inválida"));
    }
    validate_path_tree(intent.entries.iter().map(|entry| entry.path.as_str()), &intent.directories)?;
    let mut seen = HashSet::new();
    for directory in &intent.directories {
        let target = safe_path(root, directory)?;
        if !seen.insert(directory.to_ascii_lowercase()) || target.exists() && !target.is_dir() {
            return Err(DomainError::invalid("directorio duplicado o ocupado por un archivo"));
        }
    }
    for entry in &intent.entries {
        if !seen.insert(entry.path.to_ascii_lowercase()) {
            return Err(DomainError::invalid("rutas repetidas en intención"));
        }
        let target = safe_path(root, &entry.path)?;
        let disk = current_digest(entry, &target)?;
        if disk != entry.before && disk.as_deref() != Some(after_digest(entry).as_str()) {
            return Err(DomainError::precondition(format!("edición externa durante publicación: {}", entry.path)));
        }
        if disk.as_deref() != Some(after_digest(entry).as_str())
            && let Some(file) = &entry.file
        {
            verify_source(file)?;
        }
    }
    for directory in &intent.directories {
        fs::create_dir_all(safe_path(root, directory)?)?;
    }
    for (n, entry) in intent.entries.iter().enumerate() {
        if stop_after == Some(n) {
            return Err(DomainError::io("interrupción sintética"));
        }
        let path = safe_path(root, &entry.path)?;
        let disk = current_digest(entry, &path)?;
        let after = after_digest(entry);
        if disk != entry.before && disk.as_deref() != Some(&after) {
            return Err(DomainError::precondition("el documento cambió antes de publicar"));
        }
        if disk.as_deref() != Some(&after) {
            if let Some(file) = &entry.file {
                copy_verified(file, &path, &entry.before)?;
            } else {
                crate::store::atomic_write(&path, entry.after.as_bytes())?;
            }
        }
    }
    let receipt = serde_json::json!({"schema":"transcriptor-document-receipt/1","id":intent.id,"directories":intent.directories,"documents":intent.entries.iter().map(|e|serde_json::json!({"path":e.path,"digest":after_digest(e),"digest_kind":if e.file.is_some(){"sha256-bytes"}else{"canonical-json-string"}})).collect::<Vec<_>>()});
    crate::store::atomic_write(&root.join("receipts").join(format!("{}.json", intent.id)), &serde_json::to_vec(&receipt)?)?;
    fs::remove_file(root.join(INTENT))?;
    Ok(())
}

fn file_digest(path: &Path) -> DomainResult<Option<String>> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(Some(hex::encode(hasher.finalize())))
}
pub(crate) fn verify_source(file: &SourceFile) -> DomainResult<()> {
    let meta = fs::symlink_metadata(&file.source)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != file.size || file_digest(&file.source)?.as_deref() != Some(&file.sha256) {
        return Err(DomainError::precondition(format!("archivo de origen cambió o no está disponible: {}", file.source.display())));
    }
    Ok(())
}
fn current_digest(entry: &Entry, path: &Path) -> DomainResult<Option<String>> {
    if entry.file.is_some() { file_digest(path) } else { Ok(read(path)?.map(|text| digest(&text))) }
}
fn after_digest(entry: &Entry) -> String {
    entry.file.as_ref().map(|f| f.sha256.clone()).unwrap_or_else(|| digest(&entry.after))
}
pub(crate) fn copy_verified(file: &SourceFile, target: &Path, before: &Option<String>) -> DomainResult<()> {
    let parent = target.parent().ok_or_else(|| DomainError::invalid("destino sin carpeta"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut source = fs::File::open(&file.source)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut size = 0u64;
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        temporary.write_all(&buffer[..n])?;
        hasher.update(&buffer[..n]);
        size += n as u64;
    }
    if size != file.size || hex::encode(hasher.finalize()) != file.sha256 {
        return Err(DomainError::precondition("origen cambió durante la copia"));
    }
    temporary.as_file().sync_all()?;
    let current = file_digest(target)?;
    if &current != before && current.as_deref() != Some(&file.sha256) {
        return Err(DomainError::precondition("destino cambió durante la copia"));
    }
    temporary.persist(target).map_err(|e| DomainError::io(e.to_string()))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_file_ancestors_before_persisting_or_recovering_any_output() {
        for directories in [BTreeSet::new(), BTreeSet::from(["TREE/nested".into()])] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("output");
            create(&root).unwrap();
            let mut documents = BTreeMap::from([("tree".into(), "parent".into())]);
            if directories.is_empty() {
                documents.insert("TREE/child.json".into(), "child".into());
            }
            assert!(publish_with_directories(&root, &documents, &BTreeMap::new(), &directories).is_err());
            assert!(!root.join(INTENT).exists(), "invalid request must not block subsequent publication");
            assert!(!root.join("tree").exists(), "no partial output from invalid request");
            let intent = Intent {
                schema: "transcriptor-documents/3".into(),
                id: "0123456789ab".into(),
                directories,
                entries: documents.into_iter().map(|(path, after)| Entry { path, before: None, after, file: None }).collect(),
            };
            let bytes = serde_json::to_vec(&intent).unwrap();
            crate::store::atomic_write(&root.join(INTENT), &bytes).unwrap();
            assert!(recover(&root).is_err());
            assert!(!root.join("tree").exists());
            assert_eq!(fs::read(root.join(INTENT)).unwrap(), bytes);
        }
    }
    #[test]
    fn empty_directories_recover_with_documents_and_reject_path_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("output");
        create(&root).unwrap();
        let intent = Intent {
            schema: "transcriptor-documents/3".into(),
            id: "0123456789ab".into(),
            directories: BTreeSet::from(["empty/nested".into()]),
            entries: vec![Entry { path: "master.json".into(), before: None, after: "{}".into(), file: None }],
        };
        crate::store::atomic_write(&root.join(INTENT), &serde_json::to_vec(&intent).unwrap()).unwrap();
        assert!(finish(&root, Some(0)).is_err());
        assert!(root.join("empty/nested").is_dir());
        assert!(!root.join("master.json").exists());
        recover(&root).unwrap();
        recover(&root).unwrap();
        assert_eq!(fs::read_to_string(root.join("master.json")).unwrap(), "{}");
        assert!(publish_with_directories(&root, &BTreeMap::new(), &BTreeMap::new(), &BTreeSet::from(["../escape".into()])).is_err());
        assert!(publish_with_directories(&root, &BTreeMap::new(), &BTreeMap::new(), &BTreeSet::from(["master.json".into()])).is_err());
    }
    #[test]
    fn binary_publication_recovers_boundaries_and_preserves_changed_sources_and_targets() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.bin");
        let bytes: Vec<u8> = (0..200000).map(|n| (n % 256) as u8).collect();
        fs::write(&source, &bytes).unwrap();
        let file = SourceFile { source: source.clone(), size: bytes.len() as u64, sha256: file_digest(&source).unwrap().unwrap() };
        for boundary in 0..=2 {
            let root = temp.path().join(format!("output-{boundary}"));
            create(&root).unwrap();
            let intent = Intent {
                schema: "transcriptor-documents/2".into(),
                id: "0123456789ab".into(),
                directories: Default::default(),
                entries: vec![
                    Entry { path: "master.json".into(), before: None, after: "{}".into(), file: None },
                    Entry { path: "tracks/A/audio.bin".into(), before: None, after: String::new(), file: Some(file.clone()) },
                ],
            };
            crate::store::atomic_write(&root.join(INTENT), &serde_json::to_vec(&intent).unwrap()).unwrap();
            let _ = finish(&root, Some(boundary));
            recover(&root).unwrap();
            recover(&root).unwrap();
            assert_eq!(fs::read(root.join("tracks/A/audio.bin")).unwrap(), bytes);
            assert!(!root.join(INTENT).exists());
        }
        let root = temp.path().join("changed");
        create(&root).unwrap();
        fs::write(&source, b"changed source").unwrap();
        assert!(
            publish_with_files(&root, &BTreeMap::from([("master.json".into(), "{}".into())]), &BTreeMap::from([("audio.bin".into(), file.clone())]))
                .is_err()
        );
        assert!(!root.join("master.json").exists());
        assert!(!root.join(INTENT).exists());
        fs::write(&source, &bytes).unwrap();
        let intent = Intent {
            schema: "transcriptor-documents/2".into(),
            id: "0123456789ab".into(),
            directories: Default::default(),
            entries: vec![Entry { path: "audio.bin".into(), before: None, after: String::new(), file: Some(file) }],
        };
        crate::store::atomic_write(&root.join(INTENT), &serde_json::to_vec(&intent).unwrap()).unwrap();
        fs::write(root.join("audio.bin"), b"external edit").unwrap();
        assert!(recover(&root).is_err());
        assert_eq!(fs::read(root.join("audio.bin")).unwrap(), b"external edit");
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }
    #[test]
    fn multi_document_recovery_all_boundaries_and_external_conflict() {
        for stop in 0..=3 {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("output");
            create(&root).unwrap();
            let entries =
                (0..3).map(|n| Entry { path: format!("views/{n}.json"), before: None, after: format!("{{\"n\":{n}}}"), file: None }).collect();
            let intent = Intent { schema: "transcriptor-documents/1".into(), id: "0123456789ab".into(), entries, directories: Default::default() };
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
            directories: Default::default(),
            entries: vec![
                Entry { path: "a.json".into(), before: None, after: "new".into(), file: None },
                Entry { path: "b.json".into(), before: None, after: "other".into(), file: None },
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
