//! One immutable base per gesture; exact domain preview computed off the UI
//! thread. Release commits the latest prepared result, including allocated IDs.
use crate::app::TranscriptorApp;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tv2_application::{CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{Clip, ClipId, Command, LayerId, Project, TimeRange, error::DomainResult};

pub struct Geometry {
    pub layer: LayerId,
    pub ranges: Vec<TimeRange>,
    pub removed: bool,
}
struct ResultPreview {
    key: u64,
    prepared: PreparedCommand,
    geometry: Vec<Geometry>,
    clips: HashMap<ClipId, Clip>,
}
type PreviewReceiver = crossbeam_channel::Receiver<(u64, DomainResult<ResultPreview>)>;
#[derive(Default)]
pub struct GesturePreview {
    base: Option<Arc<Project>>,
    desired: Option<(u64, Command)>,
    generation: u64,
    pending: Option<PreviewReceiver>,
    ready: Option<ResultPreview>,
    pub commit: bool,
    pub error: Option<String>,
}
impl GesturePreview {
    fn current(&self) -> Option<&ResultPreview> {
        self.ready.as_ref().filter(|r| self.desired.as_ref().is_some_and(|(key, _)| *key == r.key))
    }
    pub fn geometry(&self) -> &[Geometry] {
        self.current().map(|r| r.geometry.as_slice()).unwrap_or_default()
    }
    pub fn clip(&self, id: &ClipId) -> Option<&Clip> {
        self.current()?.clips.get(id)
    }
    pub fn active(&self) -> bool {
        self.pending.is_some() || self.commit
    }
}
impl TranscriptorApp {
    pub fn preview_gesture(&mut self, command: Command, release: bool, ctx: &egui::Context) {
        if self.gesture_preview.base.as_ref().is_none_or(|p| p.project_id != self.project().project_id || p.revision != self.session.revision()) {
            self.gesture_preview = GesturePreview { base: Some(Arc::new(self.project().clone())), ..Default::default() };
        }
        // Equality avoids serializing/hash-cloning every selected ID on each
        // stationary pointer frame. A generation binds one exact command/result.
        if self.gesture_preview.desired.as_ref().is_none_or(|(_, old)| old != &command) {
            self.gesture_preview.generation = self.gesture_preview.generation.wrapping_add(1);
            self.gesture_preview.desired = Some((self.gesture_preview.generation, command));
            self.gesture_preview.error = None;
        }
        self.gesture_preview.commit |= release;
        self.poll_gesture_preview(ctx);
    }
    pub fn poll_gesture_preview(&mut self, ctx: &egui::Context) {
        let mut state = std::mem::take(&mut self.gesture_preview);
        if ctx.input(|i| !i.focused || i.key_pressed(egui::Key::Escape)) {
            return;
        }
        if state.base.as_ref().is_some_and(|p| p.project_id != self.project().project_id || p.revision != self.session.revision()) {
            if state.commit {
                self.toast(crate::app::Severity::Warn, "La revisión cambió; se canceló el gesto pendiente");
            }
            return;
        }
        if let Some(rx) = &state.pending {
            let result = match rx.try_recv() {
                Ok((key, value)) => {
                    state.pending = None;
                    if state.desired.as_ref().is_some_and(|(wanted, _)| *wanted == key) { Some(value) } else { None }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => None,
                Err(_) => Some(Err(tv2_domain::DomainError::process("Worker de preview terminó sin resultado"))),
            };
            if let Some(result) = result {
                state.pending = None;
                match result {
                    Ok(result) => state.ready = Some(result),
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
        }
        // A failed dry-run can arrive before release. Releasing that same target
        // must terminate/report it, not leave commit=true spinning indefinitely.
        if state.commit
            && let Some(error) = &state.error
        {
            self.toast(crate::app::Severity::Warn, error.clone());
            return;
        }
        if let Some((key, command)) = &state.desired {
            if state.ready.as_ref().is_some_and(|r| r.key == *key) && state.commit {
                let layer = match command {
                    Command::BoxEdit { layer_id, .. } => Some(layer_id.clone()),
                    _ => None,
                };
                match self.session.commit_prepared(state.ready.take().unwrap().prepared) {
                    Ok(result) => self.accept_command_result(&result, layer),
                    Err(error) => self.report(error),
                }
                return;
            }
            if state.pending.is_none() && state.error.is_none() && state.ready.as_ref().is_none_or(|r| r.key != *key) {
                let base = state.base.as_ref().unwrap().clone();
                let key = *key;
                let command = command.clone();
                let wake = ctx.clone();
                let (tx, rx) = crossbeam_channel::bounded(1);
                match std::thread::Builder::new().name("gesture-preview".into()).spawn(move || {
                    let work = || -> DomainResult<ResultPreview> {
                        let prepared =
                            ProjectSession::new((*base).clone()).prepare_command(CommandEnvelope::human(command).with_base(base.revision))?;
                        let mut geometry = vec![];
                        for layer in &prepared.preview().layers {
                            let before = base.layer(&layer.layer_id);
                            let old: HashMap<_, _> = before.into_iter().flat_map(|l| &l.items).map(|item| (&item.item_id, item)).collect();
                            let new: HashSet<_> = layer.items.iter().map(|item| &item.item_id).collect();
                            for item in &layer.items {
                                if old.get(&item.item_id).copied() != Some(item) {
                                    geometry.push(Geometry { layer: layer.layer_id.clone(), ranges: item.ranges.clone(), removed: false });
                                }
                            }
                            for item in old.values() {
                                if !new.contains(&item.item_id) {
                                    geometry.push(Geometry { layer: layer.layer_id.clone(), ranges: item.ranges.clone(), removed: true });
                                }
                            }
                        }
                        let old: HashMap<_, _> = base.sequences.iter().flat_map(|sequence| &sequence.clips).map(|clip| (&clip.id, clip)).collect();
                        let clips = prepared
                            .preview()
                            .sequences
                            .iter()
                            .flat_map(|sequence| &sequence.clips)
                            .filter(|clip| old.get(&clip.id).copied() != Some(*clip))
                            .map(|clip| (clip.id.clone(), clip.clone()))
                            .collect();
                        Ok(ResultPreview { key, prepared, geometry, clips })
                    };
                    let _ = tx.send((key, work()));
                    wake.request_repaint();
                }) {
                    Ok(_) => state.pending = Some(rx),
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
        }
        self.gesture_preview = state;
    }
}
