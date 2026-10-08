//! Small contract fixtures exercise incorporation and durability. They do not
//! stand in for the separately recorded real Whisper inference runs.
use super::*;
use tv2_application::ProjectStore;
use tv2_domain::{Command as ProjectCommand, ItemState, LayerKind, SemanticItem, SemanticLayer, Ticks, TimeRange};

struct Fixture {
    _temp: tempfile::TempDir,
    snapshot: Project,
    record: JobRecord<AnalysisPayload>,
    result: AnalysisResult,
    manual: SemanticLayer,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut asset = tv2_domain::commands::tests_support::fake_video("audio-source", 10);
        let source = temp.path().join("synthetic-source.bin");
        fs::write(&source, b"synthetic source for result contracts").unwrap();
        asset.path = source.to_string_lossy().replace('\\', "/");
        let mut manual = SemanticLayer::new(asset.id.clone(), LayerKind::User, "Human decisions before analysis");
        manual.layer_id = "human-notes".into();
        let mut accepted = SemanticItem::new(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(2)), "Keep this human choice");
        accepted.item_id = "human-accepted".into();
        accepted.state = ItemState::Accepted;
        let mut disabled = SemanticItem::new(TimeRange::new(Ticks::from_seconds(3), Ticks::from_seconds(4)), "Rejected by human");
        disabled.item_id = "human-disabled".into();
        disabled.state = ItemState::Disabled;
        manual.items.extend([accepted, disabled]);
        manual.deleted_item_ids.push("human-deleted".into());
        let mut snapshot = Project::new("analysis contracts");
        snapshot.assets.push(asset.clone());
        snapshot.layers.push(manual.clone());
        snapshot.validate().unwrap();
        let cancel = AtomicBool::new(false);
        let payload = AnalysisPayload {
            project_digest: ProjectSession::new(snapshot.clone()).digest().unwrap(),
            asset: asset.clone(),
            source_path: source.clone(),
            source_sha256: sha256_file(&source, &cancel).unwrap(),
            model_digest: "synthetic-model-digest".into(),
            worker_sha256: "synthetic-worker-digest".into(),
            runtime: Runtime {
                python: temp.path().join("unused-python"),
                worker: temp.path().join("unused-worker"),
                model: temp.path().join("unused-model"),
                work_root: temp.path().join("analysis"),
            },
            parameters: Parameters::default(),
            ffmpeg_path: temp.path().join("unused-ffmpeg"),
        };
        let record = jobs::enqueue(&payload.runtime.work_root.join("jobs"), snapshot.project_id.clone(), snapshot.revision, payload).unwrap();
        let job_root = record.payload.runtime.work_root.join(&record.id);
        fs::create_dir_all(&job_root).unwrap();
        let master = json!({
            "schema":"editorial-master/1", "project":{"name":"synthetic inference output"},
            "media":{"fingerprint":asset.fingerprint,"path":asset.path,"duration":10.0,"t0":0.0},
            "tracks":{"A":{"words":[{"word_id":"word-1","text":"fixture evidence","t_ini":1.0,"t_fin":2.0}],"utterances":[],"laughter":[],"arousal":[]}},
            "conversation":{"utterances":[],"clean_utterance_ids":[]}
        });
        let master_path = "project.editorial.master.json".to_string();
        fs::write(job_root.join(&master_path), serde_json::to_vec_pretty(&master).unwrap()).unwrap();
        let artifacts = BTreeMap::from([(master_path.clone(), sha256_file(&job_root.join(&master_path), &cancel).unwrap())]);
        let result = AnalysisResult {
            job_id: record.id.clone(),
            project_id: record.project_id.clone(),
            revision: record.revision,
            project_digest: record.payload.project_digest.clone(),
            source_sha256: record.payload.source_sha256.clone(),
            model_digest: record.payload.model_digest.clone(),
            master_path,
            artifacts,
        };
        Self { _temp: temp, snapshot, record, result, manual }
    }
    fn job_root(&self) -> PathBuf {
        self.record.payload.runtime.work_root.join(&self.record.id)
    }
    fn prepare(&self) -> DomainResult<PreparedCommand> {
        prepare_result(self.snapshot.clone(), &self.record, &self.result, &AtomicBool::new(false))
    }
}

#[test]
fn modified_job_payload_is_rejected_before_claim_or_worker_spawn() {
    let fixture = Fixture::new();
    let mut changed = fixture.record.clone();
    changed.payload.parameters.threads = 4;
    let cancel = Arc::new(AtomicBool::new(false));
    assert_eq!(run(&changed, false, cancel.clone(), |_| {}).unwrap_err().code, ErrorCode::Precondition);
    assert_eq!(validate_result(&changed, &fixture.result, &cancel).unwrap_err().code, ErrorCode::Precondition);
    let persisted = jobs::discover::<AnalysisPayload>(&fixture.record.payload.runtime.work_root.join("jobs")).unwrap();
    assert_eq!(persisted[0].state, JobState::Queued);
    assert!(persisted[0].attempts.is_empty());
}

