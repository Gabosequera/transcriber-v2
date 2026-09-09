use serde_json::json;
use tv2_application::{Actor, CommandEnvelope, ProjectSession};
use tv2_domain::{Command, LayerKind, Project, SemanticItem, SemanticLayer, Ticks, TimeRange};

fn fixture(words: serde_json::Value) -> (ProjectSession, tv2_domain::LayerId, tv2_domain::ItemId) {
    let asset = tv2_domain::commands::tests_support::fake_video("source", 10);
    let document: tv2_domain::evidence::EvidenceDocument = json!({
        "schema":"editorial-master/1","media":{"duration":10.0,"fingerprint":asset.fingerprint},
        "tracks":{"A":{"words":words,"laughter":[]}},"conversation":{"utterances":[],"clean_utterance_ids":[]}
    })
    .into();
    let mut layer = SemanticLayer::new(asset.id.clone(), LayerKind::Blocks, "Bloques");
    layer.extra.insert("v1_chunks_header".into(), json!({"schema":"editorial-chunks/1","planner":"model-original"}));
    layer.source_master_digest = Some(document.source_digest().into());
    let mut first = SemanticItem::new(TimeRange::new(Ticks::ZERO, Ticks::from_seconds(5)), "Uno");
    first.extra.insert("confidence".into(), json!(0.4));
    let item_id = first.item_id.clone();
    layer.items.extend([first, SemanticItem::new(TimeRange::new(Ticks::from_seconds(5), Ticks::from_seconds(10)), "Dos")]);
    // This fixture represents model output awaiting its first human review.
    for item in &mut layer.items {
        item.edited = false;
        item.origin = Some("ai".into());
    }
    let layer_id = layer.layer_id.clone();
    let mut project = Project::new("Block review");
    project.masters.push(tv2_domain::evidence::MasterEvidence {
        asset_id: asset.id.clone(),
        source_digest: document.source_digest().into(),
        document,
        source_bundle: None,
    });
    project.assets.push(asset);
    project.layers.push(layer);
    project.validate().unwrap();
    (ProjectSession::new(project), layer_id, item_id)
}
fn edit(layer: &tv2_domain::LayerId, item: &tv2_domain::ItemId, radius: Ticks) -> Command {
    Command::Batch {
        label: "Revisión manual".into(),
        commands: vec![
            Command::SetItemStructure {
                layer_id: layer.clone(),
                item_id: item.clone(),
                parent_id: None,
                ranges: vec![TimeRange::new(Ticks::ZERO, Ticks::from_seconds(5))],
            },
            Command::SetItemProps { layer_id: layer.clone(), item_id: item.clone(), label: Some("Revisado".into()), comment: None, ranges: None },
            Command::SetBlockConfidence { layer_id: layer.clone(), item_id: item.clone(), confidence: 0.9 },
            Command::SnapBlockBoundaries { layer_id: layer.clone(), radius },
        ],
    }
}

#[test]
fn block_review_prepares_exact_safe_bounds_and_preserves_evidence_through_undo() {
    let (mut session, layer, item) = fixture(json!([{"t_ini":4.0,"t_fin":6.0,"text":"habla"}]));
    let before = session.project().clone();
    let prepared = session.prepare_command(CommandEnvelope::human(edit(&layer, &item, Ticks::from_seconds(2))).with_base(0)).unwrap();
    let preview = prepared.preview().layer(&layer).unwrap();
    assert_eq!(preview.extra["v1_chunks_header"]["planner"], "manual-review");
    assert_eq!(preview.item(&item).unwrap().extra["confidence"], 0.9);
    let boundary = preview.item(&item).unwrap().end().as_seconds_f64();
    assert!(!(3.96..6.04).contains(&boundary));
    assert_eq!(session.project(), &before);
    session.commit_prepared(prepared).unwrap();
    assert_eq!(session.project().masters, before.masters);
    session.undo(Actor::Human).unwrap();
    assert_eq!(session.project().layers, before.layers);
}

#[test]
fn unsafe_or_missing_evidence_rejects_the_whole_review_batch() {
    let (mut session, layer, item) = fixture(json!([{"t_ini":0.0,"t_fin":10.0,"text":"continua"}]));
    let before = session.project().clone();
    assert!(session.execute(CommandEnvelope::human(edit(&layer, &item, Ticks::from_seconds(1)))).is_err());
    assert_eq!(session.project(), &before);
    let mut missing = before.clone();
    missing.masters.clear();
    let mut missing = ProjectSession::new(missing);
    let before_missing = missing.project().clone();
    assert!(missing.execute(CommandEnvelope::human(edit(&layer, &item, Ticks::from_seconds(1)))).is_err());
    assert_eq!(missing.project(), &before_missing);
}

#[test]
fn confidence_is_bounded_agent_attributed_and_stale_prepared_review_rejected() {
    let (mut session, layer, item) = fixture(json!([]));
    for confidence in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        assert!(
            session
                .execute(CommandEnvelope::human(Command::SetBlockConfidence { layer_id: layer.clone(), item_id: item.clone(), confidence }))
                .is_err()
        );
    }
    session
        .execute(
            CommandEnvelope::human(Command::SetBlockConfidence { layer_id: layer.clone(), item_id: item.clone(), confidence: 0.7 })
                .with_actor(Actor::Agent { name: "assistant".into() }),
        )
        .unwrap();
    let reviewed = session.project().layer(&layer).unwrap();
    assert!(!reviewed.item(&item).unwrap().edited);
    assert_eq!(reviewed.extra["v1_chunks_header"]["planner"], "model-original");
    let prepared =
        session.prepare_command(CommandEnvelope::human(edit(&layer, &item, Ticks::from_seconds(1))).with_base(session.revision())).unwrap();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "New base".into() })).unwrap();
    assert!(session.commit_prepared(prepared).is_err());
    assert_eq!(session.project().layer(&layer).unwrap().item(&item).unwrap().extra["confidence"], 0.7);
}
