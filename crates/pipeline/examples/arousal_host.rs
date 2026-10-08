//! Real MMS+audEERING host chain over invented ASR words. No project incorporation.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tv2_application::{
    CommandEnvelope, ProjectSession,
    jobs::{self, JobRecord, JobState},
};
use tv2_domain::{Command, ErrorCode, Project};
use tv2_pipeline::{
    AnalysisPayload, AnalysisResult, Parameters, Runtime,
    alignment::{self, AlignmentParameters, AlignmentPayload, AlignmentRuntime},
    arousal::{self, ArousalParameters, ArousalPayload, ArousalRuntime},
};

fn write(path: impl AsRef<std::path::Path>, value: &impl serde::Serialize) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn completed<T: serde::de::DeserializeOwned + serde::Serialize>(
    root: &std::path::Path,
    id: &str,
) -> Result<JobRecord<T>, Box<dyn std::error::Error>> {
    Ok(jobs::discover::<T>(root)?.into_iter().find(|record| record.id == id).ok_or("completed receipt missing")?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if arguments.len() != 3 {
        return Err("arousal_host REPO_ROOT NEW_WORK_ROOT KNOWN_TRANSCRIPT_JSON".into());
    }
    let repo = &arguments[0];
    let work = &arguments[1];
    if work.exists() || !work.is_absolute() || !work.parent().ok_or("work root without parent")?.canonicalize()?.starts_with(repo.canonicalize()?) {
        return Err("Use new absolute work root within V2".into());
    }
    fs::create_dir_all(work)?;
    let source = repo.join(".local/e5-python-tests-20260930/fixtures/speech-a.wav");
    let tools = tv2_media::FfmpegTools::locate()?;
    let asset = tools.import(&source, source.to_string_lossy().into_owned())?;
    let mut session = ProjectSession::new(Project::new("Invented ASR parent; real MMS and audEERING"));
    session.execute(CommandEnvelope::human(Command::ImportAsset { asset: asset.clone() }))?;
    let before = session.project().clone();
    write(work.join("project.json"), &before)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut track: Value = serde_json::from_slice(&fs::read(&arguments[2])?)?;
    let words = track["words"].as_array().ok_or("known transcript without words")?;
    assert_eq!(words.len(), 23);
    let word_ids = words.iter().map(|word| word["word_id"].clone()).collect::<Vec<_>>();
    let text = words.iter().map(|word| word["text"].as_str().unwrap_or_default()).collect::<Vec<_>>().join(" ");
    let utterance = json!({"utterance_id":"a0-u-000001","track_id":"a0","text":text,"t_ini":0.0,
        "t_fin":asset.duration().as_seconds_f64(),"word_ids":word_ids,"signals":{},"fixture":"invented known text, no ASR"});
    track["label"] = json!("Known invented SAPI; no ASR");
    track["stream_index"] = json!(asset.probe.audio[0].stream_index);
    track["audio_index"] = json!(0);
    track["offset"] = json!(0.0);
    track["utterances"] = json!([utterance.clone()]);
    track["segments"] = json!([]);
    track["asr"] = json!({"fixture":true,"model_decoding_executed":false,"language":"en","alignment":"not_run"});
    track["words"][0]["edited"] = json!(true);
    track["words"][0]["state"] = json!("disabled");
    track["words"][0]["comment"] = json!("Human fixture annotation retained in every observation stage");
    let asr_runtime = Runtime {
        python: repo.join(".local/e5-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/worker.py"),
        model: repo.join(".local/models/whisper-tiny"),
        work_root: work.join("invented-asr-parent"),
    };
    let parent_queued = tv2_pipeline::enqueue(
        &session,
        asset.clone(),
        source.clone(),
        asr_runtime,
        Parameters { language: "en".into(), ..Default::default() },
        &tools,
        &cancel,
    )?;
    let parent_root = parent_queued.payload.runtime.work_root.join(&parent_queued.id);
    for directory in [".work/audio", ".work/transcripts", "editorial"] {
        fs::create_dir_all(parent_root.join(directory))?;
    }
    fs::copy(&source, parent_root.join(".work/audio/a0.wav"))?;
    write(parent_root.join(".work/transcripts/a0.json"), &track)?;
    let master = json!({"schema":"editorial-master/1","project":{"name":"Explicitly invented ASR fixture"},
        "media":{"path":source,"duration":asset.duration().as_seconds_f64(),"t0":0.0,"fingerprint":asset.fingerprint},
        "tracks":{"a0":track},"conversation":{"utterances":[utterance],"clean_utterance_ids":["a0-u-000001"]},"chunks":[],
        "analysis":{"fixture_only":true,"asr_executed":false,"editorial_decisions":"none"},"fixture":"invented known transcript, never a decoder result"});
    tv2_v1compat::master::V1Master::parse(master.clone())?.projections(&asset.id)?;
    write(parent_root.join("editorial/analysis.editorial.master.json"), &master)?;
    let mut artifacts = BTreeMap::new();
    for name in [".work/audio/a0.wav", ".work/transcripts/a0.json", "editorial/analysis.editorial.master.json"] {
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
    let lease = jobs::claim::<AnalysisPayload>(
        &parent_queued.payload.runtime.work_root.join("jobs"),
        &parent_queued.id,
        &parent_queued.payload_digest,
        false,
    )?;
    lease.finish(
        JobState::Succeeded,
        Some(serde_json::to_value(&parent_result)?),
        Some("FIXTURE ONLY: known invented words; no ASR inference".into()),
    )?;
    let parent = completed::<AnalysisPayload>(&parent_queued.payload.runtime.work_root.join("jobs"), &parent_queued.id)?;
    tv2_pipeline::validate_result(&parent, &parent_result, &cancel)?;
    write(work.join("parent-asr.json"), &parent)?;
    write(work.join("parent-asr-result.json"), &parent_result)?;
    let alignment_runtime = AlignmentRuntime {
        python: repo.join(".local/e5-alignment-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/alignment_worker.py"),
        model: repo.join(".local/models/mms-fa/model.pt"),
        model_manifest: repo.join(".local/models/mms-fa/model-manifest.json"),
        lock: repo.join("workers/python/requirements-alignment.lock"),
        work_root: work.join("alignment"),
    };
    let aligned_queued =
        alignment::enqueue(parent.clone(), parent_result.clone(), 0, alignment_runtime.clone(), AlignmentParameters::default(), &cancel)?;
    let events = Mutex::new(Vec::new());
    let started = Instant::now();
    let aligned_result = alignment::run(&aligned_queued, false, cancel.clone(), |event| events.lock().unwrap().push(event))?;
    let mms_seconds = started.elapsed().as_secs_f64();
    let aligned = completed::<AlignmentPayload>(&alignment_runtime.work_root.join("jobs"), &aligned_queued.id)?;
    write(work.join("alignment-job.json"), &aligned)?;
    write(work.join("alignment-result.json"), &aligned_result)?;
    let runtime = ArousalRuntime {
        python: repo.join(".local/e5-arousal-venv/Scripts/python.exe"),
        worker: repo.join("workers/python/arousal_worker.py"),
        model: repo.join(".local/models/arousal-audeering"),
        model_manifest: repo.join(".local/models/arousal-audeering/model-manifest.json"),
        lock: repo.join("workers/python/requirements-arousal.lock"),
        work_root: work.join("arousal"),
    };
    let record = arousal::enqueue(aligned.clone(), aligned_result.clone(), runtime.clone(), ArousalParameters::default(), &cancel)?;
    let mut altered_record = record.clone();
    altered_record.payload.parameters.scope = "full-audio".into();
    assert_eq!(arousal::run(&altered_record, false, cancel.clone(), |_| {}).unwrap_err().code, ErrorCode::Precondition);
    assert_eq!(completed::<ArousalPayload>(&runtime.work_root.join("jobs"), &record.id)?.state, JobState::Queued);
    let started = Instant::now();
    let result = arousal::run(&record, false, cancel.clone(), |event| events.lock().unwrap().push(event))?;
    let arousal_seconds = started.elapsed().as_secs_f64();
    let durable = completed::<ArousalPayload>(&runtime.work_root.join("jobs"), &record.id)?;
    arousal::validate_result(&durable, &result, &cancel)?;
    write(work.join("arousal-job.json"), &durable)?;
    write(work.join("arousal-result.json"), &result)?;
    let path = runtime.work_root.join(&record.id).join(arousal::OUTPUT);
    let bytes = fs::read(&path)?;
    let output: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(output["words"].as_array().unwrap().len(), 23);
    assert_eq!(output["arousal"]["events"].as_array().unwrap().len(), 5);
    assert_eq!(output["words"][0]["edited"], true);
    assert_eq!(output["words"][0]["state"], "disabled");
    let mut stale = result.clone();
    stale.revision += 1;
    assert!(arousal::validate_result(&record, &stale, &cancel).is_err());
    let mut traversal = result.clone();
    traversal.arousal_path = "../escaped.json".into();
    assert!(arousal::validate_result(&record, &traversal, &cancel).is_err());
    let mut sibling = result.clone();
    sibling.artifacts.insert("extra.json".into(), "0".repeat(64));
    assert!(arousal::validate_result(&record, &sibling, &cancel).is_err());
    let mut unknown = serde_json::to_value(&result)?;
    unknown["extra"] = json!(true);
    assert!(serde_json::from_value::<arousal::ArousalResult>(unknown).is_err());
    let mut unknown_payload = serde_json::to_value(&record.payload)?;
    unknown_payload["extra"] = json!(true);
    assert!(serde_json::from_value::<ArousalPayload>(unknown_payload).is_err());
    for name in
        ["word-id", "asr-origin", "human-comment", "word-association", "baseline", "metadata-extra", "event-extra", "runtime-lock", "source-lineage"]
    {
        let mut altered = output.clone();
        match name {
            "word-id" => altered["words"][0]["word_id"] = json!("forged"),
            "asr-origin" => altered["words"][0]["asr_original"]["text"] = json!("forged"),
            "human-comment" => altered["words"][0]["comment"] = json!("overwritten"),
            "word-association" => altered["words"][0]["arousal"] = json!(999.0),
            "baseline" => altered["arousal"]["baseline"]["mean"] = json!(999.0),
            "metadata-extra" => altered["arousal_analysis"]["extra"] = json!(true),
            "event-extra" => altered["arousal"]["events"][0]["extra"] = json!(true),
            "runtime-lock" => altered["arousal_analysis"]["runtime"]["lock_sha256"] = json!("0".repeat(64)),
            _ => altered["arousal_analysis"]["source_sha256"] = json!("0".repeat(64)),
        }
        write(&path, &altered)?;
        let mut forged = result.clone();
        forged.artifacts.insert(arousal::OUTPUT.into(), tv2_pipeline::sha256_file(&path, &cancel)?);
        assert!(arousal::validate_result(&record, &forged, &cancel).is_err(), "{name} tamper accepted after rehash");
    }
    fs::write(&path, &bytes)?;
    arousal::validate_result(&durable, &result, &cancel)?;
    let pending = arousal::enqueue(aligned, aligned_result, runtime.clone(), ArousalParameters::default(), &cancel)?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let failure = arousal::run(&pending, false, cancellation.clone(), |event| {
        if event["stage"] == "verify-inputs" {
            cancellation.store(true, Ordering::Release);
        }
    })
    .err()
    .ok_or("host cancel unexpectedly succeeded")?;
    assert_eq!(failure.code, ErrorCode::Cancelled);
    let cancelled = completed::<ArousalPayload>(&runtime.work_root.join("jobs"), &pending.id)?;
    assert_eq!(cancelled.state, JobState::Cancelled);
    write(work.join("arousal-cancelled-job.json"), &cancelled)?;
    assert_eq!(session.project(), &before, "Observation hosts cannot change project");
    write(
        work.join("host-result.json"),
        &json!({"parent_asr_fixture_only":true,"asr_exercised":false,"real_mms_host_accepted":true,
        "real_arousal_host_accepted":true,"project_unchanged":true,"gui_exercised":false,"mms_seconds":mms_seconds,"arousal_seconds":arousal_seconds,
        "project_path":work.join("project.json"),"parent_asr_path":work.join("parent-asr.json"),"alignment_job_path":work.join("alignment-job.json"),
        "arousal_job_path":work.join("arousal-job.json"),"parent_asr_id":parent.id,"alignment_id":record.payload.parent.id,"arousal_id":record.id,
        "validation_cases":["stale-revision","traversal","sibling","unknown-result","unknown-payload","word-id","asr-origin","human-comment",
            "word-association","baseline","metadata-extra","event-extra","runtime-lock","source-lineage"],"cancelled_job":pending.id,"cancel_error":failure,
        "result":result,"events":events.into_inner().unwrap()}),
    )?;
    println!("PASS real MMS+audEERING host; explicit ASR fixture, no project incorporation. {}", work.join("host-result.json").display());
    Ok(())
}
