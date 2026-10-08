//! Real local topics proposals over an explicitly synthetic transcript fixture.
use serde_json::json;
use std::{
    fs, io,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use tv2_application::{Actor, ProjectSession, ProjectStore, jobs};
use tv2_pipeline::editorial::{self, EditorialParameters, EditorialPayload, EditorialRuntime};
use tv2_v1compat::contracts::ReviewKind;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt().with_writer(std::io::stderr).try_init();
    let args = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("editorial_host REPO_ROOT FRESH_WORK_ROOT SYNTHETIC_PROJECT".into());
    }
    let repo = args[0].canonicalize()?;
    let work = &args[1];
    if work.exists() || !work.is_absolute() || !work.parent().ok_or("Missing work parent")?.canonicalize()?.starts_with(&repo) {
        return Err("Use a new work root inside V2".into());
    }
    if !args[2].canonicalize()?.starts_with(&repo) {
        return Err("Only V2-owned synthetic project fixtures".into());
    }
    // The supervising harness assigns a memory-limited JobObject first.
    let mut permission = String::new();
    io::stdin().read_line(&mut permission)?;
    if permission.trim() != "owned-job-ready" {
        return Err("Missing owned-process supervisor handshake".into());
    }
    let project = ProjectStore::at(args[2].clone()).load_session()?.project().clone();
    let asset = project.masters.first().ok_or("Fixture has no master")?.asset_id.clone();
    let original = project.clone();
    let runtime = EditorialRuntime {
        python: repo.join(".local/e5-editorial-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/editorial_worker.py"),
        model: repo.join(".local/models/qwen2.5-1.5b-instruct"),
        model_manifest: repo.join(".local/models/qwen2.5-1.5b-instruct/model-manifest.json"),
        lock: repo.join("workers/python/requirements-editorial.lock"),
        work_root: work.clone(),
    };
    let parameters = EditorialParameters { temperature: 0.0, max_tokens: 1024, timeout_seconds: 600, ..Default::default() };
    let request_id = format!("request-{}", tv2_domain::ids::random_hex12());
    let mut input = editorial::prepare_input(&project, &asset, &request_id, ReviewKind::Topics, None, parameters)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut session = ProjectSession::new(project);
    let start = std::time::Instant::now();
    let mut passes = Vec::new();
    for pass in 1..=2 {
        let record = editorial::enqueue(input.clone(), runtime.clone(), &cancel)?;
        let result = editorial::run(&record, false, cancel.clone(), |event| println!("{event}"))?;
        let saved =
            jobs::discover::<EditorialPayload>(&work.join("jobs"))?.into_iter().find(|job| job.id == record.id).ok_or("Missing editorial job")?;
        assert_eq!(saved.state, jobs::JobState::Succeeded);
        let prepared = editorial::prepare_result(session.project().clone(), &saved, &result, &cancel)?;
        let proposal = editorial::read_verified_proposal(&saved, &result, &cancel)?;
        if pass == 1 {
            assert!(prepared.command.is_none());
            assert_eq!(session.project(), &original);
            input = editorial::continue_topics(&saved, &result, &cancel)?;
        } else {
            session.commit_prepared(prepared.command.ok_or("Second topics pass must prepare command")?)?;
        }
        let review_root = work.join(format!("review-pass{pass}"));
        tv2_application::documents::create(&review_root)?;
        tv2_application::documents::publish(&review_root, &prepared.review.documents)?;
        passes.push(json!({"pass":pass,"job_id":saved.id,"result":result,"proposal":proposal}));
    }
    let store = ProjectStore::at(work.join("accepted-editorial.transcriptor"));
    let mut saved = session.project().clone();
    let mut history = session.history_snapshot();
    store.materialize_source_bundles(&mut saved, &mut history)?;
    store.save_checkpoint(&saved, session.pending_journal(), Some(&history))?;
    let mut reopened = store.load_session()?;
    let layers = reopened.project().layers.clone();
    reopened.undo(Actor::Human)?;
    reopened.redo(Actor::Human)?;
    assert_eq!(reopened.project().layers, layers);
    let report = json!({"asr_fixture_only":true,"native_llm_inference":true,"gui_exercised":false,
        "duration_seconds":start.elapsed().as_secs_f64(),"passes":passes,"first_pass_no_project_mutation":true,
        "common_command_commit":true,"save_reopen_undo_redo":true,"fixture":args[2],"saved_fixture":store.root});
    fs::write(work.join("result.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", json!({"status":"PASS","duration_seconds":start.elapsed().as_secs_f64(),"report":work.join("result.json")}));
    Ok(())
}
