//! `editorial-master/1`: se lee lo necesario para identidad, duración y nombre;
//! el documento completo se conserva como `serde_json::Value` (transcripción,
//! señales y conversación se proyectan a capas en E3/E5).

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
}
