//! Importación de una carpeta `editorial/` de V1 a un proyecto V2 como comandos.
//!
//! Estructura V1 reconocida (relativa a la carpeta editorial):
//! `<nombre>.editorial.master.json`, `layers/*.json`, `views/trims.json`,
//! `views/montaje.json`. La identidad del medio se compara por fingerprint con
//! los assets del proyecto; si no hay coincidencia se informa (`needs_media`).

use crate::layers::layer_from_v1;
use crate::master::V1Master;
use crate::montaje::V1Montaje;
use crate::trims::trims_from_v1;
use crate::{V1Error, V1Result, invalid};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tv2_domain::asset::Asset;
use tv2_domain::commands::Command;
use tv2_domain::ids::AssetId;
use tv2_domain::layers::SemanticLayer;
use tv2_domain::project::Project;
use tv2_domain::time::Rational;
use tv2_domain::timeline::Sequence;

#[derive(Clone, Debug, Default)]
pub struct V1ImportReport {
    pub master_path: Option<PathBuf>,
    pub master_digest: Option<String>,
    pub media_path: String,
    pub layers: usize,
    pub trims_layers: usize,
    pub montage_pieces: usize,
    pub warnings: Vec<String>,
}

pub struct V1Import {
    pub author_candidates: Vec<crate::author_candidates::Candidate>,
    pub master: tv2_domain::evidence::MasterEvidence,
    pub report: V1ImportReport,
    pub layers: Vec<SemanticLayer>,
    pub sequence: Option<Sequence>,
}

/// Localiza el master en `root` (carpeta editorial o su padre).
pub fn find_master(root: &Path) -> Option<PathBuf> {
    for dir in [root.to_path_buf(), root.join("editorial")] {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let paths: Vec<_> =
                entries.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".editorial.master.json")).map(|e| e.path()).collect();
            if paths.len() > 1 {
                return None;
            }
            if let Some(path) = paths.into_iter().next() {
                return Some(path);
            }
        }
    }
    None
}

/// Lee la carpeta V1 y prepara capas y secuencia para `asset` (ya importado en V2 y
/// con la misma identidad que el master).
pub fn read_v1_editorial(root: &Path, asset: &Asset) -> V1Result<V1Import> {
    read_v1_editorial_with_stores(root, asset, &[])
}

pub fn read_v1_editorial_with_stores(root: &Path, asset: &Asset, stores: &[PathBuf]) -> V1Result<V1Import> {
    read_v1_editorial_with_cancel(root, asset, stores, &std::sync::atomic::AtomicBool::new(false))
}

