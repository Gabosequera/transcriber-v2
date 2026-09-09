//! Proyecto: identidad, revisión, assets, secuencias y capas.

use crate::asset::Asset;
use crate::ids::{AssetId, LayerId, SequenceId};
use crate::layers::SemanticLayer;
use crate::time::Rational;
use crate::timeline::Sequence;
use serde::{Deserialize, Serialize};

/// /2 requires a reader which understands project-owned source-bundle locators.
/// Legacy /1 snapshots remain readable and migrate at an explicit save boundary.
pub const PROJECT_SCHEMA: &str = "transcriptor-project/2";
pub const LEGACY_PROJECT_SCHEMA: &str = "transcriptor-project/1";

/// Revisión monótona del proyecto. Cada comando persistente confirmado la incrementa;
/// undo/redo también producen una revisión nueva (nunca rebobinan).
pub type Revision = u64;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ProjectSettings {
    #[serde(default)]
    pub snapping: bool,
    #[serde(default)]
    pub skip_trims_on_play: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_export_dir: Option<String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings { snapping: true, skip_trims_on_play: false, last_export_dir: None }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Project {
    pub schema: String,
    pub project_id: String,
    pub name: String,
    pub revision: Revision,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub sequences: Vec<Sequence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_sequence: Option<SequenceId>,
    #[serde(default)]
    pub layers: Vec<SemanticLayer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masters: Vec<crate::evidence::MasterEvidence>,
    /// Orden de carriles semánticos en la UI (IDs de capa). Presentación.
    #[serde(default)]
    pub layer_order: Vec<LayerId>,
    #[serde(default)]
    pub settings: ProjectSettings,
    /// Campos desconocidos conservados en round-trip.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        let seq = Sequence::new("Secuencia 1", Rational::new(30, 1), 1920, 1080, 48_000);
        let now = now_iso();
        Project {
            schema: PROJECT_SCHEMA.to_string(),
            project_id: format!("proj-{}", crate::ids::random_hex12()),
            name: name.into(),
            revision: 0,
            created_at: now.clone(),
            updated_at: now,
            assets: Vec::new(),
            active_sequence: Some(seq.id.clone()),
            sequences: vec![seq],
            layers: Vec::new(),
            masters: Vec::new(),
            layer_order: Vec::new(),
            settings: ProjectSettings::default(),
            extra: Default::default(),
        }
    }

    pub fn asset(&self, id: &AssetId) -> Option<&Asset> {
        self.assets.iter().find(|a| &a.id == id)
    }

    pub fn asset_mut(&mut self, id: &AssetId) -> Option<&mut Asset> {
        self.assets.iter_mut().find(|a| &a.id == id)
    }

    pub fn sequence(&self, id: &SequenceId) -> Option<&Sequence> {
        self.sequences.iter().find(|s| &s.id == id)
    }

    pub fn sequence_mut(&mut self, id: &SequenceId) -> Option<&mut Sequence> {
        self.sequences.iter_mut().find(|s| &s.id == id)
    }

    pub fn active(&self) -> Option<&Sequence> {
        self.active_sequence.as_ref().and_then(|id| self.sequence(id)).or_else(|| self.sequences.first())
    }

    pub fn active_mut(&mut self) -> Option<&mut Sequence> {
        let id = self.active_sequence.clone().or_else(|| self.sequences.first().map(|s| s.id.clone()))?;
        self.sequence_mut(&id)
    }

    pub fn layer(&self, id: &LayerId) -> Option<&SemanticLayer> {
        self.layers.iter().find(|l| &l.layer_id == id)
    }

    pub fn layer_mut(&mut self, id: &LayerId) -> Option<&mut SemanticLayer> {
        self.layers.iter_mut().find(|l| &l.layer_id == id)
    }

    /// Capas visibles (no borradas) en el orden de carriles guardado; las
    /// desconocidas van después en orden de creación.
    pub fn ordered_layers(&self) -> Vec<&SemanticLayer> {
        let mut out: Vec<&SemanticLayer> = Vec::new();
        for id in &self.layer_order {
            if let Some(l) = self.layer(id)
                && !l.deleted
            {
                out.push(l);
            }
        }
        for l in &self.layers {
            if !l.deleted && !out.iter().any(|o| o.layer_id == l.layer_id) {
                out.push(l);
            }
        }
        out
    }
}

/// Fecha ISO-8601 UTC con segundos, sin dependencias externas.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Algoritmo de Howard Hinnant (días civiles desde epoch).
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00", y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_survive_round_trip() {
        let mut p = Project::new("demo");
        p.extra.insert("campo_futuro".into(), serde_json::json!({"a": 1}));
        let text = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&text).unwrap();
        assert_eq!(back.extra["campo_futuro"]["a"], 1);
        assert_eq!(back.schema, PROJECT_SCHEMA);
        assert_eq!(back, p);
    }

    #[test]
    fn iso_date_looks_right() {
        let s = now_iso();
        assert_eq!(s.len(), "2026-09-07T17:00:00+00:00".len(), "{s}");
        assert!(s.starts_with("20"));
    }
}
