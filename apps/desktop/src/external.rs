//! Bounded background rescans; no filesystem read/hash on the frame loop.
use crossbeam_channel::Receiver;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tv2_application::{ProjectStore, reconcile::ExternalChange};
use tv2_domain::{DomainError, Project};

type ProbeResult = (PathBuf, String, u64, Result<Option<ExternalChange>, DomainError>);

pub struct ExternalApply {
    pub store: ProjectStore,
    pub result: Receiver<tv2_domain::error::DomainResult<tv2_application::store::PreparedExternal>>,
}

pub struct ExternalMonitor {
    pub change: Option<ExternalChange>,
    pub error: Option<String>,
    pending: Option<Receiver<ProbeResult>>,
    last_scan: Instant,
    dismissed: Option<String>,
    watcher: Option<crate::watcher::Watcher>,
    notified: bool,
    pub watcher_error: Option<String>,
}

impl Default for ExternalMonitor {
    fn default() -> Self {
        Self {
            change: None,
            error: None,
            pending: None,
            last_scan: Instant::now(),
            dismissed: None,
            watcher: None,
            notified: false,
            watcher_error: None,
        }
    }
}

impl ExternalMonitor {
    pub fn dismiss(&mut self) {
        self.dismissed = self.change.take().map(|c| c.external_digest);
    }

    pub fn poll(&mut self, store: Option<&ProjectStore>, local: &Project, ctx: &egui::Context) {
        if self.watcher.as_ref().map(|w| &w.root) != store.map(|s| &s.root) {
            self.watcher = None;
            if let Some(store) = store {
                match crate::watcher::Watcher::start(store.root.clone(), ctx.clone()) {
                    Ok(w) => self.watcher = Some(w),
                    Err(e) => self.watcher_error = Some(e.to_string()),
                }
            }
        }
        if let Some(result) = self.watcher.as_ref().and_then(|w| w.changed()) {
            match result {
                Ok(()) => self.notified = true,
                Err(e) => self.watcher_error = Some(format!("Watcher: {e}. Rescaneo periódico activo.")),
            }
        }
        if self.change.as_ref().is_some_and(|c| c.base_revision != local.revision) {
            self.change = None;
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok((root, project_id, revision, result)) => {
                    self.pending = None;
                    if store.is_some_and(|s| s.root == root) && local.project_id == project_id && local.revision == revision {
                        match result {
                            Ok(change) => {
                                self.error = None;
                                if change.as_ref().map(|c| &c.external_digest) != self.change.as_ref().map(|c| &c.external_digest) {
                                    self.change = change.filter(|c| self.dismissed.as_ref() != Some(&c.external_digest));
                                }
                            }
                            Err(e) => {
                                self.change = None;
                                self.error = Some(e.to_string());
                            }
                        }
                    }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.error = Some("Falló el lector externo".into());
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none()
            && (self.last_scan.elapsed() >= Duration::from_secs(2) || self.notified && self.last_scan.elapsed() >= Duration::from_millis(200))
            && let Some(store) = store
        {
            self.last_scan = Instant::now();
            self.notified = false;
            let store = store.clone();
            let local = local.clone();
            let wake = ctx.clone();
            let (tx, rx) = crossbeam_channel::bounded(1);
            match std::thread::Builder::new().name("external-project-reader".into()).spawn(move || {
                let result = store.external_change(&local);
                let _ = tx.send((store.root, local.project_id, local.revision, result));
                wake.request_repaint();
            }) {
                Ok(_) => self.pending = Some(rx),
                Err(e) => self.error = Some(format!("No se pudo iniciar lectura externa: {e}")),
            }
        }
        if store.is_some() {
            ctx.request_repaint_after(if self.notified { Duration::from_millis(200) } else { Duration::from_secs(2) });
        }
    }
}