pub fn read_v1_editorial_with_cancel(root: &Path, asset: &Asset, stores: &[PathBuf], cancel: &std::sync::atomic::AtomicBool) -> V1Result<V1Import> {
    let master_path = find_master(root).ok_or_else(|| invalid(format!("no se encontró *.editorial.master.json en {}", root.display())))?;
    let editorial_dir = master_path.parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
    let original = crate::folder::snapshot_with_cancel(&editorial_dir, master_path.file_name().unwrap().to_string_lossy().into_owned(), cancel)?;
    let read_json = |path: &Path| -> V1Result<Value> {
        let relative = path.strip_prefix(&editorial_dir).map_err(|_| invalid("documento fuera de carpeta"))?.to_string_lossy().replace('\\', "/");
        crate::folder::read_snapshot_json(&original, &relative, cancel)
    };
    let master = V1Master::parse(read_json(&master_path)?)?;
    let mut report = V1ImportReport {
        master_path: Some(master_path.clone()),
        master_digest: Some(master.source_master_digest()),
        media_path: master.media_path.clone(),
        ..Default::default()
    };
    if !master.fingerprint.same_identity(&asset.fingerprint) {
        return Err(invalid(format!(
            "el medio «{}» no coincide con la identidad del master (size {} / hash {}…)",
            asset.name,
            master.fingerprint.size,
            master.fingerprint.hash_muestreado.chars().take(12).collect::<String>()
        )));
    }
    let duration = master.duration.max(asset.duration());
    let mut layers = Vec::new();
    let layers_dir = editorial_dir.join("layers");
    if layers_dir.is_dir() {
        let mut paths: Vec<PathBuf> =
            std::fs::read_dir(&layers_dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
        paths.sort();
        for p in paths {
            match read_json(&p).and_then(|v| layer_from_v1(&v, &asset.id, Some(&asset.fingerprint), duration)) {
                Ok(mut l) => {
                    if p.file_stem().is_some_and(|s| s != l.layer_id.as_str()) {
                        report.warnings.push(format!("{}: nombre de archivo incoherente con layer_id {}", p.display(), l.layer_id));
                    }
                    l.extra.insert(
                        "tv2_v1_source_path".into(),
                        serde_json::json!(p.strip_prefix(&editorial_dir).unwrap().to_string_lossy().replace('\\', "/")),
                    );
                    layers.push(l);
                }
                Err(e) => return Err(invalid(format!("{}: {e}; no se importó la carpeta", p.display()))),
            }
        }
    }
    report.layers = layers.len();
    let mut author_paths = crate::author_candidates::paths(asset, Some(&editorial_dir), stores)?;
    author_paths.push(editorial_dir.join("autor.marcas.json"));
    if let Some(stem) = std::path::Path::new(&master.media_path).file_stem() {
        author_paths.push(editorial_dir.join(format!("{}.marcas.json", stem.to_string_lossy())));
    }
    author_paths.push(editorial_dir.join("marcas_store").join(format!("{}.marcas.json", asset.fingerprint.hash_muestreado)));
    author_paths.sort();
    author_paths.dedup();
    let candidates = crate::author_candidates::discover(&author_paths, asset);
    let mut author_candidates = Vec::new();
    match crate::author_candidates::preferred(&candidates) {
        Ok(Some(index)) => layers.push(candidates[index].layer.as_ref().unwrap().clone()),
        Err(e) => {
            report.warnings.push(e.to_string());
            author_candidates = candidates;
        }
        Ok(None) => {}
    }
    if !layers.iter().any(|l| l.kind == tv2_domain::LayerKind::Author) && master.raw.pointer("/streams/autor.marcas").is_some() {
        report.warnings.push("El master contiene evidencia de autor; importa el sidecar autoritativo para editar las marcas".into());
    }
    // Selected plan takes precedence over generated views. Never import both.
    let selected_plan = editorial_dir.join(".work/chunks.selected.json");
    let plan_path = if selected_plan.exists() { selected_plan } else { editorial_dir.join("views/chunks.json") };
    if plan_path.is_file() {
        layers.push(crate::chunks::from_v1(&read_json(&plan_path)?, &asset.id, asset.duration(), &master.source_master_digest())?);
    }
    let trims_path = editorial_dir.join("views").join("trims.json");
    if trims_path.is_file() {
        match read_json(&trims_path).and_then(|v| trims_from_v1(&v, &asset.id, Some(&asset.fingerprint))) {
            Ok(t) => {
                report.trims_layers = t.layers.len();
                layers.extend(t.layers);
            }
            Err(e) => return Err(invalid(format!("views/trims.json: {e}"))),
        }
    }
    let mut sequence = None;
    let montaje_path = editorial_dir.join("views").join("montaje.json");
    if montaje_path.is_file() {
        match read_json(&montaje_path).and_then(|v| V1Montaje::parse(v, Some(&asset.fingerprint))) {
            Ok(m) => {
                let (w, h) = asset.probe.video.as_ref().map(|v| v.display_size()).unwrap_or((1920, 1080));
                let fps = asset.frame_rate().unwrap_or(Rational::new(30, 1));
                let seq = m.to_sequence(&asset.id, fps, w, h, asset.probe.audio.len() as u32);
                report.montage_pieces = seq.clips.iter().filter(|c| c.audio_stream.is_none()).count();
                sequence = Some(seq);
            }
            Err(e) => return Err(invalid(format!("views/montaje.json: {e}"))),
        }
    }
    layers.extend(master.projections(&asset.id)?);
    crate::folder::verify_snapshot(&editorial_dir, &original, cancel)?;
    let mut evidence = tv2_domain::evidence::MasterEvidence {
        asset_id: asset.id.clone(),
        source_digest: master.source_master_digest(),
        document: master.raw.into(),
        source_bundle: None,
    };
    let original = if original.files().contains_key(original.master_path()) {
        original.with_external_master_digest(evidence.document.full_digest().into())
    } else {
        original
    };
    evidence.source_bundle = Some(std::sync::Arc::new(original));
    Ok(V1Import { master: evidence, report, layers, sequence, author_candidates })
}

/// Vincular la carpeta de un proyecto antiguo no vuelve a importar su edición.
/// Se compara el documento completo: el digest editorial omite campos como
/// `chunks` y por sí solo no identifica la misma evidencia inmutable.
pub fn is_source_bundle_enrichment(import: &V1Import, project: &Project) -> bool {
    import.master.source_bundle.is_some()
        && project.masters.iter().any(|existing| {
            existing.asset_id == import.master.asset_id
                && existing.source_bundle.is_none()
                && existing.source_digest == import.master.source_digest
                && existing.document == import.master.document
        })
}

/// Comandos para incorporar la importación a un proyecto (batch todo-o-nada).
/// Para evidencia antigua idéntica sin carpeta solo adjunta el origen: conserva
/// todas las capas, secuencias y metadatos del master ya presente.
pub fn import_commands(import: &V1Import, project: &Project, _asset_id: &AssetId) -> Vec<Command> {
    if is_source_bundle_enrichment(import, project) {
        let mut master = project.masters.iter().find(|existing| existing.asset_id == import.master.asset_id).unwrap().clone();
        master.source_bundle = import.master.source_bundle.clone();
        return vec![Command::AttachMaster { master }];
    }
    let mut cmds = vec![Command::AttachMaster { master: import.master.clone() }];
    for l in &import.layers {
        let mut layer = l.clone();
        if layer.kind == tv2_domain::LayerKind::Author
            && let Some(existing) =
                project.layers.iter().find(|l| l.kind == tv2_domain::LayerKind::Author && l.asset_id == layer.asset_id && !l.deleted)
        {
            layer.layer_id = existing.layer_id.clone();
        }
        if let Some(existing) = project.layer(&layer.layer_id) {
            // conservar el orden/visibilidad locales
            layer.visible = existing.visible;
            if !layer.kind.is_editable()
                && existing.items == layer.items
                && existing.extra == layer.extra
                && existing.source_master_digest == layer.source_master_digest
            {
                continue;
            }
        }
        cmds.push(Command::ReplaceLayer { layer });
    }
    if let Some(seq) = &import.sequence {
        cmds.push(Command::AddSequence { sequence: seq.clone(), activate: true });
    }
    cmds
}

impl From<V1Error> for tv2_domain::error::DomainError {
    fn from(e: V1Error) -> Self {
        tv2_domain::error::DomainError::invalid(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::master::fixtures;
    use serde_json::json;
    use tv2_domain::commands::tests_support::fake_video;
    #[test]
    fn external_master_uses_verified_bytes_and_exports_original_reference() {
        let root = tempfile::tempdir().unwrap();
        let asset = fake_video("external", 12);
        let mut raw = fixtures::master(12.0);
        raw["media"]["fingerprint"] = serde_json::json!(asset.fingerprint);
        raw["future_large_metadata"] = serde_json::json!("immutable".repeat(10000));
        let name = "external.editorial.master.json";
        let bytes = format!("\u{feff}{}\r\n", raw);
        std::fs::write(root.path().join(name), &bytes).unwrap();
        let imported = read_v1_editorial(root.path(), &asset).unwrap();
        let bundle = imported.master.source_bundle.as_ref().unwrap();
        assert!(!bundle.documents().contains_key(name));
        assert!(bundle.files().contains_key(name));
        assert_eq!(bundle.external_master_digest(), Some(imported.master.document.full_digest()));
        let mut p = Project::new("external");
        p.assets.push(asset.clone());
        Command::Batch { label: "import".into(), commands: import_commands(&imported, &p, &asset.id) }.apply(&mut p).unwrap();
        p.validate().unwrap();
        let exported = crate::folder::export(&p, &asset.id, None).unwrap();
        assert!(exported.files.contains_key(name));
        assert!(!exported.documents.contains_key(name));
        std::fs::write(root.path().join(name), "changed").unwrap();
        assert!(crate::folder::read_snapshot_json(bundle, name, &std::sync::atomic::AtomicBool::new(false)).is_err());
    }

    #[test]
    fn imports_folder_with_layers_trims_and_montage() {
        let dir = tempfile::tempdir().unwrap();
        let ed = dir.path().join("editorial");
        std::fs::create_dir_all(ed.join("layers")).unwrap();
        std::fs::create_dir_all(ed.join("views")).unwrap();
        let mut master = fixtures::master(100.0);
        master["media"]["fingerprint"] = json!({"size": 1000, "hash_muestreado": "fx-h", "inventario_sha256": "fx-i"});
        std::fs::write(ed.join("padre.editorial.master.json"), master.to_string()).unwrap();
        std::fs::write(ed.join("layers").join("layer-abc.json"), json!({"schema": "editorial-layer/1", "layer_id": "layer-abc", "kind": "user", "name": "Notas", "color": "#d09947",
            "media_fingerprint": {"size": 1000, "hash_muestreado": "fx-h", "inventario_sha256": "fx-i"}, "source_master_digest": "x", "revision": 1,
            "items": [{"item_id": "item-1", "label": "uno", "comment": "", "state": "proposed", "edited": true, "parent_id": null, "ranges": [{"t_ini": 1, "t_fin": 2}]}]}).to_string()).unwrap();
        std::fs::write(ed.join("views").join("trims.json"), json!({"schema": "editorial-trims/1", "media": {"size": 1000, "hash_muestreado": "fx-h", "inventario_sha256": "fx-i"}, "duration": 100.0, "revision": 1, "next_id": 2,
            "lanes": [{"lane_id": "main", "name": "Recortes", "color": "#728bd0"}], "cuts": [{"cut_id": "cut-000001", "t_ini": 3.0, "t_fin": 7.0, "origin": "user", "lane": "main", "enabled": true, "accepted": false, "edited": true, "reason": "r"}]}).to_string()).unwrap();
        std::fs::write(ed.join("views").join("montaje.json"), json!({"schema": "editorial-montaje/1", "media": {"size": 1000, "hash_muestreado": "fx-h", "inventario_sha256": "fx-i"}, "duration_source": 100.0, "revision": 2, "next_id": 3,
            "tracks": [{"track_id": "V1", "name": "V1"}, {"track_id": "V2", "name": "V2"}], "clips": [{"clip_id": "clip-000001", "track_id": "V1", "source_ini": 10.0, "source_fin": 30.0, "seq_ini": 0.0}, {"clip_id": "clip-000002", "track_id": "V2", "source_ini": 50.0, "source_fin": 53.0, "seq_ini": 8.0}]}).to_string()).unwrap();
        let mut project = Project::new("p");
        let mut asset = fake_video("a", 100);
        asset.fingerprint.hash_muestreado = "fx-h".into();
        asset.fingerprint.inventario_sha256 = "fx-i".into();
        Command::ImportAsset { asset: asset.clone() }.apply(&mut project).unwrap();
        let import = read_v1_editorial(dir.path(), &asset).unwrap();
        assert_eq!(import.report.layers, 1);
        assert_eq!(import.report.trims_layers, 1);
        assert_eq!(import.report.montage_pieces, 3);
        assert!(import.report.warnings.is_empty(), "{:?}", import.report.warnings);
        let cmds = import_commands(&import, &project, &asset.id);
        Command::Batch { label: "v1".into(), commands: cmds }.apply(&mut project).unwrap();
        assert_eq!(project.layers.len(), 12);
        assert_eq!(project.masters.len(), 1);
        let seq = project.active().unwrap();
        assert_eq!(seq.name, "Montaje V1");
        assert_eq!(seq.extent(), tv2_domain::time::Ticks::from_seconds(20));

        // Reattaching a moved original folder must not replay old layer or
        // montage documents over subsequent native edits.
        let mut legacy = project.clone();
        legacy.masters[0].source_bundle = None;
        legacy.layers[0].name = "Edición humana posterior".into();
        legacy.sequences[0].name = "Montaje humano posterior".into();
        let layers_before = legacy.layers.clone();
        let sequences_before = legacy.sequences.clone();
        let active_before = legacy.active_sequence.clone();
        let document_before = legacy.masters[0].document.clone();
        assert!(is_source_bundle_enrichment(&import, &legacy));
        let commands = import_commands(&import, &legacy, &asset.id);
        assert!(matches!(commands.as_slice(), [Command::AttachMaster { .. }]));
        Command::Batch { label: "Vincular carpeta".into(), commands }.apply(&mut legacy).unwrap();
        assert_eq!(legacy.layers, layers_before);
        assert_eq!(legacy.sequences, sequences_before);
        assert_eq!(legacy.active_sequence, active_before);
        assert!(legacy.masters[0].document.shares_storage(&document_before));
        assert!(legacy.masters[0].source_bundle.is_some());
        assert!(!is_source_bundle_enrichment(&import, &legacy), "an existing bundle is not an enrichment");
        legacy.masters[0].source_bundle = None;
        let mut different = (*legacy.masters[0].document).clone();
        different["generated_at"] = json!("different source metadata");
        legacy.masters[0].document = different.into();
        assert_eq!(legacy.masters[0].document.source_digest(), import.master.source_digest);
        assert!(!is_source_bundle_enrichment(&import, &legacy), "editorial digest equality must not substitute full evidence equality");

        std::fs::write(ed.join("layers").join("invalid.json"), "{broken").unwrap();
        assert!(read_v1_editorial(dir.path(), &asset).is_err(), "recognized invalid documents must abort the whole import");
        // identidad distinta → rechazo explícito
        let other = fake_video("b", 100);
        assert!(read_v1_editorial(dir.path(), &other).is_err());
    }
}
