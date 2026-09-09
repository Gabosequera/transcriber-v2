//! Compare effects, including batches and imports, so a new command cannot
//! bypass human decisions by using a different mutation path.
use crate::session::Actor;
use tv2_domain::error::DomainResult;
use tv2_domain::{DomainError, Project};

/// Commands describe editorial intent; only a human actor can create human review.
/// External imports retain review recorded by their source and still pass protection.
pub(crate) fn attribute(before: &Project, after: &mut Project, actor: &Actor) -> DomainResult<()> {
    let old_clips: std::collections::HashMap<_, _> =
        before.sequences.iter().flat_map(|sequence| &sequence.clips).map(|clip| (&clip.id, clip)).collect();
    for sequence in &mut after.sequences {
        for clip in &mut sequence.clips {
            let old = old_clips.get(&clip.id).copied();
            if old == Some(clip) {
                continue;
            }
            if matches!(actor, Actor::Agent { .. }) {
                if clip.extra.get("v1").is_some_and(|v| v["state"] == "accepted")
                    && old.is_none_or(|old| old.extra.get("v1").is_none_or(|v| v["state"] != "accepted"))
                {
                    return Err(DomainError::precondition("un agente no puede registrar aceptación humana de clips"));
                }
                if let Some(meta) = clip.extra.get_mut("v1").and_then(|v| v.as_object_mut()) {
                    meta.insert("edited".into(), serde_json::json!(old.and_then(|old| old.extra.get("v1")).is_some_and(|v| v["edited"] == true)));
                }
            } else if matches!(actor, Actor::Human)
                && old.is_some()
                && let Some(meta) = clip.extra.get_mut("v1").and_then(|v| v.as_object_mut())
            {
                meta.insert("edited".into(), serde_json::json!(true));
            }
        }
    }
    for layer in &mut after.layers {
        if layer.kind != tv2_domain::LayerKind::Blocks {
            continue;
        }
        let old = before.layer(&layer.layer_id);
        let previous_planner = old.and_then(|layer| layer.extra.get("v1_chunks_header")).and_then(|header| header.get("planner"));
        if let Some(header) = layer.extra.get_mut("v1_chunks_header").and_then(|value| value.as_object_mut()) {
            if matches!(actor, Actor::Human) && old.is_some_and(|old| old.items != layer.items) {
                header.insert("planner".into(), serde_json::json!("manual-review"));
            } else if matches!(actor, Actor::Agent { .. })
                && header.get("planner").is_some_and(|value| value == "manual-review")
                && previous_planner.is_none_or(|value| value != "manual-review")
            {
                return Err(DomainError::precondition("un agente no puede atribuir revisión manual de bloques"));
            }
        }
    }
    if !matches!(actor, Actor::Agent { .. }) {
        return Ok(());
    }
    for layer in &mut after.layers {
        if before.layer(&layer.layer_id).is_some_and(|old| old.items.shares_storage(&layer.items)) {
            continue;
        }
        let original: std::collections::HashMap<_, _> =
            before.layer(&layer.layer_id).into_iter().flat_map(|layer| &layer.items).map(|item| (&item.item_id, item)).collect();
        for item in &mut layer.items {
            let old = original.get(&item.item_id).copied();
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
    if !matches!(actor, Actor::Human) {
        let next_clips: std::collections::HashMap<_, _> =
            after.sequences.iter().flat_map(|sequence| &sequence.clips).map(|clip| (&clip.id, clip)).collect();
        for sequence in &before.sequences {
            for clip in &sequence.clips {
                if clip.extra.get("v1").is_some_and(|v| v["edited"] == true || v["state"] == "accepted")
                    && next_clips.get(&clip.id).copied() != Some(clip)
                {
                    return Err(DomainError::precondition("la operación externa modificaría una decisión humana del montaje")
                        .with("clip_id", clip.id.to_string()));
                }
            }
        }
    }
    for master in &before.masters {
        if after.masters.iter().find(|m| m.asset_id == master.asset_id).is_none_or(|next| !master.allows_source_enrichment(next)) {
            return Err(DomainError::precondition("el master original es evidencia protegida"));
        }
    }
    for layer in &before.layers {
        let next = after.layer(&layer.layer_id);
        if next == Some(layer) {
            continue;
        }
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
        let next_items: std::collections::HashMap<_, _> = next.into_iter().flat_map(|layer| &layer.items).map(|item| (&item.item_id, item)).collect();
        for item in &layer.items {
            if (item.edited || item.is_human_accepted())
                && (next.is_none_or(|n| n.deleted != layer.deleted) || next_items.get(&item.item_id).copied() != Some(item))
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
    fn clip_editorial_decisions_cover_linked_streams_and_protect_human_review() {
        let mut project = Project::new("montage review");
        project.assets.push(fake_video("a", 10));
        let mut session = ProjectSession::new(project);
        session
            .execute(CommandEnvelope::human(Command::InsertAssetLinked {
                asset_id: "a".into(),
                position: Ticks::ZERO,
                video_track: None,
                source: None,
            }))
            .unwrap();
        let ids: Vec<_> = session.project().active().unwrap().clips.iter().map(|c| c.id.clone()).collect();
        assert!(ids.len() > 1);
        let accept = Command::SetClipEditorial {
            clip_ids: vec![ids[0].clone()],
            state: Some(ItemState::Accepted),
            reason: Some("Conservar respuesta".into()),
        };
        let revision = session.revision();
        assert!(session.execute(CommandEnvelope::human(accept.clone()).with_actor(Actor::Agent { name: "review".into() })).is_err());
        assert_eq!(session.revision(), revision);
        session.execute(CommandEnvelope::human(accept)).unwrap();
        for clip in &session.project().active().unwrap().clips {
            assert_eq!(clip.extra["v1"]["state"], "accepted");
            assert_eq!(clip.extra["v1"]["reason"], "Conservar respuesta");
            assert!(clip.enabled);
        }
        assert!(
            session
                .execute(
                    CommandEnvelope::human(Command::SetClipProps {
                        clip_id: ids[0].clone(),
                        name: Some("replace human".into()),
                        gain_db: None,
                        transform: None
                    })
                    .with_actor(Actor::Agent { name: "review".into() })
                )
                .is_err()
        );
        session.undo(Actor::Human).unwrap();
        assert!(session.project().active().unwrap().clips.iter().all(|c| !c.extra.contains_key("v1")));
    }

    #[test]
    fn locked_tracks_reject_split_properties_and_editorial_review_atomically() {
        let mut project = Project::new("locked");
        project.assets.push(fake_video("a", 10));
        let mut session = ProjectSession::new(project);
        session
            .execute(CommandEnvelope::human(Command::InsertAssetLinked {
                asset_id: "a".into(),
                position: Ticks::ZERO,
                video_track: None,
                source: None,
            }))
            .unwrap();
        let clip = session.project().active().unwrap().clips[0].clone();
        session
            .execute(CommandEnvelope::human(Command::SetTrackProps {
                track_id: clip.track_id.clone(),
                name: None,
                muted: None,
                solo: None,
                locked: Some(true),
                visible: None,
                gain_db: None,
                height: None,
            }))
            .unwrap();
        let before = session.project().clone();
        for command in [
            Command::SplitClip { clip_id: clip.id.clone(), at: Ticks::from_seconds(2) },
            Command::SetClipProps { clip_id: clip.id.clone(), name: Some("blocked".into()), gain_db: None, transform: None },
            Command::SetClipEditorial { clip_ids: vec![clip.id.clone()], state: Some(ItemState::Accepted), reason: None },
        ] {
            assert!(session.execute(CommandEnvelope::human(command)).is_err());
            assert_eq!(session.project(), &before);
        }
    }

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
            Command::ReplaceLayer { layer: SemanticLayer { items: vec![].into(), ..l.clone() } },
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
