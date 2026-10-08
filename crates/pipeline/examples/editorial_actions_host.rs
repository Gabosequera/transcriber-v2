//! One real local LLM action over a V2-owned, explicitly invented ASR fixture.
//! Requires the external memory-limited supervisor handshake before any job.
//! This exercises technical transport/commands, not human editorial quality.
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};
use tv2_application::{Actor, ProjectSession, ProjectStore, jobs};
use tv2_domain::Project;
use tv2_pipeline::editorial::{self, EditorialParameters, EditorialPayload, EditorialRuntime};
use tv2_v1compat::contracts::ReviewKind;

type HostResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Compare editorial content independently of monotonic revisions and relocated
/// project-owned bundle paths. Master documents and their identity remain exact.
fn editorial_content(project: &Project) -> Value {
    let masters = project
        .masters
        .iter()
        .map(|master| {
            json!({
                "asset_id":master.asset_id,"source_digest":master.source_digest,
                "document_sha256":master.document.full_digest(),
            })
        })
        .collect::<Vec<_>>();
    json!({"layers":project.layers,"layer_order":project.layer_order,"sequences":project.sequences,
        "active_sequence":project.active_sequence,"masters":masters})
}

fn proposed_elements(proposal: &Value, kind: ReviewKind) -> usize {
    match kind {
        ReviewKind::Layers => proposal
            .get("layer")
            .into_iter()
            .chain(proposal.get("layers").and_then(Value::as_array).into_iter().flatten())
            .map(|layer| layer["items"].as_array().map_or(0, Vec::len))
            .sum(),
        ReviewKind::Trims => proposal["cuts"].as_array().map_or(0, Vec::len),
        ReviewKind::Montage => proposal["clips"].as_array().map_or(0, Vec::len),
        ReviewKind::Topics => 0, // Not an accepted CLI action in this example.
    }
}

fn run_action(repo: &Path, work: &Path, fixture: &Path, kind: ReviewKind) -> HostResult<Value> {
    let cancel = Arc::new(AtomicBool::new(false));
    let fixture_store = ProjectStore::at(fixture.to_owned());
    let source_project_sha = tv2_pipeline::sha256_file(&fixture_store.project_path(), &cancel)?;
    let project = fixture_store.load_session()?.project().clone();
    let asset = project.masters.first().ok_or("Synthetic fixture has no master")?.asset_id.clone();
    if kind == ReviewKind::Montage && project.asset(&asset).ok_or("Fixture master asset absent")?.probe.video.is_none() {
        return Err("Montage requires a real V2-owned video fixture; the SAPI WAV asset cannot exercise this V1 contract".into());
    }
    let original = project.clone();
    let before_content = editorial_content(&original);
    // Each invocation loads the original fixture afresh; no earlier action's
    // accepted copy is adopted as the next action's snapshot.
    let mut session = ProjectSession::new(project);
    let runtime = EditorialRuntime {
        python: repo.join(".local/e5-editorial-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/editorial_worker.py"),
        model: repo.join(".local/models/qwen2.5-1.5b-instruct"),
        model_manifest: repo.join(".local/models/qwen2.5-1.5b-instruct/model-manifest.json"),
        lock: repo.join("workers/python/requirements-editorial.lock"),
        work_root: work.to_owned(),
    };
    let parameters = EditorialParameters { temperature: 0.0, max_tokens: 1024, timeout_seconds: 600, ..Default::default() };
    let request_id = format!("request-{}", tv2_domain::ids::random_hex12());
    let input = editorial::prepare_input(session.project(), &asset, &request_id, kind, None, parameters)?;
    // Keep the canonical request, including content/ai trims or montage guidance.
    // Never override a bound request to force a model response to validate.
    let request = input.request.clone();
    let start = Instant::now();
    let queued = editorial::enqueue(input, runtime, &cancel)?;
    let result = editorial::run(&queued, false, cancel.clone(), |event| println!("{event}"))?;
    let record = jobs::discover::<EditorialPayload>(&work.join("jobs"))?
        .into_iter()
        .find(|record| record.id == queued.id)
        .ok_or("Missing durable editorial job")?;
    assert_eq!(record.state, jobs::JobState::Succeeded);
    assert_eq!(record.result.as_ref(), Some(&serde_json::to_value(&result)?));
    editorial::validate_result(&record, &result, &cancel)?;
    let proposal = editorial::read_verified_proposal(&record, &result, &cancel)?;
    let prepared = editorial::prepare_result(session.project().clone(), &record, &result, &cancel)?;
    assert_eq!(session.project(), &original);
    let count = proposed_elements(&proposal, kind);
    let receipt = prepared.command.map(|command| session.commit_prepared(command)).transpose()?;
    let after_content = editorial_content(session.project());
    let content_changed = after_content != before_content;
    let sequence_source_seconds =
        session.project().active().map(|sequence| sequence.clips.iter().map(|clip| clip.duration().as_seconds_f64()).sum::<f64>());
    let sequence_extent_seconds = session.project().active().map(|sequence| sequence.extent().as_seconds_f64());
    let declared_repeats = proposal["clips"].as_array().map_or(0, |clips| clips.iter().filter(|clip| clip["repeat"] == true).count());
    // V1 keep can reproduce unchanged protected clips in a new sequence. The
    // global clip-ID diff may then be zero; verify the actual new destination.
    let montage_sequence_with_content = kind == ReviewKind::Montage
        && session.project().active().is_some_and(|sequence| original.sequence(&sequence.id).is_none() && !sequence.clips.is_empty());
    let nonempty_element_effect = count > 0
        && receipt.as_ref().is_some_and(|receipt| {
            let diff = &receipt.diff;
            diff.items_added > 0
                || diff.items_removed > 0
                || diff.items_changed > 0
                || diff.clips_added > 0
                || diff.clips_removed > 0
                || diff.clips_changed > 0
                || montage_sequence_with_content
        });
    // An empty valid cuts/items list can still commit layer/header metadata.
    // Report that receipt separately; never claim it applied nonempty content.
    if count == 0 {
        assert!(!nonempty_element_effect);
    } else {
        assert!(nonempty_element_effect, "Nonempty proposal produced no editorial elements; do not report successful application");
    }
    assert_eq!(session.project().masters, original.masters);

    let review_root = work.join("review");
    tv2_application::documents::create(&review_root)?;
    tv2_application::documents::publish(&review_root, &prepared.review.documents)?;
    let store = ProjectStore::at(work.join(format!("accepted-{}.transcriptor", kind.stem())));
    let mut saved = session.project().clone();
    let mut history = session.history_snapshot();
    store.materialize_source_bundles(&mut saved, &mut history)?;
    store.save_checkpoint(&saved, session.pending_journal(), Some(&history))?;
    let mut reopened = store.load_session()?;
    assert_eq!(editorial_content(reopened.project()), after_content);
    let undo_redo = receipt.is_some();
    if undo_redo {
        assert!(reopened.can_undo());
        reopened.undo(Actor::Human)?;
        assert_eq!(editorial_content(reopened.project()), before_content);
        reopened.redo(Actor::Human)?;
        assert_eq!(editorial_content(reopened.project()), after_content);
    } else {
        assert!(!reopened.can_undo());
        assert_eq!(after_content, before_content);
    }
    assert_eq!(tv2_pipeline::sha256_file(&fixture_store.project_path(), &cancel)?, source_project_sha);
    editorial::validate_result(&record, &result, &cancel)?;
    Ok(json!({"status":"PASS","kind":kind,"asr_fixture_only":true,"native_llm_inference":true,
        "gui_exercised":false,"human_editorial_quality_accepted":false,
        "supervisor_handshake":true,"required_external_memory_limit_bytes":8u64*1024*1024*1024,
        "duration_seconds":start.elapsed().as_secs_f64(),"fixture":fixture,"fixture_project_sha256":source_project_sha,
        "fixture_project_unchanged":true,"project_snapshot_digest":record.payload.input.project_digest,
        "job_id":record.id,"job_state":record.state,"request":request,"result":result,"proposal":proposal,
        "proposed_elements":count,"empty_valid_proposal":count==0,"prepare_no_project_mutation":true,
        "common_command_committed":receipt.is_some(),"command_receipt":receipt,"editorial_content_changed":content_changed,
        "nonempty_editorial_element_effect":nonempty_element_effect,"save_reopen_verified":true,
        "active_sequence_source_clip_seconds_sum":sequence_source_seconds,"active_sequence_extent_seconds":sequence_extent_seconds,
        "declared_source_repeats":declared_repeats,
        "new_montage_sequence_with_content":montage_sequence_with_content,
        "undo_redo_exercised":undo_redo,"warnings":prepared.review.warnings,"review_root":review_root,
        "saved_fixture":store.root}))
}

