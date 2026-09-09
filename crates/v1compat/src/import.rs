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
    pub master: tv2_domain::evidence::MasterEvidence,
    pub report: V1ImportReport,
    pub layers: Vec<SemanticLayer>,
    pub sequence: Option<Sequence>,
}

/// Localiza el master en `root` (carpeta editorial o su padre).
pub fn find_master(root: &Path) -> Option<PathBuf> {
    for dir in [root.to_path_buf(), root.join("editorial")] {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".editorial.master.json") {
                    return Some(e.path());
                }
            }
        }
    }
    None
}

fn read_json(path: &Path) -> V1Result<Value> {
    let text = std::fs::read_to_string(path)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(serde_json::from_str(text)?)
}

/// Lee la carpeta V1 y prepara capas y secuencia para `asset` (ya importado en V2 y
/// con la misma identidad que el master).
pub fn read_v1_editorial(root: &Path, asset: &Asset) -> V1Result<V1Import> {
    let master_path = find_master(root).ok_or_else(|| invalid(format!("no se encontró *.editorial.master.json en {}", root.display())))?;
    let editorial_dir = master_path.parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
    let master = V1Master::load(&master_path)?;
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
                Ok(l) => {
                    if p.file_stem().is_some_and(|s| s != l.layer_id.as_str()) {
                        report.warnings.push(format!("{}: nombre de archivo incoherente con layer_id {}", p.display(), l.layer_id));
                    }
                    layers.push(l);
                }
                Err(e) => return Err(invalid(format!("{}: {e}; no se importó la carpeta", p.display()))),
            }
        }
    }
    report.layers = layers.len();
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
    Ok(V1Import { master: master.evidence(&asset.id), report, layers, sequence })
}

/// Comandos para incorporar la importación a un proyecto (batch todo-o-nada).
pub fn import_commands(import: &V1Import, project: &Project, _asset_id: &AssetId) -> Vec<Command> {
    let mut cmds = vec![Command::AttachMaster { master: import.master.clone() }];
    for l in &import.layers {
        let mut layer = l.clone();
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
        std::fs::write(ed.join("layers").join("invalid.json"), "{broken").unwrap();
        assert!(read_v1_editorial(dir.path(), &asset).is_err(), "recognized invalid documents must abort the whole import");
        // identidad distinta → rechazo explícito
        let other = fake_video("b", 100);
        assert!(read_v1_editorial(dir.path(), &other).is_err());
    }
}
