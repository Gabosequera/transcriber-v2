//! `editorial-layer/1` ↔ `SemanticLayer`.
//!
//! Conserva: `layer_id`, `kind`, `name`, `color`, `revision`, `deleted`,
//! `deleted_item_ids`, `media_fingerprint`, `source_master_digest`, y por item
//! `item_id`, `label`, `comment`, `state`, `edited`, `parent_id`, `ranges` y
//! cualquier campo desconocido (en `extra`). No inventa estado `deleted` por item.
//! Los tiempos se escriben como float (V1 persiste `float(...)` tras validar).

use crate::master::{identity_json, parse_fingerprint};
use crate::{V1Result, invalid, secs_json, secs_to_ticks};
use serde_json::{Map, Value, json};
use tv2_domain::asset::Fingerprint;
use tv2_domain::ids::{AssetId, ItemId, LayerId};
use tv2_domain::layers::{ItemState, LayerKind, SemanticItem, SemanticLayer, validate_layer};
use tv2_domain::time::{Ticks, TimeRange};

pub const SCHEMA_LAYER: &str = "editorial-layer/1";

const KNOWN_LAYER_KEYS: &[&str] =
    &["schema", "layer_id", "kind", "name", "color", "media_fingerprint", "source_master_digest", "revision", "items", "deleted", "deleted_item_ids"];
const KNOWN_ITEM_KEYS: &[&str] = &["item_id", "label", "comment", "state", "edited", "parent_id", "ranges"];

pub fn kind_from_v1(kind: &str) -> LayerKind {
    match kind {
        "user" => LayerKind::User,
        "topics" => LayerKind::Topics,
        "ai" => LayerKind::Ai,
        "autor" => LayerKind::Author,
        "bloques" => LayerKind::Blocks,
        "recortes" => LayerKind::Trims,
        other => LayerKind::Other(other.to_string()),
    }
}

pub fn state_from_v1(s: &str) -> V1Result<ItemState> {
    match s {
        "proposed" => Ok(ItemState::Proposed),
        "accepted" => Ok(ItemState::Accepted),
        "disabled" => Ok(ItemState::Disabled),
        other => Err(invalid(format!("estado de capa inválido: {other}"))),
    }
}

pub fn item_from_v1(v: &Value) -> V1Result<SemanticItem> {
    let obj = v.as_object().ok_or_else(|| invalid("item no es un objeto"))?;
    let item_id = obj.get("item_id").and_then(|x| x.as_str()).ok_or_else(|| invalid("item sin item_id"))?;
    let ranges_v = obj.get("ranges").and_then(|x| x.as_array()).ok_or_else(|| invalid(format!("item {item_id} sin rangos")))?;
    let mut ranges = Vec::new();
    for r in ranges_v {
        let a = r.get("t_ini").and_then(|x| x.as_f64()).ok_or_else(|| invalid(format!("item {item_id}: t_ini inválido")))?;
        let b = r.get("t_fin").and_then(|x| x.as_f64()).ok_or_else(|| invalid(format!("item {item_id}: t_fin inválido")))?;
        if !a.is_finite() || !b.is_finite() {
            return Err(invalid(format!("item {item_id}: rango no finito")));
        }
        ranges.push(TimeRange::new(secs_to_ticks(a), secs_to_ticks(b)));
    }
    let mut extra = Map::new();
    for (k, val) in obj {
        if !KNOWN_ITEM_KEYS.contains(&k.as_str()) {
            extra.insert(k.clone(), val.clone());
        }
    }
    if ranges_v.iter().any(|r| r.as_object().is_some_and(|obj| obj.keys().any(|k| k != "t_ini" && k != "t_fin"))) {
        // Keep per-range provenance outside the mutable time model. The exact
        // original range remains archived after an edit; only matching ranges
        // regain those annotations on V1 export.
        extra.insert("tv2_v1_range_evidence".into(), Value::Array(ranges_v.clone()));
    }
    Ok(SemanticItem {
        item_id: ItemId::new(item_id),
        label: obj.get("label").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        comment: obj.get("comment").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        state: state_from_v1(obj.get("state").and_then(|x| x.as_str()).unwrap_or("proposed"))?,
        edited: obj.get("edited").and_then(|x| x.as_bool()).unwrap_or(false),
        parent_id: obj.get("parent_id").and_then(|x| x.as_str()).map(ItemId::new),
        ranges,
        origin: obj.get("origin").and_then(|x| x.as_str()).map(|s| s.to_string()),
        extra,
    })
}

