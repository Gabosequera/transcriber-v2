//! Explicit document export. Each supported adapter rejects unrepresentable
//! values instead of silently rounding or discarding them.
use crate::{V1Result, invalid};
use serde_json::Value;
use tv2_domain::{LayerId, LayerKind, Project, SequenceId};

pub fn layer_document(project: &Project, id: &LayerId) -> V1Result<Value> {
    project.validate().map_err(|e| invalid(e.to_string()))?;
    let layer = project.layer(id).ok_or_else(|| invalid("Capa inexistente"))?;
    if layer.kind == LayerKind::Trims {
        return trims_document(project, &layer.asset_id);
    }
    if layer.kind == LayerKind::Author {
        let asset = project.asset(&layer.asset_id).ok_or_else(|| invalid("Medio inexistente"))?;
        return crate::author::to_v1(layer, asset);
    }
    if layer.kind == LayerKind::Blocks {
        let asset = project.asset(&layer.asset_id).ok_or_else(|| invalid("Medio inexistente"))?;
        return crate::chunks::to_v1(layer, asset.duration());
    }
    if !matches!(layer.kind, LayerKind::User | LayerKind::Topics | LayerKind::Ai) {
        return Err(invalid("Este carril necesita su adaptador autoritativo V1; exportarlo como editorial-layer perdería semántica"));
    }
    for item in &layer.items {
        for range in &item.ranges {
            for t in [range.start, range.end] {
                if crate::secs_to_ticks(crate::ticks_to_secs(t)) != t {
                    return Err(invalid(format!("El item {} contiene tiempo submilisegundo no representable en V1", item.item_id)));
                }
            }
        }
    }
    let asset = project.asset(&layer.asset_id).ok_or_else(|| invalid("Medio inexistente"))?;
    let value = crate::layers::layer_to_v1(layer, &asset.fingerprint);
    crate::layers::layer_from_v1(&value, &asset.id, Some(&asset.fingerprint), asset.duration())?;
    Ok(value)
}

/// Exporting any trim lane exports its complete document, including other lanes.
pub fn trims_document(project: &Project, asset_id: &tv2_domain::AssetId) -> V1Result<Value> {
    project.validate().map_err(|e| invalid(e.to_string()))?;
    let asset = project.asset(asset_id).ok_or_else(|| invalid("Medio inexistente"))?;
    let related: Vec<_> = project.layers.iter().filter(|l| &l.asset_id == asset_id && l.kind == LayerKind::Trims).collect();
    let header = related
        .iter()
        .find_map(|l| l.extra.get("v1_trims_header").and_then(Value::as_object))
        .cloned()
        .ok_or_else(|| invalid("Proyecto antiguo sin cabecera de trims conservada; reimporta el documento antes de exportar"))?;
    let layers: Vec<_> = project.ordered_layers().into_iter().filter(|l| &l.asset_id == asset_id && l.kind == LayerKind::Trims).cloned().collect();
    let mut next_id = header.get("next_id").and_then(Value::as_u64).unwrap_or(1);
    for item in layers.iter().flat_map(|l| &l.items) {
        let number = item
            .item_id
            .as_str()
            .strip_prefix("cut-")
            .filter(|s| s.len() >= 6)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| invalid(format!("ID de recorte no representable en V1: {}", item.item_id)))?;
        next_id = next_id.max(number.checked_add(1).ok_or_else(|| invalid("contador de recortes agotado"))?);
        for range in &item.ranges {
            for t in [range.start, range.end] {
                if crate::secs_to_ticks(crate::ticks_to_secs(t)) != t {
                    return Err(invalid("V1 no admite precisión submilisegundo"));
                }
            }
        }
    }
    for id in related.iter().flat_map(|l| &l.deleted_item_ids) {
        if let Some(number) = id.as_str().strip_prefix("cut-").and_then(|s| s.parse::<u64>().ok()) {
            next_id = next_id.max(number.checked_add(1).ok_or_else(|| invalid("contador agotado"))?);
        }
    }
    let lanes = layers
        .iter()
        .map(|l| crate::trims::Lane {
            lane_id: l.lane.clone().unwrap_or_else(|| l.layer_id.to_string()),
            name: l.name.clone(),
            color: l.color.clone(),
            extra: l.extra.get("v1_lane_metadata").and_then(Value::as_object).cloned().unwrap_or_default(),
        })
        .collect();
    let mut revision = header.get("revision").and_then(Value::as_u64).unwrap_or(0);
    for layer in &related {
        let base = layer.extra.get("v1_import_layer_revision").and_then(Value::as_u64).unwrap_or(0);
        revision = revision.checked_add(layer.revision.saturating_sub(base)).ok_or_else(|| invalid("revisión de recortes agotada"))?;
    }
    let document = crate::trims::V1Trims { duration: asset.duration(), revision, next_id, lanes, layers, header };
    let raw = crate::trims::trims_to_v1(&document, &asset.fingerprint);
    crate::trims::trims_from_v1(&raw, asset_id, Some(&asset.fingerprint))?;
    Ok(raw)
}

