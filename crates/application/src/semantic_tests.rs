use crate::{Actor, CommandEnvelope, ProjectSession};
use tv2_domain::{ClipEdge, Command, ItemState, LayerKind, Project, SemanticItem, SemanticLayer, Ticks, TimeRange};
fn s(n: i64) -> Ticks {
    Ticks::from_seconds(n)
}

#[test]
fn source_bundle_survives_history_reopen_and_cannot_be_rewritten() {
    use std::{collections::BTreeMap, sync::Arc};
    use tv2_domain::evidence::{EvidenceDocument, MasterEvidence, SourceBundle};
    let asset = tv2_domain::commands::tests_support::fake_video("a", 30);
    let raw = serde_json::json!({"schema":"editorial-master/1","media":{"fingerprint":asset.fingerprint,"duration":30.0},"tracks":{}});
    let document: EvidenceDocument = raw.clone().into();
    let bundle = Arc::new(SourceBundle::new(
        "original.editorial.master.json".into(),
        BTreeMap::from([("original.editorial.master.json".into(), raw.to_string()), ("future.md".into(), "Evidence ñ\r\n".into())]),
    ));
    let mut p = Project::new("evidence");
    p.assets.push(asset.clone());
    p.masters.push(MasterEvidence {
        asset_id: asset.id,
        source_digest: document.source_digest().into(),
        document,
        source_bundle: Some(bundle.clone()),
    });
    let mut session = ProjectSession::new(p);
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "edited".into() })).unwrap();
    assert!(Arc::ptr_eq(session.project().masters[0].source_bundle.as_ref().unwrap(), &bundle));
    let temp = tempfile::tempdir().unwrap();
    let store = crate::ProjectStore::at(temp.path());
    store.save_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
    let mut restored = store.load_session().unwrap();
    restored.undo(Actor::Human).unwrap();
    assert_eq!(restored.project().masters[0].source_bundle.as_ref().unwrap().documents()["future.md"], "Evidence ñ\r\n");
    restored.redo(Actor::Human).unwrap();
    let mut changed = restored.project().clone();
    changed.masters[0].source_bundle = None;
    assert!(crate::protection::check_transition(restored.project(), &changed, &Actor::Human).is_err());
    let mut serialized = serde_json::to_value(restored.project()).unwrap();
    serialized["masters"][0]["source_bundle"]["documents"]["original.editorial.master.json"] = serde_json::json!("{}");
    assert!(serde_json::from_value::<Project>(serialized).unwrap().validate().is_err());
}

#[test]
fn prepared_command_keeps_generated_ids_rejects_stale_and_shares_master_evidence() {
    let mut p = Project::new("prepared");
    let asset = tv2_domain::commands::tests_support::fake_video("a", 30);
    p.assets.push(asset.clone());
    let raw = serde_json::json!({"schema":"editorial-master/1","media":{"fingerprint":asset.fingerprint,"duration":30.0},"tracks":{},"chunks":[],"generated_at":"now"});
    let document: tv2_domain::evidence::EvidenceDocument = raw.clone().into();
    p.masters.push(tv2_domain::evidence::MasterEvidence {
        asset_id: asset.id,
        source_digest: document.source_digest().into(),
        document,
        source_bundle: None,
    });
    let mut session = ProjectSession::new(p);
    let request = CommandEnvelope::human(Command::CreateLayer {
        asset_id: "a".into(),
        kind: LayerKind::User,
        name: "Notes".into(),
        color: None,
        layer_id: None,
    })
    .with_base(0);
    let prepared = session.prepare_command(request.clone()).unwrap();
    let id = prepared.preview().layers[0].layer_id.clone();
    assert!(prepared.preview().masters[0].document.shares_storage(&session.project().masters[0].document));
    session.commit_prepared(prepared).unwrap();
    assert_eq!(session.project().layers[0].layer_id, id);
    assert!(session.prepare_command(request).is_err());
    let prepared = session.prepare_command(CommandEnvelope::human(Command::RenameProject { name: "worker".into() })).unwrap();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "human".into() })).unwrap();
    assert!(session.commit_prepared(prepared).is_err());
    assert_eq!(session.project().name, "human");
    let mut serialized = serde_json::to_value(session.project()).unwrap();
    serialized["masters"][0]["document"]["media"]["duration"] = serde_json::json!(29);
    assert!(serde_json::from_value::<Project>(serialized).unwrap().validate().is_err());
}

