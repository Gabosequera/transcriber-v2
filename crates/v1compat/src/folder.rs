//! Immutable editorial-folder snapshots and explicit exports into V2 copies.
use crate::{V1Result, invalid};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io::{BufRead, Read},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tv2_domain::{
    AssetId, LayerKind, Project, SequenceId,
    evidence::{SourceBundle, SourceFile},
};

const LIMIT: usize = 32 * 1024 * 1024;
pub fn validate_path(path: &str) -> V1Result<()> {
    if path.len() > 230
        || path.split('/').any(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            part.is_empty()
                || matches!(part, "." | "..")
                || part.ends_with(['.', ' '])
                || part.chars().any(|c| c < ' ' || "\\<>:\"|?*".contains(c))
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.starts_with("COM") || stem.starts_with("LPT")) && stem[3..].parse::<u8>().is_ok_and(|n| (1..=9).contains(&n))
        })
    {
        return Err(invalid(format!("ruta documental no portable: {path}")));
    }
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> V1Result<()> {
    if cancel.load(Ordering::Relaxed) { Err(invalid("captura de carpeta cancelada; no se importó parcialmente")) } else { Ok(()) }
}

fn walk(root: &Path, cancel: &AtomicBool, visit: &mut dyn FnMut(&Path, String, bool) -> V1Result<()>) -> V1Result<()> {
    let owned = std::fs::read_to_string(root.join(".transcriptor-export.json"))
        .ok()
        .and_then(|s| parse(&s).ok())
        .is_some_and(|v| v["schema"] == "transcriptor-export/1");
    let mut pending = vec![root.to_path_buf()];
    let mut seen = HashSet::new();
    while let Some(dir) = pending.pop() {
        check_cancel(cancel)?;
        if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
            return Err(invalid("carpeta enlazada no admitida"));
        }
        for entry in std::fs::read_dir(&dir)? {
            check_cancel(cancel)?;
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid("ruta fuera de origen"))?
                .to_str()
                .ok_or_else(|| invalid("ruta no Unicode"))?
                .replace('\\', "/");
            // These are publication mechanics, never V1 authoritative documents.
            if owned && matches!(relative.as_str(), ".transcriptor-export.json" | ".write.lock" | "receipts") {
                continue;
            }
            if relative == ".pending-documents.json" {
                return Err(invalid("recupera primero la publicación V2 interrumpida"));
            }
            validate_path(&relative)?;
            if !seen.insert(relative.to_lowercase()) {
                return Err(invalid("rutas documentales repetidas"));
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(invalid(format!("enlace no permitido: {relative}")));
            }
            if kind.is_dir() {
                visit(&path, relative, true)?;
                pending.push(path);
                continue;
            }
            if !kind.is_file() {
                return Err(invalid(format!("entrada no documental: {relative}")));
            }
            visit(&path, relative, false)?;
        }
    }
    Ok(())
}

