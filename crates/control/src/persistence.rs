use crate::{ControlEvent, ProposalSummary};
use crossbeam_channel::{Sender, bounded};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub schema: String,
    pub project_id: String,
    pub proposals: Vec<ProposalSummary>,
    pub events: Vec<ControlEvent>,
}
pub(crate) struct Persistence {
    latest: Arc<Mutex<Pending>>,
    wake: Sender<()>,
    status: Arc<Mutex<Result<String, String>>>,
}
#[derive(Default)]
struct Pending {
    snapshot: Option<Snapshot>,
    audit: Vec<ControlEvent>,
    queued: BTreeMap<String, u64>,
}
impl Persistence {
    pub fn start(path: PathBuf) -> Result<Self, String> {
        let parent = path.parent().ok_or("Control store needs a parent directory")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(|e| e.to_string())?;
        // A previous controller can be finishing its asynchronous final write.
        // Brief bounded retry does not steal a live owner's OS lock.
        let mut attempts = 0;
        loop {
            match lease.try_lock() {
                Ok(()) => break,
                Err(error) if attempts >= 5 => return Err(format!("Control project store already in use: {error}")),
                Err(_) => {
                    attempts += 1;
                    thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        let latest = Arc::new(Mutex::new(Pending::default()));
        let (wake, receive) = bounded(1);
        let status = Arc::new(Mutex::new(Ok("idle".into())));
        let worker_latest = latest.clone();
        let worker_status = status.clone();
        thread::spawn(move || {
            let _lease = lease;
            while receive.recv().is_ok() {
                let (snapshot, audit) = {
                    let mut pending = worker_latest.lock().unwrap();
                    (pending.snapshot.take(), std::mem::take(&mut pending.audit))
                };
                if let Some(snapshot) = snapshot {
                    let result = write(&path, &snapshot, &audit).map(|_| "saved".into());
                    let mut pending = worker_latest.lock().unwrap();
                    if result.is_err() {
                        // Coalescing snapshots must never discard not-yet-durable
                        // audit, including a burst larger than the UI ring.
                        pending.audit.splice(0..0, audit);
                        if pending.snapshot.is_none() {
                            pending.snapshot = Some(snapshot);
                        }
                    }
                    *worker_status.lock().unwrap() = if pending.snapshot.is_some() && result.is_ok() { Ok("pending".into()) } else { result };
                }
            }
            // A final dirty snapshot can replace an already consumed wake token.
            let mut pending = worker_latest.lock().unwrap();
            if let Some(snapshot) = pending.snapshot.take() {
                *worker_status.lock().unwrap() = write(&path, &snapshot, &pending.audit).map(|_| "saved".into());
            }
        });
        Ok(Self { latest, wake, status })
    }
    pub fn enqueue(&self, snapshot: Snapshot) {
        let mut pending = self.latest.lock().unwrap();
        for event in &snapshot.events {
            if pending.queued.get(&event.session_id).is_none_or(|sequence| event.sequence > *sequence) {
                pending.queued.insert(event.session_id.clone(), event.sequence);
                pending.audit.push(event.clone());
            }
        }
        pending.snapshot = Some(snapshot);
        *self.status.lock().unwrap() = Ok("pending".into());
        let _ = self.wake.try_send(());
    }
    pub fn status(&self) -> Result<String, String> {
        self.status.lock().unwrap().clone()
    }
}
fn write(path: &Path, snapshot: &Snapshot, events: &[ControlEvent]) -> Result<(), String> {
    let parent = path.parent().ok_or("Control store needs a parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let audit = path.with_extension("audit");
    std::fs::create_dir_all(&audit).map_err(|e| e.to_string())?;
    // Independent immutable audit documents survive snapshot retention/compaction.
    for event in events {
        let bytes = serde_json::to_vec(event).map_err(|e| e.to_string())?;
        let digest = tv2_domain::digest::digest_json(&serde_json::to_value(event).map_err(|e| e.to_string())?);
        let destination = audit.join(format!("{}-{}-{digest}.json", event.session_id, event.sequence));
        if !destination.exists() {
            tv2_application::store::atomic_write(&destination, &bytes).map_err(|e| e.to_string())?;
        }
    }
    let bytes = serde_json::to_vec(snapshot).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Control snapshot exceeds 8 MiB; audit archived but snapshot not saved".into());
    }
    tv2_application::store::atomic_write(path, &bytes).map_err(|e| e.to_string())
}
pub(crate) fn read(path: &Path) -> Result<Option<Snapshot>, String> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Control snapshot exceeds 8 MiB".into());
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|e| e.to_string())
}
