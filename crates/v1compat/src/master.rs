//! `editorial-master/1`: original evidence plus read-only temporal projections.

use crate::{V1Result, invalid, secs_to_ticks};
use serde_json::Value;
use std::path::Path;
use tv2_domain::asset::Fingerprint;
use tv2_domain::digest::digest_json;
use tv2_domain::time::Ticks;

pub const SCHEMA_MASTER: &str = "editorial-master/1";

#[derive(Clone, Debug)]
pub struct V1Master {
    pub project_name: String,
    pub media_path: String,
    pub duration: Ticks,
    pub t0: Ticks,
    pub fingerprint: Fingerprint,
    pub raw: Value,
}

impl V1Master {
    pub fn parse(raw: Value) -> V1Result<V1Master> {
        if raw.get("schema").and_then(|v| v.as_str()) != Some(SCHEMA_MASTER) {
            return Err(invalid(format!("schema de master desconocido: {:?}", raw.get("schema"))));
        }
        let media = raw.get("media").ok_or_else(|| invalid("master sin `media`"))?;
        let duration = media.get("duration").and_then(|v| v.as_f64()).ok_or_else(|| invalid("master sin `media.duration`"))?;
        if !duration.is_finite() || duration <= 0.0 || duration > Ticks::MAX.as_seconds_f64() {
            return Err(invalid("duración de master inválida"));
        }
        let fingerprint = parse_fingerprint(media.get("fingerprint").ok_or_else(|| invalid("master sin `media.fingerprint`"))?)?;
        Ok(V1Master {
            project_name: raw.pointer("/project/name").and_then(|v| v.as_str()).unwrap_or("proyecto").to_string(),
            media_path: media.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            duration: secs_to_ticks(duration),
            t0: secs_to_ticks(media.get("t0").and_then(|v| v.as_f64()).unwrap_or(0.0)),
            fingerprint,
            raw,
        })
    }

    pub fn load(path: &Path) -> V1Result<V1Master> {
        let text = std::fs::read_to_string(path)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        V1Master::parse(serde_json::from_str(text)?)
    }

    /// `editorial_chunks.source_master_digest`: digest del master sin `generated_at` ni `chunks`.
    pub fn source_master_digest(&self) -> String {
        let mut canonical = self.raw.clone();
        if let Some(obj) = canonical.as_object_mut() {
            obj.remove("generated_at");
            obj.remove("chunks");
        }
        digest_json(&canonical)
    }

    pub fn evidence(&self, asset_id: &tv2_domain::AssetId) -> tv2_domain::evidence::MasterEvidence {
        tv2_domain::evidence::MasterEvidence {
            asset_id: asset_id.clone(),
            source_digest: self.source_master_digest(),
            document: self.raw.clone().into(),
            source_bundle: None,
        }
    }

    /// Project words, utterances/speakers and signals without rewriting the
    /// master. Original records and IDs remain available in each item's evidence.
    pub fn projections(&self, asset_id: &tv2_domain::AssetId) -> V1Result<Vec<tv2_domain::SemanticLayer>> {
        use tv2_domain::{ItemId, ItemState, LayerId, LayerKind, SemanticItem, SemanticLayer, TimeRange};
        let mut layers = Vec::new();
        let Some(tracks) = self.raw.get("tracks").and_then(Value::as_object) else {
            return Ok(layers);
        };
        for (track_id, track) in tracks {
            for (field, id_key, kind, label) in [
                ("words", "word_id", LayerKind::Transcript, "Palabras"),
                ("utterances", "utterance_id", LayerKind::Speakers, "Intervenciones"),
                ("laughter", "event_id", LayerKind::Laughter, "Risas"),
                ("arousal", "event_id", LayerKind::Arousal, "Arousal"),
                ("emotions", "event_id", LayerKind::Signals, "Emociones"),
            ] {
                let Some(value) = track.get(field) else {
                    continue;
                };
                let records = value.as_array().ok_or_else(|| invalid(format!("tracks/{track_id}/{field} no es una lista")))?;
                if records.is_empty() {
                    continue;
                }
                let key = digest_json(&serde_json::json!([asset_id, track_id, field]));
                let mut layer = SemanticLayer::new(asset_id.clone(), kind, format!("{label} · {track_id}"));
                layer.layer_id = LayerId::new(format!("master-{}", &key[..24]));
                layer.locked = true;
                layer.source_master_digest = Some(self.source_master_digest());
                layer.extra.insert("master_projection".into(), serde_json::json!({"track_id":track_id,"collection":field}));
                for record in records {
                    let id = record.get(id_key).and_then(Value::as_str).ok_or_else(|| invalid(format!("{field}: falta {id_key}")))?;
                    let start = record.get("t_ini").and_then(Value::as_f64).ok_or_else(|| invalid(format!("{id}: falta t_ini")))?;
                    let end = record.get("t_fin").and_then(Value::as_f64).ok_or_else(|| invalid(format!("{id}: falta t_fin")))?;
                    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || end > self.duration.as_seconds_f64() {
                        return Err(invalid(format!("{id}: rango de evidencia inválido")));
                    }
                    let item_id = if tv2_domain::ids::is_valid_v1_id(id) {
                        ItemId::new(id)
                    } else {
                        ItemId::new(format!("evidence-{}", &digest_json(&serde_json::json!(id))[..24]))
                    };
                    let mut extra = serde_json::Map::new();
                    extra.insert("evidence".into(), record.clone());
                    layer.items.push(SemanticItem {
                        item_id,
                        label: record.get("text").and_then(Value::as_str).unwrap_or(id).to_string(),
                        comment: String::new(),
                        state: ItemState::Proposed,
                        edited: false,
                        parent_id: None,
                        ranges: vec![TimeRange::new(secs_to_ticks(start), secs_to_ticks(end))],
                        origin: Some("master-v1".into()),
                        extra,
                    });
                }
                tv2_domain::layers::validate_layer(&layer, self.duration).map_err(|e| invalid(e.to_string()))?;
                layers.push(layer);
            }
        }
        Ok(layers)
    }
}

