//! Pure orchestration contracts: inert resource bytes and durable generic jobs.
//! No Python process, fake inference protocol, ASR, or native library is executed.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixture() -> (tempfile::TempDir, JobRecord<WorkflowPayload>) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let files = root.join("resources");
    fs::create_dir_all(&files).unwrap();
    for name in ["source.wav", "python.bin", "worker.py", "ffmpeg.bin", "requirements-transcription.lock", "finalize_worker.py", "finalize.py"] {
        fs::write(files.join(name), format!("INERT CONTRACT FIXTURE {name}")).unwrap();
    }
    let model = files.join("model");
    fs::create_dir_all(&model).unwrap();
    for name in ["config.json", "model.bin", "tokenizer.json", "vocabulary.txt"] {
        fs::write(model.join(name), name).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let source = files.join("source.wav");
    let asset = serde_json::from_value(json!({"id":"asset-fixture","kind":"audio","name":"invented.wav","path":source,
        "probe":{"container":"wav","duration":705600000,"start_time":0,"audio":[{"stream_index":0,"audio_index":0,
        "codec":"pcm_s16le","channels":1,"channel_layout":"","sample_rate":16000,"start_time":0,"duration":705600000,"title":""}],"size":100},
        "fingerprint":{"size":100,"mtime_ns":0,"hash_muestreado":"a".repeat(64),"inventario_sha256":"b".repeat(64)},"missing":false}))
    .unwrap();
    let asr_payload = AnalysisPayload {
        project_digest: "a".repeat(64),
        asset,
        source_path: source.clone(),
        source_sha256: crate::sha256_file(&source, &cancel).unwrap(),
        model_digest: crate::model_digest(&model, &cancel).unwrap(),
        worker_sha256: crate::sha256_file(&files.join("worker.py"), &cancel).unwrap(),
        runtime: crate::Runtime { python: files.join("python.bin"), worker: files.join("worker.py"), model, work_root: root.join("analysis") },
        parameters: crate::Parameters::default(),
        ffmpeg_path: files.join("ffmpeg.bin"),
    };
    let asr = jobs::enqueue(&asr_payload.runtime.work_root.join("jobs"), "proj-fixture".into(), 1, asr_payload).unwrap();
    let plan = WorkflowPlan {
        alignment: None,
        arousal: None,
        laughter: None,
        generation: generation::GenerationRuntime {
            python: files.join("python.bin"),
            worker: files.join("finalize_worker.py"),
            assembler: files.join("finalize.py"),
            derivation: files.join("worker.py"),
            work_root: root.join("analysis/generation"),
        },
    };
    let record = enqueue(asr, plan, root.join("analysis/workflows"), &cancel).unwrap();
    (temp, record)
}
fn generic_child(record: &JobRecord<WorkflowPayload>) -> JobRecord<Value> {
    jobs::enqueue(
        &record.payload.plan.generation.work_root.join("jobs"),
        record.project_id.clone(),
        record.revision,
        json!({"contract":"durable-job-only"}),
    )
    .unwrap()
}
fn generic_checkpoint(record: &JobRecord<WorkflowPayload>, child: &JobRecord<Value>) -> Checkpoint {
    let mut checkpoint = new_checkpoint(record).unwrap();
    checkpoint.children.insert("generation".into(), ChildRef { id: child.id.clone(), payload_digest: child.payload_digest.clone() });
    save_checkpoint(record, &checkpoint).unwrap();
    checkpoint
}
fn generic_step(
    record: &JobRecord<WorkflowPayload>,
    checkpoint: Checkpoint,
    resume: bool,
    cancel: Arc<AtomicBool>,
    progress: impl Fn(Value),
    executions: &AtomicUsize,
) -> DomainResult<generation::Stage<Value, Value>> {
    let root = record.payload.plan.generation.work_root.join("jobs");
    let mut runner = Runner { record, resume, cancel, progress: &progress, checkpoint, finished: 1, total: 2 };
    runner.step(
        "generation",
        &root,
        || Ok(generic_child(record)),
        |_| Ok(()),
        |child, resume, _, _| {
            executions.fetch_add(1, Ordering::SeqCst);
            let lease = jobs::claim::<Value>(&root, &child.id, &child.payload_digest, resume)?;
            let result = json!({"contract":"no-inference"});
            lease.finish(JobState::Succeeded, Some(result.clone()), None)?;
            Ok(result)
        },
        |_, _, _| Ok(()),
    )
}

