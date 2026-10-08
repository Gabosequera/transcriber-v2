//! Invented contract fixtures only. No backend, model or network is executed.
use super::*;
use tv2_domain::{ItemState, SemanticItem, SemanticLayer};

fn project() -> (Project, AssetId) {
    let mut project = Project::new("Invented editorial contract");
    let asset = tv2_domain::commands::tests_support::fake_video("invented-source", 12);
    let master = json!({"schema":"editorial-master/1","media":{"duration":12.0,"path":"C:/private/recording.mp4","fingerprint":asset.fingerprint},
        "tracks":{"a0":{"words":[
            {"word_id":"word-one","text":"Invented","t_ini":1.1,"t_fin":1.4},
            {"word_id":"word-two","text":"fixture","t_ini":5.1,"t_fin":5.5}],"utterances":[]},
            "a1":{"words":[{"word_id":"word-other-track","text":"conversation","t_ini":6.5,"t_fin":6.7}],"utterances":[]}},
        "conversation":{"utterances":[],"clean_utterance_ids":[]},"chunks":[],
        "analysis":{"lineage":{"source_path":"C:/private/recording.mp4"},"generation":{"runtime":{"work_root":"C:/private/jobs"}}}});
    project.masters.push(tv2_v1compat::master::V1Master::parse(master).unwrap().evidence(&asset.id));
    let id = asset.id.clone();
    project.assets.push(asset);
    project.validate().unwrap();
    (project, id)
}
fn input(kind: ReviewKind) -> EditorialInput {
    let (project, asset) = project();
    prepare_input(&project, &asset, "invented-cycle", kind, None, Default::default()).unwrap()
}
fn topics_with_tracks(tracks: Value, scope: Option<TimeRange>) -> EditorialInput {
    let (mut project, asset) = project();
    let mut master = (*project.masters[0].document).clone();
    master["tracks"] = tracks;
    project.masters[0] = tv2_v1compat::master::V1Master::parse(master).unwrap().evidence(&asset);
    project.validate().unwrap();
    prepare_input(&project, &asset, "invented-coverage-cycle", ReviewKind::Topics, scope, Default::default()).unwrap()
}
fn response(input: &EditorialInput) -> Value {
    json!({"schema":format!("editorial-{}-proposal/1",input.kind.stem()),"request_id":input.request["request_id"],
        "source_master_digest":input.request["source_master_digest"],"source_layers_digest":input.request["source_layers_digest"]})
}
fn topic(id: &str, start: f64, end: f64) -> Value {
    json!({"item_id":id,"label":"invented topic","comment":"contract fixture, no inference","state":"proposed","edited":false,"parent_id":null,"ranges":[{"t_ini":start,"t_fin":end}]})
}
fn first(input: &EditorialInput) -> Value {
    let mut value = response(input);
    value["pass"] = json!(1);
    value["complete"] = json!(true);
    value["items"] = json!([topic("map-one", 1.0, 3.0), topic("map-two", 5.0, 7.0)]);
    value
}
fn second(input: &EditorialInput) -> Value {
    let mut value = response(input);
    value["pass"] = json!(2);
    value["complete"] = json!(true);
    value["previous_pass_digest"] = input.request["previous_pass_digest"].clone();
    value["items"] = json!([{"item_id":"unified","label":"invented recurrence","state":"proposed","ranges":[{"t_ini":1.0,"t_fin":3.0},{"t_ini":5.0,"t_fin":7.0}],"source_item_ids":["map-one","map-two"]}]);
    value
}
fn runtime(root: &Path) -> EditorialRuntime {
    let runtime = EditorialRuntime {
        python: root.join("not-executable-python-fixture"),
        worker: root.join("not-executable-worker-fixture"),
        model: root.join("synthetic-resources"),
        model_manifest: root.join("model-manifest.json"),
        lock: root.join("synthetic.lock"),
        work_root: root.join("jobs-root"),
    };
    fs::create_dir_all(&runtime.model).unwrap();
    fs::write(&runtime.python, b"NOT AN EXECUTABLE: contract metadata only").unwrap();
    fs::write(&runtime.worker, b"NOT A WORKER: contract metadata only").unwrap();
    fs::write(&runtime.lock, b"synthetic-contract-package==0.0.1\n").unwrap();
    let bytes = b"NOT MODEL WEIGHTS: no inference";
    fs::write(runtime.model.join("fixture.bin"), bytes).unwrap();
    fs::write(&runtime.model_manifest,serde_json::to_vec(&json!({"schema":"tv2-editorial-model/1","model":"invented-contract-only","revision":"fixture-1",
        "license":"fixture-not-a-model","source":"invented test metadata","files":{"fixture.bin":{"size":bytes.len(),"sha256":hex::encode(Sha256::digest(bytes))}}})).unwrap()).unwrap();
    runtime
}