#[test]
fn result_linkage_rejects_each_changed_identity_and_missing_receipt_entry() {
    let fixture = Fixture::new();
    let cancel = AtomicBool::new(false);
    validate_result(&fixture.record, &fixture.result, &cancel).unwrap();
    for field in ["job_id", "project_id", "revision", "project_digest", "source_sha256", "model_digest"] {
        let mut raw = serde_json::to_value(&fixture.result).unwrap();
        raw[field] = if field == "revision" { json!(1) } else { json!("changed") };
        let changed = serde_json::from_value(raw).unwrap();
        assert_eq!(validate_result(&fixture.record, &changed, &cancel).unwrap_err().code, ErrorCode::Precondition, "{field}");
    }
    let mut missing = fixture.result.clone();
    missing.artifacts.clear();
    assert_eq!(validate_result(&fixture.record, &missing, &cancel).unwrap_err().code, ErrorCode::Invalid);
}

#[test]
fn tampered_or_missing_artifact_is_rejected_before_preparing_any_change() {
    let fixture = Fixture::new();
    let path = fixture.job_root().join(&fixture.result.master_path);
    let original = fs::read(&path).unwrap();
    fs::write(&path, b"changed by another writer").unwrap();
    assert_eq!(validate_result(&fixture.record, &fixture.result, &AtomicBool::new(false)).unwrap_err().code, ErrorCode::Precondition);
    assert!(fixture.prepare().is_err());
    fs::write(&path, original).unwrap();
    fixture.prepare().unwrap();
    fs::rename(&path, fixture.job_root().join("retired-master.json")).unwrap();
    assert!(fixture.prepare().is_err());
}

#[test]
fn artifact_paths_cannot_escape_the_owned_job_even_with_matching_bytes() {
    let fixture = Fixture::new();
    let outside = fixture.record.payload.runtime.work_root.join("outside.json");
    fs::write(&outside, b"outside artifact").unwrap();
    let hash = sha256_file(&outside, &AtomicBool::new(false)).unwrap();
    for name in ["../outside.json".to_string(), "../outside.json/".into(), outside.to_string_lossy().into_owned(), "..\\outside.json".into()] {
        let mut result = fixture.result.clone();
        result.master_path = name.clone();
        result.artifacts = BTreeMap::from([(name, hash.clone())]);
        assert!(validate_result(&fixture.record, &result, &AtomicBool::new(false)).is_err());
    }
    assert_eq!(fs::read(outside).unwrap(), b"outside artifact");
}

#[test]
fn stale_project_revision_identity_digest_and_asset_are_rejected() {
    let fixture = Fixture::new();
    fixture.prepare().unwrap();
    let mut changed = fixture.snapshot.clone();
    changed.revision += 1;
    assert!(prepare_result(changed, &fixture.record, &fixture.result, &AtomicBool::new(false)).is_err());
    let mut changed = fixture.snapshot.clone();
    changed.project_id = "other-project".into();
    assert!(prepare_result(changed, &fixture.record, &fixture.result, &AtomicBool::new(false)).is_err());
    let mut changed = fixture.snapshot.clone();
    changed.name = "same revision, changed content".into();
    assert!(prepare_result(changed, &fixture.record, &fixture.result, &AtomicBool::new(false)).is_err());
    let mut record = fixture.record.clone();
    record.payload.asset.name = "forged asset record".into();
    assert!(prepare_result(fixture.snapshot.clone(), &record, &fixture.result, &AtomicBool::new(false)).is_err());
}

#[test]
fn incorporation_preserves_human_decisions_preview_and_idempotent_receipts() {
    let fixture = Fixture::new();
    let first = fixture.prepare().unwrap();
    let retry = fixture.prepare().unwrap();
    assert_eq!(first.preview().layer(&fixture.manual.layer_id).unwrap(), &fixture.manual);
    assert_eq!(first.preview().masters.len(), 1);
    assert!(first.preview().layers.iter().any(|layer| layer.kind == LayerKind::Transcript && layer.locked));
    let mut session = ProjectSession::new(fixture.snapshot.clone());
    let applied = session.commit_prepared(first).unwrap();
    let replay = session.commit_prepared(retry).unwrap();
    assert!(replay.replayed);
    assert_eq!(applied.new_revision, replay.new_revision);
    assert_eq!(session.revision(), 1);
    assert_eq!(session.pending_journal().len(), 1);
    assert_eq!(session.project().layer(&fixture.manual.layer_id).unwrap(), &fixture.manual);
    assert!(session.command_receipt(&format!("analysis-{}", fixture.record.id)).unwrap().is_some());
}

