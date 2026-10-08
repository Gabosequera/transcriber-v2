//! Optional Python is loaded only by explicit analysis; all writes to the
//! project still go through ProjectSession::prepare_command/commit_prepared.
pub mod alignment;
pub mod arousal;
pub mod editorial;
pub mod generation;
pub mod laughter;
mod process_tree;
pub mod workflow;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tv2_application::{
    Actor, CommandEnvelope, ProjectSession,
    jobs::{self, JobRecord, JobState},
    session::PreparedCommand,
};
use tv2_domain::{Asset, DomainError, ErrorCode, Project, error::DomainResult};

pub const PROTOCOL: &str = "tv2-worker/1";
const MAX_LINE: usize = 1024 * 1024;
type WriteRequest = (Vec<u8>, crossbeam_channel::Sender<DomainResult<()>>);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Runtime {
    pub python: PathBuf,
    pub worker: PathBuf,
    pub model: PathBuf,
    pub work_root: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parameters {
    pub language: String,
    pub device: String,
    pub threads: u32,
    pub beam_size: u32,
    pub steps: Vec<String>,
}
impl Default for Parameters {
    fn default() -> Self {
        Self { language: "es".into(), device: "cpu".into(), threads: 2, beam_size: 5, steps: vec!["extract".into(), "transcribe".into()] }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisPayload {
    pub project_digest: String,
    pub asset: Asset,
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub model_digest: String,
    pub worker_sha256: String,
    pub runtime: Runtime,
    pub parameters: Parameters,
    pub ffmpeg_path: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub job_id: String,
    pub project_id: String,
    pub revision: u64,
    pub project_digest: String,
    pub source_sha256: String,
    pub model_digest: String,
    pub master_path: String,
    pub artifacts: BTreeMap<String, String>,
}

pub fn sha256_file(path: &Path, cancel: &AtomicBool) -> DomainResult<String> {
    let mut input = fs::File::open(path)?;
    let mut sha = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    Ok(hex::encode(sha.finalize()))
}
pub fn model_digest(path: &Path, cancel: &AtomicBool) -> DomainResult<String> {
    let mut files = BTreeMap::new();
    for name in ["config.json", "model.bin", "tokenizer.json", "vocabulary.txt"] {
        files.insert(name, sha256_file(&path.join(name), cancel)?);
    }
    Ok(tv2_domain::digest::digest_json(&json!(files)))
}
fn check_cancel(cancel: &AtomicBool) -> DomainResult<()> {
    if cancel.load(Ordering::Acquire) { Err(DomainError::new(ErrorCode::Cancelled, "Análisis cancelado")) } else { Ok(()) }
}

fn validate_record<T: Serialize>(record: &JobRecord<T>) -> DomainResult<()> {
    if record.schema != "transcriptor-job/1"
        || record.id.len() != 16
        || !record.id.starts_with("job-")
        || !record.id[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
        || record.payload_digest != tv2_domain::digest::digest_json(&serde_json::to_value(&record.payload)?)
    {
        return Err(DomainError::precondition("El payload del job cambió fuera de su recibo"));
    }
    Ok(())
}

pub fn enqueue(
    snapshot: &ProjectSession,
    asset: Asset,
    source_path: PathBuf,
    runtime: Runtime,
    parameters: Parameters,
    tools: &tv2_media::FfmpegTools,
    cancel: &Arc<AtomicBool>,
) -> DomainResult<JobRecord<AnalysisPayload>> {
    if asset.missing || asset.probe.audio.is_empty() {
        return Err(DomainError::unsupported("El medio no tiene audio disponible"));
    }
    if parameters.device != "cpu" || !(1..=4).contains(&parameters.threads) || !(1..=10).contains(&parameters.beam_size) {
        return Err(DomainError::invalid("Este incremento admite CPU, 1–4 hilos y beam 1–10"));
    }
    if parameters.steps != ["extract", "transcribe"] {
        return Err(DomainError::unsupported("Alineación y señales aún no están integradas en este worker"));
    }
    let actual = tools.import_cancellable(&source_path, source_path.to_string_lossy().into(), cancel.clone())?;
    if !actual.fingerprint.same_identity(&asset.fingerprint) {
        return Err(DomainError::precondition("Cambió el medio antes de iniciar análisis"));
    }
    let payload = AnalysisPayload {
        project_digest: snapshot.digest()?,
        source_sha256: sha256_file(&source_path, cancel)?,
        model_digest: model_digest(&runtime.model, cancel)?,
        worker_sha256: sha256_file(&runtime.worker, cancel)?,
        asset,
        source_path,
        runtime,
        parameters,
        ffmpeg_path: tools.ffmpeg.clone(),
    };
    check_cancel(cancel)?;
    jobs::enqueue(&payload.runtime.work_root.join("jobs"), snapshot.project().project_id.clone(), snapshot.revision(), payload)
}

struct OwnedWorker {
    protocol: &'static str,
    child: Child,
    writes: crossbeam_channel::Sender<WriteRequest>,
    lines: crossbeam_channel::Receiver<DomainResult<Value>>,
    _tree: process_tree::ProcessTree,
}
impl OwnedWorker {
    fn spawn(runtime: &Runtime) -> DomainResult<Self> {
        Self::spawn_protocol(runtime, PROTOCOL)
    }
    fn spawn_protocol(runtime: &Runtime, protocol: &'static str) -> DomainResult<Self> {
        let mut command = Command::new(&runtime.python);
        command
            .args(["-I", "-u"])
            .arg(&runtime.worker)
            .arg("--work-root")
            .arg(&runtime.work_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("HF_HUB_DISABLE_IMPLICIT_TOKEN", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1");
        for secret in ["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY"] {
            command.env_remove(secret);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Assign the launcher to the owned job before it can create its
            // interpreter or any other descendants.
            command.creation_flags(0x08000000 | 0x00000004);
        }
        let mut child = command.spawn().map_err(|e| DomainError::process(format!("No se pudo abrir Python: {e}")))?;
        let tree = match process_tree::ProcessTree::attach(&child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(DomainError::process(format!("No se pudo supervisar árbol Python: {error}")));
            }
        };
        #[cfg(windows)]
        if let Err(error) = tree.resume_suspended(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DomainError::process(format!("No se pudo iniciar el worker supervisado: {error}")));
        }
        let mut input = child.stdin.take().unwrap();
        let (writes, requests) = crossbeam_channel::bounded::<WriteRequest>(1);
        std::thread::spawn(move || {
            while let Ok((bytes, done)) = requests.recv() {
                let result = input.write_all(&bytes).and_then(|_| input.flush()).map_err(DomainError::from);
                let failed = result.is_err();
                let _ = done.send(result);
                if failed {
                    break;
                }
            }
        });
        let output = child.stdout.take().unwrap();
        let errors = child.stderr.take().unwrap();
        let (tx, lines) = crossbeam_channel::bounded(128);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let read = reader.by_ref().take((MAX_LINE + 1) as u64).read_until(b'\n', &mut bytes);
                let value = match read {
                    Ok(0) => break,
                    Ok(_) if bytes.len() <= MAX_LINE && bytes.last() == Some(&b'\n') => {
                        serde_json::from_slice(&bytes).map_err(|e| DomainError::process(format!("Protocolo Python inválido: {e}")))
                    }
                    Ok(_) => Err(DomainError::process("Respuesta Python excesiva o sin frontera NDJSON")),
                    Err(e) => Err(e.into()),
                };
                let stop = value.is_err();
                if tx.send(value).is_err() || stop {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = errors;
            let mut tail = Vec::new();
            let mut buffer = [0; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                tail.extend_from_slice(&buffer[..count]);
                if tail.len() > 16384 {
                    tail.drain(..tail.len() - 16384);
                }
            }
            if !tail.is_empty() {
                tracing::warn!("Worker Python stderr: {}", String::from_utf8_lossy(&tail));
            }
        });
        Ok(Self { protocol, child, writes, lines, _tree: tree })
    }
    fn send(&self, id: &str, method: &str, params: Value, timeout: Duration, cancel: &AtomicBool) -> DomainResult<()> {
        check_cancel(cancel)?;
        let mut bytes = serde_json::to_vec(&json!({"protocol":self.protocol,"id":id,"method":method,"params":params}))?;
        if bytes.len() >= MAX_LINE {
            return Err(DomainError::invalid("Petición Python excesiva"));
        }
        bytes.push(b'\n');
        let (done, completion) = crossbeam_channel::bounded(1);
        self.writes.try_send((bytes, done)).map_err(|_| DomainError::process("Escritura Python pendiente o cerrada"))?;
        let deadline = Instant::now() + timeout;
        loop {
            check_cancel(cancel)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(DomainError::process("Python no consumió la petición dentro del plazo"));
            }
            match completion.recv_timeout(remaining.min(Duration::from_millis(50))) {
                Ok(result) => return result,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(DomainError::process("Escritura Python cerró sin completar")),
            }
        }
    }
    fn response(&self, id: &str, job: Option<&str>, timeout: Duration, cancel: &AtomicBool, progress: &impl Fn(Value)) -> DomainResult<Value> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            check_cancel(cancel)?;
            match self.lines.recv_timeout(Duration::from_millis(50)) {
                Ok(value) => {
                    let value = value?;
                    if value.get("protocol").and_then(Value::as_str) != Some(self.protocol) {
                        return Err(DomainError::unsupported("Envelope Python incompatible"));
                    }
                    if value.get("event").is_some() {
                        if value.get("event").and_then(Value::as_str) != Some("progress")
                            || job.is_none()
                            || value.get("job_id").and_then(Value::as_str) != job
                        {
                            return Err(DomainError::process("Evento Python no correlacionado con el job"));
                        }
                        progress(value);
                        continue;
                    }
                    if value.get("id").and_then(Value::as_str) != Some(id) {
                        return Err(DomainError::process("ID de respuesta Python no correlacionado"));
                    }
                    if let Some(error) = value.get("error") {
                        let code = match error.get("code").and_then(Value::as_str) {
                            Some("E_CANCELLED") => ErrorCode::Cancelled,
                            Some("E_DEPENDENCY_BLOCKED" | "runtime_mismatch" | "E_UNSUPPORTED") => ErrorCode::Unsupported,
                            Some("E_ARGUMENT" | "E_PATH" | "E_PROTOCOL" | "E_LIMIT") => ErrorCode::Invalid,
                            _ => ErrorCode::Process,
                        };
                        return Err(DomainError::new(code, format!("Worker Python: {error}")));
                    }
                    return value.get("result").cloned().ok_or_else(|| DomainError::process("Respuesta Python sin resultado"));
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(DomainError::process("Python cerró sin respuesta")),
            }
        }
        Err(DomainError::process("El worker excedió el plazo de la operación"))
    }
    fn close(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let _ = self.send("shutdown", "shutdown", json!({}), Duration::from_millis(500), &AtomicBool::new(false));
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for OwnedWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn capabilities(runtime: &Runtime, cancel: &AtomicBool) -> DomainResult<Value> {
    let mut worker = OwnedWorker::spawn(runtime)?;
    worker.send("hello", "hello", json!({}), Duration::from_secs(10), cancel)?;
    let result = worker.response("hello", None, Duration::from_secs(30), cancel, &|_| {});
    worker.close();
    result
}

pub fn run(record: &JobRecord<AnalysisPayload>, resume: bool, cancel: Arc<AtomicBool>, progress: impl Fn(Value)) -> DomainResult<AnalysisResult> {
    validate_record(record)?;
    let payload = &record.payload;
    let lease = jobs::claim::<AnalysisPayload>(&payload.runtime.work_root.join("jobs"), &record.id, &record.payload_digest, resume)?;
    let result = (|| {
        if sha256_file(&payload.source_path, &cancel)? != payload.source_sha256
            || model_digest(&payload.runtime.model, &cancel)? != payload.model_digest
            || sha256_file(&payload.runtime.worker, &cancel)? != payload.worker_sha256
        {
            return Err(DomainError::precondition("Cambió la entrada, modelo o worker; crea una solicitud nueva"));
        }
        let mut worker = OwnedWorker::spawn(&payload.runtime)?;
        worker.send("hello", "hello", json!({}), Duration::from_secs(10), &cancel)?;
        let hello = worker.response("hello", None, Duration::from_secs(30), &cancel, &progress)?;
        if hello.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
            return Err(DomainError::unsupported("Versión de worker incompatible"));
        }
        worker.send("run", "run", json!({
            "job_id":record.id,"project_id":record.project_id,"revision":record.revision,"project_digest":payload.project_digest,
            "asset":payload.asset,"source_path":payload.source_path,"source_sha256":payload.source_sha256,
            "model_path":payload.runtime.model,"model_digest":payload.model_digest,"parameters":payload.parameters,"ffmpeg_path":payload.ffmpeg_path,
        }), Duration::from_secs(10), &cancel)?;
        let raw = worker.response("run", Some(&record.id), Duration::from_secs(24 * 3600), &cancel, &progress);
        if cancel.load(Ordering::Acquire) {
            let _ = worker.send("cancel", "cancel", json!({"job_id":record.id}), Duration::from_millis(250), &AtomicBool::new(false));
        }
        worker.close();
        let result: AnalysisResult = serde_json::from_value(raw?)?;
        validate_result(record, &result, &cancel)?;
        Ok(result)
    })();
    let state = match &result {
        Ok(_) => JobState::Succeeded,
        Err(e) if e.code == ErrorCode::Cancelled => JobState::Cancelled,
        Err(_) => JobState::Failed,
    };
    lease.finish(state, result.as_ref().ok().map(serde_json::to_value).transpose()?, result.as_ref().err().map(ToString::to_string))?;
    result
}
fn checked_artifact(root: &Path, relative: &str) -> DomainResult<PathBuf> {
    if relative.is_empty() || relative.contains('\\') || Path::new(relative).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(DomainError::invalid("Ruta de artefacto insegura"));
    }
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(root.canonicalize()?) || !path.is_file() {
        return Err(DomainError::invalid("Artefacto fuera del job"));
    }
    Ok(path)
}
pub fn validate_result(record: &JobRecord<AnalysisPayload>, result: &AnalysisResult, cancel: &AtomicBool) -> DomainResult<()> {
    validate_record(record)?;
    if result.job_id != record.id
        || result.project_id != record.project_id
        || result.revision != record.revision
        || result.project_digest != record.payload.project_digest
        || result.source_sha256 != record.payload.source_sha256
        || result.model_digest != record.payload.model_digest
    {
        return Err(DomainError::precondition("Resultado no ligado a la solicitud"));
    }
    if !result.artifacts.contains_key(&result.master_path) {
        return Err(DomainError::invalid("Master no verificado en el recibo"));
    }
    let root = record.payload.runtime.work_root.join(&record.id);
    for (name, expected) in &result.artifacts {
        if sha256_file(&checked_artifact(&root, name)?, cancel)? != *expected {
            return Err(DomainError::precondition(format!("Artefacto alterado: {name}")));
        }
    }
    Ok(())
}
pub fn prepare_result(
    snapshot: Project,
    record: &JobRecord<AnalysisPayload>,
    result: &AnalysisResult,
    cancel: &AtomicBool,
) -> DomainResult<PreparedCommand> {
    prepare_result_impl(snapshot, record, result, cancel, false)
}

/// Explicit local review can reconcile a result against a newer project base.
pub fn prepare_rebased_result(
    snapshot: Project,
    record: &JobRecord<AnalysisPayload>,
    result: &AnalysisResult,
    cancel: &AtomicBool,
) -> DomainResult<PreparedCommand> {
    prepare_result_impl(snapshot, record, result, cancel, true)
}
fn prepare_result_impl(
    snapshot: Project,
    record: &JobRecord<AnalysisPayload>,
    result: &AnalysisResult,
    cancel: &AtomicBool,
    rebase: bool,
) -> DomainResult<PreparedCommand> {
    validate_result(record, result, cancel)?;
    prepare_artifacts(
        snapshot,
        ArtifactPreparation {
            project_id: &record.project_id,
            revision: record.revision,
            project_digest: &record.payload.project_digest,
            asset: &record.payload.asset,
            source_path: &record.payload.source_path,
            source_sha256: &result.source_sha256,
            job_id: &record.id,
            root: &record.payload.runtime.work_root.join(&record.id),
            master_path: &result.master_path,
            artifacts: &result.artifacts,
        },
        cancel,
        rebase,
    )
}

/// Shared command boundary for ASR and a separately verified derived generation.
/// Each caller must validate its own complete upstream receipt graph first.
struct ArtifactPreparation<'a> {
    project_id: &'a str,
    revision: u64,
    project_digest: &'a str,
    asset: &'a Asset,
    source_path: &'a Path,
    source_sha256: &'a str,
    job_id: &'a str,
    root: &'a Path,
    master_path: &'a str,
    artifacts: &'a BTreeMap<String, String>,
}