#[test]
fn unconfigured_and_missing_backend_do_not_claim_inference_or_create_work() {
    let cancel = AtomicBool::new(false);
    assert!(matches!(backend_status(None, &cancel).unwrap(), BackendStatus::NotConfigured));
    let root = tempfile::tempdir().unwrap();
    let configured = runtime(root.path());
    fs::remove_file(&configured.worker).unwrap();
    assert!(matches!(backend_status(Some(&configured), &cancel).unwrap(), BackendStatus::MissingResources { .. }));
    assert!(enqueue(input(ReviewKind::Topics), configured.clone(), &cancel).is_err());
    assert!(!configured.work_root.exists());
}
#[test]
fn configured_files_are_unverified_and_hash_swaps_are_rejected_before_execution() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let cancel = AtomicBool::new(false);
    assert!(matches!(backend_status(Some(&runtime), &cancel).unwrap(), BackendStatus::ConfiguredUnverified { .. }));
    let record = enqueue(input(ReviewKind::Topics), runtime.clone(), &cancel).unwrap();
    assert_eq!(record.state, JobState::Queued);
    fs::write(&runtime.lock, b"synthetic-contract-package==0.0.2\n").unwrap();
    assert!(verify_payload(&record.payload, &cancel).is_err());
    assert_eq!(jobs::discover::<EditorialPayload>(&runtime.work_root.join("jobs")).unwrap()[0].state, JobState::Queued);
}
#[test]
fn payload_tampering_fails_before_claim_and_explicit_cancel_is_durable_without_backend() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let mut record = enqueue(input(ReviewKind::Topics), runtime.clone(), &AtomicBool::new(false)).unwrap();
    record.payload.input.parameters.max_tokens += 1;
    assert!(run(&record, false, Arc::new(AtomicBool::new(true)), |_| {}).is_err());
    assert_eq!(jobs::discover::<EditorialPayload>(&runtime.work_root.join("jobs")).unwrap()[0].state, JobState::Queued);
    record.payload.input.parameters.max_tokens -= 1;
    assert_eq!(run(&record, false, Arc::new(AtomicBool::new(true)), |_| {}).unwrap_err().code, ErrorCode::Cancelled);
    let saved = jobs::discover::<EditorialPayload>(&runtime.work_root.join("jobs")).unwrap().remove(0);
    assert_eq!(saved.state, JobState::Cancelled);
    assert!(saved.result.is_none());
    assert!(!runtime.work_root.join(&record.id).exists());
}
#[test]
fn topics_advance_uses_validated_map_and_only_second_pass_creates_proposals() {
    let initial = input(ReviewKind::Topics);
    let original = initial.project.clone();
    let first = first(&initial);
    let review = validate_proposal(&initial, &first).unwrap();
    assert!(review.command.is_none());
    let next = advance_topics(&initial, &first).unwrap();
    assert_eq!(next.project, original);
    assert_eq!(next.request["pass_required"], 2);
    assert_eq!(next.request["previous_pass_digest"], digest_json(next.previous.as_ref().unwrap()));
    assert!(next.previous.as_ref().unwrap().get("source_proposal_digest").is_some());
    assert_ne!(next.previous.as_ref().unwrap(), &first);
    let response = second(&next);
    let review = validate_proposal(&next, &response).unwrap();
    assert!(review.command.is_some());
    let mut session = ProjectSession::new(original.clone());
    let prepared = session
        .prepare_command(
            CommandEnvelope::human(review.command.unwrap()).with_actor(Actor::External { source: "invented-contract-no-model".into() }).with_base(0),
        )
        .unwrap();
    assert_eq!(session.project(), &original);
    session.commit_prepared(prepared).unwrap();
    assert!(session.project().layers.iter().any(|l| l.kind == LayerKind::Topics));
    session.undo(Actor::Human).unwrap();
    assert_eq!(session.project().masters, original.masters);
}
#[test]
fn local_topics_require_all_spoken_intervals_but_allow_silent_gaps() {
    let initial = input(ReviewKind::Topics);
    let complete = first(&initial);
    // The leading/trailing silence and gap between the two topics are permitted.
    validate_proposal(&initial, &complete).unwrap();
    for items in [json!([]), json!([topic("only-beginning", 0.0, 1.0)]), json!([topic("missing-other-track", 1.0, 5.8)])] {
        let mut incomplete = complete.clone();
        incomplete["items"] = items;
        assert!(validate_proposal(&initial, &incomplete).unwrap_err().to_string().contains("omite palabras"));
        assert!(advance_topics(&initial, &incomplete).is_err());
        // The interoperable V1 validator remains deliberately unchanged.
        assert!(contracts::prepare_proposal(&initial.project, &initial.asset_id, &initial.request, &incomplete, None).is_ok());
    }
}
#[test]
fn local_topics_missing_words_are_insufficient_context_not_success() {
    let initial = topics_with_tracks(json!({"a0":{"words":[],"utterances":[]}}), None);
    assert!(validate_proposal(&initial, &first(&initial)).unwrap_err().to_string().contains("Contexto insuficiente"));
    let scoped = topics_with_tracks(
        json!({"a0":{"words":[{"word_id":"outside","text":"invented","t_ini":9.0,"t_fin":10.0}]}}),
        Some(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(7))),
    );
    assert!(validate_proposal(&scoped, &first(&scoped)).unwrap_err().to_string().contains("Contexto insuficiente"));
}
#[test]
fn local_topics_coverage_uses_adjusted_map_and_clips_words_to_scope() {
    let initial = topics_with_tracks(json!({"a0":{"words":[{"word_id":"inside","text":"invented","t_ini":5.1,"t_fin":5.5}]}}), None);
    let mut raw = first(&initial);
    raw["items"] = json!([topic("safe-snap", 5.0, 5.45)]);
    // The raw endpoint misses the end of the word, but safe snap repairs it.
    assert!(validate_topics_word_coverage(&initial, &raw).is_err());
    let review = validate_proposal(&initial, &raw).unwrap();
    let validated: Value = serde_json::from_str(&review.documents["views/topics-pass1.json"]).unwrap();
    assert!(validated["items"][0]["ranges"][0]["t_fin"].as_f64().unwrap() >= 5.5);
    let scoped = topics_with_tracks(
        json!({"a0":{"words":[{"word_id":"cross-start","text":"invented","t_ini":0.5,"t_fin":1.5},
            {"word_id":"cross-end","text":"words","t_ini":6.5,"t_fin":7.5}]}}),
        Some(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(7))),
    );
    let map = json!({"items":[topic("clipped",1.0,7.0)]});
    validate_topics_word_coverage(&scoped, &map).unwrap();
}
#[test]
fn local_topics_coverage_union_allows_only_one_millisecond_rounding() {
    let initial = topics_with_tracks(json!({"a0":{"words":[{"word_id":"spans-union","text":"invented","t_ini":1.0,"t_fin":2.0}]}}), None);
    let map = json!({"items":[topic("left",1.0005,1.5),topic("right",1.5008,1.9995)]});
    validate_topics_word_coverage(&initial, &map).unwrap();
    let mut changed = map;
    changed["items"][1]["ranges"][0]["t_ini"] = json!(1.502);
    assert!(validate_topics_word_coverage(&initial, &changed).is_err());
}
#[test]
fn local_topics_reject_nine_observed_duplicate_copies_without_repairing_them() {
    let initial = topics_with_tracks(
        json!({"a0":{"words":[{"word_id":"invented","text":"software","t_ini":1.0,"t_fin":2.0}]}}),
        Some(TimeRange::new(Ticks::ZERO, Ticks::from_seconds_f64(9.531))),
    );
    let mut proposal = first(&initial);
    let mut items = vec![topic("recording", 0.0, 9.531)];
    items[0]["label"] = json!("Synthetic recording");
    for index in 0..9 {
        items.push(json!({"item_id":format!("software-{index}"),"label":"Software Test","state":"proposed","edited":false,
            "ranges":[{"t_ini":0.0,"t_fin":9.531}]}));
    }
    proposal["items"] = json!(items);
    let preserved = proposal.clone();
    assert!(contracts::prepare_proposal(&initial.project, &initial.asset_id, &initial.request, &proposal, None).is_ok());
    assert!(validate_proposal(&initial, &proposal).unwrap_err().to_string().contains("temas duplicado"));
    assert!(advance_topics(&initial, &proposal).is_err());
    assert_eq!(proposal, preserved);
}
#[test]
fn local_topics_duplicate_key_normalizes_title_and_exact_range_union_after_snap() {
    let initial = topics_with_tracks(json!({"a0":{"words":[{"word_id":"inside","text":"invented","t_ini":5.1,"t_fin":5.5}]}}), None);
    let mut proposal = first(&initial);
    let mut one = topic("one", 5.09, 5.6);
    let mut two = topic("two", 5.095, 5.6);
    one["label"] = json!("Software Test");
    two["label"] = json!("  SOFTWARE\t test \n");
    one.as_object_mut().unwrap().remove("comment");
    two["comment"] = Value::Null;
    proposal["items"] = json!([one, two]);
    // Different raw boundaries become identical only after safe snapping.
    validate_topics_word_coverage(&initial, &proposal).unwrap();
    assert!(validate_proposal(&initial, &proposal).unwrap_err().to_string().contains("temas duplicado"));
    let map = json!({"items":[topic("whole",5.0,6.0),
        {"item_id":"parts","label":"invented topic","comment":"contract fixture, no inference",
        "ranges":[{"t_ini":5.0,"t_fin":5.5},{"t_ini":5.5,"t_fin":6.0}]}]});
    assert!(validate_topics_word_coverage(&initial, &map).unwrap_err().to_string().contains("temas duplicado"));
}
#[test]
fn local_topics_allow_recurrences_distinct_parents_and_distinct_comments() {
    let initial = input(ReviewKind::Topics);
    // The fixture has the same title/comment in two disjoint ranges.
    validate_proposal(&initial, &first(&initial)).unwrap();
    let mut proposal = first(&initial);
    let mut left = topic("child-left", 1.0, 3.0);
    let mut right = topic("child-right", 1.0, 3.0);
    left["parent_id"] = json!("parent-left");
    right["parent_id"] = json!("parent-right");
    let mut parent_left = topic("parent-left", 0.0, 12.0);
    let mut parent_right = topic("parent-right", 0.0, 12.0);
    parent_left["label"] = json!("Left subject");
    parent_right["label"] = json!("Right subject");
    proposal["items"] = json!([parent_left, parent_right, left, right]);
    validate_proposal(&initial, &proposal).unwrap();
    let mut one = topic("explanation-one", 0.0, 12.0);
    let mut two = topic("explanation-two", 0.0, 12.0);
    one["comment"] = json!("First editorial reason");
    two["comment"] = json!("Different editorial reason");
    proposal["items"] = json!([one, two]);
    validate_proposal(&initial, &proposal).unwrap();
}
#[test]
fn recovered_second_pass_cannot_inherit_duplicate_first_map_content() {
    let initial = input(ReviewKind::Topics);
    let mut duplicates = first(&initial);
    duplicates["items"] = json!([topic("map-one", 0.0, 12.0), topic("map-two", 0.0, 12.0)]);
    let old = contracts::prepare_proposal(&initial.project, &initial.asset_id, &initial.request, &duplicates, None).unwrap();
    let mut next = initial.clone();
    next.request = serde_json::from_str(&old.documents["views/topics-request.json"]).unwrap();
    next.previous = Some(serde_json::from_str(&old.documents["views/topics-pass1.json"]).unwrap());
    next.documents.extend(old.documents);
    redact_documents(&mut next.documents).unwrap();
    let mut response = second(&next);
    response["items"][0]["ranges"] = json!([{"t_ini":0.0,"t_fin":12.0}]);
    assert!(contracts::prepare_proposal(&next.project, &next.asset_id, &next.request, &response, next.previous.as_ref()).is_ok());
    assert!(validate_proposal(&next, &response).unwrap_err().to_string().contains("temas duplicado"));
}
#[test]
fn recovered_second_pass_cannot_inherit_an_incomplete_old_map() {
    let initial = input(ReviewKind::Topics);
    let mut incomplete = first(&initial);
    incomplete["items"] = json!([topic("map-one", 1.0, 3.0)]);
    let old = contracts::prepare_proposal(&initial.project, &initial.asset_id, &initial.request, &incomplete, None).unwrap();
    // Explicit old V1 fixture, not a newly accepted local inference.
    let mut next = initial.clone();
    next.request = serde_json::from_str(&old.documents["views/topics-request.json"]).unwrap();
    next.previous = Some(serde_json::from_str(&old.documents["views/topics-pass1.json"]).unwrap());
    next.documents.extend(old.documents);
    redact_documents(&mut next.documents).unwrap();
    validate_input(&next).unwrap();
    let mut response = second(&next);
    response["items"] =
        json!([{"item_id":"unified","label":"invented","state":"proposed","ranges":[{"t_ini":1.0,"t_fin":3.0}],"source_item_ids":["map-one"]}]);
    assert!(contracts::prepare_proposal(&next.project, &next.asset_id, &next.request, &response, next.previous.as_ref()).is_ok());
    assert!(validate_proposal(&next, &response).unwrap_err().to_string().contains("omite palabras"));
}
#[test]
fn topics_reject_wrong_pass_digest_omission_duplication_and_changed_ranges() {
    let initial = input(ReviewKind::Topics);
    let next = advance_topics(&initial, &first(&initial)).unwrap();
    let original = second(&next);
    for pointer in ["/request_id", "/source_master_digest", "/source_layers_digest", "/previous_pass_digest"] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("other");
        assert!(validate_proposal(&next, &changed).is_err(), "{pointer}");
    }
    let mut changed = original.clone();
    changed["pass"] = json!(true);
    assert!(validate_proposal(&next, &changed).is_err());
    let mut changed = original.clone();
    changed["items"][0]["source_item_ids"] = json!(["map-one"]);
    assert!(validate_proposal(&next, &changed).is_err());
    let mut changed = original.clone();
    changed["items"][0]["source_item_ids"] = json!(["map-one", "map-one", "map-two"]);
    assert!(validate_proposal(&next, &changed).is_err());
    let mut changed = original.clone();
    changed["items"][0]["ranges"][0]["t_ini"] = json!(1.1);
    assert!(validate_proposal(&next, &changed).is_err());
    let mut changed = next;
    changed.documents.get_mut("views/topics-pass1.json").unwrap().push('x');
    assert!(validate_input(&changed).is_err());
}
#[test]
fn requests_parameters_and_snapshot_are_bound_to_canonical_scope_and_pass() {
    let initial = input(ReviewKind::Topics);
    for pointer in ["/pass_required", "/source_layers_digest", "/scope/t_fin", "/layer_id"] {
        let mut changed = initial.clone();
        *changed.request.pointer_mut(pointer).unwrap() = json!("forged");
        assert!(validate_input(&changed).is_err(), "{pointer}");
    }
    let mut changed = initial.clone();
    changed.project.name = "human edit".into();
    changed.project_digest = digest_json(&serde_json::to_value(&changed.project).unwrap());
    changed.project.revision += 1;
    assert!(validate_input(&changed).is_err());
    let mut changed = initial;
    changed.parameters.temperature = f64::NAN;
    assert!(validate_input(&changed).is_err());
}
#[test]
fn human_attribution_and_destructive_flags_are_rejected_before_normalization() {
    let input = input(ReviewKind::Topics);
    let original = first(&input);
    for (key, value) in [
        ("edited", json!(true)),
        ("accepted", json!(true)),
        ("origin", json!("human")),
        ("actor", json!({"kind":"human"})),
        ("state", json!("accepted")),
        ("deleted", json!(true)),
        ("locked", json!(true)),
        ("deleted_item_ids", json!(["protected"])),
        ("enabled", json!(false)),
    ] {
        let mut changed = original.clone();
        changed["items"][0][key] = value;
        assert!(validate_proposal(&input, &changed).is_err(), "{key}");
    }
}
#[test]
fn model_proposals_preserve_human_items_and_tombstones_through_common_command() {
    let (mut project, asset) = project();
    let mut layer = SemanticLayer::new(asset.clone(), LayerKind::Ai, "existing");
    layer.layer_id = "existing-ai".into();
    let mut human = SemanticItem::new(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(3)), "human decision");
    human.item_id = "protected-item".into();
    human.state = ItemState::Accepted;
    human.comment = "keep human comment".into();
    layer.items.push(human.clone());
    layer.deleted_item_ids.push("deleted-item".into());
    project.layers.push(layer);
    let input = prepare_input(&project, &asset, "invented-layer-cycle", ReviewKind::Layers, None, Default::default()).unwrap();
    let mut response = response(&input);
    response["layer"] = json!({"schema":"editorial-layer/1","layer_id":"existing-ai","kind":"ai","name":"model suggestion","media_fingerprint":project.assets[0].fingerprint,
        "items":[topic("protected-item",4.0,5.0),topic("deleted-item",6.0,7.0),topic("fresh-item",8.0,9.0)]});
    let review = validate_proposal(&input, &response).unwrap();
    let mut session = ProjectSession::new(project.clone());
    let prepared = session
        .prepare_command(CommandEnvelope::human(review.command.unwrap()).with_actor(Actor::External { source: "invented-contract-no-model".into() }))
        .unwrap();
    assert_eq!(session.project(), &project);
    session.commit_prepared(prepared).unwrap();
    let layer = session.project().layer(&"existing-ai".into()).unwrap();
    assert_eq!(layer.items.iter().find(|i| i.item_id == human.item_id), Some(&human));
    assert!(!layer.items.iter().any(|i| i.item_id.as_str() == "deleted-item"));
    assert_eq!(layer.deleted_item_ids, vec!["deleted-item".into()]);
}
#[test]
fn deep_trims_require_selected_lane_and_current_trims_digest() {
    let (project, asset) = project();
    let parameters = EditorialParameters { trim_mode: TrimMode::Deep, ..Default::default() };
    let input = prepare_input(&project, &asset, "invented-deep", ReviewKind::Trims, None, parameters).unwrap();
    let mut response = response(&input);
    response["mode"] = json!("deep");
    response["lane"] = json!("ai-deep");
    response["trims_digest"] = input.request["trims_digest"].clone();
    response["cuts"] = json!([{"t_ini":8.0,"t_fin":9.0,"reason":"invented-contract-only"}]);
    assert!(validate_proposal(&input, &response).unwrap().command.is_some());
    for (key, value) in [("lane", json!("ai-other")), ("mode", json!("content")), ("trims_digest", json!("forged"))] {
        let mut changed = response.clone();
        changed[key] = value;
        assert!(validate_proposal(&input, &changed).is_err(), "{key}");
    }
}
#[test]
fn montage_pass_and_digest_reject_rebinding_and_unexplained_repetition() {
    let input = input(ReviewKind::Montage);
    let mut response = response(&input);
    response["pass"] = json!(1);
    response["montage_digest"] = Value::Null;
    response["clips"] = json!([{"source_ini":8.0,"source_fin":9.0,"reason":"invented-contract-only"}]);
    assert!(validate_proposal(&input, &response).unwrap().command.is_some());
    let mut changed = response.clone();
    changed["pass"] = json!(2);
    assert!(validate_proposal(&input, &changed).is_err());
    let mut changed = response.clone();
    changed["montage_digest"] = json!("forged");
    assert!(validate_proposal(&input, &changed).is_err());
    response["clips"].as_array_mut().unwrap().push(json!({"source_ini":8.0,"source_fin":9.0}));
    assert!(validate_proposal(&input, &response).is_err());
}
#[test]
fn context_redacts_personal_locators_preserves_original_and_rejects_added_documents() {
    let input = input(ReviewKind::Topics);
    let original = input.project.masters[0].document.clone();
    let context: Value = serde_json::from_str(&input.documents["project.editorial.master.json"]).unwrap();
    assert_ne!(context["media"]["path"], original["media"]["path"]);
    assert!(!input.documents.values().any(|s| s.contains("C:/private")));
    assert_eq!(input.project.masters[0].document, original);
    let mut changed = input.clone();
    changed.documents.insert("views/unrequested.json".into(), "{}".into());
    assert!(validate_input(&changed).is_err());
    let mut changed = input;
    changed.documents.insert("project.editorial.master.json".into(), serde_json::to_string(&*original).unwrap());
    assert!(validate_input(&changed).is_err());
}
#[test]
fn owned_input_references_are_sha_bound_and_traversal_or_alteration_fails() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let cancel = AtomicBool::new(false);
    let record = enqueue(input(ReviewKind::Topics), runtime, &cancel).unwrap();
    let refs = input_files(&record, true, &cancel).unwrap();
    assert!(refs["views/topics-request.json"]["sha256"].as_str().is_some_and(valid_sha));
    let request = backend_request(&record, refs);
    assert!(request.get("project").is_none());
    assert!(request.get("snapshot").is_none());
    let path = record.payload.runtime.work_root.join(&record.id).join("inputs/views/topics-request.json");
    fs::write(path, b"{}").unwrap();
    assert!(input_files(&record, false, &cancel).is_err());
    let mut changed = record.payload.input.clone();
    changed.documents.insert("../escape.json".into(), "{}".into());
    assert!(validate_input(&changed).is_err());
}

