//! Semantic edits preserve evidence and use source time. The session commits atomically.
use crate::error::DomainResult;
use crate::ids::ItemId;
use crate::{ClipEdge, Command, CommandEffect, DomainError, ItemState, LayerKind, SemanticItem, SemanticLayer, Ticks, TimeRange};
use std::collections::{HashMap, HashSet};

fn descendants(layer: &SemanticLayer, ids: &[ItemId]) -> DomainResult<HashSet<ItemId>> {
    let mut selected: HashSet<_> = ids.iter().cloned().collect();
    if selected.is_empty() || selected.len() != ids.len() {
        return Err(DomainError::invalid("selección vacía o duplicada"));
    }
    for id in ids {
        layer.item(id).ok_or_else(|| DomainError::not_found("item", id))?;
    }
    loop {
        let count = selected.len();
        for item in &layer.items {
            if item.parent_id.as_ref().is_some_and(|p| selected.contains(p)) {
                selected.insert(item.item_id.clone());
            }
        }
        if count == selected.len() {
            return Ok(selected);
        }
    }
}

fn intersections(ranges: &[TimeRange], parents: &[TimeRange]) -> Vec<TimeRange> {
    ranges
        .iter()
        .flat_map(|r| {
            parents.iter().filter_map(move |p| {
                let start = r.start.max(p.start);
                let end = r.end.min(p.end);
                (start < end || (r.is_point() && p.start <= r.start && r.start < p.end)).then_some(TimeRange::new(start, end))
            })
        })
        .collect()
}