#[test]
fn prepared_analysis_cannot_commit_after_local_human_edit() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare().unwrap();
    let mut session = ProjectSession::new(fixture.snapshot);
    session.execute(CommandEnvelope::human(ProjectCommand::RenameProject { name: "concurrent human edit".into() })).unwrap();
    let before = session.project().clone();
    assert!(session.commit_prepared(prepared).is_err());
    assert_eq!(session.project(), &before);
    assert!(session.project().masters.is_empty());
    assert_eq!(session.pending_journal().len(), 1);
}

#[test]
fn undeclared_editorial_sibling_never_becomes_part_of_inference_proposal() {
    let fixture = Fixture::new();
    let mut forged = SemanticLayer::new(fixture.record.payload.asset.id.clone(), LayerKind::User, "Unreceipted human acceptance");
    forged.layer_id = "unreceipted-editorial".into();
    let mut item = SemanticItem::new(TimeRange::new(Ticks::from_seconds(5), Ticks::from_seconds(6)), "must never be imported");
    item.state = ItemState::Accepted;
    forged.items.push(item);
    let raw = tv2_v1compat::layers::layer_to_v1(&forged, &fixture.record.payload.asset.fingerprint);
    fs::create_dir(fixture.job_root().join("layers")).unwrap();
    fs::write(fixture.job_root().join("layers/unreceipted-editorial.json"), serde_json::to_vec(&raw).unwrap()).unwrap();
    // Either reject the extra file or consume only declared artifacts. In
    // particular a well-formed, unsigned sibling must not create human review.
    if let Ok(prepared) = fixture.prepare() {
        assert!(prepared.preview().layer(&forged.layer_id).is_none());
        assert_eq!(prepared.preview().layer(&fixture.manual.layer_id).unwrap(), &fixture.manual);
    }
}

#[test]
fn analysis_evidence_history_receipt_and_save_as_survive_job_folder_move() {
    let fixture = Fixture::new();
    let mut session = ProjectSession::new(fixture.snapshot.clone());
    session.commit_prepared(fixture.prepare().unwrap()).unwrap();
    let digest = session.project().masters[0].source_digest.clone();
    let store = ProjectStore::at(fixture._temp.path().join("saved.transcriptor"));
    let mut project = session.project().clone();
    let mut history = session.history_snapshot();
    store.materialize_source_bundles(&mut project, &mut history).unwrap();
    store.save_checkpoint(&project, session.pending_journal(), Some(&history)).unwrap();
    let mut reopened = ProjectStore::at(&store.root).load_session().unwrap();
    assert_eq!(reopened.project().masters[0].source_digest, digest);
    assert_eq!(reopened.project().layer(&fixture.manual.layer_id).unwrap(), &fixture.manual);
    // The desktop Save As worker resolves the source checkpoint's @project
    // locators before materializing them under a different store root.
    reopened.resolve_paths(|path| store.resolve_path(path).to_string_lossy().into());
    let mut project = reopened.project().clone();
    let mut history = reopened.history_snapshot();
    let destination = ProjectStore::at(fixture._temp.path().join("save-as.transcriptor"));
    destination.copy_audit_from(&store, &session.project().project_id, session.revision()).unwrap();
    destination.materialize_source_bundles(&mut project, &mut history).unwrap();
    destination.save_checkpoint(&project, &[], Some(&history)).unwrap();
    fs::rename(fixture.job_root(), fixture.record.payload.runtime.work_root.join("moved-owned-job")).unwrap();
    let mut copied = ProjectStore::at(&destination.root).load_session().unwrap();
    assert_eq!(copied.project().masters[0].source_digest, digest);
    assert_eq!(copied.project().layer(&fixture.manual.layer_id).unwrap(), &fixture.manual);
    assert!(copied.command_receipt(&format!("analysis-{}", fixture.record.id)).unwrap().is_some());
    copied.undo(Actor::Human).unwrap();
    copied.redo(Actor::Human).unwrap();
    assert_eq!(copied.project().masters[0].document["tracks"]["A"]["words"][0]["word_id"], "word-1");
}

