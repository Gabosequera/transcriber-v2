//! Real laughter inference over known negative SAPI; parent ASR is a fixture.
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
    jobs::{self, JobRecord, JobState},
};
use tv2_domain::{ErrorCode, Project};
use tv2_pipeline::{
    AnalysisPayload, AnalysisResult, Parameters, Runtime,
    laughter::{self, LaughterParameters, LaughterRuntime},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if !(2..=3).contains(&arguments.len()) {
        return Err("laughter_host REPO NEW_WORK [EXISTING_PARENT_FIXTURE_JSON]".into());
    }
    let repo = &arguments[0];
    let canonical_repo = repo.canonicalize()?;
    let work = &arguments[1];
    if work.exists() || !work.is_absolute() || !work.parent().ok_or("work without parent")?.canonicalize()?.starts_with(&canonical_repo) {
        return Err("New absolute work root inside V2 required".into());
    }
    fs::create_dir_all(work)?;
    let source = repo.join(".local/e5-python-tests-20260930/fixtures/speech-a.wav");
    let tools = tv2_media::FfmpegTools::locate()?;
    let asset = tools.import(&source, source.to_string_lossy().into_owned())?;
    let session = ProjectSession::new(Project::new("Fixture ASR; real laughter negative"));
    let before = session.project().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let parent: JobRecord<AnalysisPayload> = if let Some(path) = arguments.get(2) {
        if !path.canonicalize()?.starts_with(&canonical_repo) {
            return Err("Parent fixture outside V2".into());
        }
        let record: JobRecord<AnalysisPayload> = serde_json::from_slice(&fs::read(path)?)?;
        if record.payload.source_path.canonicalize()? != source.canonicalize()? {
            return Err("Only known invented SAPI fixture permitted".into());
        }
        record
    } else {
        let runtime = Runtime {
            python: repo.join(".local/e5-venv/Scripts/python.exe"),
            worker: repo.join("workers/python/worker.py"),
            model: repo.join(".local/models/whisper-tiny"),
            work_root: work.join("invented-parent"),
        };
        let queued = tv2_pipeline::enqueue(&session, asset.clone(), source.clone(), runtime, Parameters::default(), &tools, &cancel)?;
        let root = queued.payload.runtime.work_root.join(&queued.id);
        fs::create_dir_all(root.join(".work/audio"))?;
        fs::create_dir_all(root.join(".work/transcripts"))?;
        fs::create_dir_all(root.join("editorial"))?;
        fs::copy(&source, root.join(".work/audio/a0.wav"))?;
        fs::copy(repo.join(".local/e5-mms-real-03/known-transcript.json"), root.join(".work/transcripts/a0.json"))?;
        let master = json!({"schema":"editorial-master/1","project":{"name":"Invented transcript; no ASR"},
            "media":{"path":source,"duration":asset.duration().as_seconds_f64(),"t0":0.0,"fingerprint":asset.fingerprint},
            "tracks":{},"conversation":{},"chunks":[],"fixture":"not ASR; no model decoding"});
        fs::write(root.join("editorial/analysis.editorial.master.json"), serde_json::to_vec(&master)?)?;
        let mut artifacts = BTreeMap::new();
        for name in [".work/audio/a0.wav", ".work/transcripts/a0.json", "editorial/analysis.editorial.master.json"] {
            artifacts.insert(name.into(), tv2_pipeline::sha256_file(&root.join(name), &cancel)?);
        }
        let result = AnalysisResult {
            job_id: queued.id.clone(),
            project_id: queued.project_id.clone(),
            revision: queued.revision,
            project_digest: queued.payload.project_digest.clone(),
            source_sha256: queued.payload.source_sha256.clone(),
            model_digest: queued.payload.model_digest.clone(),
            master_path: "editorial/analysis.editorial.master.json".into(),
            artifacts,
        };
        let lease = jobs::claim::<AnalysisPayload>(&queued.payload.runtime.work_root.join("jobs"), &queued.id, &queued.payload_digest, false)?;
        lease.finish(JobState::Succeeded, Some(serde_json::to_value(result)?), Some("FIXTURE: invented transcript; ASR not executed".into()))?;
        jobs::discover::<AnalysisPayload>(&queued.payload.runtime.work_root.join("jobs"))?
            .into_iter()
            .find(|job| job.id == queued.id)
            .ok_or("Missing parent fixture")?
    };
    let parent_result: AnalysisResult = serde_json::from_value(parent.result.clone().ok_or("Parent missing result")?)?;
    let runtime = LaughterRuntime {
        python: repo.join(".local/e5-laughter-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/laughter_worker.py"),
        model: repo.join(".local/models/laughter-omine/model.safetensors"),
        config: repo.join(".local/models/laughter-omine/config.json"),
        model_manifest: repo.join(".local/models/laughter-omine/model-manifest.json"),
        lock: repo.join("workers/python/requirements-laughter.lock"),
        work_root: work.join("laughter"),
    };
    assert!(laughter::enqueue(parent.clone(), parent_result.clone(), 9, runtime.clone(), LaughterParameters::default(), &cancel).is_err());
    let invalid = LaughterParameters { threads: 4, ..Default::default() };
    assert!(laughter::enqueue(parent.clone(), parent_result.clone(), 0, runtime.clone(), invalid, &cancel).is_err());
    let record = laughter::enqueue(parent.clone(), parent_result.clone(), 0, runtime.clone(), LaughterParameters::default(), &cancel)?;
    let events = std::sync::Mutex::new(Vec::new());
    let started = Instant::now();
    let result = laughter::run(&record, false, cancel.clone(), |event| events.lock().unwrap().push(event))?;
    let elapsed = started.elapsed().as_secs_f64();
    laughter::validate_result(&record, &result, &cancel)?;
    let path = runtime.work_root.join(&record.id).join(laughter::OUTPUT);
    let bytes = fs::read(&path)?;
    let output: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(output["events"], json!([]), "Known negative produced false positive");
    let mut stale = result.clone();
    stale.revision += 1;
    assert!(laughter::validate_result(&record, &stale, &cancel).is_err());
    let mut sibling = result.clone();
    sibling.artifacts.insert("sibling.json".into(), "0".repeat(64));
    assert!(laughter::validate_result(&record, &sibling, &cancel).is_err());
    let mut changed_record = record.clone();
    changed_record.payload.parameters.threshold = 0.6;
    assert!(laughter::validate_result(&changed_record, &result, &cancel).is_err());
    let mut unknown = serde_json::to_value(&result)?;
    unknown["extra"] = json!(true);
    assert!(serde_json::from_value::<laughter::LaughterResult>(unknown).is_err());
    let mut unknown = serde_json::to_value(&record.payload)?;
    unknown["extra"] = json!(true);
    assert!(serde_json::from_value::<laughter::LaughterPayload>(unknown).is_err());
    for name in ["binding", "manifest", "offset", "probability", "range", "event-id"] {
        let mut altered = output.clone();
        // Synthetic adversarial evidence only, never a positive model claim.
        let event = json!({"t_ini":0.0,"t_fin":0.5,"dur":0.5,"tipo":"laughter","conf":0.8,"max_conf":0.8,"mean_conf":0.7,"track_id":"a0","event_id":"a0-laugh-00001"});
        if matches!(name, "probability" | "range" | "event-id") {
            altered["laughter"]["maximum_frame_probability"] = json!(0.9);
        }
        match name {
            "binding" => altered["project_digest"] = json!("0".repeat(64)),
            "manifest" => altered["laughter"]["model_manifest_sha256"] = json!("0".repeat(64)),
            "offset" => altered["audio_offset_ticks"] = json!(1),
            "probability" => {
                altered["events"] = json!([event]);
                altered["events"][0]["conf"] = json!(1.2);
            }
            "range" => {
                altered["events"] = json!([event]);
                altered["events"][0]["t_fin"] = json!(100.0);
            }
            _ => {
                altered["events"] = json!([event]);
                altered["events"][0]["event_id"] = json!("forged");
            }
        }
        fs::write(&path, serde_json::to_vec(&altered)?)?;
        let mut forged = result.clone();
        forged.artifacts.insert(laughter::OUTPUT.into(), tv2_pipeline::sha256_file(&path, &cancel)?);
        assert!(laughter::validate_result(&record, &forged, &cancel).is_err(), "{name} accepted");
    }
    fs::write(&path, &bytes)?;
    laughter::validate_result(&record, &result, &cancel)?;
    let pending = laughter::enqueue(parent.clone(), parent_result, 0, runtime.clone(), LaughterParameters::default(), &cancel)?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancelled = laughter::run(&pending, false, cancellation.clone(), |event| {
        if event["stage"] == "verify-inputs" {
            cancellation.store(true, Ordering::Release);
        }
    })
    .err()
    .ok_or("Cancel unexpectedly succeeded")?;
    assert_eq!(cancelled.code, ErrorCode::Cancelled);
    let durable = jobs::discover::<laughter::LaughterPayload>(&runtime.work_root.join("jobs"))?;
    let final_record = durable.iter().find(|job| job.id == record.id).ok_or("Missing final laughter record")?;
    assert_eq!(final_record.state, JobState::Succeeded);
    assert!(durable.iter().any(|job| job.id == pending.id && job.state == JobState::Cancelled));
    assert_eq!(session.project(), &before);
    fs::write(work.join("parent-record.json"), serde_json::to_vec_pretty(&parent)?)?;
    fs::write(work.join("laughter-record.json"), serde_json::to_vec_pretty(final_record)?)?;
    fs::write(work.join("laughter-result.json"), serde_json::to_vec_pretty(&result)?)?;
    let report = json!({"parent_asr_fixture_only":true,"asr_exercised":false,"laughter_host_accepted":true,"gui_exercised":false,"project_unchanged":true,
        "real_laughter_seconds":elapsed,"result":result,"parent_job_id":record.payload.parent.id,"record":final_record,"output":output,"events":events.into_inner().unwrap(),
        "validation_cases":["bad-audio-index","CPU4","stale-revision","sibling-artifact","record-payload-digest","unknown-fields","binding","manifest","offset","probability","range","event-id"],
        "cancelled_job":pending.id,"cancel_error":cancelled});
    fs::write(work.join("host-result.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("PASS Rust/laughter real negative; fixture parent ASR only. {}", work.join("host-result.json").display());
    Ok(())
}
