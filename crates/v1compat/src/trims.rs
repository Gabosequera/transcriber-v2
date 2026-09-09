//! `editorial-trims/1` ↔ capas de recortes (una por `lane`, id `trims-<lane>`;
//! V1 usa `trims:<lane>` solo como id de carril de UI, no persistido).
//!
//! Regla V1 (`editorial_layers.cut_state`): `enabled=false` → `disabled`;
//! `accepted` → `accepted`; si no → `proposed`. La exportación mira solo
//! `enabled` (unión de todos los carriles). Se conservan `origin`, `reason`,
//! `confidence`, `chunk_id`, `evidence`, `warnings`, `edited` y campos desconocidos.

use crate::master::{identity_json, parse_fingerprint};
use crate::{V1Result, invalid, secs_json, secs_to_ticks};
use serde_json::{Map, Value, json};
use tv2_domain::asset::Fingerprint;
use tv2_domain::ids::{AssetId, ItemId, LayerId};
use tv2_domain::layers::{ItemState, LayerKind, SemanticItem, SemanticLayer, merge_intervals};
use tv2_domain::time::{Ticks, TimeRange};

pub const SCHEMA_TRIMS: &str = "editorial-trims/1";

const KNOWN_CUT_KEYS: &[&str] = &["cut_id", "t_ini", "t_fin", "origin", "lane", "enabled", "accepted", "edited", "reason", "tv2_label"];

#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub lane_id: String,
    pub name: String,
    pub color: String,
    pub extra: Map<String, Value>,
}

pub fn default_lanes() -> Vec<Lane> {
    vec![
        Lane { lane_id: "main".into(), name: "Recortes".into(), color: "#728bd0".into(), extra: Map::new() },
        Lane { lane_id: "ai".into(), name: "Cortes sugeridos (AI)".into(), color: "#8ab06b".into(), extra: Map::new() },
    ]
}

/// Documento de recortes V1 convertido: capas por carril más los campos de cabecera.
#[derive(Clone, Debug)]
pub struct V1Trims {
    pub duration: Ticks,
    pub revision: u64,
    pub next_id: u64,
    pub lanes: Vec<Lane>,
    pub layers: Vec<SemanticLayer>,
    /// Cabecera V1 sin `cuts`/`lanes` (silence, ai, updated_at, media…), para reescribir.
    pub header: Map<String, Value>,
}

pub fn trims_layer_id(lane: &str) -> LayerId {
    LayerId::new(format!("trims-{lane}"))
}

pub fn cut_state(cut: &Value) -> ItemState {
    if !cut.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true) {
        ItemState::Disabled
    } else if cut.get("accepted").and_then(|v| v.as_bool()).unwrap_or(false) {
        ItemState::Accepted
    } else {
        ItemState::Proposed
    }
}