/// `editorial_trims.identity`: solo las tres claves de identidad.
pub fn parse_fingerprint(v: &Value) -> V1Result<Fingerprint> {
    let size = v.get("size").and_then(|s| s.as_u64()).ok_or_else(|| invalid("fingerprint sin `size`"))?;
    let hash = v.get("hash_muestreado").and_then(|s| s.as_str()).ok_or_else(|| invalid("fingerprint sin `hash_muestreado`"))?;
    let inv = v.get("inventario_sha256").and_then(|s| s.as_str()).ok_or_else(|| invalid("fingerprint sin `inventario_sha256`"))?;
    Ok(Fingerprint {
        size,
        mtime_ns: v.get("mtime_ns").and_then(|s| s.as_i64()).map(|n| n as i128),
        hash_muestreado: hash.to_string(),
        inventario_sha256: inv.to_string(),
    })
}

pub fn identity_json(fp: &Fingerprint) -> Value {
    serde_json::json!({"size": fp.size, "hash_muestreado": fp.hash_muestreado, "inventario_sha256": fp.inventario_sha256})
}

#[cfg(test)]
pub(crate) mod fixtures {
    use serde_json::{Value, json};

    /// Equivalente a `tests/test_projects.py::fixture` de V1 (12 s, pistas A y B).
    pub fn master(duration: f64) -> Value {
        let mut tracks = serde_json::Map::new();
        let mut utterances = Vec::new();
        for (i, tid) in ["A", "B"].iter().enumerate() {
            let words: Vec<Value> = [(1, 2, "inicio"), (4, 5, "eliminado"), (8, 9, "retorno")]
                .iter()
                .enumerate()
                .map(|(j, (a, b, text))| json!({"word_id": format!("{tid}-w-{j}"), "track_id": tid, "text": text, "t_ini": a, "t_fin": b, "arousal": 0.7, "intensity_z": 1.2, "emphasis_score": 0.8}))
                .collect();
            let utt = json!({"utterance_id": format!("{tid}-u-1"), "track_id": tid, "text": "inicio eliminado retorno", "t_ini": 1, "t_fin": 9, "word_ids": words.iter().map(|w| w["word_id"].clone()).collect::<Vec<_>>(), "signals": {}});
            utterances.push(utt.clone());
            tracks.insert(
                tid.to_string(),
                json!({"track_id": tid, "label": tid, "stream_index": i, "offset": 0, "words": words, "utterances": [utt],
                "laughter": [{"event_id": format!("{tid}-r-1"), "t_ini": 2, "t_fin": 8, "max_conf": 0.9}],
                "arousal": [{"event_id": format!("{tid}-a-1"), "t_ini": 0, "t_fin": 10, "arousal_z": 0.8, "valence": 0.6, "dominance": 0.5}],
                "emotions": [{"event_id": format!("{tid}-e-1"), "t_ini": 8, "t_fin": 9, "value": 0.5}]}),
            );
        }
        let ids: Vec<Value> = utterances.iter().map(|u| u["utterance_id"].clone()).collect();
        json!({"schema": "editorial-master/1", "project": {"name": "padre"},
               "media": {"path": "padre.mkv", "duration": duration, "fingerprint": {"size": 1, "hash_muestreado": "a", "inventario_sha256": "b"}},
               "tracks": tracks, "conversation": {"utterances": utterances, "clean_utterance_ids": ids}, "chunks": []})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_master_and_digest_ignores_chunks() {
        let m = V1Master::parse(fixtures::master(12.0)).unwrap();
        assert_eq!(m.duration, Ticks::from_seconds(12));
        assert_eq!(m.fingerprint.hash_muestreado, "a");
        let d1 = m.source_master_digest();
        let mut raw2 = fixtures::master(12.0);
        raw2["chunks"] = serde_json::json!([{"x": 1}]);
        raw2["generated_at"] = serde_json::json!("2026");
        assert_eq!(V1Master::parse(raw2).unwrap().source_master_digest(), d1);
    }

    #[test]
    fn projections_preserve_original_records_and_have_stable_ids() {
        let master = V1Master::parse(fixtures::master(12.0)).unwrap();
        let layers = master.projections(&"asset-a".into()).unwrap();
        assert_eq!(layers.len(), 10);
        assert_eq!(master.projections(&"asset-a".into()).unwrap(), layers);
        assert!(layers.iter().all(|l| l.locked && !l.kind.is_editable()));
        let words = layers.iter().find(|l| l.name == "Palabras · A").unwrap();
        assert_eq!(words.items[0].extra["evidence"], master.raw["tracks"]["A"]["words"][0]);
        assert_eq!(master.evidence(&"asset-a".into()).document, fixtures::master(12.0));
    }
}
