//! Durable ordered analysis. Successful observations are reused, never rebound.
//! The workflow returns a verified generation and never commits project changes.
use crate::{AnalysisPayload, AnalysisResult, alignment, arousal, generation, laughter};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
use tv2_application::{
    jobs::{self, JobRecord, JobState},
    store::atomic_write,
};
use tv2_domain::{DomainError, ErrorCode, error::DomainResult};

pub const CHECKPOINT_SCHEMA: &str = "tv2-workflow-checkpoint/1";
const CHECKPOINT: &str = ".work/workflow.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentStep {
    pub runtime: alignment::AlignmentRuntime,
    pub parameters: alignment::AlignmentParameters,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArousalStep {
    pub runtime: arousal::ArousalRuntime,
    pub parameters: arousal::ArousalParameters,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaughterStep {
    pub runtime: laughter::LaughterRuntime,
    pub parameters: laughter::LaughterParameters,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlan {
    pub alignment: Option<AlignmentStep>,
    pub arousal: Option<ArousalStep>,
    pub laughter: Option<LaughterStep>,
    pub generation: generation::GenerationRuntime,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPayload {
    pub asr: JobRecord<AnalysisPayload>,
    pub plan: WorkflowPlan,
    pub resources: BTreeMap<String, String>,
    pub work_root: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowResult {
    pub generation: JobRecord<generation::GenerationPayload>,
    pub result: generation::GenerationResult,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildRef {
    id: String,
    payload_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    schema: String,
    workflow_id: String,
    payload_digest: String,
    plan_digest: String,
    resources_digest: String,
    children: BTreeMap<String, ChildRef>,
}
fn digest<T: Serialize>(value: &T) -> DomainResult<String> {
    Ok(tv2_domain::digest::digest_json(&serde_json::to_value(value)?))
}
fn same<T: Serialize>(left: &T, right: &T) -> DomainResult<bool> {
    Ok(serde_json::to_value(left)? == serde_json::to_value(right)?)
}
fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.starts_with("job-") && id[4..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn valid_sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn resolved(path: &Path) -> DomainResult<PathBuf> {
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir)) {
        return Err(DomainError::invalid("Workflow requiere rutas absolutas sin navegación relativa"));
    }
    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(ancestor.file_name().ok_or_else(|| DomainError::invalid("Ruta sin ancestro existente"))?.to_owned());
        ancestor = ancestor.parent().ok_or_else(|| DomainError::invalid("Ruta sin raíz"))?;
    }
    let mut result = ancestor.canonicalize()?;
    for part in suffix.into_iter().rev() {
        result.push(part);
    }
    Ok(result)
}
fn path_key(path: &Path) -> DomainResult<String> {
    let key = resolved(path)?.to_string_lossy().into_owned();
    Ok(if cfg!(windows) { key.to_lowercase() } else { key })
}
fn executable(path: &Path) -> DomainResult<PathBuf> {
    if path.is_absolute() {
        return resolved(path);
    }
    // Legacy ASR receipts may name ffmpeg.exe. Preserve that immutable payload,
    // but bind the currently resolved executable bytes on every verification.
    if path.components().count() != 1 || !matches!(path.components().next(), Some(Component::Normal(_))) {
        return Err(DomainError::invalid("Ejecutable relativo debe ser un nombre sin navegación"));
    }
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let candidate = directory.join(path);
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
    }
    Err(DomainError::precondition("Ejecutable del recibo ASR no disponible en PATH"))
}
fn indices(payload: &WorkflowPayload) -> DomainResult<BTreeSet<u32>> {
    let list = &payload.asr.payload.asset.probe.audio;
    let indices = list.iter().map(|audio| audio.audio_index).collect::<BTreeSet<_>>();
    if indices.is_empty() || indices.len() != list.len() {
        return Err(DomainError::invalid("Workflow requiere índices de audio únicos"));
    }
    Ok(indices)
}
fn keys(payload: &WorkflowPayload) -> DomainResult<Vec<String>> {
    let mut result = vec!["asr".into()];
    for index in indices(payload)? {
        if payload.plan.alignment.is_some() {
            result.push(format!("alignment/a{index}"));
        }
        if payload.plan.arousal.is_some() {
            result.push(format!("arousal/a{index}"));
        }
        if payload.plan.laughter.is_some() {
            result.push(format!("laughter/a{index}"));
        }
    }
    result.push("generation".into());
    Ok(result)
}
fn plan_valid(payload: &WorkflowPayload) -> DomainResult<()> {
    crate::validate_record(&payload.asr)?;
    indices(payload)?;
    let parameters = &payload.asr.payload.parameters;
    if payload.plan.arousal.is_some() && payload.plan.alignment.is_none() {
        return Err(DomainError::invalid("Arousal requiere MMS en el mismo plan"));
    }
    if parameters.device != "cpu"
        || !(1..=4).contains(&parameters.threads)
        || !(1..=10).contains(&parameters.beam_size)
        || parameters.steps != ["extract", "transcribe"]
    {
        return Err(DomainError::invalid("Workflow requiere parámetros ASR completos válidos"));
    }
    let mut roots = vec![&payload.asr.payload.runtime.work_root, &payload.work_root, &payload.plan.generation.work_root];
    if let Some(step) = &payload.plan.alignment {
        step.parameters.validate()?;
        roots.push(&step.runtime.work_root);
    }
    if let Some(step) = &payload.plan.arousal {
        step.parameters.validate()?;
        roots.push(&step.runtime.work_root);
    }
    if let Some(step) = &payload.plan.laughter {
        step.parameters.validate()?;
        roots.push(&step.runtime.work_root);
    }
    let mut seen = BTreeSet::new();
    for root in roots {
        if !seen.insert(path_key(root)?) {
            return Err(DomainError::invalid("Los roles del workflow requieren roots distintos"));
        }
    }
    Ok(())
}
fn capture(payload: &WorkflowPayload, cancel: &AtomicBool) -> DomainResult<BTreeMap<String, String>> {
    crate::check_cancel(cancel)?;
    plan_valid(payload)?;
    let mut values = BTreeMap::new();
    let asr = &payload.asr.payload;
    values.insert("asr.model".into(), crate::model_digest(&asr.runtime.model, cancel)?);
    let mut files = vec![
        ("asr.source".to_owned(), asr.source_path.clone()),
        ("asr.worker".into(), asr.runtime.worker.clone()),
        ("asr.python".into(), asr.runtime.python.clone()),
        ("asr.ffmpeg".into(), executable(&asr.ffmpeg_path)?),
        ("asr.lock".into(), asr.runtime.worker.with_file_name("requirements-transcription.lock")),
        ("generation.worker".into(), payload.plan.generation.worker.clone()),
        ("generation.python".into(), payload.plan.generation.python.clone()),
        ("generation.assembler".into(), payload.plan.generation.assembler.clone()),
        ("generation.derivation".into(), payload.plan.generation.derivation.clone()),
    ];
    if let Some(step) = &payload.plan.alignment {
        files.extend([
            ("alignment.python".into(), step.runtime.python.clone()),
            ("alignment.worker".into(), step.runtime.worker.clone()),
            ("alignment.model".into(), step.runtime.model.clone()),
            ("alignment.manifest".into(), step.runtime.model_manifest.clone()),
            ("alignment.lock".into(), step.runtime.lock.clone()),
        ]);
    }
    if let Some(step) = &payload.plan.arousal {
        files.extend([
            ("arousal.python".into(), step.runtime.python.clone()),
            ("arousal.worker".into(), step.runtime.worker.clone()),
            ("arousal.manifest".into(), step.runtime.model_manifest.clone()),
            ("arousal.lock".into(), step.runtime.lock.clone()),
        ]);
        for name in ["config.json", "model.safetensors", "preprocessor_config.json"] {
            files.push((format!("arousal.{name}"), step.runtime.model.join(name)));
        }
    }
    if let Some(step) = &payload.plan.laughter {
        files.extend([
            ("laughter.python".into(), step.runtime.python.clone()),
            ("laughter.worker".into(), step.runtime.worker.clone()),
            ("laughter.model".into(), step.runtime.model.clone()),
            ("laughter.config".into(), step.runtime.config.clone()),
            ("laughter.manifest".into(), step.runtime.model_manifest.clone()),
            ("laughter.lock".into(), step.runtime.lock.clone()),
        ]);
    }
    for (key, path) in files {
        resolved(&path)?;
        values.insert(key, crate::sha256_file(&path, cancel)?);
    }
    if values["asr.source"] != asr.source_sha256 || values["asr.worker"] != asr.worker_sha256 || values["asr.model"] != asr.model_digest {
        return Err(DomainError::precondition("ASR original ya no corresponde a fuente/modelo/código"));
    }
    Ok(values)
}
fn verify(payload: &WorkflowPayload, cancel: &AtomicBool) -> DomainResult<()> {
    if capture(payload, cancel)? != payload.resources {
        return Err(DomainError::precondition("Cambió recurso/código/lock del plan; crea otro workflow"));
    }
    Ok(())
}
fn captured(payload: &WorkflowPayload, key: &str, value: &str) -> DomainResult<()> {
    if payload.resources.get(key).map(String::as_str) != Some(value) {
        return Err(DomainError::precondition(format!("Recurso de child difiere del plan: {key}")));
    }
    Ok(())
}
pub fn enqueue(
    asr: JobRecord<AnalysisPayload>,
    plan: WorkflowPlan,
    work_root: PathBuf,
    cancel: &AtomicBool,
) -> DomainResult<JobRecord<WorkflowPayload>> {
    let mut payload = WorkflowPayload { asr, plan, resources: BTreeMap::new(), work_root };
    payload.resources = capture(&payload, cancel)?;
    jobs::enqueue(&payload.work_root.join("jobs"), payload.asr.project_id.clone(), payload.asr.revision, payload)
}
fn checkpoint_path(record: &JobRecord<WorkflowPayload>) -> DomainResult<PathBuf> {
    crate::validate_record(record)?;
    let root = &record.payload.work_root;
    let job = root.join(&record.id);
    let canonical_root = resolved(root)?;
    let canonical_job = resolved(&job)?;
    if !canonical_job.starts_with(&canonical_root) {
        return Err(DomainError::invalid("Directorio workflow fuera del root"));
    }
    let target = job.join(CHECKPOINT);
    for path in [&job, &job.join(".work"), &target] {
        if let Ok(metadata) = fs::symlink_metadata(path) {
            #[cfg(windows)]
            let linked = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let linked = metadata.file_type().is_symlink();
            if linked || (path != &target && !metadata.is_dir()) || (path == &target && !metadata.is_file()) {
                return Err(DomainError::invalid("Checkpoint workflow enlazado o tipo inválido"));
            }
        }
        if !resolved(path)?.starts_with(&canonical_job) {
            return Err(DomainError::invalid("Checkpoint workflow fuera del job"));
        }
    }
    Ok(target)
}
fn new_checkpoint(record: &JobRecord<WorkflowPayload>) -> DomainResult<Checkpoint> {
    Ok(Checkpoint {
        schema: CHECKPOINT_SCHEMA.into(),
        workflow_id: record.id.clone(),
        payload_digest: record.payload_digest.clone(),
        plan_digest: digest(&record.payload.plan)?,
        resources_digest: digest(&record.payload.resources)?,
        children: BTreeMap::from([(
            "asr".into(),
            ChildRef { id: record.payload.asr.id.clone(), payload_digest: record.payload.asr.payload_digest.clone() },
        )]),
    })
}
fn checkpoint_valid(record: &JobRecord<WorkflowPayload>, checkpoint: &Checkpoint) -> DomainResult<()> {
    if checkpoint.schema != CHECKPOINT_SCHEMA
        || checkpoint.workflow_id != record.id
        || checkpoint.payload_digest != record.payload_digest
        || checkpoint.plan_digest != digest(&record.payload.plan)?
        || checkpoint.resources_digest != digest(&record.payload.resources)?
    {
        return Err(DomainError::precondition("Checkpoint de otro workflow/plan/recursos"));
    }
    let ordered = keys(&record.payload)?;
    let allowed = ordered.iter().collect::<BTreeSet<_>>();
    if checkpoint.children.keys().any(|key| !allowed.contains(key)) {
        return Err(DomainError::invalid("Checkpoint contiene etapa ajena al plan"));
    }
    let mut gap = false;
    for key in ordered {
        if let Some(child) = checkpoint.children.get(&key) {
            if gap || !valid_id(&child.id) || !valid_sha(&child.payload_digest) {
                return Err(DomainError::invalid("Checkpoint fuera de orden o child inválido"));
            }
        } else {
            gap = true;
        }
    }
    let asr = checkpoint.children.get("asr").ok_or_else(|| DomainError::invalid("Checkpoint sin ASR original"))?;
    if asr.id != record.payload.asr.id || asr.payload_digest != record.payload.asr.payload_digest {
        return Err(DomainError::precondition("Checkpoint rebind ASR"));
    }
    Ok(())
}
fn save_checkpoint(record: &JobRecord<WorkflowPayload>, checkpoint: &Checkpoint) -> DomainResult<()> {
    checkpoint_valid(record, checkpoint)?;
    let bytes = serde_json::to_vec(checkpoint)?;
    if bytes.len() > 1024 * 1024 {
        return Err(DomainError::invalid("Checkpoint workflow supera1MiB"));
    }
    let path = checkpoint_path(record)?;
    fs::create_dir_all(path.parent().ok_or_else(|| DomainError::invalid("Checkpoint sin parent"))?)?;
    // Recheck after creation, including Windows junctions, before any write.
    atomic_write(&checkpoint_path(record)?, &bytes)
}
fn load_checkpoint(record: &JobRecord<WorkflowPayload>, required: bool) -> DomainResult<Checkpoint> {
    let path = checkpoint_path(record)?;
    let checkpoint = if path.exists() {
        if fs::metadata(&path)?.len() > 1024 * 1024 {
            return Err(DomainError::invalid("Checkpoint workflow supera1MiB"));
        }
        serde_json::from_slice(&fs::read(path)?)?
    } else if !required {
        new_checkpoint(record)?
    } else {
        return Err(DomainError::precondition("Falta checkpoint durable del workflow"));
    };
    checkpoint_valid(record, &checkpoint)?;
    Ok(checkpoint)
}
fn load_child<T: Serialize + DeserializeOwned>(root: &Path, reference: &ChildRef) -> DomainResult<JobRecord<T>> {
    if !valid_id(&reference.id) || !valid_sha(&reference.payload_digest) {
        return Err(DomainError::invalid("Referencia child inválida"));
    }
    let path = root.join(format!("{}.json", reference.id));
    if fs::symlink_metadata(&path)?.file_type().is_symlink()
        || !path.canonicalize()?.starts_with(&root.canonicalize()?)
        || fs::metadata(&path)?.len() > 128 * 1024 * 1024
    {
        return Err(DomainError::invalid("Recibo child enlazado/fuera del root/excesivo"));
    }
    let record: JobRecord<T> = serde_json::from_slice(&fs::read(path)?)?;
    crate::validate_record(&record)?;
    if record.id != reference.id || record.payload_digest != reference.payload_digest {
        return Err(DomainError::precondition("Child cambió identidad o payload"));
    }
    Ok(record)
}
fn completed<T: Serialize, R: Serialize>(record: &JobRecord<T>, result: &R) -> DomainResult<()> {
    crate::validate_record(record)?;
    if record.state != JobState::Succeeded || record.result.as_ref() != Some(&serde_json::to_value(result)?) {
        return Err(DomainError::precondition("Child no tiene recibo final válido"));
    }
    Ok(())
}

struct Runner<'a, P: Fn(Value)> {
    record: &'a JobRecord<WorkflowPayload>,
    resume: bool,
    cancel: Arc<AtomicBool>,
    progress: &'a P,
    checkpoint: Checkpoint,
    finished: usize,
    total: usize,
}
impl<P: Fn(Value)> Runner<'_, P> {
    fn step<T, R, C, E, X, V>(
        &mut self,
        key: &str,
        root: &Path,
        create: C,
        expected: E,
        execute: X,
        validate: V,
    ) -> DomainResult<generation::Stage<T, R>>
    where
        T: Clone + Serialize + DeserializeOwned,
        R: Clone + Serialize + DeserializeOwned,
        C: FnOnce() -> DomainResult<JobRecord<T>>,
        E: Fn(&JobRecord<T>) -> DomainResult<()>,
        X: Fn(&JobRecord<T>, bool, Arc<AtomicBool>, &dyn Fn(Value)) -> DomainResult<R>,
        V: Fn(&JobRecord<T>, &R, &AtomicBool) -> DomainResult<()>,
    {
        crate::check_cancel(&self.cancel)?;
        verify(&self.record.payload, &self.cancel)?;
        let reference = if let Some(reference) = self.checkpoint.children.get(key) {
            reference.clone()
        } else {
            let child = create()?;
            expected(&child)?;
            let reference = ChildRef { id: child.id.clone(), payload_digest: child.payload_digest.clone() };
            self.checkpoint.children.insert(key.into(), reference.clone());
            save_checkpoint(self.record, &self.checkpoint)?;
            reference
        };
        let mut child = load_child::<T>(root, &reference)?;
        expected(&child)?;
        let cached = child.state == JobState::Succeeded;
        let emit = |detail: Value| {
            (self.progress)(json!({"event":"progress","job_id":self.record.id,"stage":"workflow","step":key,
            "child_job_id":reference.id,"fraction":(self.finished as f64+detail["fraction"].as_f64().unwrap_or(0.0).clamp(0.0,1.0))/self.total as f64,"detail":detail}));
        };
        emit(json!({"fraction":0.0,"cached":cached}));
        crate::check_cancel(&self.cancel)?;
        let result = if cached {
            serde_json::from_value(child.result.clone().ok_or_else(|| DomainError::invalid("Child succeeded sin result"))?)?
        } else {
            if child.state != JobState::Queued && !self.resume {
                return Err(DomainError::precondition("Recuperación child requiere resume explícito"));
            }
            if child.state == JobState::Running && self.resume {
                // Discover marks only orphans Interrupted; a live owner stays locked.
                jobs::discover::<T>(root)?;
                child = load_child(root, &reference)?;
                expected(&child)?;
            }
            crate::check_cancel(&self.cancel)?;
            let result = execute(&child, self.resume, self.cancel.clone(), &emit)?;
            child = load_child(root, &reference)?;
            expected(&child)?;
            result
        };
        completed(&child, &result)?;
        validate(&child, &result, &self.cancel)?;
        verify(&self.record.payload, &self.cancel)?;
        emit(json!({"fraction":1.0,"cached":cached}));
        crate::check_cancel(&self.cancel)?;
        self.finished += 1;
        Ok(generation::Stage { record: child, result })
    }
}