pub fn trims_from_v1(v: &Value, asset_id: &AssetId, expected: Option<&Fingerprint>) -> V1Result<V1Trims> {
    let obj = v.as_object().ok_or_else(|| invalid("trims no es un objeto"))?;
    if obj.get("schema").and_then(|x| x.as_str()) != Some(SCHEMA_TRIMS) {
        return Err(invalid(format!("schema de recortes desconocido: {:?}", obj.get("schema"))));
    }
    if let (Some(exp), Some(fp)) = (expected, obj.get("media").map(parse_fingerprint).transpose()?)
        && !exp.same_identity(&fp)
    {
        return Err(invalid("los recortes pertenecen a otro video"));
    }
    let duration = secs_to_ticks(obj.get("duration").and_then(|x| x.as_f64()).ok_or_else(|| invalid("trims sin duration"))?);
    if duration <= Ticks::ZERO || duration == Ticks::MAX {
        return Err(invalid("duración de recortes no representable"));
    }
    let lanes: Vec<Lane> = match obj.get("lanes").and_then(|x| x.as_array()) {
        Some(arr) => arr
            .iter()
            .map(|l| Lane {
                lane_id: l.get("lane_id").and_then(|x| x.as_str()).unwrap_or("main").to_string(),
                name: l.get("name").and_then(|x| x.as_str()).unwrap_or("Recortes").to_string(),
                color: l.get("color").and_then(|x| x.as_str()).unwrap_or("#728bd0").to_string(),
                extra: l.as_object().cloned().unwrap_or_default(),
            })
            .collect(),
        None => default_lanes(),
    };
    let cuts = obj.get("cuts").and_then(|x| x.as_array()).ok_or_else(|| invalid("trims sin cuts"))?;
    let mut lanes = lanes;
    let mut layers: Vec<SemanticLayer> = Vec::new();
    for cut in cuts {
        let c = cut.as_object().ok_or_else(|| invalid("recorte no es un objeto"))?;
        let cut_id = c.get("cut_id").and_then(|x| x.as_str()).ok_or_else(|| invalid("recorte sin cut_id"))?;
        let origin = c.get("origin").and_then(|x| x.as_str()).unwrap_or("user").to_string();
        let lane =
            c.get("lane").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| if origin == "ai" { "ai".into() } else { "main".into() });
        if !lanes.iter().any(|l| l.lane_id == lane) {
            lanes.push(Lane { lane_id: lane.clone(), name: lane.clone(), color: "#c58e43".into(), extra: Map::new() });
        }
        let a = c.get("t_ini").and_then(|x| x.as_f64()).ok_or_else(|| invalid(format!("{cut_id}: t_ini inválido")))?;
        let b = c.get("t_fin").and_then(|x| x.as_f64()).ok_or_else(|| invalid(format!("{cut_id}: t_fin inválido")))?;
        let mut extra = Map::new();
        for (k, val) in c {
            if !KNOWN_CUT_KEYS.contains(&k.as_str()) {
                extra.insert(k.clone(), val.clone());
            }
        }
        extra.insert("enabled".into(), json!(c.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true)));
        extra.insert("accepted".into(), json!(c.get("accepted").and_then(|x| x.as_bool()).unwrap_or(false)));
        let item = SemanticItem {
            item_id: ItemId::new(cut_id),
            label: c.get("tv2_label").and_then(Value::as_str).unwrap_or(cut_id).to_string(),
            comment: c.get("reason").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            state: cut_state(cut),
            edited: c.get("edited").and_then(|x| x.as_bool()).unwrap_or(false),
            parent_id: None,
            ranges: vec![TimeRange::new(secs_to_ticks(a), secs_to_ticks(b))],
            origin: Some(origin),
            extra,
        };
        let layer = match layers.iter_mut().find(|l| l.lane.as_deref() == Some(lane.as_str())) {
            Some(l) => l,
            None => {
                let meta = lanes.iter().find(|l| l.lane_id == lane).cloned().unwrap();
                let mut l = SemanticLayer::new(asset_id.clone(), LayerKind::Trims, meta.name.clone());
                l.layer_id = trims_layer_id(&lane);
                l.color = meta.color.clone();
                l.lane = Some(lane.clone());
                layers.push(l);
                layers.last_mut().unwrap()
            }
        };
        layer.items.push(item);
    }
    // carriles sin cortes también existen (para poder añadir)
    for lane in &lanes {
        if !layers.iter().any(|l| l.lane.as_deref() == Some(lane.lane_id.as_str())) {
            let mut l = SemanticLayer::new(asset_id.clone(), LayerKind::Trims, lane.name.clone());
            l.layer_id = trims_layer_id(&lane.lane_id);
            l.color = lane.color.clone();
            l.lane = Some(lane.lane_id.clone());
            layers.push(l);
        }
    }
    let mut header = Map::new();
    for (k, val) in obj {
        if k != "cuts" && k != "lanes" {
            header.insert(k.clone(), val.clone());
        }
    }
    let revision = obj.get("revision").and_then(|x| x.as_u64()).unwrap_or(0);
    for l in &mut layers {
        l.revision = revision;
        l.extra.insert("v1_import_layer_revision".into(), json!(revision));
        l.extra.insert("v1_media_fingerprint".into(), obj.get("media").cloned().unwrap_or(Value::Null));
        let meta = lanes.iter().find(|m| Some(m.lane_id.as_str()) == l.lane.as_deref()).unwrap();
        l.extra.insert("v1_lane_metadata".into(), Value::Object(meta.extra.clone()));
        tv2_domain::layers::validate_layer(l, duration).map_err(|e| invalid(e.to_string()))?;
    }
    let ids: std::collections::HashSet<_> = layers.iter().flat_map(|l| l.items.iter().map(|i| &i.item_id)).collect();
    if ids.len() != cuts.len() {
        return Err(invalid("cut_id duplicado entre carriles"));
    }
    // One normalized authoritative document: header stored once, cuts in lanes.
    // No second editable copy of cuts is retained.
    if let Some(owner) = layers.first_mut() {
        owner.extra.insert("v1_trims_header".into(), Value::Object(header.clone()));
    }
    Ok(V1Trims { duration, revision, next_id: obj.get("next_id").and_then(|x| x.as_u64()).unwrap_or(1), lanes, layers, header })
}

