//! The selected plan is normalized into one header + one item collection.
//! A chunks view is never imported as a second independent editable layer.
use crate::{V1Result, invalid, secs_json, secs_to_ticks};
use serde_json::{Value, json};
use tv2_domain::{AssetId, LayerKind, SemanticItem, SemanticLayer, Ticks, TimeRange};

pub fn from_v1(raw: &Value, asset: &AssetId, duration: Ticks, master_digest: &str) -> V1Result<SemanticLayer> {
    if raw["schema"] != "editorial-chunks/1" {
        return Err(invalid("schema de chunks desconocido"));
    }
    if raw.get("source_master_digest").is_some_and(|v| !v.is_null() && v.as_str() != Some(master_digest)) {
        return Err(invalid("plan de otro master o metadata desactualizada"));
    }
    let chunks = raw["chunks"].as_array().ok_or_else(|| invalid("plan sin chunks"))?;
    let mut layer = SemanticLayer::new(asset.clone(), LayerKind::Blocks, "Bloques");
    layer.layer_id = tv2_domain::LayerId::new(format!("blocks-{}", &tv2_domain::digest::digest_json(&json!(asset))[..16]));
    layer.source_master_digest = Some(master_digest.into());
    let mut header = raw.as_object().cloned().ok_or_else(|| invalid("plan inválido"))?;
    header.remove("chunks");
    layer.extra.insert("v1_chunks_header".into(), Value::Object(header));
    layer.revision = raw["revision"].as_u64().unwrap_or(0);
    let mut previous = Ticks::ZERO;
    for chunk in chunks {
        let id = chunk["chunk_id"].as_str().ok_or_else(|| invalid("chunk sin id"))?;
        let start = chunk["t_ini"].as_f64().ok_or_else(|| invalid("chunk sin inicio"))?;
        let end = chunk["t_fin"].as_f64().ok_or_else(|| invalid("chunk sin fin"))?;
        if start < 0.0
            || !start.is_finite()
            || !end.is_finite()
            || start > duration.as_seconds_f64()
            || end < start
            || end > duration.as_seconds_f64() + 0.001
        {
            return Err(invalid("tiempo inválido"));
        }
        let mut start = secs_to_ticks(start);
        let mut end = secs_to_ticks(end);
        if (start - previous).0.abs() <= Ticks::from_millis(1).0 {
            start = previous;
        }
        if (end - duration).0.abs() <= Ticks::from_millis(1).0 {
            end = duration;
        }
        let mut item = SemanticItem::new(TimeRange::new(start, end), chunk["title"].as_str().unwrap_or(id));
        item.item_id = id.into();
        item.comment = chunk.get("comment").or_else(|| chunk.get("summary")).and_then(Value::as_str).unwrap_or("").into();
        item.edited = chunk["edited"].as_bool().unwrap_or(false);
        item.origin = Some("import-v1".into());
        item.extra = chunk.as_object().cloned().ok_or_else(|| invalid("chunk inválido"))?;
        for key in ["chunk_id", "title", "comment", "t_ini", "t_fin", "edited"] {
            item.extra.remove(key);
        }
        item.extra.insert("tv2_original_range".into(), json!([start, end]));
        layer.items.push(item);
        previous = end;
    }
    tv2_domain::layers::validate_layer(&layer, duration).map_err(|e| invalid(e.to_string()))?;
    Ok(layer)
}