#[test]
fn recovery_restores_full_undo_and_redo_and_survives_save_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::ProjectStore::at(dir.path());
    let saved = Project::new("saved");
    store.save(&saved).unwrap();
    let mut original = store.load_session().unwrap();
    for name in ["one", "two", "three"] {
        original.execute(CommandEnvelope::human(Command::RenameProject { name: name.into() })).unwrap();
    }
    original.undo(Actor::Human).unwrap();
    store.save_autosave_checkpoint(original.project(), original.pending_journal(), Some(&original.history_snapshot())).unwrap();
    let candidate = store.recovery_checkpoint(&saved).unwrap().unwrap();
    let mut recovered = store.load_session().unwrap();
    recovered.recover_checkpoint(candidate.project, candidate.events, candidate.history).unwrap();
    assert_eq!(recovered.project().name, "two");
    assert!(recovered.can_redo());
    recovered.redo(Actor::Human).unwrap();
    assert_eq!(recovered.project().name, "three");
    recovered.undo(Actor::Human).unwrap();
    recovered.undo(Actor::Human).unwrap();
    assert_eq!(recovered.project().name, "one");
    store.save_checkpoint(recovered.project(), recovered.pending_journal(), Some(&recovered.history_snapshot())).unwrap();
    let mut reopened = store.load_session().unwrap();
    reopened.undo(Actor::Human).unwrap();
    assert_eq!(reopened.project().name, "saved");
    for _ in 0..3 {
        reopened.redo(Actor::Human).unwrap();
    }
    assert_eq!(reopened.project().name, "three");
}

#[test]
fn cut_box_union_subtract_metadata_tombstones_and_history() {
    let mut p = Project::new("box");
    p.assets.push(tv2_domain::commands::tests_support::fake_video("a", 30));
    let mut layer = SemanticLayer::new("a".into(), LayerKind::Trims, "Recortes");
    layer.layer_id = "trims-test".into();
    layer.lane = Some("test".into());
    for (id, a, b, state) in [("cut-000001", 2, 5, ItemState::Accepted), ("cut-000002", 8, 12, ItemState::Disabled)] {
        let mut i = SemanticItem::new(TimeRange::new(s(a), s(b)), id);
        i.item_id = id.into();
        i.state = state;
        i.comment = id.into();
        i.extra.insert("evidence".into(), serde_json::json!({"word":id}));
        layer.items.push(i);
    }
    p.layers.push(layer);
    let initial = p.layers.clone();
    let mut session = ProjectSession::new(p);
    let request = CommandEnvelope::human(Command::BoxEdit { layer_id: "trims-test".into(), range: TimeRange::new(s(4), s(9)), subtract: false })
        .with_idempotency("box");
    session.dry_run(&request).unwrap();
    assert_eq!(session.project().layers, initial);
    session.execute(request.clone()).unwrap();
    let layer = session.project().layer(&"trims-test".into()).unwrap();
    assert_eq!(layer.items.len(), 1);
    assert_eq!(layer.items[0].ranges, vec![TimeRange::new(s(2), s(12))]);
    assert_eq!(layer.items[0].state, ItemState::Accepted);
    assert_eq!(layer.items[0].comment, "cut-000001 · cut-000002");
    assert_eq!(layer.items[0].extra["tv2_absorbed"][0]["extra"]["evidence"]["word"], "cut-000002");
    assert!(layer.deleted_item_ids.contains(&"cut-000002".into()));
    assert!(session.execute(request).unwrap().replayed);
    session
        .execute(CommandEnvelope::human(Command::BoxEdit { layer_id: "trims-test".into(), range: TimeRange::new(s(5), s(8)), subtract: true }))
        .unwrap();
    let layer = session.project().layer(&"trims-test".into()).unwrap();
    assert_eq!(layer.items.len(), 2);
    assert!(layer.items.iter().all(|i| i.state == ItemState::Accepted && i.extra.contains_key("evidence")));
    assert!(layer.items.iter().any(|i| i.ranges == vec![TimeRange::new(s(8), s(12))]));
    session.undo(Actor::Human).unwrap();
    session.undo(Actor::Human).unwrap();
    assert_eq!(session.project().layers, initial);
    session.redo(Actor::Human).unwrap();
    session.redo(Actor::Human).unwrap();
    session
        .execute(CommandEnvelope::human(Command::BoxEdit { layer_id: "trims-test".into(), range: TimeRange::new(s(20), s(22)), subtract: false }))
        .unwrap();
    assert_eq!(session.project().layers[0].items.len(), 3);
}