/// Unión de los cortes habilitados de todas las capas de recortes (como `enabled_intervals`).
pub fn enabled_intervals(layers: &[&SemanticLayer]) -> Vec<TimeRange> {
    let mut all: Vec<TimeRange> =
        layers.iter().flat_map(|l| l.items.iter().filter(|i| i.state.is_enabled()).flat_map(|i| i.ranges.iter().copied())).collect();
    merge_intervals(&mut all)
}

/// Reescribe el documento `editorial-trims/1` a partir de las capas de recortes.
pub fn trims_to_v1(t: &V1Trims, fingerprint: &Fingerprint) -> Value {
    let mut obj = t.header.clone();
    obj.insert("schema".into(), json!(SCHEMA_TRIMS));
    obj.entry("media".to_string()).or_insert_with(|| identity_json(fingerprint));
    obj.insert("duration".into(), secs_json(t.duration));
    obj.insert("revision".into(), json!(t.revision));
    obj.insert("next_id".into(), json!(t.next_id));
    obj.insert(
        "lanes".into(),
        Value::Array(
            t.lanes
                .iter()
                .map(|l| {
                    let mut lane = l.extra.clone();
                    lane.insert("lane_id".into(), json!(l.lane_id));
                    lane.insert("name".into(), json!(l.name));
                    lane.insert("color".into(), json!(l.color));
                    Value::Object(lane)
                })
                .collect(),
        ),
    );
    let mut cuts: Vec<(Ticks, Value)> = Vec::new();
    for layer in &t.layers {
        for it in &layer.items {
            let mut c = Map::new();
            c.insert("cut_id".into(), json!(it.item_id.as_str()));
            c.insert("t_ini".into(), secs_json(it.ranges[0].start));
            c.insert("t_fin".into(), secs_json(it.ranges[0].end));
            c.insert("origin".into(), json!(it.origin.clone().unwrap_or_else(|| "user".into())));
            c.insert("lane".into(), json!(layer.lane.clone().unwrap_or_else(|| "main".into())));
            c.insert("enabled".into(), json!(it.state.is_enabled()));
            c.insert("accepted".into(), json!(it.is_human_accepted()));
            c.insert("edited".into(), json!(it.edited));
            c.insert("reason".into(), json!(it.comment));
            if it.label != it.item_id.as_str() {
                c.insert("tv2_label".into(), json!(it.label));
            }
            for (k, v) in &it.extra {
                if k != "enabled" && k != "accepted" {
                    c.entry(k.clone()).or_insert(v.clone());
                }
            }
            cuts.push((it.ranges[0].start, Value::Object(c)));
        }
    }
    cuts.sort_by_key(|(t, v)| (*t, v["cut_id"].as_str().unwrap_or("").to_string()));
    obj.insert("cuts".into(), Value::Array(cuts.into_iter().map(|(_, v)| v).collect()));
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::digest::digest_json;

    fn fp() -> Fingerprint {
        Fingerprint { size: 123, mtime_ns: None, hash_muestreado: "abc".into(), inventario_sha256: "def".into() }
    }

    fn doc() -> Value {
        json!({"schema": "editorial-trims/1", "media": {"size": 123, "hash_muestreado": "abc", "inventario_sha256": "def"},
        "duration": 60.0, "revision": 4, "next_id": 4, "updated_at": "2026-09-07T00:00:00+00:00", "silence": null, "ai": {"pass": 1},
        "lanes": [{"lane_id": "main", "name": "Recortes", "color": "#728bd0"}, {"lane_id": "ai", "name": "Cortes sugeridos (AI)", "color": "#8ab06b"}],
        "cuts": [
          {"cut_id": "cut-000001", "t_ini": 3.0, "t_fin": 7.0, "origin": "user", "lane": "main", "enabled": true, "accepted": true, "edited": true, "reason": "prueba", "confidence": null, "chunk_id": null, "evidence": {}, "warnings": []},
          {"cut_id": "cut-000002", "t_ini": 5.0, "t_fin": 9.5, "origin": "ai", "lane": "ai", "enabled": true, "accepted": false, "edited": false, "reason": "tangente", "confidence": 0.8, "chunk_id": "c1", "evidence": {"utterance_ids": ["A-u-1"]}, "warnings": ["w"]},
          {"cut_id": "cut-000003", "t_ini": 20.0, "t_fin": 21.0, "origin": "silence", "lane": "main", "enabled": false, "accepted": false, "edited": true, "reason": "", "confidence": null, "chunk_id": null, "evidence": {}, "warnings": []}
        ]})
    }

    #[test]
    fn states_follow_cut_state_and_export_union_ignores_acceptance() {
        let t = trims_from_v1(&doc(), &AssetId::new("a"), Some(&fp())).unwrap();
        assert_eq!(t.layers.len(), 2);
        let main = t.layers.iter().find(|l| l.lane.as_deref() == Some("main")).unwrap();
        let ai = t.layers.iter().find(|l| l.lane.as_deref() == Some("ai")).unwrap();
        assert_eq!(main.items[0].state, ItemState::Accepted);
        assert_eq!(main.items[1].state, ItemState::Disabled);
        assert_eq!(ai.items[0].state, ItemState::Proposed);
        // unión de habilitados: 3–7 (aceptado) ∪ 5–9.5 (propuesto) → 3–9.5; el desactivado no corta
        assert_eq!(enabled_intervals(&[main, ai]), vec![TimeRange::new(Ticks::from_seconds(3), Ticks::from_millis(9500))]);
        assert_eq!(main.layer_id.as_str(), "trims-main");
    }

    #[test]
    fn round_trip_is_canonically_identical() {
        let raw = doc();
        let t = trims_from_v1(&raw, &AssetId::new("a"), Some(&fp())).unwrap();
        let back = trims_to_v1(&t, &fp());
        assert_eq!(digest_json(&back), digest_json(&raw), "\n{}\n{}", back, raw);
    }

    #[test]
    fn disabled_cut_retains_acceptance_through_import_export_and_activation() {
        use tv2_domain::{Command, Project};
        let mut raw = doc();
        raw["cuts"][0]["enabled"] = json!(false);
        let mut t = trims_from_v1(&raw, &AssetId::new("a"), Some(&fp())).unwrap();
        assert_eq!(trims_to_v1(&t, &fp()), raw);
        let l = t.layers.iter().find(|l| l.lane.as_deref() == Some("main")).unwrap();
        let mut p = Project::new("review");
        p.layers = t.layers.clone();
        Command::SetItemState { layer_id: l.layer_id.clone(), item_ids: vec!["cut-000001".into()], state: ItemState::Proposed }
            .apply(&mut p)
            .unwrap();
        t.layers = p.layers;
        let exported = trims_to_v1(&t, &fp());
        assert_eq!(exported["cuts"][0]["enabled"], true);
        assert_eq!(exported["cuts"][0]["accepted"], true);
    }

    #[test]
    fn old_document_without_lane_gets_lane_by_origin() {
        let mut raw = doc();
        raw.as_object_mut().unwrap().remove("lanes");
        for c in raw["cuts"].as_array_mut().unwrap() {
            c.as_object_mut().unwrap().remove("lane");
        }
        let t = trims_from_v1(&raw, &AssetId::new("a"), None).unwrap();
        let ai = t.layers.iter().find(|l| l.lane.as_deref() == Some("ai")).unwrap();
        assert_eq!(ai.items.len(), 1);
        assert_eq!(t.lanes.len(), 2);
    }
}