fn expected_asr(workflow: &WorkflowPayload, record: &JobRecord<AnalysisPayload>) -> DomainResult<()> {
    if record.id != workflow.asr.id
        || record.project_id != workflow.asr.project_id
        || record.revision != workflow.asr.revision
        || record.payload_digest != workflow.asr.payload_digest
        || !same(&record.payload, &workflow.asr.payload)?
    {
        return Err(DomainError::precondition("ASR child pertenece a otra solicitud"));
    }
    Ok(())
}
fn expected_alignment(
    payload: &WorkflowPayload,
    index: u32,
    parent: &generation::Stage<AnalysisPayload, AnalysisResult>,
    child: &JobRecord<alignment::AlignmentPayload>,
) -> DomainResult<()> {
    let step = payload.plan.alignment.as_ref().ok_or_else(|| DomainError::invalid("MMS no seleccionado"))?;
    let value = &child.payload;
    if child.project_id != parent.record.project_id
        || child.revision != parent.record.revision
        || value.audio_index != index
        || !same(&value.parent, &parent.record)?
        || !same(&value.parent_result, &parent.result)?
        || !same(&value.runtime, &step.runtime)?
        || !same(&value.parameters, &step.parameters)?
    {
        return Err(DomainError::precondition("MMS child de otra pista/plan/parent"));
    }
    for (key, actual) in [
        ("alignment.model", &value.model_sha256),
        ("alignment.manifest", &value.model_manifest_sha256),
        ("alignment.worker", &value.worker_sha256),
        ("alignment.lock", &value.lock_sha256),
    ] {
        captured(payload, key, actual)?;
    }
    Ok(())
}
fn arousal_digest(payload: &WorkflowPayload) -> DomainResult<String> {
    let mut files = BTreeMap::new();
    for name in ["config.json", "model.safetensors", "preprocessor_config.json"] {
        files.insert(name, payload.resources.get(&format!("arousal.{name}")).ok_or_else(|| DomainError::invalid("Falta recurso arousal"))?);
    }
    digest(&files)
}
fn expected_arousal(
    payload: &WorkflowPayload,
    parent: &generation::Stage<alignment::AlignmentPayload, alignment::AlignmentResult>,
    child: &JobRecord<arousal::ArousalPayload>,
) -> DomainResult<()> {
    let step = payload.plan.arousal.as_ref().ok_or_else(|| DomainError::invalid("Arousal no seleccionado"))?;
    let value = &child.payload;
    if child.project_id != parent.record.project_id
        || child.revision != parent.record.revision
        || !same(&value.parent, &parent.record)?
        || !same(&value.parent_result, &parent.result)?
        || !same(&value.runtime, &step.runtime)?
        || !same(&value.parameters, &step.parameters)?
        || value.model_digest != arousal_digest(payload)?
    {
        return Err(DomainError::precondition("Arousal child de otro plan/alineación/modelo"));
    }
    for (key, actual) in
        [("arousal.manifest", &value.model_manifest_sha256), ("arousal.worker", &value.worker_sha256), ("arousal.lock", &value.lock_sha256)]
    {
        captured(payload, key, actual)?;
    }
    Ok(())
}
fn expected_laughter(
    payload: &WorkflowPayload,
    index: u32,
    parent: &generation::Stage<AnalysisPayload, AnalysisResult>,
    child: &JobRecord<laughter::LaughterPayload>,
) -> DomainResult<()> {
    let step = payload.plan.laughter.as_ref().ok_or_else(|| DomainError::invalid("Risas no seleccionado"))?;
    let value = &child.payload;
    if child.project_id != parent.record.project_id
        || child.revision != parent.record.revision
        || value.audio_index != index
        || !same(&value.parent, &parent.record)?
        || !same(&value.parent_result, &parent.result)?
        || !same(&value.runtime, &step.runtime)?
        || !same(&value.parameters, &step.parameters)?
    {
        return Err(DomainError::precondition("Risas child de otra pista/plan/parent"));
    }
    for (key, actual) in [
        ("laughter.model", &value.model_sha256),
        ("laughter.config", &value.config_sha256),
        ("laughter.manifest", &value.model_manifest_sha256),
        ("laughter.worker", &value.worker_sha256),
        ("laughter.lock", &value.lock_sha256),
    ] {
        captured(payload, key, actual)?;
    }
    Ok(())
}
fn generation_payload(
    payload: &WorkflowPayload,
    parent: generation::Stage<AnalysisPayload, AnalysisResult>,
    alignments: BTreeMap<u32, generation::Stage<alignment::AlignmentPayload, alignment::AlignmentResult>>,
    arousals: BTreeMap<u32, generation::Stage<arousal::ArousalPayload, arousal::ArousalResult>>,
    laughter: BTreeMap<u32, generation::Stage<laughter::LaughterPayload, laughter::LaughterResult>>,
) -> DomainResult<generation::GenerationPayload> {
    let code_hashes = [("worker", "generation.worker"), ("finalize", "generation.assembler"), ("derivation", "generation.derivation")]
        .into_iter()
        .map(|(key, resource)| {
            Ok((key.into(), payload.resources.get(resource).ok_or_else(|| DomainError::invalid("Falta código de generación"))?.clone()))
        })
        .collect::<DomainResult<BTreeMap<String, String>>>()?;
    Ok(generation::GenerationPayload {
        parent: parent.record,
        parent_result: parent.result,
        alignments,
        arousals,
        laughter,
        runtime: payload.plan.generation.clone(),
        code_hashes,
    })
}
fn expected_generation(
    payload: &WorkflowPayload,
    expected: &generation::GenerationPayload,
    child: &JobRecord<generation::GenerationPayload>,
) -> DomainResult<()> {
    if child.project_id != payload.asr.project_id || child.revision != payload.asr.revision || !same(&child.payload, expected)? {
        return Err(DomainError::precondition("Generation child de otro plan/parent/etapas"));
    }
    Ok(())
}