#[test]
fn coalesce_preserves_enabled_groups_actor_identity_and_lane_removal() {
    let mut p = Project::new("lanes");
    p.assets.push(tv2_domain::commands::tests_support::fake_video("a", 30));
    let mut main = SemanticLayer::new("a".into(), LayerKind::Trims, "Main");
    main.layer_id = "trims-main".into();
    main.lane = Some("main".into());
    for (id, a, b, state) in [
        ("cut-000001", 1, 5, ItemState::Proposed),
        ("cut-000002", 4, 8, ItemState::Accepted),
        ("cut-000003", 2, 9, ItemState::Disabled),
        ("cut-000004", 8, 10, ItemState::Proposed),
    ] {
        let mut item = SemanticItem::new(TimeRange::new(s(a), s(b)), id);
        item.item_id = id.into();
        item.state = state;
        main.items.push(item);
    }
    let mut other = SemanticLayer::new("a".into(), LayerKind::Trims, "Other");
    other.layer_id = "other".into();
    other.lane = Some("other".into());
    p.layers = vec![main, other];
    let mut session = ProjectSession::new(p);
    session.execute(CommandEnvelope::human(Command::CoalesceTrims { layer_id: "trims-main".into(), actor_id: Some("cut-000001".into()) })).unwrap();
    let main = &session.project().layers[0];
    assert_eq!(main.items.len(), 3);
    assert_eq!(main.item(&"cut-000001".into()).unwrap().state, ItemState::Proposed);
    assert_eq!(main.item(&"cut-000001".into()).unwrap().end(), s(8)); // merely touching cut-4 remains separate
    assert!(session.execute(CommandEnvelope::human(Command::DeleteLayer { layer_id: "trims-main".into() })).is_err());
    session
        .execute(CommandEnvelope::human(Command::MoveTrimItems {
            layer_id: "trims-main".into(),
            target_layer_id: "other".into(),
            item_ids: vec!["cut-000004".into()],
        }))
        .unwrap();
    assert_eq!(session.project().layers[1].items[0].item_id.as_str(), "cut-000004");
    session.execute(CommandEnvelope::human(Command::RemoveTrimLane { layer_id: "other".into(), move_to: Some("trims-main".into()) })).unwrap();
    assert_eq!(session.project().layers[0].items.len(), 3);
    assert!(session.project().layers[1].deleted);
    session.undo(Actor::Human).unwrap();
    assert!(!session.project().layers[1].deleted);
}

