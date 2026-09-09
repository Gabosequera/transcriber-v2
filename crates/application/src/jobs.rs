//! Typed durable job records. A per-job OS lock distinguishes a lost worker from
//! a live owner; reopening never repeats external effects automatically.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tv2_domain::{DomainError, error::DomainResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Interrupted,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobRecord<T> {
    pub schema: String,
    pub id: String,
    pub project_id: String,
    pub revision: u64,
    pub state: JobState,
    pub payload_digest: String,
    pub payload: T,
    pub updated_at: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    #[serde(default)]
    pub attempts: Vec<JobAttempt>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobAttempt {
    pub started_at: String,
    pub ended_at: Option<String>,
    pub state: JobState,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}
pub struct JobLease<T> {
    pub record: JobRecord<T>,
    path: PathBuf,
    _lock: fs::File,
}
fn digest<T: Serialize>(payload: &T) -> DomainResult<String> {
    Ok(tv2_domain::digest::digest_json(&serde_json::to_value(payload)?))
}
fn job_path(root: &Path, id: &str) -> DomainResult<PathBuf> {
    if !id.starts_with("job-") || !id[4..].bytes().all(|b| b.is_ascii_hexdigit()) || id.len() != 16 {
        return Err(DomainError::invalid("ID de job inválido"));
    }
    Ok(root.join(format!("{id}.json")))
}
fn lock(root: &Path, id: &str) -> DomainResult<fs::File> {
    let path = job_path(root, id)?.with_extension("lock");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(DomainError::invalid("job lock enlazado"));
    }
    let file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    file.try_lock().map_err(|e| DomainError::precondition(format!("Job ocupado por otro worker: {e}")))?;
    Ok(file)
}
fn read<T: Serialize + DeserializeOwned>(path: &Path) -> DomainResult<JobRecord<T>> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(DomainError::invalid("job enlazado"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(DomainError::invalid("job supera 128 MiB"));
    }
    let record: JobRecord<T> = serde_json::from_slice(&bytes)?;
    if record.schema != "transcriptor-job/1"
        || record.payload_digest != digest(&record.payload)?
        || path.file_stem().and_then(|s| s.to_str()) != Some(&record.id)
    {
        return Err(DomainError::invalid("job corrupto o schema desconocido"));
    }
    Ok(record)
}
fn write<T: Serialize>(path: &Path, record: &JobRecord<T>) -> DomainResult<()> {
    let bytes = serde_json::to_vec(record)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(DomainError::invalid("job supera 128 MiB; no se guardó"));
    }
    crate::store::atomic_write(path, &bytes)
}