#[test]
fn checkpoint_reads_do_not_create_missing_directories() {
    let (_temp, record) = fixture();
    let job = record.payload.work_root.join(&record.id);
    assert!(!job.exists());
    assert!(load_checkpoint(&record, true).is_err());
    assert!(load_checkpoint(&record, false).is_ok());
    assert!(!job.exists());
    save_checkpoint(&record, &new_checkpoint(&record).unwrap()).unwrap();
    assert!(load_checkpoint(&record, true).is_ok());
}
#[test]
fn validation_missing_checkpoint_is_readonly() {
    let (_temp, mut record) = fixture();
    let asr = &record.payload.asr;
    let result = AnalysisResult {
        job_id: asr.id.clone(),
        project_id: asr.project_id.clone(),
        revision: asr.revision,
        project_digest: asr.payload.project_digest.clone(),
        source_sha256: asr.payload.source_sha256.clone(),
        model_digest: asr.payload.model_digest.clone(),
        master_path: "editorial/fixture.json".into(),
        artifacts: BTreeMap::new(),
    };
    let payload =
        generation_payload(&record.payload, generation::Stage { record: asr.clone(), result }, BTreeMap::new(), BTreeMap::new(), BTreeMap::new())
            .unwrap();
    let final_record = jobs::enqueue(&payload.runtime.work_root.join("jobs"), asr.project_id.clone(), asr.revision, payload).unwrap();
    let final_result = generation::GenerationResult {
        job_id: final_record.id.clone(),
        project_id: asr.project_id.clone(),
        revision: asr.revision,
        project_digest: asr.payload.project_digest.clone(),
        asset_id: asr.payload.asset.id.to_string(),
        source_sha256: asr.payload.source_sha256.clone(),
        input_digest: "a".repeat(64),
        master_path: generation::OUTPUT.into(),
        artifacts: BTreeMap::new(),
    };
    let before = fs::read_dir(&record.payload.work_root).unwrap().count();
    let result = WorkflowResult { generation: final_record, result: final_result };
    record.state = JobState::Succeeded;
    record.result = Some(serde_json::to_value(&result).unwrap());
    let error = validate_result(&record, &result, &AtomicBool::new(false)).unwrap_err();
    assert!(error.message.contains("Falta checkpoint"));
    assert_eq!(before, fs::read_dir(&record.payload.work_root).unwrap().count());
    assert!(!record.payload.work_root.join(&record.id).exists());
}
#[test]
fn checkpoint_rejects_work_file_before_writing() {
    let (_temp, record) = fixture();
    let job = record.payload.work_root.join(&record.id);
    fs::create_dir_all(&job).unwrap();
    fs::write(job.join(".work"), b"keep").unwrap();
    assert!(save_checkpoint(&record, &new_checkpoint(&record).unwrap()).is_err());
    assert_eq!(fs::read(job.join(".work")).unwrap(), b"keep");
}
#[test]
fn checkpoint_header_and_parent_tampering_rejected() {
    let (_temp, record) = fixture();
    for field in ["schema", "workflow_id", "payload_digest", "plan_digest", "resources_digest"] {
        let mut value = serde_json::to_value(new_checkpoint(&record).unwrap()).unwrap();
        value[field] = json!("changed");
        assert!(checkpoint_valid(&record, &serde_json::from_value(value).unwrap()).is_err());
    }
    let mut checkpoint = new_checkpoint(&record).unwrap();
    checkpoint.children.get_mut("asr").unwrap().id = "job-000000000000".into();
    assert!(checkpoint_valid(&record, &checkpoint).is_err());
    let mut checkpoint = new_checkpoint(&record).unwrap();
    checkpoint.children.insert("laughter/a0".into(), ChildRef { id: "job-000000000000".into(), payload_digest: "a".repeat(64) });
    assert!(checkpoint_valid(&record, &checkpoint).is_err());
}
#[test]
fn resource_change_rejected_before_workflow_claim() {
    let (_temp, record) = fixture();
    fs::write(&record.payload.plan.generation.assembler, b"changed assembler").unwrap();
    assert!(run(&record, false, Arc::new(AtomicBool::new(false)), |_| {}).is_err());
    let stored: JobRecord<WorkflowPayload> =
        serde_json::from_slice(&fs::read(record.payload.work_root.join("jobs").join(format!("{}.json", record.id))).unwrap()).unwrap();
    assert_eq!(stored.state, JobState::Queued);
    assert!(stored.attempts.is_empty());
}
#[test]
fn cancellation_keeps_checkpointed_child_queued_before_execute() {
    let (_temp, record) = fixture();
    let cancel = Arc::new(AtomicBool::new(false));
    let count = AtomicUsize::new(0);
    assert!(
        generic_step(&record, new_checkpoint(&record).unwrap(), false, cancel.clone(), |_| cancel.store(true, Ordering::SeqCst), &count).is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let checkpoint = load_checkpoint(&record, true).unwrap();
    let child: JobRecord<Value> = load_child(&record.payload.plan.generation.work_root.join("jobs"), &checkpoint.children["generation"]).unwrap();
    assert_eq!(child.state, JobState::Queued);
    assert!(child.attempts.is_empty());
}
#[test]
fn failed_cancelled_interrupted_children_require_explicit_resume() {
    for state in [JobState::Failed, JobState::Cancelled, JobState::Interrupted] {
        let (_temp, record) = fixture();
        let child = generic_child(&record);
        let root = record.payload.plan.generation.work_root.join("jobs");
        let lease = jobs::claim::<Value>(&root, &child.id, &child.payload_digest, false).unwrap();
        if state == JobState::Interrupted {
            drop(lease);
            assert_eq!(jobs::discover::<Value>(&root).unwrap()[0].state, JobState::Interrupted);
        } else {
            lease.finish(state, None, Some("contract failure".into())).unwrap();
        }
        let checkpoint = generic_checkpoint(&record, &child);
        let count = AtomicUsize::new(0);
        assert!(generic_step(&record, checkpoint.clone(), false, Arc::new(AtomicBool::new(false)), |_| {}, &count).is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let stage = generic_step(&record, checkpoint, true, Arc::new(AtomicBool::new(false)), |_| {}, &count).unwrap();
        assert_eq!(stage.record.state, JobState::Succeeded);
        assert_eq!(stage.record.attempts.len(), 2);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn succeeded_child_is_verified_and_reused_without_execute() {
    let (_temp, record) = fixture();
    let child = generic_child(&record);
    jobs::claim::<Value>(&record.payload.plan.generation.work_root.join("jobs"), &child.id, &child.payload_digest, false)
        .unwrap()
        .finish(JobState::Succeeded, Some(json!({"contract":"no-inference"})), None)
        .unwrap();
    let checkpoint = generic_checkpoint(&record, &child);
    let count = AtomicUsize::new(0);
    let stage = generic_step(&record, checkpoint, false, Arc::new(AtomicBool::new(false)), |_| {}, &count).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(stage.record.attempts.len(), 1);
}
#[test]
fn missing_checkpointed_child_is_never_recreated() {
    let (_temp, record) = fixture();
    let child = generic_child(&record);
    let checkpoint = generic_checkpoint(&record, &child);
    fs::remove_file(record.payload.plan.generation.work_root.join("jobs").join(format!("{}.json", child.id))).unwrap();
    let count = AtomicUsize::new(0);
    assert!(generic_step(&record, checkpoint, true, Arc::new(AtomicBool::new(false)), |_| {}, &count).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
}
#[test]
fn plan_rejects_equal_roots_duplicate_tracks_and_arousal_without_alignment() {
    let (_temp, record) = fixture();
    assert!(plan_valid(&record.payload).is_ok()); // Descendant roots are supported.
    let mut payload = record.payload.clone();
    payload.plan.generation.work_root = payload.asr.payload.runtime.work_root.clone();
    assert!(plan_valid(&payload).is_err());
    let mut payload = record.payload.clone();
    payload.asr.payload.asset.probe.audio.push(payload.asr.payload.asset.probe.audio[0].clone());
    payload.asr.payload_digest = digest(&payload.asr.payload).unwrap();
    assert!(plan_valid(&payload).is_err());
    let mut payload = record.payload.clone();
    let runtime = &payload.asr.payload.runtime;
    payload.plan.arousal = Some(ArousalStep {
        runtime: arousal::ArousalRuntime {
            python: runtime.python.clone(),
            worker: runtime.worker.clone(),
            model: runtime.model.clone(),
            model_manifest: runtime.model.join("manifest.json"),
            lock: runtime.worker.with_file_name("lock.txt"),
            work_root: runtime.work_root.join("arousal"),
        },
        parameters: arousal::ArousalParameters::default(),
    });
    assert!(plan_valid(&payload).unwrap_err().message.contains("requiere MMS"));
}
#[test]
fn plan_payload_unknown_fields_and_changed_digest_rejected() {
    let (_temp, record) = fixture();
    let mut plan = serde_json::to_value(&record.payload.plan).unwrap();
    plan["unexpected"] = json!(true);
    assert!(serde_json::from_value::<WorkflowPlan>(plan).is_err());
    let mut changed = record.clone();
    changed.payload.asr.payload.parameters.language = "changed".into();
    assert!(crate::validate_record(&changed).is_err());
}

#[cfg(windows)]
#[test]
fn checkpoint_junction_is_rejected_without_touching_destination() {
    let (_temp, record) = fixture();
    let job = record.payload.work_root.join(&record.id);
    fs::create_dir_all(&job).unwrap();
    let outside = record.payload.work_root.join("owned-junction-destination");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"preserved").unwrap();
    let junction = job.join(".work");
    let status = std::process::Command::new("cmd.exe").args(["/c", "mklink", "/J"]).arg(&junction).arg(&outside).output().unwrap();
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    assert!(load_checkpoint(&record, true).is_err());
    assert!(save_checkpoint(&record, &new_checkpoint(&record).unwrap()).is_err());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"preserved");
    // Remove only the owned junction itself before TempDir cleanup.
    fs::remove_dir(&junction).unwrap();
    assert!(outside.join("sentinel").exists());
}
