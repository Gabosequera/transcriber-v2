//! Finalize verified stage receipts into a new immutable editorial generation.
//! This does not replace an attached original master or bypass common commands.
use crate::{
    AnalysisPayload, AnalysisResult, ArtifactPreparation, OwnedWorker, Runtime, alignment, arousal, check_cancel, checked_artifact, laughter,
    model_digest, prepare_artifacts, sha256_file,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tv2_application::{
    jobs::{self, JobRecord, JobState},
    session::PreparedCommand,
};
use tv2_domain::{DomainError, ErrorCode, Project, error::DomainResult};

pub const PROTOCOL: &str = "tv2-finalize/1";
pub const OUTPUT: &str = "editorial/generation.editorial.master.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationRuntime {
    pub python: PathBuf,
    pub worker: PathBuf,
    pub assembler: PathBuf,
    pub derivation: PathBuf,
    pub work_root: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage<T, R> {
    pub record: JobRecord<T>,
    pub result: R,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationPayload {
    pub parent: JobRecord<AnalysisPayload>,
    pub parent_result: AnalysisResult,
    pub alignments: BTreeMap<u32, Stage<alignment::AlignmentPayload, alignment::AlignmentResult>>,
    pub arousals: BTreeMap<u32, Stage<arousal::ArousalPayload, arousal::ArousalResult>>,
    pub laughter: BTreeMap<u32, Stage<laughter::LaughterPayload, laughter::LaughterResult>>,
    pub runtime: GenerationRuntime,
    pub code_hashes: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationResult {
    pub job_id: String,
    pub project_id: String,
    pub revision: u64,
    pub project_digest: String,
    pub asset_id: String,
    pub source_sha256: String,
    pub input_digest: String,
    pub master_path: String,
    pub artifacts: BTreeMap<String, String>,
}

fn completed<T: Serialize, R: Serialize>(record: &JobRecord<T>, result: &R) -> DomainResult<()> {
    if record.schema != "transcriptor-job/1"
        || record.state != JobState::Succeeded
        || record.id.len() != 16
        || !record.id.starts_with("job-")
        || !record.id[4..].bytes().all(|b| b.is_ascii_hexdigit())
        || record.payload_digest != tv2_domain::digest::digest_json(&serde_json::to_value(&record.payload)?)
        || record.result.as_ref() != Some(&serde_json::to_value(result)?)
    {
        return Err(DomainError::precondition("La generación requiere recibos finales completos"));
    }
    Ok(())
}
fn same<T: Serialize>(left: &T, right: &T) -> DomainResult<bool> {
    Ok(serde_json::to_value(left)? == serde_json::to_value(right)?)
}
fn hashes(runtime: &GenerationRuntime, cancel: &AtomicBool) -> DomainResult<BTreeMap<String, String>> {
    [("worker", &runtime.worker), ("finalize", &runtime.assembler), ("derivation", &runtime.derivation)]
        .into_iter()
        .map(|(name, path)| Ok((name.to_owned(), sha256_file(path, cancel)?)))
        .collect()
}
fn verify_inputs(payload: &GenerationPayload, cancel: &AtomicBool) -> DomainResult<()> {
    check_cancel(cancel)?;
    completed(&payload.parent, &payload.parent_result)?;
    crate::validate_result(&payload.parent, &payload.parent_result, cancel)?;
    let parent = &payload.parent.payload;
    if sha256_file(&parent.source_path, cancel)? != parent.source_sha256
        || sha256_file(&parent.runtime.worker, cancel)? != parent.worker_sha256
        || model_digest(&parent.runtime.model, cancel)? != parent.model_digest
        || hashes(&payload.runtime, cancel)? != payload.code_hashes
    {
        return Err(DomainError::precondition("Cambió una entrada o código de la generación"));
    }
    let exists = |index| parent.asset.probe.audio.iter().any(|stream| stream.audio_index == index);
    for (index, stage) in &payload.alignments {
        completed(&stage.record, &stage.result)?;
        if !exists(*index)
            || *index != stage.record.payload.audio_index
            || !same(&stage.record.payload.parent, &payload.parent)?
            || !same(&stage.record.payload.parent_result, &payload.parent_result)?
        {
            return Err(DomainError::precondition("Alineación de otra pista o ascendencia"));
        }
        alignment::validate_result(&stage.record, &stage.result, cancel)?;
    }
    for (index, stage) in &payload.arousals {
        completed(&stage.record, &stage.result)?;
        let aligned = payload.alignments.get(index).ok_or_else(|| DomainError::precondition("Arousal requiere su alineación exacta"))?;
        if !same(&stage.record.payload.parent, &aligned.record)? || !same(&stage.record.payload.parent_result, &aligned.result)? {
            return Err(DomainError::precondition("Arousal pertenece a otra generación de palabras"));
        }
        arousal::validate_result(&stage.record, &stage.result, cancel)?;
    }
    for (index, stage) in &payload.laughter {
        completed(&stage.record, &stage.result)?;
        if !exists(*index)
            || *index != stage.record.payload.audio_index
            || !same(&stage.record.payload.parent, &payload.parent)?
            || !same(&stage.record.payload.parent_result, &payload.parent_result)?
        {
            return Err(DomainError::precondition("Risas de otra pista o ascendencia"));
        }
        laughter::validate_result(&stage.record, &stage.result, cancel)?;
    }
    Ok(())
}

pub fn enqueue(mut payload: GenerationPayload, cancel: &AtomicBool) -> DomainResult<JobRecord<GenerationPayload>> {
    payload.code_hashes = hashes(&payload.runtime, cancel)?;
    verify_inputs(&payload, cancel)?;
    jobs::enqueue(&payload.runtime.work_root.join("jobs"), payload.parent.project_id.clone(), payload.parent.revision, payload)
}
fn artifact(root: &std::path::Path, name: &str, artifacts: &BTreeMap<String, String>) -> DomainResult<Value> {
    let sha = artifacts.get(name).ok_or_else(|| DomainError::precondition("Falta artefacto declarado"))?;
    Ok(json!({"path":checked_artifact(root,name)?,"sha256":sha}))
}
fn request(record: &JobRecord<GenerationPayload>) -> DomainResult<Value> {
    let payload = &record.payload;
    let parent = &payload.parent.payload;
    let root = parent.runtime.work_root.join(&payload.parent.id);
    let mut tracks = BTreeMap::new();
    for stream in &parent.asset.probe.audio {
        let index = stream.audio_index;
        let aligned = payload
            .alignments
            .get(&index)
            .map(|stage| {
                artifact(&stage.record.payload.runtime.work_root.join(&stage.record.id), &stage.result.alignment_path, &stage.result.artifacts)
            })
            .transpose()?;
        let arousal = payload
            .arousals
            .get(&index)
            .map(|stage| {
                artifact(&stage.record.payload.runtime.work_root.join(&stage.record.id), &stage.result.arousal_path, &stage.result.artifacts)
            })
            .transpose()?;
        let laughter = payload
            .laughter
            .get(&index)
            .map(|stage| {
                artifact(&stage.record.payload.runtime.work_root.join(&stage.record.id), &stage.result.laughter_path, &stage.result.artifacts)
            })
            .transpose()?;
        tracks.insert(format!("a{index}"),json!({"audio":artifact(&root,&format!(".work/audio/a{index}.wav"),&payload.parent_result.artifacts)?,"alignment":aligned,"arousal":arousal,"laughter":laughter}));
    }
    Ok(json!({"job_id":record.id,"project_id":record.project_id,"revision":record.revision,"project_digest":parent.project_digest,
        "asset_id":parent.asset.id,"source_sha256":parent.source_sha256,"input_digest":record.payload_digest,
        "base_master":artifact(&root,&payload.parent_result.master_path,&payload.parent_result.artifacts)?,"tracks":tracks,
        "modules":{"finalize":{"path":payload.runtime.assembler,"sha256":payload.code_hashes["finalize"]},
                   "derivation":{"path":payload.runtime.derivation,"sha256":payload.code_hashes["derivation"]}},
        "lineage":serde_json::to_value(payload)?}))
}

pub fn run(record: &JobRecord<GenerationPayload>, resume: bool, cancel: Arc<AtomicBool>, progress: impl Fn(Value)) -> DomainResult<GenerationResult> {
    super::validate_record(record)?;
    let payload = &record.payload;
    let lease = jobs::claim::<GenerationPayload>(&payload.runtime.work_root.join("jobs"), &record.id, &record.payload_digest, resume)?;
    let outcome = (|| {
        verify_inputs(payload, &cancel)?;
        let runtime = Runtime {
            python: payload.runtime.python.clone(),
            worker: payload.runtime.worker.clone(),
            model: PathBuf::new(),
            work_root: payload.runtime.work_root.clone(),
        };
        let mut worker = OwnedWorker::spawn_protocol(&runtime, PROTOCOL)?;
        worker.send("hello", "hello", json!({}), Duration::from_secs(10), &cancel)?;
        let hello = worker.response("hello", None, Duration::from_secs(30), &cancel, &progress)?;
        if hello != json!({"protocol":PROTOCOL,"stdlib_only":true,"python":"3.12.13"}) {
            return Err(DomainError::unsupported("Finalizador incompatible"));
        }
        worker.send("run", "run", request(record)?, Duration::from_secs(10), &cancel)?;
        let raw = worker.response("run", Some(&record.id), Duration::from_secs(24 * 3600), &cancel, &progress);
        if cancel.load(Ordering::Acquire) {
            let _ = worker.send("cancel", "cancel", json!({"job_id":record.id}), Duration::from_millis(250), &AtomicBool::new(false));
        }
        worker.close();
        let result: GenerationResult = serde_json::from_value(raw?)?;
        validate_result(record, &result, &cancel)?;
        Ok(result)
    })();
    let state = match &outcome {
        Ok(_) => JobState::Succeeded,
        Err(error) if error.code == ErrorCode::Cancelled => JobState::Cancelled,
        Err(_) => JobState::Failed,
    };
    lease.finish(state, outcome.as_ref().ok().map(serde_json::to_value).transpose()?, outcome.as_ref().err().map(ToString::to_string))?;
    outcome
}

pub fn validate_result(record: &JobRecord<GenerationPayload>, result: &GenerationResult, cancel: &AtomicBool) -> DomainResult<()> {
    super::validate_record(record)?;
    verify_inputs(&record.payload, cancel)?;
    let parent = &record.payload.parent.payload;
    if record.payload_digest != tv2_domain::digest::digest_json(&serde_json::to_value(&record.payload)?)
        || record.project_id != record.payload.parent.project_id
        || record.revision != record.payload.parent.revision
        || result.job_id != record.id
        || result.project_id != record.project_id
        || result.revision != record.revision
        || result.project_digest != parent.project_digest
        || result.asset_id != parent.asset.id.as_str()
        || result.source_sha256 != parent.source_sha256
        || result.input_digest != record.payload_digest
        || result.master_path != OUTPUT
        || result.artifacts.len() != 1
        || !result.artifacts.contains_key(OUTPUT)
    {
        return Err(DomainError::precondition("Recibo de generación no corresponde a sus entradas"));
    }
    let path = checked_artifact(&record.payload.runtime.work_root.join(&record.id), OUTPUT)?;
    let bytes = bounded_document(path)?;
    check_cancel(cancel)?;
    use sha2::Digest;
    if hex::encode(sha2::Sha256::digest(&bytes)) != result.artifacts[OUTPUT] {
        return Err(DomainError::precondition("Master de generación alterado"));
    }
    let raw: Value = serde_json::from_slice(&bytes)?;
    let generation = &raw["analysis"]["generation"];
    let requested = request(record)?;
    if generation["protocol"] != PROTOCOL
        || generation["job_id"] != record.id
        || generation["input_digest"] != record.payload_digest
        || generation["lineage"] != serde_json::to_value(&record.payload)?
        || generation["modules"] != requested["modules"]
        || generation["editorial_decisions"] != "none"
    {
        return Err(DomainError::precondition("Master sin ascendencia exacta"));
    }
    validate_projection(&record.payload, &raw, cancel)?;
    let master = tv2_v1compat::master::V1Master::parse(raw)?;
    if !master.fingerprint.same_identity(&parent.asset.fingerprint) || master.duration != parent.asset.duration() {
        return Err(DomainError::precondition("Master de otro medio"));
    }
    verify_derivation(record, result, requested, cancel)?;
    Ok(())
}

/// Independently rederive deterministic observations from the bound inputs.
/// The verification worker ignores publication checkpoints and writes nothing.
/// No model inference is involved, and this runs outside the GUI apply command.
fn verify_derivation(record: &JobRecord<GenerationPayload>, result: &GenerationResult, requested: Value, cancel: &AtomicBool) -> DomainResult<()> {
    check_cancel(cancel)?;
    let runtime = &record.payload.runtime;
    let transport =
        Runtime { python: runtime.python.clone(), worker: runtime.worker.clone(), model: PathBuf::new(), work_root: runtime.work_root.clone() };
    let mut worker = OwnedWorker::spawn_protocol(&transport, PROTOCOL)?;
    worker.send("hello", "hello", json!({}), Duration::from_secs(10), cancel)?;
    let hello = worker.response("hello", None, Duration::from_secs(30), cancel, &|_| {})?;
    if hello != json!({"protocol":PROTOCOL,"stdlib_only":true,"python":"3.12.13"}) {
        return Err(DomainError::unsupported("Verificador de generación incompatible"));
    }
    let params = json!({"request":requested,"master":artifact(&runtime.work_root.join(&record.id), OUTPUT, &result.artifacts)?});
    worker.send("verify", "verify", params, Duration::from_secs(10), cancel)?;
    let response = worker.response("verify", Some(&record.id), Duration::from_secs(24 * 3600), cancel, &|_| {});
    if cancel.load(Ordering::Acquire) {
        let _ = worker.send("cancel", "cancel", json!({"job_id":record.id}), Duration::from_millis(250), &AtomicBool::new(false));
    }
    worker.close();
    if response? != json!({"verified":true,"sha256":result.artifacts[OUTPUT]}) {
        return Err(DomainError::precondition("Respuesta de verificación de generación inválida"));
    }
    // Inputs, including executed source code, may have changed while verifying.
    verify_inputs(&record.payload, cancel)?;
    if sha256_file(&checked_artifact(&runtime.work_root.join(&record.id), OUTPUT)?, cancel)? != result.artifacts[OUTPUT] {
        return Err(DomainError::precondition("Master cambió durante verificación"));
    }
    Ok(())
}

fn document(root: &std::path::Path, name: &str, artifacts: &BTreeMap<String, String>, cancel: &AtomicBool) -> DomainResult<Value> {
    use sha2::Digest;
    let bytes = bounded_document(checked_artifact(root, name)?)?;
    check_cancel(cancel)?;
    if artifacts.get(name) != Some(&hex::encode(sha2::Sha256::digest(&bytes))) {
        return Err(DomainError::precondition("Artefacto cambió durante lectura de generación"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}
fn bounded_document(path: PathBuf) -> DomainResult<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(DomainError::invalid("Documento de generación supera 128 MiB"));
    }
    Ok(bytes)
}
fn without(value: &Value, fields: &[&str]) -> DomainResult<Value> {
    let mut object = value.as_object().ok_or_else(|| DomainError::invalid("Se esperaba objeto de evidencia"))?.clone();
    for field in fields {
        object.remove(*field);
    }
    Ok(Value::Object(object))
}
fn preserve_unknown(before: Option<&Value>, after: Option<&Value>, generated: &[&str]) -> DomainResult<()> {
    let empty = json!({});
    if without(before.unwrap_or(&empty), generated)? != without(after.unwrap_or(&empty), generated)? {
        return Err(DomainError::precondition("Cambió una extensión ajena a los campos derivados"));
    }
    Ok(())
}
fn validate_projection(payload: &GenerationPayload, output: &Value, cancel: &AtomicBool) -> DomainResult<()> {
    let base = document(
        &payload.parent.payload.runtime.work_root.join(&payload.parent.id),
        &payload.parent_result.master_path,
        &payload.parent_result.artifacts,
        cancel,
    )?;
    if output["asr_original"] != base
        || without(output, &["tracks", "conversation", "analysis", "asr_original", "generated_at"])?
            != without(&base, &["tracks", "conversation", "analysis", "asr_original", "generated_at"])?
    {
        return Err(DomainError::precondition("La generación cambió el original o campos ajenos al análisis"));
    }
    preserve_unknown(base.get("analysis"), output.get("analysis"), &["completed_steps", "unavailable_steps", "finalization", "generation"])?;
    preserve_unknown(
        base.get("conversation"),
        output.get("conversation"),
        &["utterances", "clean_utterance_ids", "overlap_groups", "duplicate_groups"],
    )?;
    let empty = json!({});
    for (field, generated) in [
        ("generation", &["protocol", "job_id", "input_digest", "lineage", "modules", "editorial_decisions"][..]),
        (
            "finalization",
            &[
                "algorithm",
                "base_master_digest",
                "tracks",
                "derivation_algorithm",
                "source_sha256",
                "editorial_decisions",
                "inference_executed_by_finalizer",
            ][..],
        ),
    ] {
        preserve_unknown(base.get("analysis").unwrap_or(&empty).get(field), output.get("analysis").unwrap_or(&empty).get(field), generated)?;
    }
    let original_tracks = base["tracks"].as_object().ok_or_else(|| DomainError::invalid("Master base sin pistas"))?;
    let final_tracks = output["tracks"].as_object().ok_or_else(|| DomainError::invalid("Generación sin pistas"))?;
    if original_tracks.keys().ne(final_tracks.keys()) {
        return Err(DomainError::precondition("Cambió el inventario de pistas"));
    }
    for (track_id, original) in original_tracks {
        let index: u32 =
            track_id.strip_prefix('a').and_then(|suffix| suffix.parse().ok()).ok_or_else(|| DomainError::invalid("Pista de análisis inválida"))?;
        let final_track = &final_tracks[track_id];
        let allowed_track = [
            "words",
            "utterances",
            "baselines",
            "intensity",
            "heuristics",
            "alignment",
            "arousal_analysis",
            "arousal_windows",
            "arousal",
            "laughter",
            "laughter_analysis",
        ];
        if without(original, &allowed_track)? != without(final_track, &allowed_track)? {
            return Err(DomainError::precondition("Cambió una extensión de pista"));
        }
        preserve_unknown(original.get("heuristics"), final_track.get("heuristics"), &["pauses", "instructions", "provenance"])?;
        preserve_unknown(
            original.get("intensity"),
            final_track.get("intensity"),
            &["schema", "sample_rate", "local_context_seconds", "baseline", "duration_baseline", "events"],
        )?;
        preserve_unknown(
            original.get("heuristics").unwrap_or(&empty).get("provenance"),
            final_track.get("heuristics").unwrap_or(&empty).get("provenance"),
            &[
                "algorithm",
                "timestamp_basis",
                "editorial_decisions",
                "normalized_audio_sha256",
                "finalization_algorithm",
                "base_master_digest",
                "source_transcript_sha256",
            ],
        )?;
        let aligned = payload
            .alignments
            .get(&index)
            .map(|stage| {
                document(
                    &stage.record.payload.runtime.work_root.join(&stage.record.id),
                    &stage.result.alignment_path,
                    &stage.result.artifacts,
                    cancel,
                )
            })
            .transpose()?;
        let arousal = payload
            .arousals
            .get(&index)
            .map(|stage| {
                document(&stage.record.payload.runtime.work_root.join(&stage.record.id), &stage.result.arousal_path, &stage.result.artifacts, cancel)
            })
            .transpose()?;
        let authority = arousal.as_ref().or(aligned.as_ref()).unwrap_or(original);
        if let Some(aligned) = &aligned {
            if final_track["alignment"] != aligned["alignment"] {
                return Err(DomainError::precondition("Metadata de alineación cambiada"));
            }
        } else if final_track.get("alignment") != original.get("alignment") {
            return Err(DomainError::precondition("Alineación no ejecutada introducida en generación"));
        }
        if let Some(arousal) = &arousal {
            let windows = &arousal["arousal"];
            let mut events = windows["events"].as_array().ok_or_else(|| DomainError::invalid("Arousal sin eventos"))?.clone();
            for (index, event) in events.iter_mut().enumerate() {
                event["event_id"] = json!(format!("{track_id}-arousal-{:06}", index + 1));
                event["track_id"] = json!(track_id);
            }
            if final_track["arousal_windows"] != *windows
                || final_track["arousal"] != json!(events)
                || final_track["arousal_analysis"] != arousal["arousal_analysis"]
                || final_track["baselines"]["arousal"] != windows["baseline"]
            {
                return Err(DomainError::precondition("Eventos/metadata de arousal cambiados"));
            }
        } else {
            for field in ["arousal", "arousal_windows", "arousal_analysis"] {
                if final_track.get(field) != original.get(field) {
                    return Err(DomainError::precondition("Arousal no ejecutado introducido en generación"));
                }
            }
        }
        if let Some(stage) = payload.laughter.get(&index) {
            let laughter = document(
                &stage.record.payload.runtime.work_root.join(&stage.record.id),
                &stage.result.laughter_path,
                &stage.result.artifacts,
                cancel,
            )?;
            if final_track["laughter"] != laughter["events"] || final_track["laughter_analysis"] != laughter["laughter"] {
                return Err(DomainError::precondition("Eventos/metadata de risas cambiados"));
            }
        } else {
            for field in ["laughter", "laughter_analysis"] {
                if final_track.get(field) != original.get(field) {
                    return Err(DomainError::precondition("Risas no ejecutadas introducidas en generación"));
                }
            }
        }
        let empty_baselines = json!({});
        if without(original.get("baselines").unwrap_or(&empty_baselines), &["intensity", "arousal"])?
            != without(final_track.get("baselines").unwrap_or(&empty_baselines), &["intensity", "arousal"])?
        {
            return Err(DomainError::precondition("Cambió baseline ajeno a derivación"));
        }
        let source_words = original["words"].as_array().ok_or_else(|| DomainError::invalid("Pista base sin palabras"))?;
        let final_words = final_track["words"].as_array().ok_or_else(|| DomainError::invalid("Pista final sin palabras"))?;
        let authority_words = authority["words"].as_array().ok_or_else(|| DomainError::invalid("Etapa sin palabras"))?;
        if source_words.len() != final_words.len() || source_words.len() != authority_words.len() {
            return Err(DomainError::precondition("Cambió el inventario de palabras"));
        }
        for ((before, after), timed) in source_words.iter().zip(final_words).zip(authority_words) {
            check_cancel(cancel)?;
            let mut allowed = vec![
                "t_ini",
                "t_fin",
                "alignment_source",
                "arousal",
                "arousal_z",
                "rms_dbfs",
                "peak_dbfs",
                "local_floor_dbfs",
                "local_contrast_db",
                "intensity_z",
                "emphasis_score",
            ];
            if before.get("asr_prob").is_none() {
                allowed.push("asr_prob");
            }
            if without(before, &allowed)? != without(after, &allowed)?
                || before["word_id"] != timed["word_id"]
                || after["t_ini"] != timed["t_ini"]
                || after["t_fin"] != timed["t_fin"]
            {
                return Err(DomainError::precondition("La generación cambió identidad, texto, probabilidad o tiempos verificados"));
            }
            for field in ["alignment_source", "arousal", "arousal_z"] {
                if after.get(field) != timed.get(field) {
                    return Err(DomainError::precondition("Señales por palabra no corresponden a la etapa"));
                }
            }
        }
        let before_utterances = original["utterances"].as_array().ok_or_else(|| DomainError::invalid("Pista base sin intervenciones"))?;
        let after_utterances = final_track["utterances"].as_array().ok_or_else(|| DomainError::invalid("Pista final sin intervenciones"))?;
        let by_id = final_words
            .iter()
            .map(|word| Ok((word["word_id"].as_str().ok_or_else(|| DomainError::invalid("ID de palabra inválido"))?, word)))
            .collect::<DomainResult<BTreeMap<_, _>>>()?;
        if by_id.len() != final_words.len() {
            return Err(DomainError::invalid("ID de palabra duplicado"));
        }
        if before_utterances.len() != after_utterances.len() {
            return Err(DomainError::precondition("Cambió inventario de intervenciones"));
        }
        for (before, after) in before_utterances.iter().zip(after_utterances) {
            let allowed = ["t_ini", "t_fin", "signals", "overlap_group", "duplicate_group", "duplicate_secondary"];
            if without(before, &allowed)? != without(after, &allowed)? {
                return Err(DomainError::precondition("Cambió identidad/texto/extensiones de intervención"));
            }
            let empty = json!({});
            if without(before.get("signals").unwrap_or(&empty), &["intensity_z_mean", "emphasis_max"])?
                != without(after.get("signals").unwrap_or(&empty), &["intensity_z_mean", "emphasis_max"])?
            {
                return Err(DomainError::precondition("Cambió señal ajena a la derivación"));
            }
            let ids = before["word_ids"].as_array().ok_or_else(|| DomainError::invalid("Intervención sin referencias de palabras"))?;
            if !ids.is_empty() {
                let mut start = f64::INFINITY;
                let mut end = f64::NEG_INFINITY;
                for id in ids {
                    let word = by_id
                        .get(id.as_str().ok_or_else(|| DomainError::invalid("Referencia de palabra inválida"))?)
                        .ok_or_else(|| DomainError::invalid("Palabra de intervención inexistente"))?;
                    start = start.min(word["t_ini"].as_f64().ok_or_else(|| DomainError::invalid("Inicio de palabra inválido"))?);
                    end = end.max(word["t_fin"].as_f64().ok_or_else(|| DomainError::invalid("Fin de palabra inválido"))?);
                }
                if after["t_ini"].as_f64() != Some(start) || after["t_fin"].as_f64() != Some(end) {
                    return Err(DomainError::precondition("Intervención no corresponde a sus palabras finales"));
                }
            }
        }
    }
    Ok(())
}

pub fn prepare_result(
    snapshot: Project,
    record: &JobRecord<GenerationPayload>,
    result: &GenerationResult,
    cancel: &AtomicBool,
    explicit_rebase: bool,
) -> DomainResult<PreparedCommand> {
    completed(record, result)?;
    validate_result(record, result, cancel)?;
    let parent = &record.payload.parent.payload;
    prepare_artifacts(
        snapshot,
        ArtifactPreparation {
            project_id: &record.project_id,
            revision: record.revision,
            project_digest: &parent.project_digest,
            asset: &parent.asset,
            source_path: &parent.source_path,
            source_sha256: &parent.source_sha256,
            job_id: &record.id,
            root: &record.payload.runtime.work_root.join(&record.id),
            master_path: &result.master_path,
            artifacts: &result.artifacts,
        },
        cancel,
        explicit_rebase,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &std::path::Path) -> (GenerationPayload, Value) {
        let asset = tv2_domain::commands::tests_support::fake_video("generation-source", 10);
        let project = Project::new("Invented generation contract");
        let base = json!({"schema":"editorial-master/1","chunks":[],"media":{"duration":10.0,"fingerprint":asset.fingerprint},
            "tracks":{"a0":{"label":"Original","words":[{"word_id":"a0-w-1","track_id":"a0","text":"invented","probability":0.7,"t_ini":1.0,"t_fin":2.0,"extension":{"keep":true}}],
            "utterances":[{"utterance_id":"a0-u-1","word_ids":["a0-w-1"],"text":"invented","t_ini":1.0,"t_fin":2.0,"signals":{"external":"keep"}}],
            "heuristics":{"human_extension":"keep","provenance":{"human_extension":{"keep":true}}}}},
            "analysis":{"lineage":{"source_sha256":"invented"},"human_extension":"keep","generation":{"human_extension":"keep"}},
            "conversation":{"human_extension":{"keep":true}}});
        let parent = jobs::enqueue(
            &root.join("jobs"),
            project.project_id,
            0,
            AnalysisPayload {
                project_digest: "fixture".into(),
                asset,
                source_path: root.join("source"),
                source_sha256: "fixture".into(),
                model_digest: "fixture".into(),
                worker_sha256: "fixture".into(),
                runtime: Runtime { python: root.join("unused"), worker: root.join("unused"), model: root.join("unused"), work_root: root.to_owned() },
                parameters: Default::default(),
                ffmpeg_path: root.join("unused"),
            },
        )
        .unwrap();
        let job_root = root.join(&parent.id);
        fs::create_dir_all(&job_root).unwrap();
        fs::write(job_root.join("master.json"), serde_json::to_vec(&base).unwrap()).unwrap();
        let parent_result = AnalysisResult {
            job_id: parent.id.clone(),
            project_id: parent.project_id.clone(),
            revision: 0,
            project_digest: "fixture".into(),
            source_sha256: "fixture".into(),
            model_digest: "fixture".into(),
            master_path: "master.json".into(),
            artifacts: BTreeMap::from([("master.json".into(), sha256_file(&job_root.join("master.json"), &AtomicBool::new(false)).unwrap())]),
        };
        let payload = GenerationPayload {
            parent,
            parent_result,
            alignments: BTreeMap::new(),
            arousals: BTreeMap::new(),
            laughter: BTreeMap::new(),
            code_hashes: BTreeMap::new(),
            runtime: GenerationRuntime {
                python: root.join("unused"),
                worker: root.join("unused"),
                assembler: root.join("unused"),
                derivation: root.join("unused"),
                work_root: root.join("generation"),
            },
        };
        let mut output = base.clone();
        output["asr_original"] = base;
        (payload, output)
    }
    #[test]
    fn generation_preserves_word_identity_probability_and_extensions() {
        let root = tempfile::tempdir().unwrap();
        let (payload, original) = fixture(root.path());
        let cancel = AtomicBool::new(false);
        validate_projection(&payload, &original, &cancel).unwrap();
        for (field, value) in [
            ("text", json!("forged")),
            ("word_id", json!("other")),
            ("probability", json!(0.99)),
            ("extension", json!({})),
            ("t_ini", json!(0.5)),
            ("arousal", json!(0.9)),
        ] {
            let mut changed = original.clone();
            changed["tracks"]["a0"]["words"][0][field] = value;
            assert!(validate_projection(&payload, &changed, &cancel).is_err(), "{field}");
        }
    }
    #[test]
    fn generation_rejects_forged_utterance_time_signal_and_original() {
        let root = tempfile::tempdir().unwrap();
        let (payload, original) = fixture(root.path());
        let cancel = AtomicBool::new(false);
        let mut changed = original.clone();
        changed["tracks"]["a0"]["utterances"][0]["t_fin"] = json!(3.0);
        assert!(validate_projection(&payload, &changed, &cancel).is_err());
        let mut changed = original.clone();
        changed["tracks"]["a0"]["utterances"][0]["signals"]["external"] = json!("lost");
        assert!(validate_projection(&payload, &changed, &cancel).is_err());
        let mut changed = original;
        changed["asr_original"]["tracks"] = json!({});
        assert!(validate_projection(&payload, &changed, &cancel).is_err());
    }
    #[test]
    fn generation_rejects_changed_or_missing_human_extensions_in_derived_objects() {
        let root = tempfile::tempdir().unwrap();
        let (payload, original) = fixture(root.path());
        let cancel = AtomicBool::new(false);
        validate_projection(&payload, &original, &cancel).unwrap();
        for pointer in [
            "/analysis/lineage/source_sha256",
            "/analysis/human_extension",
            "/analysis/generation/human_extension",
            "/conversation/human_extension",
            "/tracks/a0/heuristics/human_extension",
            "/tracks/a0/heuristics/provenance/human_extension",
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = json!("forged");
            assert!(validate_projection(&payload, &changed, &cancel).is_err(), "{pointer}");
        }
        let mut changed = original.clone();
        changed["tracks"]["a0"]["heuristics"].as_object_mut().unwrap().remove("human_extension");
        assert!(validate_projection(&payload, &changed, &cancel).is_err());
        let mut changed = original;
        changed["analysis"]["unapproved_extension"] = json!(true);
        assert!(validate_projection(&payload, &changed, &cancel).is_err());
    }
}
