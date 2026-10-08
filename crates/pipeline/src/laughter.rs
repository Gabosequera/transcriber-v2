//! Standalone laughter jobs; no automatic project incorporation.
use super::{
    AnalysisPayload, AnalysisResult, OwnedWorker, Runtime, check_cancel, checked_artifact, model_digest, sha256_file,
    validate_result as validate_parent_result,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tv2_application::jobs::{self, JobRecord, JobState};
use tv2_domain::{DomainError, ErrorCode, error::DomainResult};

pub const PROTOCOL: &str = "tv2-laughter/1";
pub const OUTPUT: &str = "laughter/events.json";
const TIMEBASE: &str = "flicks/705600000";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaughterRuntime {
    pub python: PathBuf,
    pub worker: PathBuf,
    pub model: PathBuf,
    pub model_manifest: PathBuf,
    pub config: PathBuf,
    pub lock: PathBuf,
    pub work_root: PathBuf,
}
impl LaughterRuntime {
    fn transport(&self) -> Runtime {
        Runtime { python: self.python.clone(), worker: self.worker.clone(), model: self.model.clone(), work_root: self.work_root.clone() }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaughterParameters {
    pub device: String,
    pub threads: u32,
    pub threshold: f64,
    pub amplitude_boost: bool,
    pub min_dur: f64,
    pub merge_gap: f64,
    pub input_sec: u32,
    pub overlap_sec: u32,
    pub batch_size: u32,
}
impl Default for LaughterParameters {
    fn default() -> Self {
        Self {
            device: "cpu".into(),
            threads: 2,
            threshold: 0.5,
            amplitude_boost: true,
            min_dur: 0.2,
            merge_gap: 0.2,
            input_sec: 7,
            overlap_sec: 2,
            batch_size: 1,
        }
    }
}
impl LaughterParameters {
    pub fn validate(&self) -> DomainResult<()> {
        if self.device != "cpu"
            || self.threads != 2
            || self.input_sec != 7
            || self.overlap_sec != 2
            || !(1..=2).contains(&self.batch_size)
            || !self.threshold.is_finite()
            || !(0.01..=0.99).contains(&self.threshold)
            || !self.min_dur.is_finite()
            || !(0.02..=10.0).contains(&self.min_dur)
            || !self.merge_gap.is_finite()
            || !(0.0..=2.0).contains(&self.merge_gap)
        {
            return Err(DomainError::invalid("Risas requiere CPU2, batch1–2, ventanas7s/solape2s y parámetros acotados"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaughterPayload {
    pub parent: JobRecord<AnalysisPayload>,
    pub parent_result: AnalysisResult,
    pub audio_index: u32,
    pub normalized_audio_sha256: String,
    pub config_sha256: String,
    pub model_sha256: String,
    pub model_manifest_sha256: String,
    pub worker_sha256: String,
    pub lock_sha256: String,
    pub runtime: LaughterRuntime,
    pub parameters: LaughterParameters,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaughterResult {
    pub job_id: String,
    pub project_id: String,
    pub revision: u64,
    pub project_digest: String,
    pub asset_id: String,
    pub track_id: String,
    pub audio_index: u32,
    pub audio_offset_ticks: i64,
    pub normalized_audio_sha256: String,
    pub config_sha256: String,
    pub model_sha256: String,
    pub source_sha256: String,
    pub source_duration_ticks: i64,
    pub timebase: String,
    pub laughter_path: String,
    pub artifacts: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeIdentity {
    python: String,
    dependencies: BTreeMap<String, String>,
    worker_sha256: String,
    lock_sha256: String,
    platform: String,
}

fn parent_audio(payload: &LaughterPayload) -> DomainResult<PathBuf> {
    if !payload.parent.payload.asset.probe.audio.iter().any(|stream| stream.audio_index == payload.audio_index) {
        return Err(DomainError::invalid("Índice de audio no existe en parent ASR"));
    }
    let root = payload.parent.payload.runtime.work_root.join(&payload.parent.id);
    let audio = format!(".work/audio/a{}.wav", payload.audio_index);
    if payload.parent_result.artifacts.get(&audio) != Some(&payload.normalized_audio_sha256) {
        return Err(DomainError::precondition("Recibo parent no contiene exactamente audio solicitado"));
    }
    checked_artifact(&root, &audio)
}

fn verify_inputs(payload: &LaughterPayload, cancel: &AtomicBool) -> DomainResult<()> {
    payload.parameters.validate()?;
    if payload.parent.payload.asset.duration().0 <= 0
        || payload.parent.payload.asset.duration().0 > 705600000i64 * 24 * 3600
        || [
            &payload.runtime.python,
            &payload.runtime.worker,
            &payload.runtime.model,
            &payload.runtime.config,
            &payload.runtime.model_manifest,
            &payload.runtime.lock,
            &payload.runtime.work_root,
        ]
        .iter()
        .any(|path| !path.is_absolute())
        || fs::metadata(&payload.runtime.model_manifest)?.len() > 1024 * 1024
        || fs::metadata(&payload.runtime.lock)?.len() > 1024 * 1024
    {
        return Err(DomainError::invalid("Risas requiere fuente≤24h, rutas absolutas y manifests acotados"));
    }
    if payload.parent.schema != "transcriptor-job/1"
        || !payload.parent.id.starts_with("job-")
        || payload.parent.id.len() != 16
        || !payload.parent.id[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
        || payload.parent.state != JobState::Succeeded
        || payload.parent.result.as_ref() != Some(&serde_json::to_value(&payload.parent_result)?)
        || payload.parent.payload_digest != tv2_domain::digest::digest_json(&serde_json::to_value(&payload.parent.payload)?)
    {
        return Err(DomainError::precondition("Parent ASR debe ser un recibo final validado"));
    }
    validate_parent_result(&payload.parent, &payload.parent_result, cancel)?;
    parent_audio(payload)?;
    let parent = &payload.parent.payload;
    if sha256_file(&parent.source_path, cancel)? != parent.source_sha256
        || model_digest(&parent.runtime.model, cancel)? != parent.model_digest
        || sha256_file(&parent.runtime.worker, cancel)? != parent.worker_sha256
        || sha256_file(&payload.runtime.model, cancel)? != payload.model_sha256
        || sha256_file(&payload.runtime.config, cancel)? != payload.config_sha256
        || sha256_file(&payload.runtime.model_manifest, cancel)? != payload.model_manifest_sha256
        || sha256_file(&payload.runtime.worker, cancel)? != payload.worker_sha256
        || sha256_file(&payload.runtime.lock, cancel)? != payload.lock_sha256
    {
        return Err(DomainError::precondition("Cambió parent, fuente, modelo, manifest, worker o runtime lock"));
    }
    let manifest: Value = serde_json::from_slice(&fs::read(&payload.runtime.model_manifest)?)?;
    if payload.model_sha256 != "449b14f73c70db26da9b4a59ee77d9a9b29fbcaceb083dd7ea27cdfaa68442a0"
        || payload.config_sha256 != "ffcc5c417fe11433447975d5053b2279fbeafd6bca03dd2753082e72ad2d36b7"
        || manifest["model"] != "omine-me/LaughterSegmentation"
        || manifest["model_license"] != "research-only"
        || manifest["model_revision"] != "cb10e3920766372f06bbd9657724f24dc39fa3e4"
        || manifest["files"]["model.safetensors"]["sha256"] != payload.model_sha256
        || manifest["files"]["model.safetensors"]["size"].as_u64() != Some(1261816628)
        || fs::metadata(&payload.runtime.model)?.len() != 1261816628
        || manifest["files"]["config.json"]["sha256"] != payload.config_sha256
        || manifest["files"]["config.json"]["size"].as_u64() != Some(1531)
        || fs::metadata(&payload.runtime.config)?.len() != 1531
    {
        return Err(DomainError::precondition("Manifest risas no corresponde al modelo/config fijados"));
    }
    Ok(())
}

pub fn enqueue(
    parent: JobRecord<AnalysisPayload>,
    parent_result: AnalysisResult,
    audio_index: u32,
    runtime: LaughterRuntime,
    parameters: LaughterParameters,
    cancel: &AtomicBool,
) -> DomainResult<JobRecord<LaughterPayload>> {
    parameters.validate()?;
    let audio = format!(".work/audio/a{audio_index}.wav");
    let payload = LaughterPayload {
        normalized_audio_sha256: parent_result.artifacts.get(&audio).cloned().ok_or_else(|| DomainError::invalid("Parent sin audio exacto"))?,
        config_sha256: sha256_file(&runtime.config, cancel)?,
        model_sha256: sha256_file(&runtime.model, cancel)?,
        model_manifest_sha256: sha256_file(&runtime.model_manifest, cancel)?,
        worker_sha256: sha256_file(&runtime.worker, cancel)?,
        lock_sha256: sha256_file(&runtime.lock, cancel)?,
        parent,
        parent_result,
        audio_index,
        runtime,
        parameters,
    };
    verify_inputs(&payload, cancel)?;
    jobs::enqueue(&payload.runtime.work_root.join("jobs"), payload.parent.project_id.clone(), payload.parent.revision, payload)
}

fn runtime_identity(payload: &LaughterPayload, identity: &RuntimeIdentity) -> DomainResult<()> {
    let mut expected = BTreeMap::new();
    for line in fs::read_to_string(&payload.runtime.lock)?.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let (name, version) = line.split_once("==").ok_or_else(|| DomainError::invalid("Lock laughter inválido"))?;
        expected.insert(name.to_string(), version.to_string());
    }
    if identity.python != "3.11.15"
        || identity.dependencies != expected
        || identity.worker_sha256 != payload.worker_sha256
        || identity.lock_sha256 != payload.lock_sha256
    {
        return Err(DomainError::precondition("Runtime laughter no coincide con solicitud"));
    }
    Ok(())
}

pub fn run(record: &JobRecord<LaughterPayload>, resume: bool, cancel: Arc<AtomicBool>, progress: impl Fn(Value)) -> DomainResult<LaughterResult> {
    record_identity(record)?;
    let payload = &record.payload;
    let lease = jobs::claim::<LaughterPayload>(&payload.runtime.work_root.join("jobs"), &record.id, &record.payload_digest, resume)?;
    let result = (|| {
        verify_inputs(payload, &cancel)?;
        if record.project_id != payload.parent.project_id || record.revision != payload.parent.revision {
            return Err(DomainError::precondition("Binding proyecto/revisión de alineación difiere del parent"));
        }
        let audio = parent_audio(payload)?;
        let mut worker = OwnedWorker::spawn_protocol(&payload.runtime.transport(), PROTOCOL)?;
        let operation = (|| {
            worker.send("hello", "hello", json!({}), Duration::from_secs(10), &cancel)?;
            let hello = worker.response("hello", None, Duration::from_secs(30), &cancel, &progress)?;
            if hello["protocol"] != PROTOCOL {
                return Err(DomainError::unsupported("Protocolo risas incompatible"));
            }
            runtime_identity(payload, &serde_json::from_value(hello["runtime"].clone())?)?;
            worker.send("run", "run", json!({
                "job_id":record.id,"project_id":record.project_id,"revision":record.revision,"project_digest":payload.parent.payload.project_digest,
                "asset_id":payload.parent.payload.asset.id,"track_id":format!("a{}",payload.audio_index),"audio_index":payload.audio_index,"audio_offset_ticks":0,
                "normalized_audio_path":audio,"normalized_audio_sha256":payload.normalized_audio_sha256,
                "model_path":payload.runtime.model,"config_path":payload.runtime.config,"config_sha256":payload.config_sha256,
                "manifest_path":payload.runtime.model_manifest,"model_sha256":payload.model_sha256,"parameters":payload.parameters,
                "source_sha256":payload.parent.payload.source_sha256,"source_duration_ticks":payload.parent.payload.asset.duration().0,"timebase":TIMEBASE,
            }), Duration::from_secs(10), &cancel)?;
            let raw = worker.response("run", Some(&record.id), Duration::from_secs(24 * 3600), &cancel, &progress)?;
            serde_json::from_value::<LaughterResult>(raw).map_err(Into::into)
        })();
        if cancel.load(Ordering::Acquire) {
            let _ = worker.send("cancel", "cancel", json!({"job_id":record.id}), Duration::from_millis(250), &AtomicBool::new(false));
        }
        worker.close();
        let result = operation?;
        validate_result(record, &result, &cancel)?;
        Ok(result)
    })();
    let state = match &result {
        Ok(_) => JobState::Succeeded,
        Err(error) if error.code == ErrorCode::Cancelled => JobState::Cancelled,
        Err(_) => JobState::Failed,
    };
    lease.finish(state, result.as_ref().ok().map(serde_json::to_value).transpose()?, result.as_ref().err().map(ToString::to_string))?;
    result
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    t_ini: f64,
    t_fin: f64,
    tipo: String,
    conf: f64,
    max_conf: f64,
    mean_conf: f64,
    dur: f64,
    track_id: String,
    event_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    schema: String,
    job_id: String,
    project_id: String,
    revision: u64,
    project_digest: String,
    asset_id: String,
    track_id: String,
    audio_index: u32,
    audio_offset_ticks: i64,
    events: Vec<Event>,
    laughter: Value,
}

fn record_identity(record: &JobRecord<LaughterPayload>) -> DomainResult<()> {
    if record.schema != "transcriptor-job/1"
        || !record.id.starts_with("job-")
        || record.id.len() != 16
        || !record.id[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
        || record.payload_digest != tv2_domain::digest::digest_json(&serde_json::to_value(&record.payload)?)
        || record.project_id != record.payload.parent.project_id
        || record.revision != record.payload.parent.revision
    {
        return Err(DomainError::precondition("Record risas no ligado a payload/proyecto/revisión"));
    }
    Ok(())
}

pub fn validate_result(record: &JobRecord<LaughterPayload>, result: &LaughterResult, cancel: &AtomicBool) -> DomainResult<()> {
    check_cancel(cancel)?;
    record_identity(record)?;
    let payload = &record.payload;
    let parent = &payload.parent.payload;
    let track = format!("a{}", payload.audio_index);
    if record.project_id != payload.parent.project_id
        || record.revision != payload.parent.revision
        || result.job_id != record.id
        || result.project_id != record.project_id
        || result.revision != record.revision
        || result.project_digest != parent.project_digest
        || result.asset_id != parent.asset.id.as_str()
        || result.track_id != track
        || result.audio_index != payload.audio_index
        || result.audio_offset_ticks != 0
        || result.normalized_audio_sha256 != payload.normalized_audio_sha256
        || result.config_sha256 != payload.config_sha256
        || result.source_sha256 != parent.source_sha256
        || result.model_sha256 != payload.model_sha256
        || result.source_duration_ticks != parent.asset.duration().0
        || result.timebase != TIMEBASE
        || result.laughter_path != OUTPUT
        || result.artifacts.len() != 1
        || !result.artifacts.contains_key(OUTPUT)
    {
        return Err(DomainError::precondition("Resultado risas no ligado exactamente a la solicitud"));
    }
    verify_inputs(payload, cancel)?;
    let path = checked_artifact(&payload.runtime.work_root.join(&record.id), OUTPUT)?;
    if fs::metadata(&path)?.len() > 128 * 1024 * 1024 {
        return Err(DomainError::invalid("Artefacto risas supera128MiB"));
    }
    let bytes = fs::read(&path)?;
    if hex::encode(sha2::Sha256::digest(&bytes)) != result.artifacts[OUTPUT] {
        return Err(DomainError::precondition("Artefacto risas alterado"));
    }
    let artifact: Artifact = serde_json::from_slice(&bytes)?;
    if artifact.schema != "tv2-laughter-events/1"
        || artifact.job_id != record.id
        || artifact.project_id != record.project_id
        || artifact.revision != record.revision
        || artifact.project_digest != result.project_digest
        || artifact.asset_id != result.asset_id
        || artifact.track_id != track
        || artifact.audio_index != payload.audio_index
        || artifact.audio_offset_ticks != 0
    {
        return Err(DomainError::precondition("Binding interno de risas difiere del recibo"));
    }
    let metadata = &artifact.laughter;
    if metadata["algorithm"] != "omine-v1-global-maxpool-streamed-amplitude/1"
        || metadata["execution_state"] != "completed"
        || metadata["model_sha256"] != payload.model_sha256
        || metadata["config_sha256"] != payload.config_sha256
        || metadata["model_manifest_sha256"] != payload.model_manifest_sha256
        || metadata["license"] != "research-only"
        || metadata["normalized_audio_sha256"] != payload.normalized_audio_sha256
        || metadata["source_sha256"] != parent.source_sha256
        || metadata["source_duration_ticks"] != result.source_duration_ticks
        || metadata["timebase"] != TIMEBASE
        || metadata["parameters"] != serde_json::to_value(&payload.parameters)?
        || metadata["editorial_decisions"] != "none"
    {
        return Err(DomainError::precondition("Metadata risas no concuerda con lineage/ejecución"));
    }
    runtime_identity(payload, &serde_json::from_value(metadata["runtime"].clone())?)?;
    let frame_duration = metadata["frame_duration_seconds"].as_f64().ok_or_else(|| DomainError::invalid("Risas sin duración de frame"))?;
    let maximum = metadata["maximum_frame_probability"].as_f64().ok_or_else(|| DomainError::invalid("Risas sin probabilidad de frames"))?;
    let windows = metadata["windows"].as_u64().ok_or_else(|| DomainError::invalid("Risas sin ventanas"))?;
    let duration = parent.asset.duration().as_seconds_f64();
    if !frame_duration.is_finite()
        || (frame_duration - 7.0 / 349.0).abs() > 1e-12
        || !maximum.is_finite()
        || !(0.0..=1.0).contains(&maximum)
        || windows == 0
        || windows > (duration / 5.0).ceil() as u64 + 1
    {
        return Err(DomainError::invalid("Estado de frames/ventanas risas inválido"));
    }
    let mut previous_end = None;
    for (index, event) in artifact.events.iter().enumerate() {
        check_cancel(cancel)?;
        if ![event.t_ini, event.t_fin, event.dur, event.conf, event.mean_conf, event.max_conf].iter().all(|v| v.is_finite())
            || event.t_ini < 0.0
            || event.t_fin <= event.t_ini
            || event.t_fin > duration
            || event.t_fin - event.t_ini + 1e-9 < payload.parameters.min_dur
            || (event.dur - (event.t_fin - event.t_ini)).abs() > 0.000501
            || event.tipo != "laughter"
            || event.track_id != track
            || event.event_id != format!("{track}-laugh-{:05}", index + 1)
            || ![event.conf, event.mean_conf, event.max_conf].iter().all(|v| (0.0..=1.0).contains(v))
            || event.conf != event.max_conf
            || event.mean_conf > event.max_conf
            || event.max_conf > maximum + 0.000501
            || event.mean_conf + 0.000501 < payload.parameters.threshold
            || previous_end.is_some_and(|end: f64| event.t_ini < end || event.t_ini - end + 1e-9 < payload.parameters.merge_gap)
        {
            return Err(DomainError::invalid("Evento risas viola rango/probabilidad/identidad/orden"));
        }
        previous_end = Some(event.t_fin);
    }
    Ok(())
}
