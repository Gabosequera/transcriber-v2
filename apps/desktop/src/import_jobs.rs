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

pub struct RelinkJob {
    pub probe: RunningImport,
    pub asset_id: tv2_domain::ids::AssetId,
    pub revision: u64,
}

impl crate::app::TranscriptorApp {
    pub fn start_relink(&mut self, asset_id: tv2_domain::ids::AssetId, path: PathBuf) {
        if self.relink_job.is_some() {
            self.toast(crate::app::Severity::Warn, "Hay un reenlace en curso; espera o cancélalo");
            return;
        }
        let tools = match &self.tools {
            Ok(tools) => tools.clone(),
            Err(e) => {
                self.toast(crate::app::Severity::Error, e.clone());
                return;
            }
        };
        match RunningImport::start(self.project().project_id.clone(), path, tools) {
            Ok(probe) => self.relink_job = Some(RelinkJob { probe, asset_id, revision: self.session.revision() }),
            Err(e) => self.report(e.into()),
        }
    }

    pub fn poll_relink(&mut self) {
        let Some(job) = &self.relink_job else { return };
        let result = match job.probe.result.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(_) => Err(tv2_domain::DomainError::process("El worker de reenlace terminó sin resultado")),
        };
        let job = self.relink_job.take().unwrap();
        if job.probe.cancel.load(Ordering::Acquire) {
            return;
        }
        if job.probe.project != self.project().project_id || job.revision != self.session.revision() {
            self.toast(crate::app::Severity::Warn, "El proyecto cambió durante el reenlace; repítelo sobre la revisión actual");
            return;
        }
        match result {
            Ok(asset) => {
                self.exec(tv2_domain::Command::RelinkAsset { asset_id: job.asset_id, path: asset.path.clone(), asset: Some(asset) });
            }
            Err(e) => self.report(e),
        }
    }
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
        // A filesystem read can remain blocked despite cancellation. The worker
        // owns read-only inputs and its child process observes the token; never
        // wait for an unbounded storage read on the UI/shutdown thread.
        if let Some(thread) = self.thread.take().filter(|thread| thread.is_finished()) {
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