#[test]
fn live_job_cannot_be_claimed_by_another_local_request() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let cancel = AtomicBool::new(false);
    let record = enqueue(input(ReviewKind::Topics), runtime.clone(), &cancel).unwrap();
    let lease = jobs::claim::<EditorialPayload>(&runtime.work_root.join("jobs"), &record.id, &record.payload_digest, false).unwrap();
    assert!(run(&record, true, Arc::new(AtomicBool::new(true)), |_| {}).is_err());
    assert_eq!(jobs::discover::<EditorialPayload>(&runtime.work_root.join("jobs")).unwrap()[0].state, JobState::Running);
    lease.finish(JobState::Cancelled, None, Some("invented lock ownership fixture; backend never executed".into())).unwrap();
}

#[test]
fn result_requires_real_inference_declaration_and_closed_receipt_fields() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let cancel = AtomicBool::new(false);
    let record = enqueue(input(ReviewKind::Topics), runtime, &cancel).unwrap();
    let result = EditorialResult {
        job_id: record.id.clone(),
        project_id: record.project_id.clone(),
        revision: record.revision,
        project_digest: record.payload.input.project_digest.clone(),
        asset_id: record.payload.input.asset_id.clone(),
        input_digest: record.payload_digest.clone(),
        request_id: record.payload.input.request["request_id"].as_str().unwrap().into(),
        request_digest: digest_json(&record.payload.input.request),
        pass: 1,
        backend: record.payload.backend.clone(),
        native_inference_executed: false,
        proposal_path: OUTPUT.into(),
        artifacts: BTreeMap::from([(OUTPUT.into(), "0".repeat(64))]),
    };
    assert!(validate_result(&record, &result, &cancel).is_err());
    assert!(!record.payload.runtime.work_root.join(&record.id).exists());
    let mut extra = serde_json::to_value(result).unwrap();
    extra["pretend_success"] = json!(true);
    assert!(serde_json::from_value::<EditorialResult>(extra).is_err());
}

