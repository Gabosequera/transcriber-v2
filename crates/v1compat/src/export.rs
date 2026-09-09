//! Explicit document export. Each supported adapter rejects unrepresentable
//! values instead of silently rounding or discarding them.
use crate::{V1Result, invalid};
use serde_json::Value;
use tv2_domain::{LayerId, LayerKind, Project, SequenceId};

pub fn layer_document(project: &Project, id: &LayerId) -> V1Result<Value> {
    project.validate().map_err(|e| invalid(e.to_string()))?;
    let layer = project.layer(id).ok_or_else(|| invalid("Capa inexistente"))?;
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

pub fn montage_document(project: &Project, id: &SequenceId) -> V1Result<Value> {
    project.validate().map_err(|e| invalid(e.to_string()))?;
    let sequence = project.sequence(id).ok_or_else(|| invalid("Secuencia inexistente"))?;
    let first = sequence.clips.first().ok_or_else(|| invalid("Montaje vacío"))?;
    if sequence.clips.iter().any(|c| c.asset_id != first.asset_id) {
        return Err(invalid("V1 no representa montajes con varios medios"));
    }
    let asset = project.asset(&first.asset_id).ok_or_else(|| invalid("Medio inexistente"))?;
    crate::montaje::sequence_to_v1(sequence, &asset.fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::{SemanticItem, SemanticLayer, Ticks, TimeRange};
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
        layer.items = vec![parent, child];
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