pub fn run(record: &JobRecord<WorkflowPayload>, resume: bool, cancel: Arc<AtomicBool>, progress: impl Fn(Value)) -> DomainResult<WorkflowResult> {
    crate::validate_record(record)?;
    verify(&record.payload, &cancel)?;
    if record.project_id != record.payload.asr.project_id || record.revision != record.payload.asr.revision {
        return Err(DomainError::precondition("Workflow rebind proyecto/revisión"));
    }
    let lease = jobs::claim::<WorkflowPayload>(&record.payload.work_root.join("jobs"), &record.id, &record.payload_digest, resume)?;
    let outcome: DomainResult<WorkflowResult> = (|| {
        let checkpoint = load_checkpoint(record, false)?;
        save_checkpoint(record, &checkpoint)?;
        let total = keys(&record.payload)?.len();
        let mut runner = Runner { record, resume, cancel: cancel.clone(), progress: &progress, checkpoint, finished: 0, total };
        let payload = &record.payload;
        let asr = runner.step(
            "asr",
            &payload.asr.payload.runtime.work_root.join("jobs"),
            || Ok(payload.asr.clone()),
            |child| expected_asr(payload, child),
            |child, resume, cancel, emit| crate::run(child, resume, cancel, emit),
            crate::validate_result,
        )?;
        let mut alignments = BTreeMap::new();
        let mut arousals = BTreeMap::new();
        let mut laughs = BTreeMap::new();
        for index in indices(payload)? {
            if let Some(step) = &payload.plan.alignment {
                let aligned = runner.step(
                    &format!("alignment/a{index}"),
                    &step.runtime.work_root.join("jobs"),
                    || alignment::enqueue(asr.record.clone(), asr.result.clone(), index, step.runtime.clone(), step.parameters.clone(), &cancel),
                    |child| expected_alignment(payload, index, &asr, child),
                    |child, resume, cancel, emit| alignment::run(child, resume, cancel, emit),
                    alignment::validate_result,
                )?;
                if let Some(step) = &payload.plan.arousal {
                    let arousal = runner.step(
                        &format!("arousal/a{index}"),
                        &step.runtime.work_root.join("jobs"),
                        || arousal::enqueue(aligned.record.clone(), aligned.result.clone(), step.runtime.clone(), step.parameters.clone(), &cancel),
                        |child| expected_arousal(payload, &aligned, child),
                        |child, resume, cancel, emit| arousal::run(child, resume, cancel, emit),
                        arousal::validate_result,
                    )?;
                    arousals.insert(index, arousal);
                }
                alignments.insert(index, aligned);
            }
            if let Some(step) = &payload.plan.laughter {
                let laughter = runner.step(
                    &format!("laughter/a{index}"),
                    &step.runtime.work_root.join("jobs"),
                    || laughter::enqueue(asr.record.clone(), asr.result.clone(), index, step.runtime.clone(), step.parameters.clone(), &cancel),
                    |child| expected_laughter(payload, index, &asr, child),
                    |child, resume, cancel, emit| laughter::run(child, resume, cancel, emit),
                    laughter::validate_result,
                )?;
                laughs.insert(index, laughter);
            }
        }
        let expected = generation_payload(payload, asr, alignments, arousals, laughs)?;
        let final_stage = runner.step(
            "generation",
            &payload.plan.generation.work_root.join("jobs"),
            || generation::enqueue(expected.clone(), &cancel),
            |child| expected_generation(payload, &expected, child),
            |child, resume, cancel, emit| generation::run(child, resume, cancel, emit),
            generation::validate_result,
        )?;
        Ok(WorkflowResult { generation: final_stage.record, result: final_stage.result })
    })();
    let state = match &outcome {
        Ok(_) => JobState::Succeeded,
        Err(error) if error.code == ErrorCode::Cancelled => JobState::Cancelled,
        Err(_) => JobState::Failed,
    };
    lease.finish(state, outcome.as_ref().ok().map(serde_json::to_value).transpose()?, outcome.as_ref().err().map(ToString::to_string))?;
    outcome
}

