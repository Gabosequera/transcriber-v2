//! Modo guion (`--script pasos.json`): ejecuta pasos sobre la aplicación real
//! (mismos métodos que disparan los botones/atajos) con capturas de pantalla
//! de la ventana nativa. Sirve para evidencia reproducible y para pruebas sin
//! intervención manual. No sustituye la prueba de gestos de ratón.

use crate::app::{Severity, TranscriptorApp, ViewMode};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tv2_domain::ids::{ClipId, LayerId};
use tv2_domain::time::Ticks;
use tv2_domain::{Command, DomainError, ErrorCode, MovePolicy, error::DomainResult};
use tv2_media::player::PlayerCommand;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportSelection {
    Clips,
    Items,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptConflictChoice {
    Local,
    External,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    ControlSetup {
        pipe_name: String,
        permissions: tv2_control::Permissions,
    },
    ControlRevoke,
    ControlStop,
    AwaitSignal {
        path: String,
        #[serde(default = "d120000")]
        ms: u64,
    },
    Import {
        path: String,
    },
    Insert {
        asset: usize,
        #[serde(default)]
        at: Option<f64>,
    },
    Seek {
        t: f64,
    },
    Play,
    Pause,
    Rate {
        value: f64,
    },
    Wait {
        ms: u64,
    },
    AssertCaches {
        wave_columns_min: usize,
        thumb_tiles_min: usize,
    },
    Viewport {
        width: f32,
        height: f32,
        zoom: f32,
    },
    /// Espera a que el reproductor haya presentado un fotograma nuevo (o al timeout).
    WaitFrame {
        #[serde(default = "d2000")]
        ms: u64,
    },
    Split,
    SelectClip {
        index: usize,
    },
    SelectClips {
        indices: Vec<usize>,
    },
    Shift {
        delta: f64,
        #[serde(default)]
        tracks: i32,
        #[serde(default)]
        expect_error: Option<tv2_domain::ErrorCode>,
    },
    Move {
        clip: usize,
        to: f64,
        #[serde(default)]
        policy: Option<String>,
    },
    Action {
        id: String,
    },
    NewLayer {
        name: String,
    },
    SetIn {
        t: f64,
    },
    SetOut {
        t: f64,
    },
    AddRange,
    SelectItem {
        layer: usize,
        index: usize,
    },
    View {
        mode: String,
    },
    Save {
        path: String,
    },
    Open {
        path: String,
    },
    NewProject,
    Export {
        preset: String,
        dest: String,
        #[serde(default)]
        range: bool,
        #[serde(default)]
        selection: Option<ExportSelection>,
    },
    WaitExport {
        #[serde(default = "d120000")]
        ms: u64,
    },
    ExportV1Folder {
        dest: String,
        #[serde(default)]
        include_montage: bool,
        #[serde(default = "d120000")]
        ms: u64,
        #[serde(default)]
        expect_error: Option<ErrorCode>,
    },
    WaitExternal {
        #[serde(default = "d120000")]
        ms: u64,
        #[serde(default)]
        fields: Option<usize>,
        #[serde(default)]
        conflicts: Option<usize>,
    },
    ApplyExternal {
        #[serde(default)]
        human_review: bool,
        #[serde(default)]
        choices: std::collections::BTreeMap<String, ScriptConflictChoice>,
        #[serde(default = "d120000")]
        ms: u64,
        #[serde(default)]
        expect_error: Option<ErrorCode>,
    },
    Screenshot {
        path: String,
    },
    Dump {
        path: String,
    },
    Undo,
    Redo,
    Assert {
        markers: Option<usize>,
        clips: Option<usize>,
        layers: Option<usize>,
        items: Option<usize>,
        revision: Option<u64>,
        playing: Option<bool>,
        #[serde(default)]
        rate: Option<f64>,
        /// Posición del reproductor en segundos: [mín, máx].
        #[serde(default)]
        position: Option<[f64; 2]>,
        #[serde(default)]
        tracks: Option<usize>,
    },
    /// Guarda el último fotograma presentado por el visor como PNG.
    SaveFrame {
        path: String,
    },
    SetLoop {
        start: f64,
        end: f64,
    },
    ClearLoop,
    #[serde(rename = "step_frames")]
    AdvanceFrames {
        frames: i64,
    },
    Mute {
        muted: bool,
    },
    ClipProps {
        clip: usize,
        #[serde(default)]
        scale: Option<f32>,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
        #[serde(default)]
        opacity: Option<f32>,
        #[serde(default)]
        gain_db: Option<f32>,
    },
    TrackProps {
        track: String,
        #[serde(default)]
        muted: Option<bool>,
        #[serde(default)]
        solo: Option<bool>,
        #[serde(default)]
        visible: Option<bool>,
        #[serde(default)]
        gain_db: Option<f32>,
    },
    ImportV1 {
        path: String,
        #[serde(default)]
        media: Option<String>,
    },
    Log {
        text: String,
    },
    Quit,
}

fn d2000() -> u64 {
    2000
}
fn d120000() -> u64 {
    120_000
}

fn receipt_valid<T>(result: &DomainResult<T>, received: bool, expected: Option<ErrorCode>, before: u64, after: u64, changes_project: bool) -> bool {
    received
        && match (result, expected) {
            (Ok(_), None) => {
                if changes_project {
                    after > before
                } else {
                    after == before
                }
            }
            (Err(error), Some(code)) => error.code == code && after == before,
            _ => false,
        }
}

pub struct ScriptRunner {
    steps: Vec<Step>,
    idx: usize,
    wait_until: Option<Instant>,
    waiting_frame: Option<(Ticks, Instant, u64)>,
    requested_frame: Option<Ticks>,
    waiting_export: Option<(Instant, u64)>,
    waiting_document: Option<(Instant, u64, Option<ErrorCode>, u64)>,
    document_result: Option<DomainResult<PathBuf>>,
    waiting_external: Option<(Instant, u64, Option<usize>, Option<usize>)>,
    waiting_external_apply: Option<(Instant, u64, Option<ErrorCode>, u64)>,
    external_apply_result: Option<DomainResult<()>>,
    waiting_signal: Option<(PathBuf, Instant, u64)>,
    pending_screenshot: Option<PathBuf>,
    pub log: Vec<String>,
    pub failed: bool,
    log_path: PathBuf,
}

impl ScriptRunner {
    pub fn load(path: &str) -> Result<ScriptRunner, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let steps: Vec<Step> = serde_json::from_str(&text).map_err(|e| format!("guion inválido: {e}"))?;
        let log_path = PathBuf::from(path).with_extension("log");
        Ok(ScriptRunner {
            steps,
            idx: 0,
            wait_until: None,
            waiting_frame: None,
            requested_frame: None,
            waiting_export: None,
            waiting_document: None,
            document_result: None,
            waiting_external: None,
            waiting_external_apply: None,
            external_apply_result: None,
            waiting_signal: None,
            pending_screenshot: None,
            log: Vec::new(),
            failed: false,
            log_path,
        })
    }

    fn note(&mut self, text: String) {
        tracing::info!("[script] {text}");
        self.log.push(text);
        let _ = std::fs::write(&self.log_path, self.log.join("\n") + "\n");
    }
}