fn authoritative(path: &str, master_path: &str) -> bool {
    path == master_path
        || (path.ends_with(".json")
            && (path.starts_with("layers/")
                || path.starts_with("views/")
                || path.starts_with(".work/manifests/")
                || path == ".work/chunks.selected.json"
                || path.ends_with(".marcas.json")))
}
fn digest_file(path: &Path, cancel: &AtomicBool, mut bytes: Option<&mut Vec<u8>>, limit: usize) -> V1Result<SourceFile> {
    let mut reader = std::fs::File::open(path)?;
    let before = reader.metadata()?;
    let mut hasher = Sha256::new();
    let mut block = [0u8; 65536];
    let mut count = 0u64;
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut block)?;
        if n == 0 {
            break;
        }
        hasher.update(&block[..n]);
        count = count.checked_add(n as u64).ok_or_else(|| invalid("tamaño de archivo no representable"))?;
        if let Some(bytes) = &mut bytes {
            if count > limit as u64 {
                return Err(invalid("archivo creció y excedió la reserva de captura; reintenta la importación"));
            }
            bytes.extend_from_slice(&block[..n]);
        }
    }
    let after = reader.metadata()?;
    if before.len() != count || after.len() != count || before.modified().ok() != after.modified().ok() {
        return Err(invalid("archivo cambió durante captura"));
    }
    Ok(SourceFile { source: std::path::absolute(path)?, size: count, sha256: hex::encode(hasher.finalize()) })
}
/// Embedded JSON is a bounded cache. Large authoritative/auxiliary documents and
/// binaries become content-addressed attachments; Save/Save As takes ownership.
/// Directory enumeration and hashing are incremental and have no 20000-entry cap.
pub fn snapshot(root: &Path, master_path: String) -> V1Result<SourceBundle> {
    snapshot_with_cancel(root, master_path, &AtomicBool::new(false))
}
pub fn snapshot_with_cancel(root: &Path, master_path: String, cancel: &AtomicBool) -> V1Result<SourceBundle> {
    let mut docs = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut authority_bytes = 0usize;
    let mut auxiliary_bytes = 0usize;
    walk(root, cancel, &mut |path, relative, directory| {
        if directory {
            directories.insert(relative);
            return Ok(());
        }
        let authority = authoritative(&relative, &master_path);
        let size = std::fs::metadata(path)?.len();
        let text = path.extension().is_some_and(|e| {
            matches!(
                e.to_str().unwrap_or("").to_ascii_lowercase().as_str(),
                "json" | "md" | "txt" | "csv" | "tsv" | "yaml" | "yml" | "xml" | "srt" | "vtt"
            )
        });
        let embed = if authority {
            size <= 65536 && authority_bytes + size as usize <= LIMIT
        } else {
            text && size <= 65536 && auxiliary_bytes + size as usize <= 1024 * 1024
        };
        let mut bytes = Vec::new();
        let file = digest_file(path, cancel, embed.then_some(&mut bytes), if authority { LIMIT - authority_bytes } else { 65536 })?;
        if embed {
            if authority {
                authority_bytes += bytes.len();
            } else {
                auxiliary_bytes += bytes.len();
            }
            match String::from_utf8(bytes) {
                Ok(text) => {
                    docs.insert(relative, text);
                }
                Err(_) if !authority => {
                    files.insert(relative, file);
                }
                Err(_) => return Err(invalid(format!("JSON autoritativo no UTF-8: {relative}"))),
            }
        } else {
            files.insert(relative, file);
        }
        Ok(())
    })?;
    if !docs.contains_key(&master_path) && !files.contains_key(&master_path) {
        return Err(invalid("snapshot sin master"));
    }
    Ok(SourceBundle::new(master_path, docs).with_files(files).with_directories(directories))
}

