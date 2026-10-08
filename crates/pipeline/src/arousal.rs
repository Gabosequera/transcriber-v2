//! AudEERING observations over an exact completed alignment job. No project writes.
use super::{OwnedWorker, Runtime, alignment, check_cancel, checked_artifact, sha256_file};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tv2_application::jobs::{self, JobRecord, JobState};
use tv2_domain::{DomainError, ErrorCode, error::DomainResult};

pub const PROTOCOL: &str = "tv2-arousal/1";
pub const OUTPUT: &str = "arousal/arousal-words.json";
const MODEL: &str = "audeering/wav2vec2-large-robust-12-ft-emotion-msp-dim";
const REVISION: &str = "6eba34a2485ea31cb03600241787c3a5edab8626";
const TIMEBASE: &str = "canonical-normalized-pcm/1";
const ALGORITHM: &str = "v1-audeering-speech-regions-windowed-cpu/1";
const PINNED_FILES: [(&str, u64, &str); 3] = [
    ("config.json", 2344, "c0962c3d1f065972bbebbba0bbffb8016ef4e9aae4a9b07e5fec22f770d2cddb"),
    ("model.safetensors", 661375508, "efa5ac1a13b2d2f42182738e44794b1eb4c0cdd221a8b4ae11304c3a5f5fae95"),
    ("preprocessor_config.json", 214, "60ca5a31e13f69ee2fbf147504c8676db5f6398fd7a6b12294341dff838edfcf"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArousalRuntime {
    pub python: PathBuf,
    pub worker: PathBuf,
    pub model: PathBuf,
    pub model_manifest: PathBuf,
    pub lock: PathBuf,
    pub work_root: PathBuf,
}
impl ArousalRuntime {
    fn transport(&self) -> Runtime {
        Runtime { python: self.python.clone(), worker: self.worker.clone(), model: self.model.clone(), work_root: self.work_root.clone() }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArousalParameters {
    pub device: String,
    pub threads: u32,
    pub window_seconds: f64,
    pub hop_seconds: f64,
    pub batch_size: u32,
    pub gap_seconds: f64,
    pub padding_seconds: f64,
    pub scope: String,
}
impl Default for ArousalParameters {
    fn default() -> Self {
        Self {
            device: "cpu".into(),
            threads: 2,
            window_seconds: 4.0,
            hop_seconds: 2.0,
            batch_size: 1,
            gap_seconds: 1.0,
            padding_seconds: 0.5,
            scope: "speech-regions".into(),
        }
    }
}
impl ArousalParameters {
    pub fn validate(&self) -> DomainResult<()> {
        if self.device != "cpu"
            || !(1..=4).contains(&self.threads)
            || !(1..=8).contains(&self.batch_size)
            || !self.window_seconds.is_finite()
            || !self.hop_seconds.is_finite()
            || !self.gap_seconds.is_finite()
            || !self.padding_seconds.is_finite()
            || !(0.1..=30.0).contains(&self.window_seconds)
            || !(0.1..=self.window_seconds).contains(&self.hop_seconds)
            || !(0.0..=30.0).contains(&self.gap_seconds)
            || !(0.0..=10.0).contains(&self.padding_seconds)
            || !matches!(self.scope.as_str(), "speech-regions" | "full-audio")
        {
            return Err(DomainError::invalid("Arousal exige CPU1–4, batch1–8 y parámetros temporales acotados"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArousalPayload {
    pub parent: JobRecord<alignment::AlignmentPayload>,
    pub parent_result: alignment::AlignmentResult,
    pub normalized_audio_sha256: String,
    pub transcript_sha256: String,
    pub model_digest: String,
    pub model_manifest_sha256: String,
    pub worker_sha256: String,
    pub lock_sha256: String,
    pub runtime: ArousalRuntime,
    pub parameters: ArousalParameters,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArousalResult {
    pub job_id: String,
    pub project_id: String,
    pub revision: u64,
    pub project_digest: String,
    pub asset_id: String,
    pub normalized_audio_sha256: String,
    pub transcript_sha256: String,
    pub model_digest: String,
    pub manifest_sha256: String,
    pub source_sha256: String,
    pub source_duration_ticks: i64,
    pub timebase: String,
    pub arousal_path: String,
    pub artifacts: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeIdentity {
    python: String,
    packages: BTreeMap<String, String>,
    platform: String,
    worker_sha256: String,
    lock_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelFile {
    size: u64,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelCard {
    path: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelManifest {
    schema: String,
    model: String,
    revision: String,
    license: String,
    files: BTreeMap<String, ModelFile>,
    model_card: ModelCard,
    source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    schema: String,
    algorithm: String,
    runtime: RuntimeIdentity,
    model_digest: String,
    manifest_sha256: String,
    normalized_audio_sha256: String,
    transcript_sha256: String,
    source_sha256: String,
    source_duration_ticks: i64,
    timebase: String,
    execution_state: String,
    native_inference_executed: bool,
    source_verification: String,
    editorial_decisions: String,
    language_validation: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    t_ini: f64,
    t_fin: f64,
    arousal: f64,
    dominance: f64,
    valence: f64,
    rms_dbfs: f64,
    arousal_z: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    mean: f64,
    std: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observations {
    schema: String,
    model: String,
    window_seconds: f64,
    hop_seconds: f64,
    scope: String,
    analyzed_regions: Vec<[f64; 2]>,
    baseline: Baseline,
    events: Vec<Event>,
}

fn read_bound(path: &Path, maximum: u64, cancel: &AtomicBool) -> DomainResult<Vec<u8>> {
    check_cancel(cancel)?;
    if fs::metadata(path)?.len() > maximum {
        return Err(DomainError::invalid("Documento arousal excede límite"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(maximum + 1).read_to_end(&mut bytes)?;
    check_cancel(cancel)?;
    if bytes.len() as u64 > maximum {
        return Err(DomainError::invalid("Documento arousal creció durante lectura"));
    }
    Ok(bytes)
}
fn model_digest(runtime: &ArousalRuntime, cancel: &AtomicBool) -> DomainResult<String> {
    let mut hashes = BTreeMap::new();
    for (name, size, sha) in PINNED_FILES {
        let path = checked_artifact(&runtime.model, name)?;
        let actual = sha256_file(&path, cancel)?;
        if actual != sha || fs::metadata(path)?.len() != size {
            return Err(DomainError::precondition("Archivo audEERING no corresponde a revisión fijada"));
        }
        hashes.insert(name, actual);
    }
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&hashes)?)))
}
fn input_paths(payload: &ArousalPayload) -> DomainResult<(PathBuf, PathBuf)> {
    let asr = &payload.parent.payload.parent;
    let audio = format!(".work/audio/a{}.wav", payload.parent.payload.audio_index);
    if asr.result.as_ref().and_then(|result| result["artifacts"][&audio].as_str()) != Some(&payload.normalized_audio_sha256)
        || payload.normalized_audio_sha256 != payload.parent.payload.normalized_audio_sha256
        || payload.parent_result.artifacts.get(alignment::OUTPUT) != Some(&payload.transcript_sha256)
    {
        return Err(DomainError::precondition("PCM/alignment no coinciden con recibos parent"));
    }
    Ok((
        checked_artifact(&asr.payload.runtime.work_root.join(&asr.id), &audio)?,
        checked_artifact(&payload.parent.payload.runtime.work_root.join(&payload.parent.id), alignment::OUTPUT)?,
    ))
}
fn verify_inputs(payload: &ArousalPayload, cancel: &AtomicBool) -> DomainResult<()> {
    payload.parameters.validate()?;
    let parent = &payload.parent;
    crate::validate_record(parent)?;
    if parent.schema != "transcriptor-job/1"
        || parent.state != JobState::Succeeded
        || parent.result.as_ref() != Some(&serde_json::to_value(&payload.parent_result)?)
        || parent.payload_digest != tv2_domain::digest::digest_json(&serde_json::to_value(&parent.payload)?)
    {
        return Err(DomainError::precondition("Parent alignment debe ser final exacto"));
    }
    alignment::validate_result(parent, &payload.parent_result, cancel)?;
    let (audio, transcript) = input_paths(payload)?;
    if sha256_file(&audio, cancel)? != payload.normalized_audio_sha256
        || sha256_file(&transcript, cancel)? != payload.transcript_sha256
        || sha256_file(&payload.runtime.worker, cancel)? != payload.worker_sha256
        || sha256_file(&payload.runtime.lock, cancel)? != payload.lock_sha256
        || model_digest(&payload.runtime, cancel)? != payload.model_digest
    {
        return Err(DomainError::precondition("Cambió input, modelo, worker o lock de arousal"));
    }
    let bytes = read_bound(&payload.runtime.model_manifest, 1024 * 1024, cancel)?;
    if hex::encode(Sha256::digest(&bytes)) != payload.model_manifest_sha256 {
        return Err(DomainError::precondition("Manifest arousal alterado"));
    }
    let manifest: ModelManifest = serde_json::from_slice(&bytes)?;
    if manifest.schema != "tv2-arousal-model/1"
        || manifest.model != MODEL
        || manifest.revision != REVISION
        || manifest.license != "cc-by-nc-sa-4.0"
        || manifest.files.len() != PINNED_FILES.len()
        || manifest.model_card.path != "README.md"
        || manifest.model_card.sha256 != "5fe5eb9afb24ab938fb3facf0eebe64a0f8a8628eb93fd772ba5c7120a45365d"
        || manifest.source != "public pinned Hugging Face resolve; no authentication"
    {
        return Err(DomainError::precondition("Manifest/licencia audEERING inválidos"));
    }
    for (name, size, sha) in PINNED_FILES {
        let entry = manifest.files.get(name).ok_or_else(|| DomainError::precondition("Manifest sin archivo fijado"))?;
        if entry.size != size || entry.sha256 != sha {
            return Err(DomainError::precondition("Manifest no corresponde a pins audEERING"));
        }
    }
    check_cancel(cancel)
}
fn runtime_identity(payload: &ArousalPayload, identity: &RuntimeIdentity) -> DomainResult<()> {
    let mut expected = BTreeMap::new();
    for line in fs::read_to_string(&payload.runtime.lock)?.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let (name, version) = line.split_once("==").ok_or_else(|| DomainError::invalid("Lock arousal inválido"))?;
        if expected.insert(name.to_string(), version.to_string()).is_some() {
            return Err(DomainError::invalid("Distribución duplicada en lock"));
        }
    }
    if identity.python != "3.12.13"
        || identity.packages != expected
        || identity.worker_sha256 != payload.worker_sha256
        || identity.lock_sha256 != payload.lock_sha256
        || identity.platform.is_empty()
        || identity.platform.len() > 1024
    {
        return Err(DomainError::precondition("Runtime arousal no ligado a solicitud"));
    }
    Ok(())
}

pub fn enqueue(
    parent: JobRecord<alignment::AlignmentPayload>,
    parent_result: alignment::AlignmentResult,
    runtime: ArousalRuntime,
    parameters: ArousalParameters,
    cancel: &AtomicBool,
) -> DomainResult<JobRecord<ArousalPayload>> {
    let payload = ArousalPayload {
        normalized_audio_sha256: parent.payload.normalized_audio_sha256.clone(),
        transcript_sha256: parent_result.artifacts.get(alignment::OUTPUT).cloned().ok_or_else(|| DomainError::invalid("Parent sin aligned words"))?,
        model_digest: model_digest(&runtime, cancel)?,
        model_manifest_sha256: sha256_file(&runtime.model_manifest, cancel)?,
        worker_sha256: sha256_file(&runtime.worker, cancel)?,
        lock_sha256: sha256_file(&runtime.lock, cancel)?,
        parent,
        parent_result,
        runtime,
        parameters,
    };
    verify_inputs(&payload, cancel)?;
    jobs::enqueue(&payload.runtime.work_root.join("jobs"), payload.parent.project_id.clone(), payload.parent.revision, payload)
}
pub fn run(record: &JobRecord<ArousalPayload>, resume: bool, cancel: Arc<AtomicBool>, progress: impl Fn(Value)) -> DomainResult<ArousalResult> {
    crate::validate_record(record)?;
    let payload = &record.payload;
    let lease = jobs::claim::<ArousalPayload>(&payload.runtime.work_root.join("jobs"), &record.id, &record.payload_digest, resume)?;
    let result = (|| {
        verify_inputs(payload, &cancel)?;
        if record.project_id != payload.parent.project_id || record.revision != payload.parent.revision {
            return Err(DomainError::precondition("Binding de arousal difiere del parent"));
        }
        let (audio, transcript) = input_paths(payload)?;
        let asr = &payload.parent.payload.parent.payload;
        let mut worker = OwnedWorker::spawn_protocol(&payload.runtime.transport(), PROTOCOL)?;
        let operation = (|| {
            worker.send("hello", "hello", json!({}), Duration::from_secs(10), &cancel)?;
            let hello = worker.response("hello", None, Duration::from_secs(30), &cancel, &progress)?;
            if hello["protocol"] != PROTOCOL {
                return Err(DomainError::unsupported("Protocolo arousal incompatible"));
            }
            runtime_identity(payload, &serde_json::from_value(hello["runtime"].clone())?)?;
            worker.send("run", "run", json!({"job_id":record.id,"project_id":record.project_id,"revision":record.revision,"project_digest":asr.project_digest,
                "asset_id":asr.asset.id,"normalized_audio_path":audio,"normalized_audio_sha256":payload.normalized_audio_sha256,
                "transcript_path":transcript,"transcript_sha256":payload.transcript_sha256,"model_path":payload.runtime.model,"model_digest":payload.model_digest,
                "manifest_path":payload.runtime.model_manifest,"manifest_sha256":payload.model_manifest_sha256,"parameters":payload.parameters,
                "source_sha256":asr.source_sha256,"source_duration_ticks":asr.asset.duration().0,"timebase":TIMEBASE}), Duration::from_secs(10), &cancel)?;
            let raw = worker.response("run", Some(&record.id), Duration::from_secs(24 * 3600), &cancel, &progress)?;
            serde_json::from_value::<ArousalResult>(raw).map_err(Into::into)
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

fn pcm_samples(path: &Path, cancel: &AtomicBool) -> DomainResult<u64> {
    let mut input = fs::File::open(path)?;
    let size = input.metadata()?.len();
    let mut header = [0u8; 12];
    input.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err(DomainError::invalid("Arousal PCM no es WAV RIFF"));
    }
    let (mut format, mut samples) = (false, None);
    while input.stream_position()? + 8 <= size {
        check_cancel(cancel)?;
        let mut chunk = [0u8; 8];
        input.read_exact(&mut chunk)?;
        let length = u32::from_le_bytes(chunk[4..8].try_into().unwrap()) as u64;
        let start = input.stream_position()?;
        let end = start.checked_add(length).ok_or_else(|| DomainError::invalid("WAV chunk overflow"))?;
        if end > size {
            return Err(DomainError::invalid("WAV PCM truncado"));
        }
        if &chunk[..4] == b"fmt " {
            if length < 16 {
                return Err(DomainError::invalid("WAV fmt truncado"));
            }
            let mut fmt = [0u8; 16];
            input.read_exact(&mut fmt)?;
            format = u16::from_le_bytes(fmt[..2].try_into().unwrap()) == 1
                && u16::from_le_bytes(fmt[2..4].try_into().unwrap()) == 1
                && u32::from_le_bytes(fmt[4..8].try_into().unwrap()) == 16000
                && u32::from_le_bytes(fmt[8..12].try_into().unwrap()) == 32000
                && u16::from_le_bytes(fmt[12..14].try_into().unwrap()) == 2
                && u16::from_le_bytes(fmt[14..16].try_into().unwrap()) == 16;
        } else if &chunk[..4] == b"data" {
            if !length.is_multiple_of(2) {
                return Err(DomainError::invalid("WAV sample PCM incompleto"));
            }
            samples = Some(length / 2);
        }
        if format && let Some(samples) = samples {
            return Ok(samples);
        }
        input.seek(SeekFrom::Start(end + length % 2))?;
    }
    Err(DomainError::invalid("Arousal exige PCM16mono16kHz"))
}
fn rounded(value: f64, factor: f64) -> f64 {
    (value * factor).round() / factor
}
fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
    actual.is_finite() && expected.is_finite() && (actual - expected).abs() <= tolerance
}
fn word_range(word: &Value) -> DomainResult<(f64, f64)> {
    let start = word["t_ini"].as_f64().ok_or_else(|| DomainError::invalid("Palabra sin t_ini"))?;
    let end = word["t_fin"].as_f64().ok_or_else(|| DomainError::invalid("Palabra sin t_fin"))?;
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err(DomainError::invalid("Rango de palabra inválido"));
    }
    Ok((start, end))
}
fn expected_regions(words: &[Value], options: &ArousalParameters, duration: f64) -> DomainResult<Vec<[f64; 2]>> {
    if options.scope == "full-audio" {
        return Ok(vec![[0.0, rounded(duration, 1000.0)]]);
    }
    let mut ranges = words.iter().map(word_range).collect::<DomainResult<Vec<_>>>()?;
    ranges.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<[f64; 2]> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last[1] + options.gap_seconds
        {
            last[1] = last[1].max(end);
        } else {
            merged.push([start, end]);
        }
    }
    let mut padded: Vec<[f64; 2]> = Vec::new();
    for [start, end] in merged {
        let region = [(start - options.padding_seconds).max(0.0), (end + options.padding_seconds).min(duration)];
        if region[1] <= region[0] {
            continue;
        }
        if let Some(last) = padded.last_mut()
            && region[0] <= last[1]
        {
            last[1] = last[1].max(region[1]);
        } else {
            padded.push(region);
        }
    }
    Ok(padded.into_iter().map(|[start, end]| [rounded(start, 1000.0), rounded(end, 1000.0)]).collect())
}
fn validate_observations(
    observations: &Observations,
    original_words: &[Value],
    options: &ArousalParameters,
    sample_count: u64,
    cancel: &AtomicBool,
) -> DomainResult<()> {
    let duration = sample_count as f64 / 16000.0;
    if observations.schema != "editorial-arousal/1"
        || observations.model != MODEL
        || observations.scope != options.scope
        || observations.window_seconds != options.window_seconds
        || observations.hop_seconds != options.hop_seconds
    {
        return Err(DomainError::precondition("Metadata arousal no coincide con parámetros"));
    }
    let regions = expected_regions(original_words, options, duration)?;
    if regions.len() != observations.analyzed_regions.len()
        || !regions
            .iter()
            .zip(&observations.analyzed_regions)
            .all(|(expected, actual)| close(actual[0], expected[0], 0.00051) && close(actual[1], expected[1], 0.00051))
    {
        return Err(DomainError::precondition("Regiones arousal no corresponden a palabras fuente"));
    }
    let hop = (options.hop_seconds * 16000.0).round() as u64;
    let window = (options.window_seconds * 16000.0).round() as u64;
    let mut offsets = std::collections::BTreeSet::new();
    for [start, end] in &observations.analyzed_regions {
        let first = (start * 16000.0 / hop as f64).floor() as u64 * hop;
        let end = (end * 16000.0).ceil().min(sample_count as f64) as u64;
        for offset in (first..end).step_by(hop as usize) {
            offsets.insert(offset);
        }
    }
    if offsets.len() != observations.events.len() {
        return Err(DomainError::precondition("Cardinalidad de ventanas arousal alterada"));
    }
    for (offset, event) in offsets.into_iter().zip(&observations.events) {
        check_cancel(cancel)?;
        if !close(event.t_ini, offset as f64 / 16000.0, 0.00051)
            || !close(event.t_fin, (offset + window).min(sample_count) as f64 / 16000.0, 0.00051)
            || event.t_fin <= event.t_ini
            || ![event.arousal, event.dominance, event.valence, event.rms_dbfs, event.arousal_z].into_iter().all(f64::is_finite)
        {
            return Err(DomainError::invalid("Evento arousal no corresponde a ventana/fuente finita"));
        }
    }
    let (mean, std) = if observations.events.is_empty() {
        (0.0, 1.0)
    } else {
        let mean = observations.events.iter().map(|event| event.arousal).sum::<f64>() / observations.events.len() as f64;
        let std = (observations.events.iter().map(|event| (event.arousal - mean).powi(2)).sum::<f64>() / observations.events.len() as f64).sqrt();
        (mean, if std == 0.0 { 1.0 } else { std })
    };
    if !close(observations.baseline.mean, mean, 0.00000051)
        || !close(observations.baseline.std, std, 0.00000051)
        || !observations.events.iter().all(|event| close(event.arousal_z, (event.arousal - mean) / std, 0.00051))
    {
        return Err(DomainError::precondition("Baseline/arousal_z no corresponden a eventos"));
    }
    Ok(())
}

pub fn validate_result(record: &JobRecord<ArousalPayload>, result: &ArousalResult, cancel: &AtomicBool) -> DomainResult<()> {
    check_cancel(cancel)?;
    crate::validate_record(record)?;
    let payload = &record.payload;
    let asr = &payload.parent.payload.parent.payload;
    if record.schema != "transcriptor-job/1"
        || record.payload_digest != tv2_domain::digest::digest_json(&serde_json::to_value(payload)?)
        || record.project_id != payload.parent.project_id
        || record.revision != payload.parent.revision
        || result.job_id != record.id
        || result.project_id != record.project_id
        || result.revision != record.revision
        || result.project_digest != asr.project_digest
        || result.asset_id != asr.asset.id.as_str()
        || result.normalized_audio_sha256 != payload.normalized_audio_sha256
        || result.transcript_sha256 != payload.transcript_sha256
        || result.model_digest != payload.model_digest
        || result.manifest_sha256 != payload.model_manifest_sha256
        || result.source_sha256 != asr.source_sha256
        || result.source_duration_ticks != asr.asset.duration().0
        || result.timebase != TIMEBASE
        || result.arousal_path != OUTPUT
        || result.artifacts.len() != 1
        || !result.artifacts.contains_key(OUTPUT)
    {
        return Err(DomainError::precondition("Resultado arousal no ligado exactamente a solicitud"));
    }
    verify_inputs(payload, cancel)?;
    let (audio, transcript) = input_paths(payload)?;
    let sample_count = pcm_samples(&audio, cancel)?;
    if sample_count == 0 || sample_count as f64 / 16000.0 > asr.asset.duration().as_seconds_f64() + 1.0 / 16000.0 {
        return Err(DomainError::precondition("PCM arousal fuera de duración fuente"));
    }
    let bytes = read_bound(&checked_artifact(&payload.runtime.work_root.join(&record.id), OUTPUT)?, 128 * 1024 * 1024, cancel)?;
    if hex::encode(Sha256::digest(&bytes)) != result.artifacts[OUTPUT] {
        return Err(DomainError::precondition("Artefacto arousal alterado"));
    }
    let output: Value = serde_json::from_slice(&bytes)?;
    check_cancel(cancel)?;
    let original_bytes = read_bound(&transcript, 64 * 1024 * 1024, cancel)?;
    if hex::encode(Sha256::digest(&original_bytes)) != payload.transcript_sha256 {
        return Err(DomainError::precondition("Alignment cambió durante lectura arousal"));
    }
    let original: Value = serde_json::from_slice(&original_bytes)?;
    let original_words = original["words"].as_array().ok_or_else(|| DomainError::invalid("Parent sin palabras"))?;
    let words = output["words"].as_array().ok_or_else(|| DomainError::invalid("Arousal sin palabras"))?;
    if original_words.len() != words.len() {
        return Err(DomainError::precondition("Cardinalidad arousal alterada"));
    }
    let mut top = output.clone();
    let object = top.as_object_mut().ok_or_else(|| DomainError::invalid("Arousal no es objeto"))?;
    object.remove("arousal");
    object.remove("arousal_analysis");
    if let Some(value) = original.get("arousal") {
        object.insert("arousal".into(), value.clone());
    }
    if let Some(value) = original.get("arousal_analysis") {
        object.insert("arousal_analysis".into(), value.clone());
    }
    top["words"] = original["words"].clone();
    if top != original {
        return Err(DomainError::precondition("Arousal modificó parent fuera de campos permitidos"));
    }
    let metadata: Metadata = serde_json::from_value(output["arousal_analysis"].clone())?;
    let observations: Observations = serde_json::from_value(output["arousal"].clone())?;
    if metadata.schema != "tv2-arousal-words/1"
        || metadata.algorithm != ALGORITHM
        || metadata.model_digest != payload.model_digest
        || metadata.manifest_sha256 != payload.model_manifest_sha256
        || metadata.normalized_audio_sha256 != payload.normalized_audio_sha256
        || metadata.transcript_sha256 != payload.transcript_sha256
        || metadata.source_sha256 != asr.source_sha256
        || metadata.source_duration_ticks != result.source_duration_ticks
        || metadata.timebase != TIMEBASE
        || metadata.editorial_decisions != "none"
        || metadata.source_verification != "source SHA is host-bound lineage; worker verifies normalized PCM and transcript bytes"
        || metadata.language_validation != "trained on English MSP-Podcast; Spanish accuracy not accepted"
        || metadata.native_inference_executed != !observations.events.is_empty()
        || metadata.execution_state != if observations.events.is_empty() { "no_speech_windows" } else { "completed" }
    {
        return Err(DomainError::precondition("Metadata arousal no coincide con lineage/ejecución"));
    }
    runtime_identity(payload, &metadata.runtime)?;
    validate_observations(&observations, original_words, &payload.parameters, sample_count, cancel)?;
    let mut indices: Vec<_> = (0..words.len()).collect();
    indices.sort_by(|&left, &right| {
        original_words[left]["t_ini"].as_f64().unwrap_or_default().total_cmp(&original_words[right]["t_ini"].as_f64().unwrap_or_default())
    });
    let mut first = 0;
    for index in indices {
        check_cancel(cancel)?;
        let source = &original_words[index];
        let word = &words[index];
        let (start, end) = word_range(source)?;
        if end > sample_count as f64 / 16000.0 {
            return Err(DomainError::invalid("Palabra fuera de PCM arousal"));
        }
        while first < observations.events.len() && observations.events[first].t_fin <= start {
            first += 1;
        }
        let (mut weight, mut value, mut value_z) = (0.0, 0.0, 0.0);
        for event in &observations.events[first..] {
            if event.t_ini >= end {
                break;
            }
            let overlap = (end.min(event.t_fin) - start.max(event.t_ini)).max(0.0);
            weight += overlap;
            value += overlap * event.arousal;
            value_z += overlap * event.arousal_z;
        }
        let mut expected = source.clone();
        expected["arousal"] = word["arousal"].clone();
        expected["arousal_z"] = word["arousal_z"].clone();
        if expected != *word {
            return Err(DomainError::precondition("Arousal cambió ID/text/time/provenance/human fields"));
        }
        let valid = if weight == 0.0 {
            word["arousal"].is_null() && word["arousal_z"].is_null()
        } else {
            word["arousal"].as_f64().is_some_and(|actual| close(actual, value / weight, 0.00000051))
                && word["arousal_z"].as_f64().is_some_and(|actual| close(actual, value_z / weight, 0.00051))
        };
        if !valid {
            return Err(DomainError::precondition("Asociación arousal de palabra no corresponde a ventanas"));
        }
    }
    check_cancel(cancel)
}
