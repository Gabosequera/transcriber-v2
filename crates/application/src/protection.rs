//! Compare effects, including batches and imports, so a new command cannot
//! bypass human decisions by using a different mutation path.
use crate::session::Actor;
use tv2_domain::error::DomainResult;
use tv2_domain::{DomainError, Project};

/// Commands describe editorial intent; only a human actor can create human review.
/// External imports retain review recorded by their source and still pass protection.
pub(crate) fn attribute(before: &Project, after: &mut Project, actor: &Actor) -> DomainResult<()> {
    if !matches!(actor, Actor::Agent { .. }) {
        return Ok(());
    }
    for layer in &mut after.layers {
        for item in &mut layer.items {
            let old = before.layer(&layer.layer_id).and_then(|l| l.item(&item.item_id));
            if old == Some(item) {
                continue;
            }
            if item.is_human_accepted() && old.is_none_or(|i| !i.is_human_accepted()) {
                return Err(DomainError::precondition("un agente no puede registrar aceptación humana"));
            }
            item.edited = old.is_some_and(|i| i.edited);
            if old.is_none() && (item.origin.is_none() || item.origin.as_deref() == Some("user")) {
                item.origin = Some("ai".into());
            }
        }
    }
    Ok(())
}

pub(crate) fn check_transition(before: &Project, after: &Project, actor: &Actor) -> DomainResult<()> {
    for master in &before.masters {
        if after.masters.iter().find(|m| m.asset_id == master.asset_id) != Some(master) {
            return Err(DomainError::precondition("el master original es evidencia protegida"));
        }
    }
    for layer in &before.layers {
        let next = after.layer(&layer.layer_id);
        if !layer.kind.is_editable() {
            let unchanged = next.is_some_and(|n| {
                n.asset_id == layer.asset_id
                    && n.kind == layer.kind
                    && n.items == layer.items
                    && n.deleted == layer.deleted
                    && n.deleted_item_ids == layer.deleted_item_ids
                    && n.extra == layer.extra
                    && n.source_master_digest == layer.source_master_digest
            });
            if !unchanged {
                return Err(DomainError::precondition("la evidencia de análisis es de solo lectura").with("layer_id", layer.layer_id.to_string()));
            }
        }
        if matches!(actor, Actor::Human) {
            continue;
        }
        if layer.deleted && next.is_some_and(|n| !n.deleted) {
            return Err(DomainError::precondition("no se puede resucitar una capa borrada"));
        }
        if layer.locked && next != Some(layer) {
            return Err(DomainError::precondition("una operación externa no puede cambiar una capa bloqueada"));
        }
        for item in &layer.items {
            if (item.edited || item.is_human_accepted())
                && (next.is_none_or(|n| n.deleted != layer.deleted) || next.and_then(|n| n.item(&item.item_id)) != Some(item))
            {
                return Err(DomainError::precondition("la operación externa modificaría una decisión humana")
                    .with("layer_id", layer.layer_id.to_string())
                    .with("item_id", item.item_id.to_string()));
            }
        }
        if next.is_some_and(|n| layer.deleted_item_ids.iter().any(|id| !n.deleted_item_ids.contains(id))) {
            return Err(DomainError::precondition("la operación externa eliminaría tombstones"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{CommandEnvelope, ProjectSession};
    use tv2_domain::commands::tests_support::fake_video;
    use tv2_domain::{Command, ItemState, LayerKind, SemanticItem, SemanticLayer, Ticks, TimeRange};

    #[test]
    fn agent_cannot_replace_delete_or_edit_a_human_decision_even_in_a_batch() {
        let mut p = Project::new("protected");
        p.assets.push(fake_video("a", 10));
        let mut l = SemanticLayer::new("a".into(), LayerKind::User, "Review");
        let item = SemanticItem::new(TimeRange::new(Ticks::ZERO, Ticks::from_seconds(1)), "Human");
        let id = item.item_id.clone();
        l.items.push(item);
        p.layers.push(l.clone());
        let commands = vec![
            Command::SetItemState { layer_id: l.layer_id.clone(), item_ids: vec![id.clone()], state: ItemState::Disabled },
            Command::DeleteLayer { layer_id: l.layer_id.clone() },
            Command::ReplaceLayer { layer: SemanticLayer { items: vec![], ..l.clone() } },
        ];
        for command in commands {
            let mut session = ProjectSession::new(p.clone());
            let e = CommandEnvelope::human(Command::Batch {
                label: "atomic".into(),
                commands: vec![Command::RenameProject { name: "Must roll back".into() }, command],
            })
            .with_actor(Actor::Agent { name: "test".into() });
            assert!(session.dry_run(&e).is_err());
            assert!(session.execute(e).is_err());
            assert_eq!(session.project(), &p);
            assert!(session.pending_journal().is_empty());
        }
        let mut session = ProjectSession::new(p);
        session.execute(CommandEnvelope::human(Command::DeleteItems { layer_id: l.layer_id, item_ids: vec![id] })).unwrap();
        session.undo(Actor::Human).unwrap();
    }
}