pub fn enqueue<T: Serialize + DeserializeOwned>(root: &Path, project_id: String, revision: u64, payload: T) -> DomainResult<JobRecord<T>> {
    fs::create_dir_all(root)?;
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(DomainError::invalid("almacén de jobs enlazado"));
    }
    let id = format!("job-{}", tv2_domain::ids::random_hex12());
    let _lock = lock(root, &id)?;
    let path = job_path(root, &id)?;
    if path.exists() {
        return Err(DomainError::precondition("ID de job ya existe"));
    }
    let record = JobRecord {
        schema: "transcriptor-job/1".into(),
        id,
        project_id,
        revision,
        state: JobState::Queued,
        payload_digest: digest(&payload)?,
        payload,
        updated_at: tv2_domain::project::now_iso(),
        result: None,
        error: None,
        attempts: vec![],
    };
    write(&path, &record)?;
    Ok(record)
}
pub fn discover<T: Serialize + DeserializeOwned>(root: &Path) -> DomainResult<Vec<JobRecord<T>>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let mut record = read::<T>(&path)?;
        if matches!(record.state, JobState::Running) {
            // A live worker remains Running. Only an orphan becomes Interrupted.
            if let Ok(_lease) = lock(root, &record.id) {
                record = read::<T>(&path)?;
                if record.state == JobState::Running {
                    record.state = JobState::Interrupted;
                    record.updated_at = tv2_domain::project::now_iso();
                    if let Some(attempt) = record.attempts.last_mut() {
                        attempt.state = JobState::Interrupted;
                        attempt.ended_at = Some(record.updated_at.clone());
                        attempt.error = Some("Worker sin propietario al recuperar".into());
                    }
                    write(&path, &record)?;
                }
            }
        }
        records.push(record);
    }
    records.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)));
    Ok(records)
}
pub fn claim<T: Serialize + DeserializeOwned>(root: &Path, id: &str, expected_digest: &str, recover: bool) -> DomainResult<JobLease<T>> {
    let held = lock(root, id)?;
    let path = job_path(root, id)?;
    let mut record = read::<T>(&path)?;
    if record.payload_digest != expected_digest {
        return Err(DomainError::precondition("El contenido del job cambió"));
    }
    if record.state != JobState::Queued
        && !(recover && matches!(record.state, JobState::Interrupted | JobState::Running | JobState::Failed | JobState::Cancelled))
    {
        return Err(DomainError::precondition("Estado del job no reanudable"));
    }
    record.state = JobState::Running;
    record.error = None;
    record.updated_at = tv2_domain::project::now_iso();
    record.attempts.push(JobAttempt { started_at: record.updated_at.clone(), ended_at: None, state: JobState::Running, result: None, error: None });
    write(&path, &record)?;
    Ok(JobLease { record, path, _lock: held })
}
impl<T: Serialize> JobLease<T> {
    pub fn finish(mut self, state: JobState, result: Option<serde_json::Value>, error: Option<String>) -> DomainResult<()> {
        if !matches!(state, JobState::Succeeded | JobState::Failed | JobState::Cancelled) {
            return Err(DomainError::invalid("estado terminal inválido"));
        }
        self.record.state = state;
        self.record.result = result;
        self.record.error = error;
        self.record.updated_at = tv2_domain::project::now_iso();
        if let Some(attempt) = self.record.attempts.last_mut() {
            attempt.state = state;
            attempt.ended_at = Some(self.record.updated_at.clone());
            attempt.result = self.record.result.clone();
            attempt.error = self.record.error.clone();
        }
        write(&self.path, &self.record)
    }
}
pub fn cancel<T: Serialize + DeserializeOwned>(root: &Path, id: &str, digest: &str) -> DomainResult<()> {
    claim::<T>(root, id, digest, true)?.finish(JobState::Cancelled, None, Some("Cancelado explícitamente".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_jobs_recover_orphans_preserve_payload_and_exclude_live_workers() {
        let root = tempfile::tempdir().unwrap();
        let record = enqueue(root.path(), "p".into(), 7, serde_json::json!({"source":"frozen","range":[2,9]})).unwrap();
        let lease = claim::<serde_json::Value>(root.path(), &record.id, &record.payload_digest, false).unwrap();
        assert!(claim::<serde_json::Value>(root.path(), &record.id, &record.payload_digest, true).is_err());
        assert_eq!(discover::<serde_json::Value>(root.path()).unwrap()[0].state, JobState::Running);
        drop(lease); // Simulated lost worker, not a physical crash test.
        let found = discover::<serde_json::Value>(root.path()).unwrap();
        assert_eq!(found[0].state, JobState::Interrupted);
        assert_eq!(found[0].payload, record.payload);
        assert!(claim::<serde_json::Value>(root.path(), &record.id, &record.payload_digest, false).is_err());
        let lease = claim::<serde_json::Value>(root.path(), &record.id, &record.payload_digest, true).unwrap();
        lease.finish(JobState::Succeeded, Some(serde_json::json!({"verified":true})), None).unwrap();
        assert_eq!(discover::<serde_json::Value>(root.path()).unwrap()[0].state, JobState::Succeeded);
        assert!(claim::<serde_json::Value>(root.path(), &record.id, &record.payload_digest, true).is_err());
    }
    #[test]
    fn corrupt_job_and_wrong_digest_are_rejected_without_replacing_record() {
        let root = tempfile::tempdir().unwrap();
        let record = enqueue(root.path(), "p".into(), 1, serde_json::json!({"value":1})).unwrap();
        assert!(claim::<serde_json::Value>(root.path(), &record.id, "wrong", false).is_err());
        cancel::<serde_json::Value>(root.path(), &record.id, &record.payload_digest).unwrap();
        assert_eq!(discover::<serde_json::Value>(root.path()).unwrap()[0].state, JobState::Cancelled);
        let path = job_path(root.path(), &record.id).unwrap();
        let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        raw["payload"]["value"] = serde_json::json!(2);
        fs::write(&path, raw.to_string()).unwrap();
        assert!(discover::<serde_json::Value>(root.path()).is_err());
        assert!(job_path(root.path(), "../foreign").is_err());
    }
}