impl TranscriptorApp {
    pub(crate) fn script_document_export_result(&mut self, result: &DomainResult<PathBuf>) {
        if let Some(runner) = &mut self.script
            && runner.waiting_document.is_some()
        {
            runner.document_result = Some(result.clone());
        }
    }
    pub(crate) fn script_external_apply_result(&mut self, result: &DomainResult<()>) {
        if let Some(runner) = &mut self.script
            && runner.waiting_external_apply.is_some()
        {
            runner.external_apply_result = Some(result.clone());
        }
    }
    pub fn script_tick(&mut self, ctx: &egui::Context) {
        let Some(mut runner) = self.script.take() else { return };
        if let Some((path, started, ms)) = &runner.waiting_signal {
            if !path.is_file() {
                if started.elapsed() < Duration::from_millis(*ms) {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    self.script = Some(runner);
                    return;
                }
                runner.note(format!("ERROR: señal no recibida antes del timeout: {}", path.display()));
                runner.failed = true;
            }
            runner.waiting_signal = None;
        }
        // capturas entregadas por eframe
        if let Some(path) = runner.pending_screenshot.clone() {
            let shot: Option<std::sync::Arc<egui::ColorImage>> = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = shot {
                let [w, h] = img.size;
                let raw: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255u8]).collect();
                match image::save_buffer(&path, &raw, w as u32, h as u32, image::ColorType::Rgba8) {
                    Ok(()) => runner.note(format!("captura guardada en {} ({w}×{h})", path.display())),
                    Err(e) => runner.note(format!("ERROR captura: {e}")),
                }
                runner.pending_screenshot = None;
            } else {
                ctx.request_repaint();
                self.script = Some(runner);
                return;
            }
        }
        if let Some(until) = runner.wait_until {
            if Instant::now() < until {
                ctx.request_repaint_after(Duration::from_millis(16));
                self.script = Some(runner);
                return;
            }
            runner.wait_until = None;
        }
        if let Some((target, started, ms)) = runner.waiting_frame {
            let presented = self.last_frame.as_ref().map(|f| f.position);
            if presented != Some(target) && started.elapsed() < Duration::from_millis(ms.max(500)) {
                ctx.request_repaint_after(Duration::from_millis(16));
                self.script = Some(runner);
                return;
            }
            if presented != Some(target) {
                runner.note(format!(
                    "ERROR: fotograma esperado {} no presentado (último {:?})",
                    target.timecode_ms(),
                    presented.map(|p| p.timecode_ms())
                ));
                runner.failed = true;
            }
            runner.note(format!(
                "fotograma presentado: seq {} (posición {})",
                self.frame_seen,
                self.last_frame.as_ref().map(|f| f.position.timecode_ms()).unwrap_or_default()
            ));
            runner.waiting_frame = None;
        }
        if let Some((started, ms)) = runner.waiting_export {
            if (self.export.running.is_some() || !self.export.pending.is_empty()) && started.elapsed() < Duration::from_millis(ms) {
                ctx.request_repaint_after(Duration::from_millis(50));
                self.script = Some(runner);
                return;
            }
            match &self.export.last_result {
                Some(Ok(r)) => runner.note(format!(
                    "export OK: {} · {} · {} frames · sha256 {}",
                    r.path.display(),
                    r.duration.timecode_ms(),
                    r.frames_written,
                    r.sha256
                )),
                Some(Err(e)) => {
                    runner.note(format!("ERROR export: {e}"));
                    runner.failed = true;
                }
                None => {
                    runner.note("ERROR export: sin resultado (timeout)".into());
                    runner.failed = true;
                }
            }
            runner.waiting_export = None;
        }
        if let Some((started, ms, expected, revision)) = runner.waiting_document {
            if runner.document_result.is_none() && started.elapsed() < Duration::from_millis(ms) {
                ctx.request_repaint_after(Duration::from_millis(50));
                self.script = Some(runner);
                return;
            }
            let received = runner.document_result.is_some();
            let result = runner.document_result.take().unwrap_or_else(|| Err(DomainError::process("Timeout de export documental")));
            let valid = receipt_valid(&result, received, expected, revision, self.session.revision(), false);
            runner.note(format!("EXPORT_V1 resultado={result:?}, esperado={expected:?}, revisión={}", self.session.revision()));
            runner.failed |= !valid;
            runner.waiting_document = None;
        }
        if let Some((started, ms, fields, conflicts)) = runner.waiting_external {
            let ready = self.external.change.as_ref().is_some_and(|change| {
                fields.is_none_or(|expected| change.fields.len() == expected) && conflicts.is_none_or(|expected| change.conflicts.len() == expected)
            });
            if !ready && started.elapsed() < Duration::from_millis(ms) {
                ctx.request_repaint_after(Duration::from_millis(50));
                self.script = Some(runner);
                return;
            }
            runner.note(format!(
                "EXTERNAL ready={ready}, fields={:?}, conflicts={:?}, error={:?}",
                self.external.change.as_ref().map(|c| c.fields.len()),
                self.external.change.as_ref().map(|c| c.conflicts.len()),
                self.external.error
            ));
            runner.failed |= !ready;
            runner.waiting_external = None;
        }
        if let Some((started, ms, expected, revision)) = runner.waiting_external_apply {
            if runner.external_apply_result.is_none() && started.elapsed() < Duration::from_millis(ms) {
                ctx.request_repaint_after(Duration::from_millis(50));
                self.script = Some(runner);
                return;
            }
            let received = runner.external_apply_result.is_some();
            let result = runner.external_apply_result.take().unwrap_or_else(|| Err(DomainError::process("Timeout de Apply externo")));
            let valid = receipt_valid(&result, received, expected, revision, self.session.revision(), true);
            runner.note(format!("APPLY_EXTERNAL resultado={result:?}, esperado={expected:?}, revisión={}", self.session.revision()));
            runner.failed |= !valid;
            runner.waiting_external_apply = None;
        }
        if self.imports.busy()
            || self.editorial_job.is_some()
            || self.persistence_job.is_some()
            || self.autosave_job.is_some()
            || self.v1_export_job.is_some()
            || self.external_apply.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(16));
            self.script = Some(runner);
            return;
        }
        if runner.idx >= runner.steps.len() {
            runner.note(format!("guion terminado; fallos={}", runner.failed));
            self.script = Some(runner);
            return;
        }
        let step = runner.steps[runner.idx].clone();
        runner.idx += 1;
        runner.note(format!("paso {}: {:?}", runner.idx, step));
        match step {
            Step::ControlSetup { pipe_name, permissions } => self.script_control_setup(pipe_name, permissions, ctx),
            Step::ControlRevoke => self.script_control_revoke(),
            Step::ControlStop => self.script_control_stop(),
            Step::AwaitSignal { path, ms } => runner.waiting_signal = Some((PathBuf::from(path), Instant::now(), ms)),
            Step::AssertCaches { wave_columns_min, thumb_tiles_min } => {
                let (wave, thumbs) = self.media_view.as_ref().map(|m| (m.wave_columns, m.thumb_tiles)).unwrap_or_default();
                let ok = wave >= wave_columns_min && thumbs >= thumb_tiles_min;
                runner.note(format!("caches: wave_columns={wave}, thumb_tiles={thumbs}, OK={ok}"));
                runner.failed |= !ok;
            }
            Step::Viewport { width, height, zoom } => {
                ctx.set_zoom_factor(zoom);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
            }
            Step::Import { path } => self.import_paths(&[PathBuf::from(path)]),
            Step::Insert { asset, at } => {
                let Some(a) = self.project().assets.get(asset).map(|a| a.id.clone()) else {
                    runner.note("ERROR: asset inexistente".into());
                    runner.failed = true;
                    self.script = Some(runner);
                    return;
                };
                if let Some(t) = at {
                    self.playhead = Ticks::from_seconds_f64(t);
                }
                self.insert_asset_at_playhead(a, None);
            }
            Step::Seek { t } => {
                self.seek(Ticks::from_seconds_f64(t));
                // A frame still queued from before the seek can update the UI
                // playhead. Keep the requested target separate for verification.
                runner.requested_frame = Some(self.playhead.floor_to_frame(self.frame_rate()));
            }
            Step::Play => self.player_send(PlayerCommand::Play),
            Step::Pause => self.player_send(PlayerCommand::Pause),
            Step::Rate { value } => self.set_rate(value),
            Step::Wait { ms } => runner.wait_until = Some(Instant::now() + Duration::from_millis(ms)),
            Step::WaitFrame { ms } => {
                let target = runner.requested_frame.take().unwrap_or_else(|| self.playhead.floor_to_frame(self.frame_rate()));
                runner.waiting_frame = Some((target, Instant::now(), ms));
                runner.wait_until = None;
            }
            Step::Split => self.split_at_playhead(),
            Step::SelectClip { index } => {
                let ids = self.sorted_clip_ids();
                match ids.get(index) {
                    Some(id) => {
                        self.selection.clips = vec![id.clone()];
                        self.selection.items.clear();
                    }
                    None => {
                        runner.note("ERROR: clip inexistente".into());
                        runner.failed = true;
                    }
                }
            }
            Step::SelectClips { indices } => {
                let ids = self.sorted_clip_ids();
                self.selection.clips = indices.iter().filter_map(|i| ids.get(*i).cloned()).collect();
            }
            Step::Shift { delta, tracks, expect_error } => {
                let ids = self.linked_selection();
                let revision = self.project().revision;
                let result = self.exec_checked(Command::ShiftClips {
                    clip_ids: ids,
                    delta: Ticks::from_seconds_f64(delta),
                    track_delta: tracks,
                    policy: MovePolicy::Reject,
                });
                let valid = match (&result, expect_error) {
                    (Ok(()), None) => true,
                    (Err(error), Some(expected)) => error.code == expected && self.project().revision == revision,
                    _ => false,
                };
                runner.note(format!("SHIFT resultado={result:?}, esperado={expect_error:?}, revisión={}", self.project().revision));
                if !valid {
                    runner.failed = true;
                }
            }
            Step::Move { clip, to, policy } => {
                let ids = self.sorted_clip_ids();
                if let Some(id) = ids.get(clip).cloned() {
                    let policy = match policy.as_deref() {
                        Some("insert") => MovePolicy::Insert,
                        Some("overwrite") => MovePolicy::Overwrite,
                        _ => MovePolicy::Reject,
                    };
                    self.exec(Command::MoveClip { clip_id: id, position: Ticks::from_seconds_f64(to), track_id: None, policy });
                }
            }
            Step::Action { id } => self.dispatch(&id),
            Step::NewLayer { name } => self.create_layer(name),
            Step::SetIn { t } => self.in_point = Some(Ticks::from_seconds_f64(t)),
            Step::SetOut { t } => self.out_point = Some(Ticks::from_seconds_f64(t)),
            Step::AddRange => self.add_range_to_layer(),
            Step::SelectItem { layer, index } => {
                let layers: Vec<LayerId> = self.project().ordered_layers().iter().map(|l| l.layer_id.clone()).collect();
                if let Some(l) = layers.get(layer).cloned() {
                    let item = self.project().layer(&l).and_then(|x| x.items.get(index)).map(|i| i.item_id.clone());
                    if let Some(i) = item {
                        self.selection.items = vec![(l.clone(), i)];
                        self.selection.layer = Some(l);
                    }
                }
            }
            Step::View { mode } => {
                let want = if mode == "source" { ViewMode::Source } else { ViewMode::Sequence };
                if self.view != want {
                    self.toggle_view();
                }
            }
            Step::Save { path } => {
                let target = tv2_application::ProjectStore::from_user_path(&PathBuf::from(path));
                // Reuse the observed baseline for repeated saves; for Save As,
                // keep the source store until the asynchronous copy succeeds.
                let target = self.store.as_ref().filter(|store| store.root == target.root).cloned().unwrap_or(target);
                let ok = self.save_project_to(target);
                if !ok {
                    runner.failed = true;
                }
            }
            Step::Open { path } => self.open_project_path(&PathBuf::from(path)),
            Step::NewProject => self.new_project(),
            Step::Export { preset, dest, range, selection } => {
                let presets = tv2_media::export::presets();
                match presets.into_iter().find(|p| p.id == preset) {
                    Some(p) => {
                        self.export.destination = dest;
                        self.export.range_mode = if range { 1 } else { 0 };
                        if let Some(selection) = selection {
                            self.export.range_mode = match selection {
                                ExportSelection::Clips => 2,
                                ExportSelection::Items => 3,
                            };
                        }
                        self.start_export(p);
                    }
                    None => {
                        runner.note("ERROR: preset desconocido".into());
                        runner.failed = true;
                    }
                }
            }
            Step::WaitExport { ms } => runner.waiting_export = Some((Instant::now(), ms)),
            Step::ExportV1Folder { dest, include_montage, ms, expect_error } => {
                runner.document_result = None;
                runner.waiting_document = Some((Instant::now(), ms.clamp(1, 120_000), expect_error, self.session.revision()));
                if let Err(error) = self.export_v1_folder_to(PathBuf::from(dest), include_montage) {
                    runner.document_result = Some(Err(error));
                }
            }
            Step::WaitExternal { ms, fields, conflicts } => runner.waiting_external = Some((Instant::now(), ms.clamp(1, 120_000), fields, conflicts)),
            Step::ApplyExternal { human_review, choices, ms, expect_error } => {
                runner.external_apply_result = None;
                runner.waiting_external_apply = Some((Instant::now(), ms.clamp(1, 120_000), expect_error, self.session.revision()));
                let result = (|| -> DomainResult<()> {
                    let change = self.external.change.as_mut().ok_or_else(|| DomainError::precondition("No hay cambio externo revisable"))?;
                    let paths: std::collections::BTreeSet<_> = change.conflicts.iter().map(|c| c.path.clone()).collect();
                    if choices.keys().cloned().collect::<std::collections::BTreeSet<_>>() != paths {
                        return Err(DomainError::invalid("Las elecciones deben nombrar exactamente los conflictos revisados"));
                    }
                    change.choices = choices
                        .into_iter()
                        .map(|(path, choice)| {
                            (
                                path,
                                match choice {
                                    ScriptConflictChoice::Local => tv2_application::reconcile::ConflictChoice::Local,
                                    ScriptConflictChoice::External => tv2_application::reconcile::ConflictChoice::External,
                                },
                            )
                        })
                        .collect();
                    change.human_review = human_review;
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        if let Err(error) = self.start_external_apply() {
                            runner.external_apply_result = Some(Err(error));
                        }
                    }
                    Err(error) => runner.external_apply_result = Some(Err(error)),
                }
            }
            Step::Screenshot { path } => {
                let p = PathBuf::from(path);
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                runner.pending_screenshot = Some(p);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            Step::Dump { path } => {
                let snap = self.player_snapshot.clone();
                let seq = self.sequence().cloned();
                let value = serde_json::json!({
                    "project_id": self.project().project_id,
                    "name": self.project().name,
                    "revision": self.session.revision(),
                    "view": format!("{:?}", self.view),
                    "playhead_ms": self.playhead.as_millis_round(),
                    "player": {"position_ms": snap.position.as_millis_round(), "playing": snap.playing, "rate": snap.rate, "generation": snap.generation, "buffering": snap.buffering, "decode_ms": snap.decode_ms, "audio_available": snap.audio_available},
                    "last_frame": self.last_frame.as_ref().map(|f| serde_json::json!({"position_ms": f.position.as_millis_round(), "w": f.frame.width, "h": f.frame.height, "generation": f.generation})),
                    "clips": seq.as_ref().map(|s| s.clips.iter().map(|c| serde_json::json!({"id": c.id, "track": s.track(&c.track_id).map(|t| t.name.clone()), "position_ms": c.position.as_millis_round(), "source_start_ms": c.source.start.as_millis_round(), "source_end_ms": c.source.end.as_millis_round(), "enabled": c.enabled, "link": c.link_group})).collect::<Vec<_>>()),
                    "layers": self.project().layers.iter().map(|l| serde_json::json!({"id": l.layer_id, "name": l.name, "items": l.items.iter().map(|i| serde_json::json!({"id": i.item_id, "label": i.label, "state": i.state.as_str(), "ranges": i.ranges.iter().map(|r| [r.start.as_seconds_ms(), r.end.as_seconds_ms()]).collect::<Vec<_>>()})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
                    "toasts": self.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>(),
                    "undo": self.session.undo_label(),
                    "external": {"error": self.external.error, "watcher_error": self.external.watcher_error,
                        "change": self.external.change.as_ref().map(|c| serde_json::json!({"base_revision": c.base_revision,
                            "local_digest": c.local_digest, "external_digest": c.external_digest, "fields": c.fields,
                            "conflicts": c.conflicts.iter().map(|conflict| serde_json::json!({"path": conflict.path,
                                "base": conflict.base, "local": conflict.local, "external": conflict.external})).collect::<Vec<_>>(),
                            "choices": c.choices.iter().map(|(path, choice)| (path.clone(), format!("{choice:?}"))).collect::<std::collections::BTreeMap<_, _>>(),
                            "human_review": c.human_review}))},
                });
                let p = PathBuf::from(path);
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&p, serde_json::to_string_pretty(&value).unwrap()) {
                    Ok(()) => runner.note(format!("estado volcado en {}", p.display())),
                    Err(e) => runner.note(format!("ERROR dump: {e}")),
                }
            }
            Step::Undo => self.undo(),
            Step::Redo => self.redo(),
            Step::SaveFrame { path } => match &self.last_frame {
                Some(f) => {
                    let p = PathBuf::from(&path);
                    if let Some(parent) = p.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match image::save_buffer(&p, &f.frame.rgba, f.frame.width, f.frame.height, image::ColorType::Rgba8) {
                        Ok(()) => runner.note(format!("fotograma del visor ({}) guardado en {}", f.position.timecode_ms(), p.display())),
                        Err(e) => runner.note(format!("ERROR save_frame: {e}")),
                    }
                }
                None => {
                    runner.note("ERROR save_frame: sin fotograma".into());
                    runner.failed = true;
                }
            },
            Step::SetLoop { start, end } => {
                let r = tv2_domain::time::TimeRange::new(Ticks::from_seconds_f64(start), Ticks::from_seconds_f64(end));
                self.loop_range = Some(r);
                self.player_send(PlayerCommand::SetLoop(Some(r)));
            }
            Step::ClearLoop => self.dispatch("loop.clear"),
            Step::AdvanceFrames { frames } => self.player_send(PlayerCommand::StepFrames(frames)),
            Step::Mute { muted } => self.player_send(PlayerCommand::SetMuted(muted)),
            Step::ClipProps { clip, scale, x, y, opacity, gain_db } => {
                let ids = self.sorted_clip_ids();
                if let Some(id) = ids.get(clip).cloned() {
                    let mut t = self.sequence().and_then(|s| s.clip(&id)).map(|c| c.transform).unwrap_or_default();
                    if let Some(v) = scale {
                        t.scale = v;
                    }
                    if let Some(v) = x {
                        t.x = v;
                    }
                    if let Some(v) = y {
                        t.y = v;
                    }
                    if let Some(v) = opacity {
                        t.opacity = v;
                    }
                    self.exec(Command::SetClipProps { clip_id: id, name: None, gain_db, transform: Some(t) });
                } else {
                    runner.note("ERROR: clip inexistente".into());
                    runner.failed = true;
                }
            }
            Step::TrackProps { track, muted, solo, visible, gain_db } => {
                let id = self.sequence().and_then(|s| s.tracks.iter().find(|t| t.name == track)).map(|t| t.id.clone());
                match id {
                    Some(id) => {
                        self.exec(Command::SetTrackProps { track_id: id, name: None, muted, solo, locked: None, visible, gain_db, height: None });
                    }
                    None => {
                        runner.note(format!("ERROR: pista {track} inexistente"));
                        runner.failed = true;
                    }
                }
            }
            Step::Assert { clips, layers, items, revision, playing, rate, position, tracks, markers } => {
                let n_clips = self.sequence().map(|s| s.clips.len()).unwrap_or(0);
                let n_layers = self.project().layers.iter().filter(|l| !l.deleted).count();
                let n_items: usize = self.project().layers.iter().map(|l| l.items.len()).sum();
                let mut ok = true;
                if let Some(expected) = markers {
                    let actual = self.sequence().map(|s| s.markers.len()).unwrap_or(0);
                    if actual != expected {
                        runner.note(format!("ASSERT markers: esperado {expected}, real {actual}"));
                        ok = false;
                    }
                }
                if let Some(c) = clips
                    && c != n_clips
                {
                    runner.note(format!("ASSERT clips: esperado {c}, real {n_clips}"));
                    ok = false;
                }
                if let Some(c) = layers
                    && c != n_layers
                {
                    runner.note(format!("ASSERT layers: esperado {c}, real {n_layers}"));
                    ok = false;
                }
                if let Some(c) = items
                    && c != n_items
                {
                    runner.note(format!("ASSERT items: esperado {c}, real {n_items}"));
                    ok = false;
                }
                if let Some(r) = revision
                    && r != self.session.revision()
                {
                    runner.note(format!("ASSERT revision: esperado {r}, real {}", self.session.revision()));
                    ok = false;
                }
                if let Some(p) = playing
                    && p != self.player_snapshot.playing
                {
                    runner.note(format!("ASSERT playing: esperado {p}, real {}", self.player_snapshot.playing));
                    ok = false;
                }
                if let Some(r) = rate
                    && (r - self.player_snapshot.rate).abs() > 1e-6
                {
                    runner.note(format!("ASSERT rate: esperado {r}, real {}", self.player_snapshot.rate));
                    ok = false;
                }
                if let Some([lo, hi]) = position {
                    let pos = self.player_snapshot.position.as_seconds_f64();
                    if pos < lo || pos > hi {
                        runner.note(format!("ASSERT position: esperado [{lo}, {hi}], real {pos:.3}"));
                        ok = false;
                    }
                }
                if let Some(t) = tracks
                    && t != self.sequence().map(|s| s.tracks.len()).unwrap_or(0)
                {
                    runner.note(format!("ASSERT tracks: esperado {t}, real {}", self.sequence().map(|s| s.tracks.len()).unwrap_or(0)));
                    ok = false;
                }
                if ok {
                    runner.note(format!(
                        "assert OK (clips {n_clips}, capas {n_layers}, items {n_items}, rev {}, pos {:.3}, rate {}, playing {})",
                        self.session.revision(),
                        self.player_snapshot.position.as_seconds_f64(),
                        self.player_snapshot.rate,
                        self.player_snapshot.playing
                    ));
                } else {
                    runner.failed = true;
                }
            }
            Step::ImportV1 { path, media } => self.import_v1_folder(&PathBuf::from(path), media.map(PathBuf::from)),
            Step::Log { text } => runner.note(text),
            Step::Quit => {
                runner.note(format!("quit; fallos={}", runner.failed));
                self.session.mark_clean();
                self.pending_close = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if !self.toasts.is_empty() {
            let last = self.toasts.last().map(|t| format!("{:?}: {}", t.severity, t.text)).unwrap_or_default();
            runner.note(format!("  toast: {last}"));
        }
        ctx.request_repaint();
        self.script = Some(runner);
    }

    fn sorted_clip_ids(&self) -> Vec<ClipId> {
        let Some(seq) = self.sequence() else { return Vec::new() };
        let mut v: Vec<&tv2_domain::timeline::Clip> = seq.clips.iter().collect();
        v.sort_by_key(|c| (c.position, seq.track_index(&c.track_id).unwrap_or(0), c.id.clone()));
        v.iter().map(|c| c.id.clone()).collect()
    }
}

#[allow(dead_code)]
fn _sev(_: Severity) {}

#[cfg(test)]
mod document_external_script_tests {
    use super::*;

    #[test]
    fn timeout_cannot_satisfy_expected_worker_error_or_mask_a_revision_change() {
        let failure: DomainResult<()> = Err(DomainError::process("worker failed"));
        assert!(receipt_valid(&failure, true, Some(ErrorCode::Process), 3, 3, false));
        assert!(!receipt_valid(&failure, false, Some(ErrorCode::Process), 3, 3, false));
        assert!(!receipt_valid(&failure, true, Some(ErrorCode::Process), 3, 4, true));
        assert!(!receipt_valid(&failure, true, Some(ErrorCode::Io), 3, 3, true));
        assert!(!receipt_valid(&Ok(()), true, None, 3, 3, true));
        assert!(receipt_valid(&Ok(()), true, None, 3, 4, true));
        assert!(!receipt_valid(&Ok(()), true, None, 3, 4, false));
    }

    #[test]
    fn external_apply_requires_explicit_human_review_and_typed_choices() {
        let step: Step = serde_json::from_str(r#"{"op":"apply_external"}"#).unwrap();
        assert!(matches!(step, Step::ApplyExternal { human_review: false, choices, .. } if choices.is_empty()));
        assert!(serde_json::from_str::<Step>(r#"{"op":"apply_external","choices":{"/name":"automatic"}}"#).is_err());
    }
}
