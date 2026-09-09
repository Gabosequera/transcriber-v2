//! Box gestures and trim lane operations. All callers commit through ProjectSession.
use crate::error::DomainResult;
use crate::{Command, CommandEffect, DomainError, ItemId, LayerId, LayerKind, Project, SemanticItem, SemanticLayer, Ticks, TimeRange};
use std::collections::HashSet;

pub(crate) fn ensure_removable(layer: &SemanticLayer) -> DomainResult<()> {
    if layer.kind == LayerKind::Trims && matches!(layer.lane.as_deref(), Some("main" | "ai")) {
        return Err(DomainError::precondition("los carriles de recortes de fábrica no se borran"));
    }
    Ok(())
}

fn merge_metadata(actor: &mut SemanticItem, others: &[SemanticItem]) {
    let mut reasons = Vec::new();
    let mut warnings = Vec::new();
    for item in std::iter::once(&*actor).chain(others) {
        for reason in item.comment.split(" · ").map(str::trim).filter(|s| !s.is_empty()) {
            if !reasons.contains(&reason.to_string()) {
                reasons.push(reason.to_string());
            }
        }
        if let Some(values) = item.extra.get("warnings").and_then(|v| v.as_array()) {
            for value in values {
                if !warnings.contains(value) {
                    warnings.push(value.clone());
                }
            }
        }
    }
    actor.comment = reasons.join(" · ");
    if !warnings.is_empty() {
        actor.extra.insert("warnings".into(), serde_json::json!(warnings));
    }
    for other in others {
        for (key, value) in &other.extra {
            if matches!(key.as_str(), "accepted" | "enabled" | "tv2_absorbed") {
                continue;
            }
            actor.extra.entry(key.clone()).or_insert_with(|| value.clone());
        }
        if actor.extra.get("evidence").is_none_or(|v| v.is_null() || v.as_object().is_some_and(|m| m.is_empty()))
            && let Some(evidence) = other.extra.get("evidence")
        {
            actor.extra.insert("evidence".into(), evidence.clone());
        }
    }
    // Immutable provenance keeps conflicting unknown fields and evidence of absorbed IDs.
    let archive = actor.extra.entry("tv2_absorbed".to_string()).or_insert_with(|| serde_json::json!([]));
    if let Some(archive) = archive.as_array_mut() {
        archive.extend(others.iter().map(|i| serde_json::json!({"item_id":i.item_id,"label":i.label,"origin":i.origin,"extra":i.extra})));
    }
    actor.edited = true;
}

fn absorb(layer: &mut SemanticLayer, mut actor: SemanticItem, others: Vec<SemanticItem>, range: TimeRange) {
    let removed: HashSet<_> = others.iter().map(|i| i.item_id.clone()).collect();
    actor.ranges = vec![range];
    merge_metadata(&mut actor, &others);
    layer.items.retain(|i| i.item_id != actor.item_id && !removed.contains(&i.item_id));
    for item in &mut layer.items {
        if item.parent_id.as_ref().is_some_and(|p| removed.contains(p)) {
            item.parent_id = Some(actor.item_id.clone());
        }
    }
    layer.deleted_item_ids.extend(removed);
    layer.items.push(actor);
}

pub(crate) fn coalesce(layer: &mut SemanticLayer, actor_id: Option<&ItemId>) -> DomainResult<()> {
    if layer.kind != LayerKind::Trims {
        return Err(DomainError::invalid("coalescencia exige recortes"));
    }
    if let Some(id) = actor_id {
        layer.item(id).ok_or_else(|| DomainError::not_found("recorte", id))?;
    }
    for enabled in [true, false] {
        let mut sorted: Vec<_> = layer.items.iter().filter(|i| i.state.is_enabled() == enabled).cloned().collect();
        sorted.sort_by_key(|i| (i.start(), i.end(), i.item_id.clone()));
        let mut offset = 0;
        while offset < sorted.len() {
            let start = offset;
            let lo = sorted[start].start();
            let mut hi = sorted[start].end();
            offset += 1;
            while offset < sorted.len() && sorted[offset].start() < hi {
                hi = hi.max(sorted[offset].end());
                offset += 1;
            }
            let group = &sorted[start..offset];
            if group.len() < 2 {
                continue;
            }
            let actor = group
                .iter()
                .find(|i| Some(&i.item_id) == actor_id)
                .unwrap_or_else(|| group.iter().max_by_key(|i| (i.total_duration(), &i.item_id)).unwrap())
                .clone();
            let others = group.iter().filter(|i| i.item_id != actor.item_id).cloned().collect();
            absorb(layer, actor, others, TimeRange::new(lo, hi));
        }
    }
    layer.items.sort_by_key(|i| (i.start(), i.item_id.clone()));
    Ok(())
}