pub fn item_to_v1(it: &SemanticItem) -> Value {
    let mut obj = Map::new();
    obj.insert("item_id".into(), json!(it.item_id.as_str()));
    obj.insert("label".into(), json!(it.label));
    obj.insert("comment".into(), json!(it.comment));
    obj.insert("state".into(), json!(it.state.as_str()));
    obj.insert("edited".into(), json!(it.edited));
    obj.insert("parent_id".into(), it.parent_id.as_ref().map(|p| json!(p.as_str())).unwrap_or(Value::Null));
    obj.insert(
        "ranges".into(),
        Value::Array(
            it.ranges
                .iter()
                .map(|r| {
                    let mut value = it
                        .extra
                        .get("tv2_v1_range_evidence")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .find(|v| {
                            v["t_ini"].as_f64().is_some_and(|a| secs_to_ticks(a) == r.start)
                                && v["t_fin"].as_f64().is_some_and(|b| secs_to_ticks(b) == r.end)
                        })
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    value["t_ini"] = secs_json(r.start);
                    value["t_fin"] = secs_json(r.end);
                    value
                })
                .collect(),
        ),
    );
    for (k, v) in &it.extra {
        obj.entry(k.clone()).or_insert(v.clone());
    }
    if let Some(o) = &it.origin
        && !obj.contains_key("origin")
    {
        obj.insert("origin".into(), json!(o));
    }
    Value::Object(obj)
}

/// Convierte una capa V1 al dominio. `asset_id` es el asset cuya identidad coincide
/// con `media_fingerprint`; `duration` valida los rangos.
pub fn layer_from_v1(v: &Value, asset_id: &AssetId, expected: Option<&Fingerprint>, duration: Ticks) -> V1Result<SemanticLayer> {
    let obj = v.as_object().ok_or_else(|| invalid("capa no es un objeto"))?;
    if obj.get("schema").and_then(|x| x.as_str()) != Some(SCHEMA_LAYER) {
        return Err(invalid(format!("schema de capa desconocido: {:?}", obj.get("schema"))));
    }
    let layer_id = obj.get("layer_id").and_then(|x| x.as_str()).ok_or_else(|| invalid("capa sin layer_id"))?;
    let fp = obj.get("media_fingerprint").map(parse_fingerprint).transpose()?;
    if let (Some(exp), Some(fp)) = (expected, &fp)
        && !exp.same_identity(fp)
    {
        return Err(invalid(format!("la capa {layer_id} pertenece a otro medio")));
    }
    let items = obj
        .get("items")
        .and_then(|x| x.as_array())
        .ok_or_else(|| invalid(format!("capa {layer_id} sin items")))?
        .iter()
        .map(item_from_v1)
        .collect::<V1Result<Vec<_>>>()?;
    let mut extra = Map::new();
    for (k, val) in obj {
        if !KNOWN_LAYER_KEYS.contains(&k.as_str()) {
            extra.insert(k.clone(), val.clone());
        }
    }
    if let Some(fp) = &fp {
        extra.insert("v1_media_fingerprint".into(), identity_json(fp));
    }
    let kind = kind_from_v1(obj.get("kind").and_then(|x| x.as_str()).unwrap_or("user"));
    let layer = SemanticLayer {
        layer_id: LayerId::new(layer_id),
        kind,
        name: obj.get("name").and_then(|x| x.as_str()).unwrap_or(layer_id).to_string(),
        color: obj.get("color").and_then(|x| x.as_str()).unwrap_or("#d09947").to_string(),
        asset_id: asset_id.clone(),
        items: items.into(),
        deleted_item_ids: obj
            .get("deleted_item_ids")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|i| i.as_str()).map(ItemId::new).collect())
            .unwrap_or_default(),
        revision: obj.get("revision").and_then(|x| x.as_u64()).unwrap_or(0),
        deleted: obj.get("deleted").and_then(|x| x.as_bool()).unwrap_or(false),
        visible: true,
        locked: false,
        lane: None,
        source_master_digest: obj.get("source_master_digest").and_then(|x| x.as_str()).map(|s| s.to_string()),
        extra,
    };
    validate_layer(&layer, duration).map_err(|e| invalid(format!("capa {layer_id}: {}", e.message)))?;
    Ok(layer)
}