pub fn to_v1(layer: &SemanticLayer, duration: Ticks) -> V1Result<Value> {
    tv2_domain::layers::validate_layer(layer, duration).map_err(|e| invalid(e.to_string()))?;
    let mut header =
        layer.extra.get("v1_chunks_header").and_then(Value::as_object).cloned().ok_or_else(|| invalid("bloques sin plan autoritativo"))?;
    let mut items: Vec<_> = layer.items.iter().collect();
    items.sort_by_key(|i| i.start());
    let mut chunks = Vec::new();
    for item in items {
        for t in [item.start(), item.end()] {
            if secs_to_ticks(crate::ticks_to_secs(t)) != t {
                return Err(invalid("bloques con precisión submilisegundo"));
            }
        }
        let mut chunk = item.extra.clone();
        let changed = chunk.remove("tv2_original_range").is_none_or(|r| r != json!([item.start(), item.end()]));
        if changed
            && (header.contains_key("boundary_snap")
                || ["first_utterance_id", "last_utterance_id", "semantic_t_ini", "semantic_t_fin"].iter().any(|k| chunk.contains_key(*k)))
        {
            return Err(invalid("Los bordes editados requieren recalcular evidencia/snap del master antes de exportar este plan"));
        }
        chunk.insert("chunk_id".into(), json!(item.item_id));
        chunk.insert("title".into(), json!(item.label));
        chunk.insert("comment".into(), json!(item.comment));
        chunk.insert("t_ini".into(), secs_json(item.start()));
        chunk.insert("t_fin".into(), secs_json(item.end()));
        if item.edited {
            chunk.insert("edited".into(), json!(true));
        }
        chunks.push(Value::Object(chunk));
    }
    header.insert("chunks".into(), json!(chunks));
    header.insert("duration".into(), secs_json(duration));
    header.insert("revision".into(), json!(layer.revision));
    Ok(Value::Object(header))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::{ClipEdge, Command, Project};
    #[test]
    fn snap_recomputes_boundary_evidence_and_materializes_without_mutating_master() {
        let asset = tv2_domain::commands::tests_support::fake_video("a", 10);
        let raw_master = json!({"schema":"editorial-master/1","media":{"duration":10.0,"fingerprint":asset.fingerprint},"tracks":{"A":{"label":"A","words":[{"word_id":"w1","t_ini":4.0,"t_fin":6.0,"text":"habla","intensity_z":1.5}],"laughter":[],"arousal":[]}},"conversation":{"clean_utterance_ids":["u1","u2"],"utterances":[{"utterance_id":"u1","track_id":"A","t_ini":1.0,"t_fin":4.5,"text":"uno"},{"utterance_id":"u2","track_id":"A","t_ini":4.5,"t_fin":9.0,"text":"dos"}]}});
        let master = crate::master::V1Master::parse(raw_master.clone()).unwrap();
        let plan = json!({"schema":"editorial-chunks/1","chunks":[{"chunk_id":"chunk-001","t_ini":0.0,"t_fin":5.0,"title":"Uno","first_utterance_id":"stale"},{"chunk_id":"chunk-002","t_ini":5.0,"t_fin":10.0,"title":"Dos"}]});
        let layer = from_v1(&plan, &asset.id, asset.duration(), &master.source_master_digest()).unwrap();
        let id = layer.layer_id.clone();
        let mut p = Project::new("safe");
        p.masters.push(master.evidence(&asset.id));
        p.assets.push(asset);
        p.layers.push(layer);
        Command::SnapBlockBoundaries { layer_id: id.clone(), radius: Ticks::from_seconds(2) }.apply(&mut p).unwrap();
        let exported = to_v1(p.layer(&id).unwrap(), Ticks::from_seconds(10)).unwrap();
        let boundary = exported["chunks"][0]["t_fin"].as_f64().unwrap();
        assert!(!(3.96..6.04).contains(&boundary));
        assert_eq!(exported["boundary_snap"], "global-safe/2");
        assert_eq!(exported["boundary_adjustments"][0]["safety"]["word_conflicts"], 0);
        let docs = crate::materialize::blocks(&p, &id).unwrap();
        assert!(docs.contains_key("chunks/chunk-001/signals-summary.json"));
        assert!(docs.contains_key(".work/chunks.selected.json"));
        let exported_master: Value = serde_json::from_str(&docs["project.editorial.master.json"]).unwrap();
        assert_eq!(exported_master["chunks"], exported["chunks"]);
        assert_eq!(p.masters[0].document, raw_master);
        let layer = p.layers[0].clone();
        assert!(Command::SnapBlockBoundaries { layer_id: id, radius: Ticks::ZERO }.apply(&mut p).is_err());
        assert_eq!(p.layers[0], layer);
    }
    #[test]
    fn selected_plan_split_and_boundary_move_preserve_partition_and_unknown_fields() {
        let raw = json!({"schema":"editorial-chunks/1", "duration":10.0, "future":{"x":1}, "chunks":[
            {"chunk_id":"chunk-001", "t_ini":0.0,"t_fin":5.0,"title":"Uno","summary":"texto","evidence":{"w":1}},
            {"chunk_id":"chunk-002", "t_ini":5.0,"t_fin":10.0,"title":"Dos","summary":""}]});
        let mut project = Project::new("blocks");
        project.assets.push(tv2_domain::commands::tests_support::fake_video("a", 10));
        let layer = from_v1(&raw, &"a".into(), Ticks::from_seconds(10), "digest").unwrap();
        let id = layer.layer_id.clone();
        project.layers.push(layer);
        Command::SplitItem { layer_id: id.clone(), item_id: "chunk-001".into(), at: Ticks::from_seconds(2) }.apply(&mut project).unwrap();
        project.validate().unwrap();
        Command::TrimItem {
            layer_id: id.clone(),
            item_id: "chunk-002".into(),
            range_index: 0,
            edge: ClipEdge::Start,
            new_time: Ticks::from_seconds(6),
        }
        .apply(&mut project)
        .unwrap();
        let exported = to_v1(project.layer(&id).unwrap(), Ticks::from_seconds(10)).unwrap();
        assert_eq!(exported["future"], raw["future"]);
        assert_eq!(exported["chunks"][0]["evidence"], raw["chunks"][0]["evidence"]);
        assert_eq!(exported["chunks"][1]["t_fin"], 6.0);
        assert_eq!(exported["chunks"][2]["t_ini"], 6.0);
        assert_eq!(from_v1(&exported, &"a".into(), Ticks::from_seconds(10), "digest").unwrap().items.len(), 3);
        let mut invalid_plan = raw.clone();
        invalid_plan["chunks"][1]["t_ini"] = json!(5.1);
        assert!(from_v1(&invalid_plan, &"a".into(), Ticks::from_seconds(10), "digest").is_err());
        invalid_plan = raw;
        invalid_plan["source_master_digest"] = json!("wrong");
        assert!(from_v1(&invalid_plan, &"a".into(), Ticks::from_seconds(10), "digest").is_err());
    }
}