#[test]
fn blocks_inspector_edits_both_neighbors_and_invalid_batch_rolls_back() {
    let mut p = Project::new("blocks");
    p.assets.push(tv2_domain::commands::tests_support::fake_video("a", 30));
    let mut layer = SemanticLayer::new("a".into(), LayerKind::Blocks, "Blocks");
    layer.layer_id = "blocks".into();
    layer.extra.insert("v1_chunks_header".into(), serde_json::json!({"schema":"editorial-chunks/1"}));
    for (id, a, b) in [("one", 0, 10), ("two", 10, 20), ("three", 20, 30)] {
        let mut i = SemanticItem::new(TimeRange::new(s(a), s(b)), id);
        i.item_id = id.into();
        layer.items.push(i);
    }
    p.layers.push(layer);
    let mut session = ProjectSession::new(p);
    session
        .execute(CommandEnvelope::human(Command::SetItemStructure {
            layer_id: "blocks".into(),
            item_id: "two".into(),
            parent_id: None,
            ranges: vec![TimeRange::new(s(8), s(22))],
        }))
        .unwrap();
    assert_eq!(session.project().layers[0].items[0].end(), s(8));
    assert_eq!(session.project().layers[0].items[2].start(), s(22));
    session
        .execute(CommandEnvelope::human(Command::SetItemProps {
            layer_id: "blocks".into(),
            item_id: "two".into(),
            label: None,
            comment: None,
            ranges: Some(vec![TimeRange::new(s(7), s(23))]),
        }))
        .unwrap();
    assert_eq!(session.project().layers[0].items[0].end(), s(7));
    let before = session.project().clone();
    assert!(
        session
            .execute(CommandEnvelope::human(Command::SetItemStructure {
                layer_id: "blocks".into(),
                item_id: "two".into(),
                parent_id: None,
                ranges: vec![TimeRange::new(s(0), s(30))]
            }))
            .is_err()
    );
    assert_eq!(session.project(), &before);
}

#[test]
fn exclusive_trim_end_maps_to_previous_clip_at_cut_and_sequence_end() {
    let left = tv2_domain::Clip::new("track".into(), "a".into(), TimeRange::new(s(10), s(12)), s(0));
    let right = tv2_domain::Clip::new("track".into(), "a".into(), TimeRange::new(s(20), s(22)), s(2));
    assert_eq!(left.seq_edge_to_source(s(2), ClipEdge::End), Some(s(12)));
    assert_eq!(right.seq_edge_to_source(s(2), ClipEdge::End), None);
    assert_eq!(left.seq_edge_to_source(s(2), ClipEdge::Start), None);
    assert_eq!(right.seq_edge_to_source(s(2), ClipEdge::Start), Some(s(20)));
    assert_eq!(right.seq_edge_to_source(s(4), ClipEdge::End), Some(s(22)));
    assert_eq!(left.seq_edge_to_source(s(0), ClipEdge::End), None);
    assert_eq!(left.seq_edge_to_source(Ticks(i64::MIN), ClipEdge::End), None);
}
fn fixture(kind: LayerKind) -> (ProjectSession, tv2_domain::LayerId) {
    let mut project = Project::new("semantic");
    project.assets.push(tv2_domain::commands::tests_support::fake_video("a", 30));
    let mut layer = SemanticLayer::new("a".into(), kind, "Editorial");
    let mut parent = SemanticItem::new(TimeRange::new(s(2), s(12)), "parent");
    parent.item_id = "parent".into();
    parent.ranges.push(TimeRange::new(s(20), s(25)));
    parent.comment = "Comentario ñ".into();
    parent.extra.insert("evidence".into(), serde_json::json!({"words":["w1"], "future":42}));
    let mut child = SemanticItem::new(TimeRange::new(s(4), s(10)), "child");
    child.item_id = "child".into();
    child.parent_id = Some(parent.item_id.clone());
    let mut grandchild = SemanticItem::new(TimeRange::new(s(7), s(9)), "grandchild");
    grandchild.item_id = "grandchild".into();
    grandchild.parent_id = Some(child.item_id.clone());
    layer.items = vec![grandchild, child, parent]; // deliberately not tree order
    let id = layer.layer_id.clone();
    project.layers.push(layer);
    (ProjectSession::new(project), id)
}

#[test]
fn split_partitions_tree_preserves_other_ranges_evidence_and_retry() {
    let (mut session, id) = fixture(LayerKind::Topics);
    let before = session.project().clone();
    let request = CommandEnvelope::human(Command::SplitItem { layer_id: id.clone(), item_id: "parent".into(), at: s(6) }).with_idempotency("split");
    let result = session.execute(request.clone()).unwrap();
    assert_eq!(result.effect.created.len(), 2);
    let layer = session.project().layer(&id).unwrap();
    let left = layer.item(&"parent".into()).unwrap();
    assert_eq!(left.ranges, vec![TimeRange::new(s(2), s(6)), TimeRange::new(s(20), s(25))]);
    let right = layer.items.iter().find(|i| i.label == "parent" && i.item_id != left.item_id).unwrap();
    assert_eq!(right.ranges, vec![TimeRange::new(s(6), s(12))]);
    assert_eq!(right.comment, left.comment);
    assert_eq!(right.extra, left.extra);
    let child_right = layer.items.iter().find(|i| i.parent_id.as_ref() == Some(&right.item_id)).unwrap();
    assert_eq!(child_right.ranges, vec![TimeRange::new(s(6), s(10))]);
    assert_eq!(layer.item(&"grandchild".into()).unwrap().parent_id.as_ref(), Some(&child_right.item_id));
    assert!(session.execute(request).unwrap().replayed);
    session.undo(Actor::Human).unwrap();
    assert_eq!(session.project().layers, before.layers);
    session.redo(Actor::Human).unwrap();
    assert_eq!(session.project().layer(&id).unwrap().items.len(), 5);
}