struct JsonReader<'a> {
    file: std::fs::File,
    cancel: &'a AtomicBool,
    hash: Sha256,
    count: u64,
}
impl Read for JsonReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("captura cancelada"));
        }
        let n = self.file.read(bytes)?;
        self.hash.update(&bytes[..n]);
        self.count = self.count.saturating_add(n as u64);
        Ok(n)
    }
}
/// Parse one external authoritative document directly from a verified stream.
/// The byte digest (including BOM/whitespace) is independent of canonical JSON.
pub fn read_snapshot_json(bundle: &SourceBundle, path: &str, cancel: &AtomicBool) -> V1Result<Value> {
    check_cancel(cancel)?;
    validate_path(path)?;
    if let Some(text) = bundle.documents().get(path) {
        return parse(text);
    }
    let source = bundle.files().get(path).ok_or_else(|| invalid("documento ausente del snapshot"))?;
    if source.source.to_string_lossy().starts_with("@project/") {
        return Err(invalid("resuelve localizadores del almacén antes de leer JSON externo"));
    }
    let reader = JsonReader { file: std::fs::File::open(&source.source)?, cancel, hash: Sha256::new(), count: 0 };
    let mut reader = std::io::BufReader::with_capacity(65536, reader);
    if reader.fill_buf()?.starts_with(&[0xef, 0xbb, 0xbf]) {
        reader.consume(3);
    }
    let value: Value = serde_json::from_reader(&mut reader)?;
    let reader = reader.into_inner();
    if reader.count != source.size || hex::encode(reader.hash.finalize()) != source.sha256 {
        return Err(invalid("JSON externo cambió respecto al snapshot"));
    }
    Ok(value)
}
fn read_snapshot_text(bundle: &SourceBundle, path: &str) -> V1Result<String> {
    if let Some(text) = bundle.documents().get(path) {
        return Ok(text.clone());
    }
    let file = bundle.files().get(path).ok_or_else(|| invalid("documento externo ausente"))?;
    let cancel = AtomicBool::new(false);
    let mut reader = JsonReader { file: std::fs::File::open(&file.source)?, cancel: &cancel, hash: Sha256::new(), count: 0 };
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    if reader.count != file.size || hex::encode(reader.hash.finalize()) != file.sha256 {
        return Err(invalid("documento externo cambió"));
    }
    Ok(text)
}
/// Verify the complete inventory and every byte without allocating a second
/// snapshot. Additions/removals, empty directories and changed hashes reject.
pub fn verify_snapshot(root: &Path, bundle: &SourceBundle, cancel: &AtomicBool) -> V1Result<()> {
    let mut remaining: HashSet<String> = bundle.documents().keys().chain(bundle.files().keys()).chain(bundle.directories()).cloned().collect();
    walk(root, cancel, &mut |path, relative, directory| {
        if !remaining.remove(&relative) {
            return Err(invalid("carpeta cambió: entrada nueva"));
        }
        if directory {
            if !bundle.directories().contains(&relative) {
                return Err(invalid("tipo de entrada cambió"));
            }
            return Ok(());
        }
        let actual = digest_file(path, cancel, None, 0)?;
        let matches = if let Some(text) = bundle.documents().get(&relative) {
            actual.size == text.len() as u64 && actual.sha256 == sha(text)
        } else {
            bundle.files().get(&relative).is_some_and(|expected| actual.size == expected.size && actual.sha256 == expected.sha256)
        };
        if !matches {
            return Err(invalid(format!("carpeta cambió: {relative}")));
        }
        Ok(())
    })?;
    if !remaining.is_empty() {
        return Err(invalid("carpeta cambió: entrada eliminada"));
    }
    Ok(())
}