pub fn montage_document(project: &Project, id: &SequenceId) -> V1Result<Value> {
    project.validate().map_err(|e| invalid(e.to_string()))?;
    let sequence = project.sequence(id).ok_or_else(|| invalid("Secuencia inexistente"))?;
    let asset = if let Some(first) = sequence.clips.first() {
        if sequence.clips.iter().any(|c| c.asset_id != first.asset_id) {
            return Err(invalid("V1 no representa montajes con varios medios"));
        }
        project.asset(&first.asset_id).ok_or_else(|| invalid("Medio inexistente"))?
    } else {
        let fp = crate::master::parse_fingerprint(
            &sequence.extra.get("v1_original_montage").ok_or_else(|| invalid("Montaje vacío sin origen V1"))?["media"],
        )?;
        project.assets.iter().find(|a| a.fingerprint.same_identity(&fp)).ok_or_else(|| invalid("Medio del montaje vacío no encontrado"))?
    };
    if sequence.extra.contains_key("v1_original_montage") {
        crate::montaje::sequence_to_v1(sequence, &asset.fingerprint)
    } else {
        // Native single-source timelines can use the same strict inverse
        // adapter. There is no fictitious historical montage: the empty origin
        // is only the seed for allocating portable V1 IDs and validation.
        let mut native = sequence.clone();
        native.extra.insert("v1_original_montage".into(),serde_json::json!({"schema":"editorial-montaje/1","media":asset.fingerprint,"duration_source":asset.duration().as_seconds_ms(),"revision":0,"next_id":1,"tracks":[{"track_id":"V1","name":"V1"}],"clips":[],"tv2_native_source":true}));
        crate::montaje::sequence_to_v1(&native, &asset.fingerprint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::{SemanticItem, SemanticLayer, Ticks, TimeRange};
    #[test]
    fn edited_trims_export_all_lanes_header_unknowns_acceptance_and_new_ids() {
        use serde_json::json;
        use tv2_domain::Command;
        let mut p = Project::new("trims");
        let asset = tv2_domain::commands::tests_support::fake_video("a", 20);
        p.assets.push(asset.clone());
        let raw = json!({"schema":"editorial-trims/1","media":asset.fingerprint,"duration":20.0,"revision":8,"next_id":3,"future":{"keep":true},"ai":{"pass":2},
            "lanes":[{"lane_id":"main","name":"Main","color":"#728bd0","future_lane":42},{"lane_id":"ai","name":"AI","color":"#8ab06b"}],
            "cuts":[{"cut_id":"cut-000001","lane":"main","t_ini":1.0,"t_fin":5.0,"enabled":false,"accepted":true,"origin":"user","edited":true,"reason":"razón","evidence":{"w":1}},
                {"cut_id":"cut-000002","lane":"ai","t_ini":8.0,"t_fin":10.0,"enabled":true,"accepted":false,"origin":"ai","edited":false,"reason":"AI"}]});
        p.layers = crate::trims::trims_from_v1(&raw, &asset.id, Some(&asset.fingerprint)).unwrap().layers;
        let result =
            Command::SplitItem { layer_id: "trims-main".into(), item_id: "cut-000001".into(), at: Ticks::from_seconds(3) }.apply(&mut p).unwrap();
        let created = &result.created[0];
        assert!(created.starts_with("cut-"));
        Command::SetItemProps {
            layer_id: "trims-ai".into(),
            item_id: "cut-000002".into(),
            label: Some("Etiqueta".into()),
            comment: None,
            ranges: None,
        }
        .apply(&mut p)
        .unwrap();
        let exported = layer_document(&p, &"trims-ai".into()).unwrap();
        assert_eq!(exported["future"], raw["future"]);
        assert_eq!(exported["ai"], raw["ai"]);
        assert_eq!(exported["lanes"][0]["future_lane"], 42);
        assert_eq!(exported["cuts"].as_array().unwrap().len(), 3);
        assert_eq!(exported["revision"], 10);
        let twin = exported["cuts"].as_array().unwrap().iter().find(|c| c["cut_id"].as_str() == Some(created)).unwrap();
        assert_eq!(twin["enabled"], false);
        assert_eq!(twin["accepted"], true);
        assert_eq!(twin["evidence"], json!({"w":1}));
        let restored = crate::trims::trims_from_v1(&exported, &asset.id, Some(&asset.fingerprint)).unwrap();
        assert_eq!(restored.layers.iter().find(|l| l.lane.as_deref() == Some("ai")).unwrap().items[0].label, "Etiqueta");
        assert!(Command::DeleteLayer { layer_id: "trims-main".into() }.apply(&mut p).is_err());
        Command::DeleteItems {
            layer_id: "trims-main".into(),
            item_ids: p.layer(&"trims-main".into()).unwrap().items.iter().map(|i| i.item_id.clone()).collect(),
        }
        .apply(&mut p)
        .unwrap();
        assert_eq!(layer_document(&p, &"trims-ai".into()).unwrap()["cuts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn native_trim_lane_is_exportable_before_any_master_exists() {
        use tv2_domain::Command;
        let mut p = Project::new("native");
        let asset = tv2_domain::commands::tests_support::fake_video("a", 20);
        p.assets.push(asset);
        Command::CreateLayer {
            asset_id: "a".into(),
            kind: LayerKind::Trims,
            name: "Recortes".into(),
            color: None,
            layer_id: Some("trims-native".into()),
        }
        .apply(&mut p)
        .unwrap();
        let mut item = SemanticItem::new(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(3)), "Nota");
        item.item_id = LayerKind::Trims.fresh_item_id();
        Command::AddItem { layer_id: "trims-native".into(), item }.apply(&mut p).unwrap();
        let raw = layer_document(&p, &"trims-native".into()).unwrap();
        assert_eq!(raw["cuts"][0]["lane"], raw["lanes"][0]["lane_id"]);
        assert_eq!(raw["cuts"][0]["tv2_label"], "Nota");
        assert!(p.masters.is_empty());
    }
    #[test]
    fn editable_layer_roundtrip_preserves_hierarchy_comment_and_rejects_precision_loss() {
        let mut project = Project::new("export");
        project.assets.push(tv2_domain::commands::tests_support::fake_video("a", 20));
        let mut layer = SemanticLayer::new("a".into(), LayerKind::Topics, "topics");
        let mut parent = SemanticItem::new(TimeRange::new(Ticks::ZERO, Ticks::from_seconds(5)), "parent");
        parent.comment = "Human comment ñ".into();
        parent.extra.insert("evidence".into(), serde_json::json!({"word_id":"w1"}));
        let mut child = SemanticItem::new(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(2)), "child");
        child.parent_id = Some(parent.item_id.clone());
        layer.items = vec![parent, child].into();
        let id = layer.layer_id.clone();
        project.layers.push(layer.clone());
        let raw = layer_document(&project, &id).unwrap();
        let restored = crate::layers::layer_from_v1(&raw, &"a".into(), None, Ticks::from_seconds(20)).unwrap();
        assert_eq!(
            restored.items.iter().map(crate::layers::item_to_v1).collect::<Vec<_>>(),
            layer.items.iter().map(crate::layers::item_to_v1).collect::<Vec<_>>()
        );
        assert_eq!(restored.items[1].parent_id, layer.items[1].parent_id);
        assert_eq!(restored.items[0].comment, layer.items[0].comment);
        project.layers[0].items[1].ranges[0].start += Ticks(1);
        assert!(layer_document(&project, &id).is_err());
        project.layers[0] = layer;
        project.layers[0].kind = LayerKind::Author;
        assert!(layer_document(&project, &id).is_err());
    }
}