#[test]
fn cached_succeeded_receipt_and_continue_topics_revalidate_word_coverage() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let cancel = AtomicBool::new(false);
    let record = enqueue(input(ReviewKind::Topics), runtime.clone(), &cancel).unwrap();
    input_files(&record, true, &cancel).unwrap();
    let mut incomplete = first(&record.payload.input);
    incomplete["items"] = json!([topic("only-first-topic", 1.0, 3.0)]);
    let bytes = serde_json::to_vec(&incomplete).unwrap();
    let output = runtime.work_root.join(&record.id).join(OUTPUT);
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, &bytes).unwrap();
    // A fabricated legacy receipt is test data for validation, not inference evidence.
    // No executable fixture or native library is loaded by this test.
    let result = EditorialResult {
        job_id: record.id.clone(),
        project_id: record.project_id.clone(),
        revision: record.revision,
        project_digest: record.payload.input.project_digest.clone(),
        asset_id: record.payload.input.asset_id.clone(),
        input_digest: record.payload_digest.clone(),
        request_id: record.payload.input.request["request_id"].as_str().unwrap().into(),
        request_digest: digest_json(&record.payload.input.request),
        pass: 1,
        backend: record.payload.backend.clone(),
        native_inference_executed: true,
        proposal_path: OUTPUT.into(),
        artifacts: BTreeMap::from([(OUTPUT.into(), hex::encode(Sha256::digest(&bytes)))]),
    };
    let lease = jobs::claim::<EditorialPayload>(&runtime.work_root.join("jobs"), &record.id, &record.payload_digest, false).unwrap();
    lease.finish(JobState::Succeeded, Some(serde_json::to_value(&result).unwrap()), None).unwrap();
    let recovered = jobs::discover::<EditorialPayload>(&runtime.work_root.join("jobs")).unwrap().remove(0);
    assert_eq!(recovered.state, JobState::Succeeded);
    let parent_bytes = fs::read(runtime.work_root.join("jobs").join(format!("{}.json", record.id))).unwrap();
    for outcome in [
        validate_result(&recovered, &result, &cancel).map(|_| ()),
        read_verified_proposal(&recovered, &result, &cancel).map(|_| ()),
        continue_topics(&recovered, &result, &cancel).map(|_| ()),
    ] {
        assert!(outcome.unwrap_err().to_string().contains("omite palabras"));
    }
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert_eq!(fs::read(runtime.work_root.join("jobs").join(format!("{}.json", record.id))).unwrap(), parent_bytes);
}