#[test]
fn nudge_moves_descendants_once_clamps_and_failed_trim_batch_is_atomic() {
    let (mut session, id) = fixture(LayerKind::Topics);
    session
        .execute(CommandEnvelope::human(Command::ShiftItems { layer_id: id.clone(), item_ids: vec!["parent".into(), "child".into()], delta: s(20) }))
        .unwrap();
    let layer = session.project().layer(&id).unwrap();
    assert_eq!(layer.item(&"parent".into()).unwrap().end(), s(30));
    assert_eq!(layer.item(&"child".into()).unwrap().start(), s(9));
    assert_eq!(layer.item(&"grandchild".into()).unwrap().start(), s(12));
    let before = session.project().clone();
    let command = Command::Batch {
        label: "fail".into(),
        commands: vec![
            Command::RenameProject { name: "rollback".into() },
            Command::TrimItem { layer_id: id.clone(), item_id: "parent".into(), range_index: 0, edge: ClipEdge::End, new_time: s(10) },
        ],
    };
    assert!(session.execute(CommandEnvelope::human(command)).is_err());
    assert_eq!(session.project(), &before);
    assert!(session.execute(CommandEnvelope::human(Command::SplitItem { layer_id: id, item_id: "parent".into(), at: s(7) })).is_err());
}

#[test]
fn author_cycle_converts_point_and_keeps_region_on_return_to_note() {
    let mut p = Project::new("marks");
    p.assets.push(tv2_domain::commands::tests_support::fake_video("a", 10));
    let mut l = SemanticLayer::new("a".into(), LayerKind::Author, "Autor");
    let mut i = SemanticItem::new(TimeRange::new(s(1), s(1)), "nota");
    i.item_id = "mark".into();
    l.items.push(i);
    let id = l.layer_id.clone();
    p.layers.push(l);
    let mut session = ProjectSession::new(p);
    for state in [ItemState::Accepted, ItemState::Disabled, ItemState::Proposed] {
        session.execute(CommandEnvelope::human(Command::CycleAuthorDecision { layer_id: id.clone(), item_ids: vec!["mark".into()] })).unwrap();
        let item = session.project().layer(&id).unwrap().item(&"mark".into()).unwrap();
        assert_eq!(item.state, state);
        assert_eq!(item.ranges, vec![TimeRange::new(s(0), s(3))]);
    }
}

#[test]
fn agent_edits_do_not_forge_human_review_or_become_human_protected() {
    let (session, id) = fixture(LayerKind::User);
    let mut p = session.project().clone();
    for i in &mut p.layers[0].items {
        i.edited = false;
    }
    let mut session = ProjectSession::new(p);
    let actor = Actor::Agent { name: "test".into() };
    for label in ["first", "second"] {
        let e = CommandEnvelope::human(Command::SetItemProps {
            layer_id: id.clone(),
            item_id: "parent".into(),
            label: Some(label.into()),
            comment: None,
            ranges: None,
        })
        .with_actor(actor.clone());
        assert!(!session.dry_run(&e).unwrap().preview.layers[0].item(&"parent".into()).unwrap().edited);
        session.execute(e).unwrap();
    }
    assert!(
        session
            .execute(
                CommandEnvelope::human(Command::SetItemState { layer_id: id.clone(), item_ids: vec!["parent".into()], state: ItemState::Accepted })
                    .with_actor(actor.clone())
            )
            .is_err()
    );
    session
        .execute(CommandEnvelope::human(Command::SetItemProps {
            layer_id: id.clone(),
            item_id: "parent".into(),
            label: Some("Human".into()),
            comment: None,
            ranges: None,
        }))
        .unwrap();
    assert!(
        session
            .execute(CommandEnvelope::human(Command::ShiftItems { layer_id: id, item_ids: vec!["parent".into()], delta: s(1) }).with_actor(actor))
            .is_err()
    );
}

