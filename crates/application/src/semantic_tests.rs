use crate::{Actor, CommandEnvelope, ProjectSession};
use tv2_domain::{ClipEdge, Command, ItemState, LayerKind, Project, SemanticItem, SemanticLayer, Ticks, TimeRange};
fn s(n: i64) -> Ticks {
    Ticks::from_seconds(n)
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