#[test]
fn cancellation_prevents_result_validation_and_preparation() {
    let fixture = Fixture::new();
    let cancelled = AtomicBool::new(true);
    assert_eq!(validate_result(&fixture.record, &fixture.result, &cancelled).unwrap_err().code, ErrorCode::Cancelled);
    let error = match prepare_result(fixture.snapshot, &fixture.record, &fixture.result, &cancelled) {
        Ok(_) => panic!("cancelled result must not prepare a write"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::Cancelled);
}

#[test]
fn explicit_rebase_preserves_new_human_edit_and_requires_current_source_identity() {
    let fixture = Fixture::new();
    let mut session = ProjectSession::new(fixture.snapshot.clone());
    session
        .execute(CommandEnvelope::human(ProjectCommand::Batch {
            label: "human edited during inference".into(),
            commands: vec![
                ProjectCommand::RenameProject { name: "human edited during inference".into() },
                ProjectCommand::SetItemProps {
                    layer_id: fixture.manual.layer_id.clone(),
                    item_id: "human-accepted".into(),
                    label: Some("new human editorial choice".into()),
                    comment: Some("preserve during explicit rebase".into()),
                    ranges: None,
                },
            ],
        }))
        .unwrap();
    let human = session.project().layer(&fixture.manual.layer_id).unwrap().clone();
    assert!(prepare_result(session.project().clone(), &fixture.record, &fixture.result, &AtomicBool::new(false)).is_err());
    let rebased = prepare_rebased_result(session.project().clone(), &fixture.record, &fixture.result, &AtomicBool::new(false)).unwrap();
    assert_eq!(rebased.preview().name, "human edited during inference");
    assert_eq!(rebased.preview().layer(&fixture.manual.layer_id).unwrap(), &human);
    session.commit_prepared(rebased).unwrap();
    assert_eq!(session.revision(), 2);
    session.undo(Actor::Human).unwrap();
    assert_eq!(session.project().name, "human edited during inference");
    assert_eq!(session.project().layer(&fixture.manual.layer_id).unwrap(), &human);
    assert!(session.project().masters.is_empty());
    let mut other = session.project().clone();
    other.project_id = "different project".into();
    assert!(prepare_rebased_result(other, &fixture.record, &fixture.result, &AtomicBool::new(false)).is_err());
    fs::write(&fixture.record.payload.source_path, b"source changed after inference").unwrap();
    assert!(prepare_rebased_result(session.project().clone(), &fixture.record, &fixture.result, &AtomicBool::new(false)).is_err());
}

#[test]
fn fresh_analysis_cannot_silently_replace_existing_immutable_master() {
    let fixture = Fixture::new();
    let mut session = ProjectSession::new(fixture.snapshot.clone());
    session.commit_prepared(fixture.prepare().unwrap()).unwrap();
    let snapshot = session.project().clone();
    let mut payload = fixture.record.payload.clone();
    payload.project_digest = session.digest().unwrap();
    let record = jobs::enqueue(&payload.runtime.work_root.join("jobs"), snapshot.project_id.clone(), snapshot.revision, payload).unwrap();
    let root = record.payload.runtime.work_root.join(&record.id);
    fs::create_dir(&root).unwrap();
    let mut master: Value = serde_json::from_slice(&fs::read(fixture.job_root().join(&fixture.result.master_path)).unwrap()).unwrap();
    master["tracks"]["A"]["words"][0]["text"] = json!("another inference result");
    fs::write(root.join(&fixture.result.master_path), serde_json::to_vec(&master).unwrap()).unwrap();
    let mut result = fixture.result.clone();
    result.job_id = record.id.clone();
    result.revision = record.revision;
    result.project_digest = record.payload.project_digest.clone();
    result.artifacts.insert(result.master_path.clone(), sha256_file(&root.join(&result.master_path), &AtomicBool::new(false)).unwrap());
    validate_result(&record, &result, &AtomicBool::new(false)).unwrap();
    assert!(prepare_result(snapshot.clone(), &record, &result, &AtomicBool::new(false)).is_err());
    assert!(prepare_rebased_result(snapshot.clone(), &record, &result, &AtomicBool::new(false)).is_err());
    assert_eq!(session.project(), &snapshot);
}

#[test]
fn receipted_master_still_requires_original_media_fingerprint_and_duration() {
    let fixture = Fixture::new();
    let path = fixture.job_root().join(&fixture.result.master_path);
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for wrong_duration in [false, true] {
        let mut master = original.clone();
        if wrong_duration {
            master["media"]["duration"] = json!(9.0);
        } else {
            master["media"]["fingerprint"]["hash_muestreado"] = json!("another media");
        }
        fs::write(&path, serde_json::to_vec(&master).unwrap()).unwrap();
        let mut result = fixture.result.clone();
        result.artifacts.insert(result.master_path.clone(), sha256_file(&path, &AtomicBool::new(false)).unwrap());
        // A valid artifact SHA proves bytes, not the editorial identity of the
        // analysis. Both must pass before a project mutation can be prepared.
        validate_result(&fixture.record, &result, &AtomicBool::new(false)).unwrap();
        assert!(prepare_result(fixture.snapshot.clone(), &fixture.record, &result, &AtomicBool::new(false)).is_err());
    }
}
