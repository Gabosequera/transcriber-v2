//! Workflow recovery over the exact existing invented ASR parent; real stdlib
//! generation only. No ASR inference, ML stage run, or project incorporation.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tv2_application::jobs::{self, JobRecord, JobState};
use tv2_pipeline::{
    generation::{GenerationPayload, GenerationResult},
    workflow::{self, WorkflowPayload, WorkflowPlan},
};

fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn current<T: serde::Serialize + serde::de::DeserializeOwned>(root: &Path, id: &str) -> Result<JobRecord<T>, Box<dyn std::error::Error>> {
    jobs::discover(root)?.into_iter().find(|job| job.id == id).ok_or_else(|| "Missing durable job".into())
}
type FileInventory = BTreeMap<PathBuf, (u64, std::time::SystemTime, String)>;
fn inventory(root: &Path) -> Result<FileInventory, Box<dyn std::error::Error>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            result.extend(inventory(&path)?);
        } else {
            let metadata = fs::metadata(&path)?;
            result.insert(path.clone(), (metadata.len(), metadata.modified()?, tv2_pipeline::sha256_file(&path, &AtomicBool::new(false))?));
        }
    }
    Ok(result)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("workflow_host REPO NEW_OUTPUT EXISTING_GENERATION_PAYLOAD".into());
    }
    let repo = args[0].canonicalize()?;
    let output = &args[1];
    if output.exists() || !output.is_absolute() || !output.parent().ok_or("Missing output parent")?.canonicalize()?.starts_with(&repo) {
        return Err("Use a fresh absolute evidence directory inside V2".into());
    }
    let existing: GenerationPayload = load(&args[2])?;
    let parent_before = serde_json::to_value(&existing.parent)?;
    let parent_path = existing.parent.payload.runtime.work_root.join("jobs").join(format!("{}.json", existing.parent.id));
    let parent_bytes = fs::read(&parent_path)?;
    fs::create_dir_all(output)?;
    let mut generation = existing.runtime.clone();
    generation.work_root = output.join("generation");
    let plan = WorkflowPlan { alignment: None, arousal: None, laughter: None, generation };
    let flag = Arc::new(AtomicBool::new(false));
    let queued = workflow::enqueue(existing.parent.clone(), plan, output.join("workflows"), &flag)?;
    let start = std::time::Instant::now();
    let cancelled = workflow::run(&queued, false, flag.clone(), |event| {
        println!("{event}");
        if event["step"] == "generation" && event["detail"]["fraction"] == 0.0 {
            flag.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert_eq!(cancelled.code, tv2_domain::ErrorCode::Cancelled);
    let checkpoint_path = queued.payload.work_root.join(&queued.id).join(".work/workflow.json");
    let checkpoint: Value = load(&checkpoint_path)?;
    let child_id = checkpoint["children"]["generation"]["id"].as_str().ok_or("Missing generation ID before run")?.to_owned();
    let first: JobRecord<GenerationPayload> = current(&queued.payload.plan.generation.work_root.join("jobs"), &child_id)?;
    assert_eq!(first.state, JobState::Queued);
    assert!(first.attempts.is_empty());
    flag.store(false, Ordering::SeqCst);
    let cancelled_record: JobRecord<WorkflowPayload> = current(&queued.payload.work_root.join("jobs"), &queued.id)?;
    assert_eq!(cancelled_record.state, JobState::Cancelled);
    assert!(workflow::run(&cancelled_record, false, flag.clone(), |_| {}).is_err());
    let cancelled_again = workflow::run(&cancelled_record, true, flag.clone(), |event| {
        println!("{event}");
        if event["step"] == "generation"
            && event["detail"]["fraction"] == 1.0
            && event["detail"]["cached"] == false
            && event["detail"].get("event").is_none()
        {
            flag.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert_eq!(cancelled_again.code, tv2_domain::ErrorCode::Cancelled);
    let second: JobRecord<GenerationPayload> = current(&queued.payload.plan.generation.work_root.join("jobs"), &child_id)?;
    assert_eq!(second.state, JobState::Succeeded);
    assert_eq!(second.attempts.len(), 1);
    let cancelled_record: JobRecord<WorkflowPayload> = current(&queued.payload.work_root.join("jobs"), &queued.id)?;
    assert_eq!(cancelled_record.state, JobState::Cancelled);
    flag.store(false, Ordering::SeqCst);
    let result = workflow::run(&cancelled_record, true, flag.clone(), |event| println!("{event}"))?;
    let final_record: JobRecord<WorkflowPayload> = current(&queued.payload.work_root.join("jobs"), &queued.id)?;
    assert_eq!(final_record.state, JobState::Succeeded);
    assert_eq!(final_record.attempts.len(), 3);
    assert_eq!(result.generation.id, child_id);
    assert_eq!(result.generation.attempts.len(), 1);
    let before = inventory(output)?;
    workflow::validate_result(&final_record, &result, &flag)?;
    assert_eq!(before, inventory(output)?);
    assert_eq!(parent_bytes, fs::read(parent_path)?);
    assert_eq!(parent_before, serde_json::to_value(&result.generation.payload.parent)?);
    let report = json!({"workflow_host_accepted":true,"asr_inference":false,"asr_parent_invented":true,"ml_stages_executed":[],
        "project_mutated":false,"workflow_id":final_record.id,"workflow_state":final_record.state,"workflow_attempts":3,
        "generation_job_id":child_id,"generation_attempts":1,"checkpoint_before_child_run":true,"resume_explicit_only":true,
        "succeeded_generation_reused":true,"validate_readonly":true,"parent_exact_unchanged":true,
        "plan":"generation-only","duration_seconds":start.elapsed().as_secs_f64(),"result":result});
    fs::write(output.join("workflow-record.json"), serde_json::to_vec_pretty(&final_record)?)?;
    fs::write(output.join("workflow-result.json"), serde_json::to_vec_pretty(&result)?)?;
    fs::write(output.join("host-result.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    let _: GenerationResult = result.result;
    Ok(())
}
