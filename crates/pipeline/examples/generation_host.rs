//! Real stdlib generation over already verified stage jobs. No ASR claim.
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use tv2_application::{Actor, ProjectSession, ProjectStore, jobs};
use tv2_domain::Project;
use tv2_pipeline::generation::{self, GenerationPayload};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("generation_host REPO_ROOT PAYLOAD_JSON PROJECT_JSON".into());
    }
    let repo = args[0].canonicalize()?;
    let payload: GenerationPayload = serde_json::from_slice(&fs::read(&args[1])?)?;
    let work = payload.runtime.work_root.clone();
    if work.exists() || !work.is_absolute() || !work.parent().ok_or("Missing work parent")?.canonicalize()?.starts_with(&repo) {
        return Err("Use a new absolute work directory inside V2".into());
    }
    let project: Project = serde_json::from_slice(&fs::read(&args[2])?)?;
    let before = project.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let queued = generation::enqueue(payload, &cancel)?;
    let start = std::time::Instant::now();
    let result = generation::run(&queued, false, cancel.clone(), |event| println!("{}", event))?;
    let record =
        jobs::discover::<GenerationPayload>(&work.join("jobs"))?.into_iter().find(|record| record.id == queued.id).ok_or("Missing durable record")?;
    assert_eq!(record.state, jobs::JobState::Succeeded);
    let prepared = generation::prepare_result(project.clone(), &record, &result, &cancel, false)?;
    assert_eq!(project, before);
    let mut session = ProjectSession::new(project);
    session.commit_prepared(prepared)?;
    let store = ProjectStore::at(work.join("accepted-fixture.transcriptor"));
    let mut saved = session.project().clone();
    let mut history = session.history_snapshot();
    store.materialize_source_bundles(&mut saved, &mut history)?;
    store.save_checkpoint(&saved, session.pending_journal(), Some(&history))?;
    let mut reopened = store.load_session()?;
    assert_eq!(reopened.project().masters.len(), 1);
    let digest = reopened.project().masters[0].source_digest.clone();
    reopened.undo(Actor::Human)?;
    assert!(reopened.project().masters.is_empty());
    reopened.redo(Actor::Human)?;
    assert_eq!(reopened.project().masters[0].source_digest, digest);

    let output = work.join(&record.id).join(&result.master_path);
    let original = fs::read(&output)?;
    let mut altered: serde_json::Value = serde_json::from_slice(&original)?;
    let track = altered["tracks"].as_object_mut().ok_or("Missing tracks")?.values_mut().next().ok_or("No track")?;
    track["words"][0]["text"] = json!("forged despite recalculated receipt SHA");
    fs::write(&output, serde_json::to_vec(&altered)?)?;
    let mut forged = result.clone();
    forged.artifacts.insert(result.master_path.clone(), tv2_pipeline::sha256_file(&output, &cancel)?);
    let rejected = generation::validate_result(&record, &forged, &cancel).is_err();
    fs::write(&output, &original)?;
    assert!(rejected);
    let mut derived_rejections = Vec::new();
    for (pointer, replacement) in [
        ("/tracks/a0/words/0/intensity_z", json!(123.456)),
        ("/tracks/a0/heuristics/pauses", json!([])),
        ("/conversation/clean_utterance_ids", json!(["forged-utterance"])),
        ("/analysis/completed_steps", json!(["forged-module"])),
    ] {
        let mut altered: serde_json::Value = serde_json::from_slice(&original)?;
        let field = altered.pointer_mut(pointer).ok_or("Missing derived test field")?;
        if *field == replacement {
            return Err(format!("Attack would not change {pointer}").into());
        }
        *field = replacement;
        fs::write(&output, serde_json::to_vec(&altered)?)?;
        let mut forged = result.clone();
        forged.artifacts.insert(result.master_path.clone(), tv2_pipeline::sha256_file(&output, &cancel)?);
        let rejected = generation::validate_result(&record, &forged, &cancel).is_err();
        fs::write(&output, &original)?;
        assert!(rejected, "Derived forgery accepted: {pointer}");
        derived_rejections.push(pointer);
    }
    generation::validate_result(&record, &result, &cancel)?;
    let report = json!({"asr_fixture_only":true,"gui_exercised":false,"job_id":record.id,"job_state":record.state,
        "duration_seconds":start.elapsed().as_secs_f64(),"generation_result":result,"prepare_no_mutation":true,
        "common_command_commit":true,"save_reopen_undo_redo":true,"forged_text_with_recomputed_sha_rejected":true,
        "derived_forgery_with_recomputed_sha_rejected":derived_rejections,
        "project_fixture":args[2],"saved_fixture":store.root});
    fs::write(work.join("result.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", report);
    Ok(())
}