/// Escribe una capa como `editorial-layer/1`.
pub fn layer_to_v1(layer: &SemanticLayer, fingerprint: &Fingerprint) -> Value {
    let mut obj = Map::new();
    obj.insert("schema".into(), json!(SCHEMA_LAYER));
    obj.insert("layer_id".into(), json!(layer.layer_id.as_str()));
    obj.insert("kind".into(), json!(layer.kind.v1_name()));
    obj.insert("name".into(), json!(layer.name));
    obj.insert("color".into(), json!(layer.color));
    obj.insert("media_fingerprint".into(), layer.extra.get("v1_media_fingerprint").cloned().unwrap_or_else(|| identity_json(fingerprint)));
    if let Some(d) = &layer.source_master_digest {
        obj.insert("source_master_digest".into(), json!(d));
    }
    obj.insert("revision".into(), json!(layer.revision));
    obj.insert("items".into(), Value::Array(layer.items.iter().map(item_to_v1).collect()));
    if layer.deleted {
        obj.insert("deleted".into(), json!(true));
    }
    if !layer.deleted_item_ids.is_empty() {
        obj.insert("deleted_item_ids".into(), Value::Array(layer.deleted_item_ids.iter().map(|i| json!(i.as_str())).collect()));
    }
    for (k, v) in &layer.extra {
        if k != "v1_media_fingerprint" && k != "tv2_v1_source_path" {
            obj.entry(k.clone()).or_insert(v.clone());
        }
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::digest::digest_json;

    fn fp() -> Fingerprint {
        Fingerprint { size: 1, mtime_ns: None, hash_muestreado: "a".into(), inventario_sha256: "b".into() }
    }

    fn layer_json() -> Value {
        json!({"schema": "editorial-layer/1", "layer_id": "layer-4c8856d5ad19", "kind": "topics", "name": "Temas", "color": "#d09947",
               "media_fingerprint": {"size": 1, "hash_muestreado": "a", "inventario_sha256": "b"},
               "source_master_digest": "abc", "revision": 3,
               "items": [
                 {"item_id": "item-t1", "label": "Tema", "comment": "", "state": "accepted", "edited": true, "parent_id": null,
                  "ranges": [{"t_ini": 0.0, "t_fin": 3.0}, {"t_ini": 7.0, "t_fin": 12.0}], "evidence": {"utterance_ids": ["A-u-1"]}},
                 {"item_id": "item-s1", "label": "Subtema", "comment": "nota", "state": "proposed", "edited": false, "parent_id": "item-t1",
                  "ranges": [{"t_ini": 8.25, "t_fin": 9.5}]}
               ],
               "deleted_item_ids": ["item-viejo"], "campo_futuro": {"x": 1}})
    }

    #[test]
    fn round_trip_preserves_ids_ranges_states_and_unknown_fields() {
        let raw = layer_json();
        let layer = layer_from_v1(&raw, &AssetId::new("asset"), Some(&fp()), Ticks::from_seconds(12)).unwrap();
        assert_eq!(layer.kind, LayerKind::Topics);
        assert_eq!(layer.items[1].parent_id.as_ref().unwrap().as_str(), "item-t1");
        assert_eq!(layer.items[1].ranges[0].start, Ticks::from_millis(8250));
        assert_eq!(layer.items[0].state, ItemState::Accepted);
        assert_eq!(layer.deleted_item_ids[0].as_str(), "item-viejo");
        assert_eq!(layer.extra["campo_futuro"]["x"], 1);
        let back = layer_to_v1(&layer, &fp());
        assert_eq!(digest_json(&back), digest_json(&raw), "round-trip canónico idéntico:\n{}\n{}", back, raw);
    }

    #[test]
    fn rejects_other_media_and_bad_hierarchy() {
        let raw = layer_json();
        let other = Fingerprint { size: 2, ..fp() };
        assert!(layer_from_v1(&raw, &AssetId::new("asset"), Some(&other), Ticks::from_seconds(12)).is_err());
        let mut bad = layer_json();
        bad["items"][1]["ranges"][0]["t_ini"] = json!(4.0);
        bad["items"][1]["ranges"][0]["t_fin"] = json!(5.0);
        assert!(layer_from_v1(&bad, &AssetId::new("asset"), Some(&fp()), Ticks::from_seconds(12)).is_err());
    }
}
