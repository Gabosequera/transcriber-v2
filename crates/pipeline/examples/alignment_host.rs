//! Host MMS over explicitly invented parent transcript: ASR is never invoked.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tv2_application::{
    ProjectSession,
    jobs::{self, JobState},
};
use tv2_domain::{ErrorCode, Project};
use tv2_pipeline::{
    AnalysisResult, Parameters, Runtime,
    alignment::{self, AlignmentParameters, AlignmentRuntime},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if arguments.len() != 3 {
        return Err("alignment_host REPO_ROOT NEW_WORK_ROOT KNOWN_TRANSCRIPT_JSON".into());
    }
    let repo = arguments[0].clone();
    let canonical_repo = repo.canonicalize()?;
    let work = &arguments[1];
    if work.exists() || !work.is_absolute() || !work.parent().ok_or("work root without parent")?.canonicalize()?.starts_with(&canonical_repo) {
        return Err("Use new absolute work root within V2".into());
    }
    fs::create_dir_all(work)?;
    let source = repo.join(".local/e5-python-tests-20260930/fixtures/speech-a.wav");
    let tools = tv2_media::FfmpegTools::locate()?;
    let asset = tools.import(&source, source.to_string_lossy().into_owned())?;
    let session = ProjectSession::new(Project::new("Invented parent; real MMS host"));
    let before = session.project().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let asr_runtime = Runtime {
        python: repo.join(".local/e5-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/worker.py"),
        model: repo.join(".local/models/whisper-tiny"),
        work_root: work.join("invented-parent"),
    };
    let parent_queued = tv2_pipeline::enqueue(&session, asset.clone(), source.clone(), asr_runtime, Parameters::default(), &tools, &cancel)?;
    let parent_root = parent_queued.payload.runtime.work_root.join(&parent_queued.id);
    fs::create_dir_all(parent_root.join(".work/audio"))?;
    fs::create_dir_all(parent_root.join(".work/transcripts"))?;
    fs::create_dir_all(parent_root.join("editorial"))?;
    fs::copy(&source, parent_root.join(".work/audio/a0.wav"))?;
    fs::copy(&arguments[2], parent_root.join(".work/transcripts/a0.json"))?;
    let master = json!({"schema":"editorial-master/1","project":{"name":"Explicitly invented parent transcript"},
        "media":{"path":source,"duration":asset.duration().as_seconds_f64(),"t0":0.0,"fingerprint":asset.fingerprint},
        "tracks":{},"conversation":{},"chunks":[],"fixture":"not ASR; no model decoding occurred"});
    fs::write(parent_root.join("editorial/analysis.editorial.master.json"), serde_json::to_vec(&master)?)?;
    let names = [".work/audio/a0.wav", ".work/transcripts/a0.json", "editorial/analysis.editorial.master.json"];
    let mut artifacts = BTreeMap::new();
    for name in names {
        artifacts.insert(name.into(), tv2_pipeline::sha256_file(&parent_root.join(name), &cancel)?);
    }
    let parent_result = AnalysisResult {
        job_id: parent_queued.id.clone(),
        project_id: parent_queued.project_id.clone(),
        revision: parent_queued.revision,
        project_digest: parent_queued.payload.project_digest.clone(),
        source_sha256: parent_queued.payload.source_sha256.clone(),
        model_digest: parent_queued.payload.model_digest.clone(),
        master_path: "editorial/analysis.editorial.master.json".into(),
        artifacts,
    };
    let lease = jobs::claim::<tv2_pipeline::AnalysisPayload>(
        &parent_queued.payload.runtime.work_root.join("jobs"),
        &parent_queued.id,
        &parent_queued.payload_digest,
        false,
    )?;
    lease.finish(JobState::Succeeded, Some(serde_json::to_value(&parent_result)?), Some("FIXTURE: invented transcript, no ASR executed".into()))?;
    let parent = jobs::discover::<tv2_pipeline::AnalysisPayload>(&parent_queued.payload.runtime.work_root.join("jobs"))?
        .into_iter()
        .find(|job| job.id == parent_queued.id)
        .ok_or("fixture parent receipt missing")?;
    let runtime = AlignmentRuntime {
        python: repo.join(".local/e5-alignment-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/alignment_worker.py"),
        model: repo.join(".local/models/mms-fa/model.pt"),
        model_manifest: repo.join(".local/models/mms-fa/model-manifest.json"),
        lock: repo.join("workers/python/requirements-alignment.lock"),
        work_root: work.join("alignment"),
    };
    assert!(alignment::enqueue(parent.clone(), parent_result.clone(), 9, runtime.clone(), AlignmentParameters::default(), &cancel).is_err());
    let invalid = AlignmentParameters { threads: 4, ..Default::default() };
    assert!(alignment::enqueue(parent.clone(), parent_result.clone(), 0, runtime.clone(), invalid, &cancel).is_err());
    let record = alignment::enqueue(parent.clone(), parent_result.clone(), 0, runtime.clone(), AlignmentParameters::default(), &cancel)?;
    let events = std::sync::Mutex::new(Vec::new());
    let started = Instant::now();
    let result = alignment::run(&record, false, cancel.clone(), |event| events.lock().unwrap().push(event))?;
    let elapsed = started.elapsed().as_secs_f64();
    alignment::validate_result(&record, &result, &cancel)?;
    let output_path = runtime.work_root.join(&record.id).join(alignment::OUTPUT);
    let bytes = fs::read(&output_path)?;
    let output: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(output["alignment"]["aligned_words"], 23);
    assert_eq!(output["alignment"]["failed_batches"], json!([]));
    let mut stale = result.clone();
    stale.revision += 1;
    assert!(alignment::validate_result(&record, &stale, &cancel).is_err());
    let mut extra = result.clone();
    extra.artifacts.insert("sibling.json".into(), "0".repeat(64));
    assert!(alignment::validate_result(&record, &extra, &cancel).is_err());
    let mut unknown = serde_json::to_value(&result)?;
    unknown["unknown"] = json!(true);
    assert!(serde_json::from_value::<alignment::AlignmentResult>(unknown).is_err());
    let mut unknown_payload = serde_json::to_value(&record.payload)?;
    unknown_payload["unknown"] = json!(true);
    assert!(serde_json::from_value::<alignment::AlignmentPayload>(unknown_payload).is_err());
    // Tampered evidence is rehashed, so the test updates the receipt deliberately
    // to reach the independent word/metadata protection checks.
    for (name, mut altered) in [("word-id", output.clone()), ("probability", output.clone()), ("manifest-lineage", output.clone())] {
        match name {
            "word-id" => altered["words"][0]["word_id"] = json!("forged"),
            "probability" => altered["words"][0]["probability"] = json!(1.0),
            _ => altered["alignment"]["model_manifest_sha256"] = json!("0".repeat(64)),
        }
        fs::write(&output_path, serde_json::to_vec(&altered)?)?;
        let mut forged = result.clone();
        forged.artifacts.insert(alignment::OUTPUT.into(), tv2_pipeline::sha256_file(&output_path, &cancel)?);
        assert!(alignment::validate_result(&record, &forged, &cancel).is_err(), "{name} tamper accepted");
    }
    fs::write(&output_path, &bytes)?;
    alignment::validate_result(&record, &result, &cancel)?;
    let pending = alignment::enqueue(parent, parent_result, 0, runtime.clone(), AlignmentParameters::default(), &cancel)?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancelled = alignment::run(&pending, false, cancellation.clone(), |event| {
        if event["stage"] == "verify-inputs" {
            cancellation.store(true, Ordering::Release);
        }
    })
    .err()
    .ok_or("host cancellation unexpectedly succeeded")?;
    assert_eq!(cancelled.code, ErrorCode::Cancelled);
    let durable = jobs::discover::<alignment::AlignmentPayload>(&runtime.work_root.join("jobs"))?;
    assert!(durable.iter().any(|job| job.id == record.id && job.state == JobState::Succeeded));
    assert!(durable.iter().any(|job| job.id == pending.id && job.state == JobState::Cancelled));
    assert_eq!(session.project(), &before, "MMS host must not edit project");
    let report = json!({"parent_asr_fixture_only":true,"asr_exercised":false,"mms_host_accepted":true,"gui_exercised":false,"project_unchanged":true,
        "real_mms_seconds":elapsed,"result":result,"parent_job_id":record.payload.parent.id,"events":events.into_inner().unwrap(),
        "validation_cases":["bad-audio-index","CPU4","stale-revision","sibling-artifact","unknown-fields","word-id","confidence","manifest-lineage"],
        "cancelled_job":pending.id,"cancel_error":cancelled,"durable_jobs":durable});
    fs::write(work.join("host-result.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("PASS real MMS host; invented parent ASR, no project incorporation. {}", work.join("host-result.json").display());
    Ok(())
}