pub(crate) fn apply(command: &Command, layer: &mut SemanticLayer, duration: Ticks) -> DomainResult<CommandEffect> {
    let before = layer.items.clone();
    let mut effect = CommandEffect { label: command.label(), ..Default::default() };
    match command {
        Command::SplitItem { item_id, at, .. } => {
            let original = layer.item(item_id).ok_or_else(|| DomainError::not_found("item", item_id))?.clone();
            let index = original
                .ranges
                .iter()
                .position(|r| r.start < *at && *at < r.end)
                .ok_or_else(|| DomainError::out_of_range("el playhead debe estar dentro de un tramo, fuera de sus bordes"))?;
            let right_range = TimeRange::new(*at, original.ranges[index].end);
            if layer.kind == LayerKind::Blocks
                && (*at - original.ranges[index].start < Ticks::from_seconds(1) || right_range.duration() < Ticks::from_seconds(1))
            {
                return Err(DomainError::out_of_range("dividir un bloque exige al menos un segundo a cada lado"));
            }
            let mut left = original.clone();
            left.ranges[index].end = *at;
            left.edited = true;
            let mut right = original.clone();
            right.item_id = layer.kind.fresh_item_id();
            right.ranges = vec![right_range];
            right.edited = true;
            let ids = descendants(layer, std::slice::from_ref(item_id))?;
            let mut children: Vec<_> = before.iter().filter(|i| ids.contains(&i.item_id) && &i.item_id != item_id).cloned().collect();
            children.sort_by_key(|i| layer.depth_of(&i.item_id));
            // Each original parent maps to its retained and/or split fragments.
            let mut mapping = HashMap::from([(item_id.clone(), vec![left.clone(), right.clone()])]);
            let mut replacements = vec![left, right];
            for child in children {
                let parents = &mapping[child.parent_id.as_ref().unwrap()];
                let mut parts = Vec::new();
                for parent in parents {
                    let ranges = intersections(&child.ranges, &parent.ranges);
                    if !ranges.is_empty() {
                        let mut part = child.clone();
                        if !parts.is_empty() {
                            part.item_id = ItemId::random();
                        }
                        part.parent_id = Some(parent.item_id.clone());
                        part.ranges = ranges;
                        if part != child {
                            part.edited = true;
                        }
                        parts.push(part);
                    }
                }
                if parts.is_empty() {
                    return Err(DomainError::invalid("la división perdería un descendiente"));
                }
                replacements.extend(parts.clone());
                mapping.insert(child.item_id, parts);
            }
            layer.items.retain(|i| !ids.contains(&i.item_id));
            layer.items.extend(replacements);
        }
        Command::TrimItem { item_id, range_index, edge, new_time, .. } => {
            if layer.kind == LayerKind::Blocks && layer.extra.contains_key("v1_chunks_header") {
                let original = layer.item(item_id).ok_or_else(|| DomainError::not_found("item", item_id))?.clone();
                let neighbor = layer
                    .items
                    .iter()
                    .find(|i| {
                        &i.item_id != item_id
                            && match edge {
                                ClipEdge::Start => i.end() == original.start(),
                                ClipEdge::End => i.start() == original.end(),
                            }
                    })
                    .map(|i| i.item_id.clone());
                let neighbor = neighbor.ok_or_else(|| DomainError::out_of_range("el borde exterior del plan está fijado al medio"))?;
                let adjacent = layer.item_mut(&neighbor).unwrap();
                match edge {
                    ClipEdge::Start => adjacent.ranges[0].end = *new_time,
                    ClipEdge::End => adjacent.ranges[0].start = *new_time,
                }
                adjacent.edited = true;
            }
            let item = layer.item_mut(item_id).ok_or_else(|| DomainError::not_found("item", item_id))?;
            let range = item.ranges.get_mut(*range_index).ok_or_else(|| DomainError::out_of_range("tramo inexistente"))?;
            match edge {
                ClipEdge::Start => range.start = *new_time,
                ClipEdge::End => range.end = *new_time,
            }
            item.edited = true;
        }
        Command::ShiftItems { item_ids, delta, .. } => {
            let selected = descendants(layer, item_ids)?;
            let lo = layer.items.iter().filter(|i| selected.contains(&i.item_id)).map(SemanticItem::start).min().unwrap();
            let hi = layer.items.iter().filter(|i| selected.contains(&i.item_id)).map(SemanticItem::end).max().unwrap();
            let delta = (*delta).clamp(-lo, duration - hi);
            for item in &mut layer.items {
                if selected.contains(&item.item_id) && delta != Ticks::ZERO {
                    for range in &mut item.ranges {
                        range.start += delta;
                        range.end += delta;
                    }
                    item.edited = true;
                }
            }
            if layer.kind == LayerKind::Blocks && layer.extra.contains_key("v1_chunks_header") && delta != Ticks::ZERO {
                for original in before.iter().filter(|i| selected.contains(&i.item_id)) {
                    for neighbor in before.iter().filter(|i| !selected.contains(&i.item_id)) {
                        let item = layer.item_mut(&neighbor.item_id).unwrap();
                        if neighbor.end() == original.start() {
                            item.ranges[0].end = original.start() + delta;
                            item.edited = true;
                        }
                        if neighbor.start() == original.end() {
                            item.ranges[0].start = original.end() + delta;
                            item.edited = true;
                        }
                    }
                }
            }
        }
        Command::CycleAuthorDecision { item_ids, .. } => {
            if layer.kind != LayerKind::Author {
                return Err(DomainError::not_available("el ciclo de decisiones solo corresponde a marcas del autor"));
            }
            for id in item_ids {
                let item = layer.item_mut(id).ok_or_else(|| DomainError::not_found("item", id))?;
                item.state = match item.state {
                    ItemState::Proposed => ItemState::Accepted,
                    ItemState::Accepted => ItemState::Disabled,
                    ItemState::Disabled => ItemState::Proposed,
                };
                if item.is_point() && item.state != ItemState::Proposed {
                    let t = item.start();
                    item.ranges = vec![TimeRange::new(
                        (t - Ticks::from_seconds(2)).max(Ticks::ZERO),
                        t.0.checked_add(Ticks::from_seconds(2).0).map(Ticks).unwrap_or(duration).min(duration),
                    )];
                }
                item.edited = true;
            }
        }
        _ => unreachable!(),
    }
    if layer.kind == LayerKind::Trims {
        match command {
            Command::TrimItem { item_id, .. } => crate::box_edit::coalesce(layer, Some(item_id))?,
            Command::ShiftItems { item_ids, .. } => crate::box_edit::coalesce(layer, item_ids.last())?,
            _ => {}
        }
    }
    crate::layers::validate_layer(layer, duration)?;
    for item in &layer.items {
        let old = before.iter().find(|i| i.item_id == item.item_id);
        if old != Some(item) {
            effect.affected.push(item.item_id.to_string());
        }
        if old.is_none() {
            effect.created.push(item.item_id.to_string());
        }
    }
    if !effect.affected.is_empty() {
        layer.revision = layer.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
    }
    Ok(effect)
}

/// Both inspector paths edit the same partition and adjust the existing neighbors.
pub(crate) fn set_block_range(layer: &mut SemanticLayer, id: &ItemId, ranges: &[TimeRange]) -> DomainResult<()> {
    if ranges.len() != 1 {
        return Err(DomainError::invalid("un bloque exige un rango"));
    }
    let old = layer.item(id).ok_or_else(|| DomainError::not_found("bloque", id))?.clone();
    for edge in [ClipEdge::Start, ClipEdge::End] {
        let (previous, next) = match edge {
            ClipEdge::Start => (old.start(), ranges[0].start),
            ClipEdge::End => (old.end(), ranges[0].end),
        };
        if previous == next {
            continue;
        }
        let neighbor = layer
            .items
            .iter_mut()
            .find(|i| {
                &i.item_id != id
                    && match edge {
                        ClipEdge::Start => i.end() == previous,
                        ClipEdge::End => i.start() == previous,
                    }
            })
            .ok_or_else(|| DomainError::out_of_range("el borde exterior del plan está fijado al medio"))?;
        match edge {
            ClipEdge::Start => neighbor.ranges[0].end = next,
            ClipEdge::End => neighbor.ranges[0].start = next,
        }
        neighbor.edited = true;
    }
    layer.item_mut(id).unwrap().ranges = ranges.to_vec();
    Ok(())
}
