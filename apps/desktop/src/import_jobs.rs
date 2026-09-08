//! One media probe at a time; only the UI thread applies completed commands.
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use tv2_domain::{asset::Asset, error::DomainResult};

#[derive(Default)]
pub struct ImportJobs {
    pub pending: VecDeque<(String, PathBuf)>,
    pub running: Option<RunningImport>,
}

pub struct RunningImport {
    pub project: String,
    pub path: PathBuf,
    pub cancel: Arc<AtomicBool>,
    pub result: crossbeam_channel::Receiver<DomainResult<Asset>>,
    thread: Option<JoinHandle<()>>,
}

impl RunningImport {
    pub fn start(project: String, path: PathBuf, tools: tv2_media::FfmpegTools) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let source = path.clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        let thread = std::thread::Builder::new().name("media-import".into()).spawn(move || {
            let value = tools.import_cancellable(&source, source.to_string_lossy().replace('\\', "/"), token);
            let _ = tx.send(value);
        })?;
        Ok(Self { project, path, cancel, result, thread: Some(thread) })
    }
}

impl Drop for RunningImport {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl ImportJobs {
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.pending.is_empty()
    }
    pub fn cancel(&mut self) {
        self.pending.clear();
        if let Some(job) = &self.running {
            job.cancel.store(true, Ordering::Release);
        }
    }
}