pub fn validate_result(record: &JobRecord<WorkflowPayload>, result: &WorkflowResult, cancel: &AtomicBool) -> DomainResult<()> {
    crate::validate_record(record)?;
    completed(record, result)?;
    verify(&record.payload, cancel)?;
    if record.project_id != record.payload.asr.project_id || record.revision != record.payload.asr.revision {
        return Err(DomainError::precondition("Workflow rebind proyecto/revisión"));
    }
    let checkpoint = load_checkpoint(record, true)?;
    let allowed = keys(&record.payload)?;
    if checkpoint.children.len() != allowed.len() {
        return Err(DomainError::precondition("Workflow aún no terminó todas las etapas"));
    }
    let payload = &record.payload;
    let asr_record: JobRecord<AnalysisPayload> = load_child(&payload.asr.payload.runtime.work_root.join("jobs"), &checkpoint.children["asr"])?;
    expected_asr(payload, &asr_record)?;
    let asr_result: AnalysisResult = serde_json::from_value(asr_record.result.clone().ok_or_else(|| DomainError::invalid("Falta ASR result"))?)?;
    completed(&asr_record, &asr_result)?;
    crate::validate_result(&asr_record, &asr_result, cancel)?;
    let asr = generation::Stage { record: asr_record, result: asr_result };
    let mut alignments = BTreeMap::new();
    let mut arousals = BTreeMap::new();
    let mut laughs = BTreeMap::new();
    for index in indices(payload)? {
        if let Some(step) = &payload.plan.alignment {
            let child: JobRecord<alignment::AlignmentPayload> =
                load_child(&step.runtime.work_root.join("jobs"), &checkpoint.children[&format!("alignment/a{index}")])?;
            expected_alignment(payload, index, &asr, &child)?;
            let result: alignment::AlignmentResult =
                serde_json::from_value(child.result.clone().ok_or_else(|| DomainError::invalid("Falta MMS result"))?)?;
            completed(&child, &result)?;
            alignment::validate_result(&child, &result, cancel)?;
            let aligned = generation::Stage { record: child, result };
            if let Some(step) = &payload.plan.arousal {
                let child: JobRecord<arousal::ArousalPayload> =
                    load_child(&step.runtime.work_root.join("jobs"), &checkpoint.children[&format!("arousal/a{index}")])?;
                expected_arousal(payload, &aligned, &child)?;
                let result: arousal::ArousalResult =
                    serde_json::from_value(child.result.clone().ok_or_else(|| DomainError::invalid("Falta arousal result"))?)?;
                completed(&child, &result)?;
                arousal::validate_result(&child, &result, cancel)?;
                arousals.insert(index, generation::Stage { record: child, result });
            }
            alignments.insert(index, aligned);
        }
        if let Some(step) = &payload.plan.laughter {
            let child: JobRecord<laughter::LaughterPayload> =
                load_child(&step.runtime.work_root.join("jobs"), &checkpoint.children[&format!("laughter/a{index}")])?;
            expected_laughter(payload, index, &asr, &child)?;
            let result: laughter::LaughterResult =
                serde_json::from_value(child.result.clone().ok_or_else(|| DomainError::invalid("Falta risas result"))?)?;
            completed(&child, &result)?;
            laughter::validate_result(&child, &result, cancel)?;
            laughs.insert(index, generation::Stage { record: child, result });
        }
    }
    let expected = generation_payload(payload, asr, alignments, arousals, laughs)?;
    let child: JobRecord<generation::GenerationPayload> =
        load_child(&payload.plan.generation.work_root.join("jobs"), &checkpoint.children["generation"])?;
    expected_generation(payload, &expected, &child)?;
    completed(&child, &result.result)?;
    if !same(&child, &result.generation)? {
        return Err(DomainError::precondition("WorkflowResult de otro recibo final"));
    }
    generation::validate_result(&child, &result.result, cancel)
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