fn prepare_artifacts(snapshot: Project, input: ArtifactPreparation<'_>, cancel: &AtomicBool, rebase: bool) -> DomainResult<PreparedCommand> {
    let session = ProjectSession::new(snapshot);
    if session.project().project_id != input.project_id
        || (!rebase && (session.revision() != input.revision || session.digest()? != input.project_digest))
    {
        return Err(DomainError::precondition("El análisis se conserva, pero su revisión ya no es vigente; revisa y reconcilia antes de incorporar"));
    }
    let asset = session.project().asset(&input.asset.id).ok_or_else(|| DomainError::not_found("Medio", &input.asset.id))?;
    if asset != input.asset {
        return Err(DomainError::precondition("Cambió el medio del resultado"));
    }
    if sha256_file(input.source_path, cancel)? != input.source_sha256 {
        return Err(DomainError::precondition("El contenido del medio cambió desde la inferencia"));
    }
    let master_path = checked_artifact(input.root, input.master_path)?;
    // Read only verified bytes, never arbitrary siblings in the worker folder.
    let bytes = fs::read(&master_path)?;
    check_cancel(cancel)?;
    if input.artifacts.get(input.master_path) != Some(&hex::encode(Sha256::digest(&bytes))) {
        return Err(DomainError::precondition("Master cambió durante su lectura"));
    }
    let master = tv2_v1compat::master::V1Master::parse(serde_json::from_slice(&bytes)?)?;
    check_cancel(cancel)?;
    if !master.fingerprint.same_identity(&asset.fingerprint) || master.duration != asset.duration() {
        return Err(DomainError::precondition("Identidad o duración del master no corresponde al medio"));
    }
    let mut evidence = master.evidence(&asset.id);
    let mut files = BTreeMap::new();
    for (name, sha256) in input.artifacts {
        check_cancel(cancel)?;
        let source = checked_artifact(input.root, name)?;
        files.insert(name.clone(), tv2_domain::evidence::SourceFile { size: fs::metadata(&source)?.len(), source, sha256: sha256.clone() });
    }
    evidence.source_bundle = Some(Arc::new(
        tv2_domain::evidence::SourceBundle::new(input.master_path.to_owned(), BTreeMap::new())
            .with_files(files)
            .with_external_master_digest(evidence.document.full_digest().into()),
    ));
    let mut commands = vec![tv2_domain::Command::AttachMaster { master: evidence }];
    commands.extend(master.projections(&asset.id)?.into_iter().map(|layer| tv2_domain::Command::ReplaceLayer { layer }));
    check_cancel(cancel)?;
    let envelope = CommandEnvelope::human(tv2_domain::Command::Batch { label: "Incorporar análisis local".into(), commands })
        .with_actor(Actor::External { source: "python-local-inference".into() })
        .with_base(session.revision())
        .with_idempotency(format!("analysis-{}", input.job_id));
    let prepared = session.prepare_command(envelope)?;
    check_cancel(cancel)?;
    Ok(prepared)
}

#[cfg(test)]
mod tests;
