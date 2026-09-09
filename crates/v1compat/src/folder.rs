//! Immutable editorial-folder snapshots and explicit exports into V2 copies.
use crate::{V1Result, invalid};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    io::Read,
    path::Path,
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

/// Text evidence is embedded; binary evidence is fingerprinted, without source writes.
pub fn snapshot(root: &Path, master_path: String) -> V1Result<SourceBundle> {
    let owned = std::fs::read_to_string(root.join(".transcriptor-export.json"))
        .ok()
        .and_then(|s| parse(&s).ok())
        .is_some_and(|v| v["schema"] == "transcriptor-export/1");
    let mut docs = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    let mut seen = HashSet::new();
    let mut size = 0usize;
    while let Some(dir) = pending.pop() {
        if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
            return Err(invalid("carpeta enlazada no admitida"));
        }
        for entry in std::fs::read_dir(&dir)? {
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
            if seen.len() > 20000 {
                return Err(invalid("carpeta supera 20000 entradas; importación no realizada"));
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(invalid(format!("enlace no permitido: {relative}")));
            }
            if kind.is_dir() {
                pending.push(path);
                continue;
            }
            if !kind.is_file() {
                return Err(invalid(format!("entrada no documental: {relative}")));
            }
            let extension = path.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
            if !matches!(extension.as_str(), "json" | "md" | "txt" | "csv" | "tsv" | "yaml" | "yml" | "xml" | "srt" | "vtt") {
                let mut reader = std::fs::File::open(&path)?;
                let mut hasher = Sha256::new();
                let mut block = [0u8; 65536];
                let mut count = 0u64;
                loop {
                    let n = reader.read(&mut block)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&block[..n]);
                    count += n as u64;
                }
                files.insert(relative, SourceFile { source: std::path::absolute(path)?, size: count, sha256: hex::encode(hasher.finalize()) });
                continue;
            }
            let mut bytes = Vec::new();
            std::fs::File::open(&path)?.take((LIMIT - size + 1) as u64).read_to_end(&mut bytes)?;
            size += bytes.len();
            if size > LIMIT {
                return Err(invalid("carpeta documental supera 32 MiB; no se importó parcialmente"));
            }
            let text = String::from_utf8(bytes).map_err(|_| invalid(format!("archivo no UTF-8: {relative}; requiere adaptador binario")))?;
            docs.insert(relative, text);
        }
    }
    if !docs.contains_key(&master_path) {
        return Err(invalid("snapshot sin master"));
    }
    Ok(SourceBundle::new(master_path, docs).with_files(files))
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
}
pub fn export(project: &Project, asset_id: &AssetId, sequence: Option<&SequenceId>) -> V1Result<FolderExport> {
    project.validate().map_err(|e| invalid(e.to_string()))?;
    let asset = project.asset(asset_id).ok_or_else(|| invalid("medio inexistente"))?;
    let master = project.masters.iter().find(|m| &m.asset_id == asset_id).ok_or_else(|| invalid("exportación integral requiere master"))?;
    let bundle =
        master.source_bundle.as_ref().ok_or_else(|| invalid("proyecto antiguo sin carpeta original conservada; importa V1 en un proyecto nuevo"))?;
    let mut docs = bundle.documents().clone();
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
                    if (path.ends_with("/moments.md") || path.ends_with("/analysis-request.md")) && docs.contains_key(&path) {
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
            _ => matching_path(&docs, layer.layer_id.as_str())?.unwrap_or_else(|| format!("layers/{}.json", layer.layer_id)),
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
    // Keep pipeline manifests only when every recorded byte digest still matches.
    let mut stale = Vec::new();
    loop {
        let manifests: Vec<_> = docs.keys().filter(|p| p.starts_with(".work/manifests/") && p.ends_with(".json")).cloned().collect();
        let prior = stale.len();
        for path in manifests {
            let text = docs.get(&path).unwrap();
            let valid = parse(text).is_ok_and(|v| {
                v["schema"] == "editorial-step/1"
                    && v["result_digest"].as_str() == Some(tv2_domain::digest::digest_object_without(&v, &["result_digest"]).as_str())
                    && v["outputs"].as_object().is_some_and(|outputs| {
                        outputs.iter().all(|(p, info)| {
                            docs.get(p).is_some_and(|data| {
                                info["size"].as_u64() == Some(data.len() as u64) && info["sha256"].as_str() == Some(sha(data).as_str())
                            }) || bundle
                                .files()
                                .get(p)
                                .is_some_and(|file| info["size"].as_u64() == Some(file.size) && info["sha256"].as_str() == Some(&file.sha256))
                        })
                    })
            });
            if !valid {
                let text = docs.remove(&path).unwrap();
                let archive = format!(".work/tv2-original/{}.txt", sha(&text));
                docs.insert(archive.clone(), text);
                stale.push(json!({"path":path,"archived_at":archive,"state":"requires_revalidation"}));
            }
        }
        if stale.len() == prior {
            break;
        }
    }
    let inventory: Vec<_> = bundle.documents().iter().map(|(path, text)| json!({"path":path,"size":text.len(),"sha256":sha(text)})).collect();
    let report = json!({"schema":"transcriptor-v1-folder-export/1","project_id":project.project_id,"revision":project.revision,
        "source_master_digest":master.source_digest,"source_master_path":bundle.master_path(),"source_documents":inventory,
        "changed_documents":changed,"stale_manifests":stale,"source_attachments":bundle.files(),"primary_media_copied":false});
    let report_path = format!(".work/tv2-exports/{}.json", tv2_domain::digest::digest_json(&report));
    docs.insert(report_path, serde_json::to_string_pretty(&report)?);
    for path in docs.keys() {
        validate_path(path)?;
    }
    Ok(FolderExport { documents: docs, files: bundle.files().clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
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