fn main() -> HostResult<()> {
    let _ = tracing_subscriber::fmt().with_writer(std::io::stderr).try_init();
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("editorial_actions_host REPO_ROOT FRESH_WORK_ROOT SYNTHETIC_PROJECT_FOLDER layers|trims|montage".into());
    }
    let repo = PathBuf::from(&args[0]).canonicalize()?;
    let work = PathBuf::from(&args[1]);
    let fixture = PathBuf::from(&args[2]).canonicalize()?;
    if work.exists() || !work.is_absolute() || !work.parent().ok_or("Missing work parent")?.canonicalize()?.starts_with(&repo) {
        return Err("Use a new absolute work root inside V2".into());
    }
    if !fixture.is_dir() || !fixture.starts_with(&repo) {
        return Err("Use only a V2-owned explicitly synthetic project folder".into());
    }
    let kind = match args[3].to_str() {
        Some("layers") => ReviewKind::Layers,
        Some("trims") => ReviewKind::Trims,
        Some("montage") => ReviewKind::Montage,
        _ => return Err("Only layers, trims or montage; each action needs its own fresh root".into()),
    };
    // Same protocol as editorial_host: the launching harness must attach this
    // process to its owned 8 GiB JobObject BEFORE writing the line below.
    let mut permission = String::new();
    io::stdin().read_line(&mut permission)?;
    if permission.trim() != "owned-job-ready" {
        return Err("Missing owned-process supervisor handshake".into());
    }
    let report = match run_action(&repo, &work, &fixture, kind) {
        Ok(report) => report,
        Err(error) => {
            // Keep durable jobs/raw untouched on failure. No accepted-copy claim.
            let failure = json!({"status":"FAIL","kind":kind,"fixture":fixture,"asr_fixture_only":true,
                "gui_exercised":false,"human_editorial_quality_accepted":false,"error":error.to_string()});
            if work.is_dir() {
                fs::write(work.join("failure.json"), serde_json::to_vec_pretty(&failure)?)?;
            }
            eprintln!("{failure}");
            return Err(error);
        }
    };
    fs::write(work.join("result.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        json!({"status":"PASS","kind":kind,"report":work.join("result.json"),
        "proposed_elements":report["proposed_elements"],"empty_valid_proposal":report["empty_valid_proposal"],
        "nonempty_editorial_element_effect":report["nonempty_editorial_element_effect"]})
    );
    Ok(())
}