#[test]
fn layer_order_requires_complete_unique_membership_and_undo() {
    let (mut session, id) = fixture(LayerKind::User);
    assert!(session.execute(CommandEnvelope::human(Command::SetLayerOrder { layer_ids: vec![id.clone(), id.clone()] })).is_err());
    assert!(session.execute(CommandEnvelope::human(Command::SetLayerOrder { layer_ids: vec![] })).is_err());
    session.execute(CommandEnvelope::human(Command::SetLayerOrder { layer_ids: vec![id.clone()] })).unwrap();
    assert_eq!(session.project().layer_order, vec![id]);
    session.undo(Actor::Human).unwrap();
    assert!(session.project().layer_order.is_empty());
}

#[test]
fn durable_history_reopens_undo_and_redo_with_receipts_and_rejects_corruption() {
    let (mut session, id) = fixture(LayerKind::Topics);
    let command = CommandEnvelope::human(Command::SplitItem { layer_id: id, item_id: "parent".into(), at: s(6) }).with_idempotency("durable-split");
    session.execute(command.clone()).unwrap();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "second".into() })).unwrap();
    session.undo(Actor::Human).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = crate::ProjectStore::at(dir.path());
    store.save_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
    let mut opened = ProjectSession::with_audit(store.load().unwrap(), &store.read_journal().unwrap()).unwrap();
    opened.restore_history(store.load_history(opened.project()).unwrap().unwrap()).unwrap();
    assert!(opened.execute(command).unwrap().replayed);
    opened.redo(Actor::Human).unwrap();
    assert_eq!(opened.project().name, "second");
    opened.undo(Actor::Human).unwrap();
    opened.undo(Actor::Human).unwrap();
    assert_eq!(opened.project().layers[0].items.len(), 3);
    let mut history = opened.history_snapshot();
    history.redo.back_mut().unwrap().before.name = "corrupt".into();
    assert!(opened.restore_history(history).is_err());
}

#[test]
fn save_acknowledgement_does_not_clean_newer_edits_or_discard_their_audit() {
    let (mut session, _) = fixture(LayerKind::User);
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "saved".into() })).unwrap();
    let revision = session.revision();
    let count = session.pending_journal().len();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "newer".into() })).unwrap();
    session.acknowledge_save(revision, count);
    assert!(session.is_dirty());
    assert_eq!(session.pending_journal().len(), 1);
    assert_eq!(session.pending_journal()[0].new_revision, session.revision());
}

#[test]
fn autosave_second_instance_preserves_first_recovery_and_rejects_external_save() {
    let dir = tempfile::tempdir().unwrap();
    let first = crate::ProjectStore::at(dir.path());
    let (mut a, _) = fixture(LayerKind::User);
    first.save(a.project()).unwrap();
    let second = crate::ProjectStore::at(dir.path());
    let mut b = ProjectSession::new(second.load().unwrap());
    a.execute(CommandEnvelope::human(Command::RenameProject { name: "first recovery".into() })).unwrap();
    b.execute(CommandEnvelope::human(Command::RenameProject { name: "second recovery".into() })).unwrap();
    first.save_autosave_checkpoint(a.project(), a.pending_journal(), Some(&a.history_snapshot())).unwrap();
    assert!(second.save_autosave(b.project(), b.pending_journal()).is_err());
    assert_eq!(first.recovery_with_audit(&first.load().unwrap()).unwrap().unwrap().0.name, "first recovery");
    second.save_with_journal(b.project(), b.pending_journal()).unwrap();
    assert!(first.save_autosave(a.project(), a.pending_journal()).is_err());
}