fn parse(text: &str) -> V1Result<Value> {
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}
fn sha(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// Preserve exact original bytes unless an authoritative adapter changed the value.
fn replace(docs: &mut BTreeMap<String, String>, path: String, value: Value, changed: &mut Vec<String>) -> V1Result<()> {
    if docs.get(&path).is_some_and(|old| parse(old).is_ok_and(|v| v == value)) {
        return Ok(());
    }
    replace_text(docs, path, serde_json::to_string_pretty(&value)?, changed)
}
fn replace_text(docs: &mut BTreeMap<String, String>, path: String, text: String, changed: &mut Vec<String>) -> V1Result<()> {
    validate_path(&path)?;
    if let Some(old) = docs.get(&path) {
        if old == &text {
            return Ok(());
        }
        // Content addressing avoids recursive archives on repeated V2/V1 round-trips.
        let archive = format!(".work/tv2-original/{}.txt", sha(old));
        if docs.get(&archive).is_some_and(|prior| prior != old) {
            return Err(invalid("colisión en archivo original"));
        }
        docs.insert(archive, old.clone());
    }
    docs.insert(path.clone(), text);
    changed.push(path);
    Ok(())
}

fn matching_path(docs: &BTreeMap<String, String>, layer_id: &str) -> V1Result<Option<String>> {
    let matches: Vec<_> = docs
        .iter()
        .filter(|(p, _)| p.starts_with("layers/") && p.ends_with(".json"))
        .filter(|(_, text)| parse(text).is_ok_and(|v| v["layer_id"] == layer_id))
        .map(|(p, _)| p.clone())
        .collect();
    if matches.len() > 1 {
        return Err(invalid("varios documentos para un layer_id"));
    }
    Ok(matches.into_iter().next())
}

pub struct FolderExport {
    pub documents: BTreeMap<String, String>,
    pub files: BTreeMap<String, SourceFile>,
    pub directories: BTreeSet<String>,
}
pub fn export(project: &Project, asset_id: &AssetId, sequence: Option<&SequenceId>) -> V1Result<FolderExport> {
    project.validate().map_err(|e| invalid(e.to_string()))?;
    let asset = project.asset(asset_id).ok_or_else(|| invalid("medio inexistente"))?;
    let master = project.masters.iter().find(|m| &m.asset_id == asset_id).ok_or_else(|| invalid("exportación integral requiere master"))?;
    let bundle =
        master.source_bundle.as_ref().ok_or_else(|| invalid("proyecto antiguo sin carpeta original conservada; importa la carpeta V1 original en este proyecto para vincularla sin reemplazar la edición"))?;
    let mut docs = bundle.documents().clone();
    let mut files = bundle.files().clone();
    // Only active review/manifests require their original JSON for rebasing
    // checks. Masters and auxiliary evidence remain streamed copy references.
    for path in files.keys().filter(|p| p.ends_with(".json") && (p.starts_with("views/") || p.starts_with(".work/manifests/"))) {
        docs.insert(path.clone(), read_snapshot_text(bundle, path)?);
    }
    let mut changed = Vec::new();
    let mut kinds = HashSet::new();
    for layer in project.layers.iter().filter(|l| &l.asset_id == asset_id && l.kind.is_editable()) {
        let path = match layer.kind {
            LayerKind::Trims => {
                if !kinds.insert("trims") {
                    continue;
                }
                "views/trims.json".into()
            }
            LayerKind::Author => {
                if !kinds.insert("author") {
                    return Err(invalid("varias capas autor activas"));
                }
                "autor.marcas.json".into()
            }
            LayerKind::Blocks => {
                if !kinds.insert("blocks") {
                    return Err(invalid("varios planes de bloques: selecciona un único plan autoritativo"));
                }
                for (mut path, text) in crate::materialize::blocks(project, &layer.layer_id)? {
                    if path == "project.editorial.master.json" {
                        path = bundle.master_path().into();
                    }
                    if (path.ends_with("/moments.md") || path.ends_with("/analysis-request.md"))
                        && (docs.contains_key(&path) || files.contains_key(&path))
                    {
                        continue;
                    }
                    if path.ends_with(".json") {
                        replace(&mut docs, path, parse(&text)?, &mut changed)?;
                    } else {
                        replace_text(&mut docs, path, text, &mut changed)?;
                    }
                }
                ".work/chunks.selected.json".into()
            }
            _ => {
                let captured = layer
                    .extra
                    .get("tv2_v1_source_path")
                    .and_then(Value::as_str)
                    .filter(|p| p.starts_with("layers/") && (docs.contains_key(*p) || files.contains_key(*p)));
                match captured {
                    Some(path) => path.to_string(),
                    None => matching_path(&docs, layer.layer_id.as_str())?.unwrap_or_else(|| format!("layers/{}.json", layer.layer_id)),
                }
            }
        };
        let value = crate::export::layer_document(project, &layer.layer_id)?;
        replace(&mut docs, path, value, &mut changed)?;
    }
    if let Some(id) = sequence {
        let value = crate::export::montage_document(project, id)?;
        let fp = crate::master::parse_fingerprint(&value["media"])?;
        if !fp.same_identity(&asset.fingerprint) {
            return Err(invalid("el montaje seleccionado pertenece a otro medio"));
        }
        replace(&mut docs, "views/montaje.json".into(), value, &mut changed)?;
    }
    let snapshot = crate::contracts::layers_snapshot(project, asset_id)?;
    replace(&mut docs, "views/layers.json".into(), snapshot.clone(), &mut changed)?;
    // A copied external request/proposal must not look current after edits.
    // Archive original bytes, including unknown contract versions, and leave a
    // durable reason in the export report rather than silently rebasing digests.
    let mut stale_contracts = Vec::new();
    let candidates: Vec<_> = docs
        .keys()
        .filter(|p| p.starts_with("views/") && p.ends_with(".json") && (p.contains("-request") || p.contains(".proposed") || p.contains("-pass")))
        .cloned()
        .collect();
    for path in candidates {
        let text = docs.get(&path).unwrap();
        let document = parse(text);
        let reason = match &document {
            Err(_) => Some("invalid_json"),
            Ok(v) => {
                let supported = matches!(
                    v["schema"].as_str(),
                    Some(
                        "editorial-layers-request/1"
                            | "editorial-layers-proposal/1"
                            | "editorial-topics-request/1"
                            | "editorial-topics-proposal/1"
                            | "editorial-trims-request/1"
                            | "editorial-trims-proposal/1"
                            | "editorial-montage-request/1"
                            | "editorial-montage-proposal/1"
                            | "editorial-chunks/1"
                    )
                );
                if !supported {
                    Some("unsupported_contract_schema")
                } else if v.get("source_master_digest").is_some_and(|d| *d != snapshot["source_master_digest"]) {
                    Some("stale_master")
                } else if v.get("source_layers_digest").is_some_and(|d| *d != snapshot["source_layers_digest"]) {
                    Some("stale_layers")
                } else if v.get("tv2_project_revision").is_some_and(|r| r.as_u64() != Some(project.revision)) {
                    Some("stale_project_revision")
                } else if v.get("montage_digest").is_some_and(|d| {
                    docs.get("views/montaje.json").and_then(|s| parse(s).ok()).is_none_or(|m| {
                        *d != if m["clips"].as_array().is_none_or(|c| c.is_empty()) {
                            Value::Null
                        } else {
                            json!(tv2_domain::digest::digest_object_without(&m, &["revision", "updated_at"]))
                        }
                    })
                }) {
                    Some("stale_montage")
                } else if v.get("trims_digest").is_some_and(|d| {
                    docs.get("views/trims.json")
                        .and_then(|s| parse(s).ok())
                        .is_none_or(|m| *d != tv2_domain::digest::digest_object_without(&m, &["revision", "updated_at"]))
                }) {
                    Some("stale_trims")
                } else {
                    None
                }
            }
        };
        if let Some(reason) = reason {
            let text = docs.remove(&path).unwrap();
            files.remove(&path);
            let archive = format!(".work/tv2-original/{}.txt", sha(&text));
            docs.insert(archive.clone(), text);
            stale_contracts.push(json!({"path":path,"archived_at":archive,"state":"requires_new_request","reason":reason}));
        }
    }
    // Keep pipeline manifests only when every recorded byte digest still matches.
    let mut stale = Vec::new();
    let mut outputs = crate::contracts::document_digests(&docs);
    for (path, file) in &files {
        outputs.entry(path.clone()).or_insert(crate::contracts::OutputDigest { size: file.size, sha256: file.sha256.clone() });
    }
    loop {
        let manifests: Vec<_> = docs.keys().filter(|p| p.starts_with(".work/manifests/") && p.ends_with(".json")).cloned().collect();
        let prior = stale.len();
        for path in manifests {
            let text = docs.get(&path).unwrap();
            let valid = parse(text).is_ok_and(|v| crate::contracts::validate_manifest(&v, &outputs, None).is_ok());
            if !valid {
                let text = docs.remove(&path).unwrap();
                files.remove(&path);
                outputs.remove(&path);
                let archive = format!(".work/tv2-original/{}.txt", sha(&text));
                docs.insert(archive.clone(), text);
                stale.push(json!({"path":path,"archived_at":archive,"state":"requires_revalidation"}));
            }
        }
        if stale.len() == prior {
            break;
        }
    }
    let overridden: Vec<_> = files.keys().filter(|p| docs.contains_key(*p)).cloned().collect();
    for path in overridden {
        let text = &docs[&path];
        let file = &files[&path];
        if text.len() as u64 == file.size && sha(text) == file.sha256 {
            docs.remove(&path);
        } else {
            let file = files.remove(&path).unwrap();
            files.insert(format!(".work/tv2-original/{}.bin", file.sha256), file);
        }
    }
    let inventory: Vec<_> = bundle.documents().iter().map(|(path, text)| json!({"path":path,"size":text.len(),"sha256":sha(text)})).collect();
    let report = json!({"schema":"transcriptor-v1-folder-export/1","project_id":project.project_id,"revision":project.revision,
        "source_master_digest":master.source_digest,"source_master_path":bundle.master_path(),"source_documents":inventory,
        "changed_documents":changed,"stale_manifests":stale,"stale_contracts":stale_contracts,"source_attachments":bundle.files(),"primary_media_copied":false});
    let report_path = format!(".work/tv2-exports/{}.json", tv2_domain::digest::digest_json(&report));
    docs.insert(report_path, serde_json::to_string_pretty(&report)?);
    for path in docs.keys() {
        validate_path(path)?;
    }
    Ok(FolderExport { documents: docs, files, directories: bundle.directories().clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_spills_auxiliary_text_preserves_empty_dirs_and_verifies_streaming() {
        let root = tempfile::tempdir().unwrap();
        let name = "project.editorial.master.json";
        std::fs::write(root.path().join(name), "{\"schema\":\"synthetic\"}").unwrap();
        std::fs::create_dir_all(root.path().join("empty/nested")).unwrap();
        std::fs::write(root.path().join("large-transcript.md"), "synthetic text\n".repeat(6000)).unwrap();
        let cancel = AtomicBool::new(false);
        let captured = snapshot_with_cancel(root.path(), name.into(), &cancel).unwrap();
        assert!(captured.files().contains_key("large-transcript.md"));
        assert!(!captured.documents().contains_key("large-transcript.md"));
        assert!(captured.directories().contains("empty/nested"));
        verify_snapshot(root.path(), &captured, &cancel).unwrap();
        std::fs::write(root.path().join("large-transcript.md"), "changed").unwrap();
        assert!(verify_snapshot(root.path(), &captured, &cancel).is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(snapshot_with_cancel(root.path(), name.into(), &cancel).is_err());
    }
    #[test]
    fn snapshot_export_keeps_unknowns_original_bytes_and_invalidates_stale_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let asset = tv2_domain::commands::tests_support::fake_video("a", 20);
        let master =
            json!({"schema":"editorial-master/1","media":{"path":"missing.mp4","duration":20.0,"fingerprint":asset.fingerprint},"tracks":{}});
        std::fs::write(dir.path().join("sample.editorial.master.json"), format!("\u{feff}{}\n", master)).unwrap();
        std::fs::create_dir_all(dir.path().join(".work/manifests")).unwrap();
        std::fs::write(dir.path().join(".work/manifests/old.json"), "{\"future\":true}").unwrap();
        std::fs::write(dir.path().join("unknown.md"), "Evidence\r\nñ\r\n").unwrap();
        let imported = crate::import::read_v1_editorial(dir.path(), &asset).unwrap();
        let mut p = Project::new("folder");
        p.assets.push(asset.clone());
        tv2_domain::Command::Batch { label: "import".into(), commands: crate::import::import_commands(&imported, &p, &asset.id) }
            .apply(&mut p)
            .unwrap();
        let output = export(&p, &asset.id, None).unwrap().documents;
        assert_eq!(output["unknown.md"], "Evidence\r\nñ\r\n");
        assert_eq!(output["sample.editorial.master.json"], std::fs::read_to_string(dir.path().join("sample.editorial.master.json")).unwrap());
        assert!(!output.contains_key(".work/manifests/old.json"));
        assert!(output.values().any(|s| s == "{\"future\":true}"));
        std::fs::write(dir.path().join("opaque.bin"), [255, 254, 0]).unwrap();
        let captured = crate::import::read_v1_editorial(dir.path(), &asset).unwrap();
        assert_eq!(captured.master.source_bundle.unwrap().files()["opaque.bin"].size, 3);
    }
}
