//! Standalone MMS jobs. Alignment never incorporates a master or edits a project.
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

pub const PROTOCOL: &str = "tv2-alignment/1";
pub const OUTPUT: &str = "alignment/aligned-words.json";
const TIMEBASE: &str = "flicks/705600000";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentRuntime {
    pub python: PathBuf,
    pub worker: PathBuf,
    pub model: PathBuf,
    pub model_manifest: PathBuf,
    pub lock: PathBuf,
    pub work_root: PathBuf,
}
impl AlignmentRuntime {
    fn transport(&self) -> Runtime {
        Runtime { python: self.python.clone(), worker: self.worker.clone(), model: self.model.clone(), work_root: self.work_root.clone() }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentParameters {
    pub device: String,
    pub threads: u32,
    pub batch_words: u32,
    pub margin: f64,
    pub max_batch_seconds: f64,
}
impl Default for AlignmentParameters {
    fn default() -> Self {
        Self { device: "cpu".into(), threads: 2, batch_words: 120, margin: 0.5, max_batch_seconds: 30.0 }
    }
}
impl AlignmentParameters {
    pub fn validate(&self) -> DomainResult<()> {
        if self.device != "cpu"
            || self.threads != 2
            || !(1..=120).contains(&self.batch_words)
            || !self.margin.is_finite()
            || !(0.0..=1.0).contains(&self.margin)
            || !self.max_batch_seconds.is_finite()
            || self.max_batch_seconds <= 0.0
            || self.max_batch_seconds > 30.0
        {
            return Err(DomainError::invalid("MMS admite CPU2, batch1–120, margen0–1 y ventana≤30s"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentPayload {
    pub parent: JobRecord<AnalysisPayload>,
    pub parent_result: AnalysisResult,
    pub audio_index: u32,
    pub normalized_audio_sha256: String,
    pub transcript_sha256: String,
    pub model_sha256: String,
    pub model_manifest_sha256: String,
    pub worker_sha256: String,
    pub lock_sha256: String,
    pub runtime: AlignmentRuntime,
    pub parameters: AlignmentParameters,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentResult {
    pub job_id: String,
    pub project_id: String,
    pub revision: u64,
    pub project_digest: String,
    pub asset_id: String,
    pub normalized_audio_sha256: String,
    pub transcript_sha256: String,
    pub model_sha256: String,
    pub source_sha256: String,
    pub source_duration_ticks: i64,
    pub timebase: String,
    pub alignment_path: String,
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

fn parent_paths(payload: &AlignmentPayload) -> DomainResult<(PathBuf, PathBuf)> {
    if !payload.parent.payload.asset.probe.audio.iter().any(|stream| stream.audio_index == payload.audio_index) {
        return Err(DomainError::invalid("Índice de audio no existe en parent ASR"));
    }
    let root = payload.parent.payload.runtime.work_root.join(&payload.parent.id);
    let audio = format!(".work/audio/a{}.wav", payload.audio_index);
    let transcript = format!(".work/transcripts/a{}.json", payload.audio_index);
    if payload.parent_result.artifacts.get(&audio) != Some(&payload.normalized_audio_sha256)
        || payload.parent_result.artifacts.get(&transcript) != Some(&payload.transcript_sha256)
    {
        return Err(DomainError::precondition("Recibo parent no contiene exactamente audio/transcript solicitados"));
    }
    Ok((checked_artifact(&root, &audio)?, checked_artifact(&root, &transcript)?))
}

fn verify_inputs(payload: &AlignmentPayload, cancel: &AtomicBool) -> DomainResult<()> {
    payload.parameters.validate()?;
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
    parent_paths(payload)?;
    let parent = &payload.parent.payload;
    if sha256_file(&parent.source_path, cancel)? != parent.source_sha256
        || model_digest(&parent.runtime.model, cancel)? != parent.model_digest
        || sha256_file(&parent.runtime.worker, cancel)? != parent.worker_sha256
        || sha256_file(&payload.runtime.model, cancel)? != payload.model_sha256
        || sha256_file(&payload.runtime.model_manifest, cancel)? != payload.model_manifest_sha256
        || sha256_file(&payload.runtime.worker, cancel)? != payload.worker_sha256
        || sha256_file(&payload.runtime.lock, cancel)? != payload.lock_sha256
    {
        return Err(DomainError::precondition("Cambió parent, fuente, modelo, manifest, worker o runtime lock"));
    }
    let manifest: Value = serde_json::from_slice(&fs::read(&payload.runtime.model_manifest)?)?;
    if manifest["sha256"].as_str() != Some(&payload.model_sha256)
        || manifest["size"].as_u64() != Some(fs::metadata(&payload.runtime.model)?.len())
        || manifest["model"] != "torchaudio.pipelines.MMS_FA"
        || manifest["torchaudio_version"] != "2.8.0+cpu"
    {
        return Err(DomainError::precondition("Manifest MMS no corresponde a modelo/runtime"));
    }
    Ok(())
}

pub fn enqueue(
    parent: JobRecord<AnalysisPayload>,
    parent_result: AnalysisResult,
    audio_index: u32,
    runtime: AlignmentRuntime,
    parameters: AlignmentParameters,
    cancel: &AtomicBool,
) -> DomainResult<JobRecord<AlignmentPayload>> {
    parameters.validate()?;
    let audio = format!(".work/audio/a{audio_index}.wav");
    let transcript = format!(".work/transcripts/a{audio_index}.json");
    let payload = AlignmentPayload {
        normalized_audio_sha256: parent_result.artifacts.get(&audio).cloned().ok_or_else(|| DomainError::invalid("Parent sin audio exacto"))?,
        transcript_sha256: parent_result.artifacts.get(&transcript).cloned().ok_or_else(|| DomainError::invalid("Parent sin transcript exacto"))?,
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

fn runtime_identity(payload: &AlignmentPayload, identity: &RuntimeIdentity) -> DomainResult<()> {
    let mut expected = BTreeMap::new();
    for line in fs::read_to_string(&payload.runtime.lock)?.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let (name, version) = line.split_once("==").ok_or_else(|| DomainError::invalid("Lock alignment inválido"))?;
        expected.insert(name.to_string(), version.to_string());
    }
    if identity.python != "3.12.13"
        || identity.dependencies != expected
        || identity.worker_sha256 != payload.worker_sha256
        || identity.lock_sha256 != payload.lock_sha256
    {
        return Err(DomainError::precondition("Runtime alignment no coincide con solicitud"));
    }
    Ok(())
}

pub fn run(record: &JobRecord<AlignmentPayload>, resume: bool, cancel: Arc<AtomicBool>, progress: impl Fn(Value)) -> DomainResult<AlignmentResult> {
    super::validate_record(record)?;
    let payload = &record.payload;
    let lease = jobs::claim::<AlignmentPayload>(&payload.runtime.work_root.join("jobs"), &record.id, &record.payload_digest, resume)?;
    let result = (|| {
        verify_inputs(payload, &cancel)?;
        if record.project_id != payload.parent.project_id || record.revision != payload.parent.revision {
            return Err(DomainError::precondition("Binding proyecto/revisión de alineación difiere del parent"));
        }
        let (audio, transcript) = parent_paths(payload)?;
        let mut worker = OwnedWorker::spawn_protocol(&payload.runtime.transport(), PROTOCOL)?;
        let operation = (|| {
            worker.send("hello", "hello", json!({}), Duration::from_secs(10), &cancel)?;
            let hello = worker.response("hello", None, Duration::from_secs(30), &cancel, &progress)?;
            if hello["protocol"] != PROTOCOL {
                return Err(DomainError::unsupported("Protocolo MMS incompatible"));
            }
            runtime_identity(payload, &serde_json::from_value(hello["runtime"].clone())?)?;
            worker.send("run", "run", json!({
                "job_id":record.id,"project_id":record.project_id,"revision":record.revision,"project_digest":payload.parent.payload.project_digest,
                "asset_id":payload.parent.payload.asset.id,"normalized_audio_path":audio,"normalized_audio_sha256":payload.normalized_audio_sha256,
                "transcript_path":transcript,"transcript_sha256":payload.transcript_sha256,"model_path":payload.runtime.model,
                "manifest_path":payload.runtime.model_manifest,"model_sha256":payload.model_sha256,"parameters":payload.parameters,
                "source_sha256":payload.parent.payload.source_sha256,"source_duration_ticks":payload.parent.payload.asset.duration().0,"timebase":TIMEBASE,
            }), Duration::from_secs(10), &cancel)?;
            let raw = worker.response("run", Some(&record.id), Duration::from_secs(24 * 3600), &cancel, &progress)?;
            serde_json::from_value::<AlignmentResult>(raw).map_err(Into::into)
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

pub fn validate_result(record: &JobRecord<AlignmentPayload>, result: &AlignmentResult, cancel: &AtomicBool) -> DomainResult<()> {
    super::validate_record(record)?;
    check_cancel(cancel)?;
    let payload = &record.payload;
    let parent = &payload.parent.payload;
    if result.job_id != record.id
        || result.project_id != record.project_id
        || result.revision != record.revision
        || result.project_digest != parent.project_digest
        || result.asset_id != parent.asset.id.as_str()
        || result.normalized_audio_sha256 != payload.normalized_audio_sha256
        || result.transcript_sha256 != payload.transcript_sha256
        || result.source_sha256 != parent.source_sha256
        || result.model_sha256 != payload.model_sha256
        || result.source_duration_ticks != parent.asset.duration().0
        || result.timebase != TIMEBASE
        || result.alignment_path != OUTPUT
        || result.artifacts.len() != 1
        || !result.artifacts.contains_key(OUTPUT)
    {
        return Err(DomainError::precondition("Resultado MMS no ligado exactamente a la solicitud"));
    }
    verify_inputs(payload, cancel)?;
    let path = checked_artifact(&payload.runtime.work_root.join(&record.id), OUTPUT)?;
    if fs::metadata(&path)?.len() > 128 * 1024 * 1024 {
        return Err(DomainError::invalid("Alignment supera128MiB"));
    }
    let bytes = fs::read(&path)?;
    if hex::encode(sha2::Sha256::digest(&bytes)) != result.artifacts[OUTPUT] {
        return Err(DomainError::precondition("Artefacto MMS alterado"));
    }
    let aligned: Value = serde_json::from_slice(&bytes)?;
    let (_, transcript) = parent_paths(payload)?;
    if fs::metadata(&transcript)?.len() > 64 * 1024 * 1024 {
        return Err(DomainError::invalid("Transcript supera64MiB"));
    }
    let original_bytes = fs::read(transcript)?;
    if hex::encode(sha2::Sha256::digest(&original_bytes)) != payload.transcript_sha256 {
        return Err(DomainError::precondition("Transcript parent cambió durante lectura"));
    }
    let original: Value = serde_json::from_slice(&original_bytes)?;
    let mut top = aligned.clone();
    top.as_object_mut().ok_or_else(|| DomainError::invalid("Alignment no es objeto"))?.remove("alignment");
    top["words"] = original["words"].clone();
    if top != original {
        return Err(DomainError::precondition("Alineación modificó transcript original fuera de words"));
    }
    let originals = original["words"].as_array().ok_or_else(|| DomainError::invalid("Transcript sin words"))?;
    let words = aligned["words"].as_array().ok_or_else(|| DomainError::invalid("Alignment sin words"))?;
    if originals.len() != words.len() {
        return Err(DomainError::precondition("Cardinalidad de palabras MMS alterada"));
    }
    let mut aligned_count = 0usize;
    let mut interpolated = 0usize;
    for (source, word) in originals.iter().zip(words) {
        check_cancel(cancel)?;
        let start = word["t_ini"].as_f64().ok_or_else(|| DomainError::invalid("MMS sin t_ini"))?;
        let end = word["t_fin"].as_f64().ok_or_else(|| DomainError::invalid("MMS sin t_fin"))?;
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || end > parent.asset.duration().as_seconds_f64() {
            return Err(DomainError::invalid("Rango MMS fuera de fuente"));
        }
        match word["alignment_source"].as_str() {
            Some("mms") => aligned_count += 1,
            Some("mms_interpolated") => interpolated += 1,
            Some("whisper_unalignable" | "whisper_fallback") => {}
            _ => return Err(DomainError::invalid("Provenance de palabra MMS desconocida")),
        }
        let mut expected = source.clone();
        expected["t_ini"] = word["t_ini"].clone();
        expected["t_fin"] = word["t_fin"].clone();
        expected["alignment_source"] = word["alignment_source"].clone();
        expected["asr_original"] = source.clone();
        if expected != *word {
            return Err(DomainError::precondition("MMS cambió ID/text/confidence/ASR original"));
        }
    }
    let metadata = &aligned["alignment"];
    if metadata["schema"] != "tv2-aligned-words/1"
        || metadata["algorithm"] != "v1-mms-fa-cpu-bounded/1"
        || metadata["model_sha256"] != payload.model_sha256
        || metadata["model_manifest_sha256"] != payload.model_manifest_sha256
        || metadata["normalized_audio_sha256"] != payload.normalized_audio_sha256
        || metadata["source_transcript_sha256"] != payload.transcript_sha256
        || metadata["source_sha256"] != parent.source_sha256
        || metadata["source_duration_ticks"] != result.source_duration_ticks
        || metadata["timebase"] != TIMEBASE
        || metadata["editorial_decisions"] != "none"
        || metadata["aligned_words"].as_u64() != Some(aligned_count as u64)
        || metadata["interpolated_words"].as_u64() != Some(interpolated as u64)
        || !metadata["failed_batches"].is_array()
    {
        return Err(DomainError::precondition("Metadata MMS no concuerda con lineage/ejecución"));
    }
    let failures = metadata["failed_batches"].as_array().unwrap();
    let state = metadata["execution_state"].as_str().ok_or_else(|| DomainError::invalid("MMS sin estado de ejecución"))?;
    if (!failures.is_empty() && state != "completed_with_fallbacks")
        || (failures.is_empty() && !matches!(state, "completed" | "no_alignable_words"))
        || (state == "no_alignable_words" && (aligned_count != 0 || interpolated != 0))
        || (state == "completed" && aligned_count == 0)
    {
        return Err(DomainError::precondition("Estado MMS no concuerda con fallos/palabras"));
    }
    for failure in failures {
        let index = failure["first_word_index"].as_u64().ok_or_else(|| DomainError::invalid("Lote fallido sin índice"))?;
        let count = failure["word_count"].as_u64().ok_or_else(|| DomainError::invalid("Lote fallido sin cantidad"))?;
        let start = failure["window"]["start"].as_f64().ok_or_else(|| DomainError::invalid("Lote fallido sin inicio"))?;
        let end = failure["window"]["end"].as_f64().ok_or_else(|| DomainError::invalid("Lote fallido sin fin"))?;
        if count == 0
            || count > 120
            || index.checked_add(count).is_none_or(|end| end > words.len() as u64)
            || !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || end <= start
            || end > parent.asset.duration().as_seconds_f64()
            || failure["error"].as_str().is_none_or(|error| error.is_empty() || error.len() > 16384)
        {
            return Err(DomainError::invalid("Metadata de lote fallido inválida"));
        }
    }
    runtime_identity(payload, &serde_json::from_value(metadata["runtime"].clone())?)?;
    Ok(())
}