pub(crate) fn apply(command: &Command, layer: &mut SemanticLayer, duration: Ticks) -> DomainResult<CommandEffect> {
    let before = layer.clone();
    match command {
        Command::CoalesceTrims { actor_id, .. } => coalesce(layer, actor_id.as_ref())?,
        Command::BoxEdit { range, subtract, .. } => {
            if layer.kind == LayerKind::Blocks {
                return Err(DomainError::not_available("en bloques, Corte mueve bordes; S divide"));
            }
            let min = if layer.kind == LayerKind::Trims { Ticks::from_millis(50) } else { Ticks::from_millis(40) };
            if range.start < Ticks::ZERO || range.end < range.start || range.end > duration || range.duration() < min {
                return Err(DomainError::out_of_range("caja fuera del medio o demasiado corta"));
            }
            // V1 boxes only target single-range items; multi-range edits use their handles.
            let mut hits: Vec<_> =
                layer.items.iter().filter(|i| i.ranges.len() == 1 && i.start() < range.end && i.end() > range.start).cloned().collect();
            hits.sort_by_key(|i| (i.start(), i.end(), i.item_id.clone()));
            if *subtract {
                for original in hits {
                    let mut parts = Vec::new();
                    if range.start - original.start() >= min {
                        parts.push(TimeRange::new(original.start(), range.start));
                    }
                    if original.end() - range.end >= min {
                        parts.push(TimeRange::new(range.end, original.end()));
                    }
                    layer.items.retain(|i| i.item_id != original.item_id);
                    if parts.is_empty() {
                        layer.deleted_item_ids.push(original.item_id.clone());
                    }
                    for (n, part) in parts.into_iter().enumerate() {
                        let mut item = original.clone();
                        if n > 0 {
                            item.item_id = layer.kind.fresh_item_id();
                        }
                        item.ranges = vec![part];
                        item.edited = true;
                        layer.items.push(item);
                    }
                }
            } else if let Some(actor) = hits.first().cloned() {
                if hits.iter().any(|i| i.parent_id != actor.parent_id) {
                    return Err(DomainError::precondition("la caja no puede unir distintos niveles de jerarquía"));
                }
                let lo = hits.iter().map(|i| i.start()).min().unwrap().min(range.start);
                let hi = hits.iter().map(|i| i.end()).max().unwrap().max(range.end);
                let id = actor.item_id.clone();
                absorb(layer, actor, hits.into_iter().skip(1).collect(), TimeRange::new(lo, hi));
                if layer.kind == LayerKind::Trims {
                    coalesce(layer, Some(&id))?;
                }
            } else {
                let mut item = SemanticItem::new(*range, "");
                item.item_id = layer.kind.fresh_item_id();
                layer.items.push(item);
            }
        }
        _ => unreachable!(),
    }
    crate::layers::validate_layer(layer, duration)?;
    let mut effect = CommandEffect { label: command.label(), ..Default::default() };
    for item in &layer.items {
        if before.item(&item.item_id) != Some(item) {
            effect.affected.push(item.item_id.to_string());
        }
        if before.item(&item.item_id).is_none() {
            effect.created.push(item.item_id.to_string());
        }
    }
    effect.affected.extend(before.items.iter().filter(|i| layer.item(&i.item_id).is_none()).map(|i| i.item_id.to_string()));
    if !effect.affected.is_empty() {
        layer.revision = layer.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
    }
    Ok(effect)
}

pub(crate) fn move_items(project: &mut Project, from: &LayerId, to: &LayerId, ids: &[ItemId]) -> DomainResult<CommandEffect> {
    if from == to {
        return Err(DomainError::invalid("el destino debe ser otro carril"));
    }
    let source = project.layer(from).ok_or_else(|| DomainError::not_found("capa", from))?;
    let target = project.layer(to).ok_or_else(|| DomainError::not_found("capa", to))?;
    for layer in [source, target] {
        if layer.kind != LayerKind::Trims || layer.locked || layer.deleted {
            return Err(DomainError::precondition("se requieren dos carriles de recortes editables"));
        }
    }
    if source.asset_id != target.asset_id {
        return Err(DomainError::invalid("los carriles pertenecen a distintos medios"));
    }
    let selected: HashSet<_> = ids.iter().collect();
    if selected.len() != ids.len() {
        return Err(DomainError::invalid("selección duplicada"));
    }
    let mut moving = Vec::new();
    for id in ids {
        if target.item(id).is_some() || target.deleted_item_ids.contains(id) {
            return Err(DomainError::precondition("ID destino ocupado o borrado"));
        }
        moving.push(source.item(id).ok_or_else(|| DomainError::not_found("recorte", id))?.clone());
    }
    let source = project.layer_mut(from).unwrap();
    source.items.retain(|i| !selected.contains(&i.item_id));
    source.revision = source.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
    let target = project.layer_mut(to).unwrap();
    target.items.extend(moving);
    coalesce(target, ids.last())?;
    target.revision = target.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
    project.validate()?;
    Ok(CommandEffect {
        label: "Mover recortes".into(),
        affected: ids.iter().map(ToString::to_string).chain([from.to_string(), to.to_string()]).collect(),
        ..Default::default()
    })
}
