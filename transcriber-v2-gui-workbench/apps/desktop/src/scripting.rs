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
use tv2_domain::{Command, MovePolicy};
use tv2_media::player::PlayerCommand;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportSelection {
    Clips,
    Items,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
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

pub struct ScriptRunner {
    steps: Vec<Step>,
    idx: usize,
    wait_until: Option<Instant>,
    waiting_frame: Option<(u64, Instant, u64)>,
    waiting_export: Option<(Instant, u64)>,
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
            waiting_export: None,
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
    pub fn script_tick(&mut self, ctx: &egui::Context) {
        let Some(mut runner) = self.script.take() else { return };
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
        if let Some((_seen, started, ms)) = runner.waiting_frame {
            let target = self.playhead.floor_to_frame(self.frame_rate());
            let presented = self.last_frame.as_ref().map(|f| f.position);
            if presented != Some(target) && started.elapsed() < Duration::from_millis(ms.max(500)) {
                ctx.request_repaint_after(Duration::from_millis(16));
                self.script = Some(runner);
                return;
            }
            if presented != Some(target) {
                runner.note(format!(
                    "AVISO: fotograma esperado {} no presentado (último {:?})",
                    target.timecode_ms(),
                    presented.map(|p| p.timecode_ms())
                ));
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
        if self.imports.busy() {
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
            Step::Seek { t } => self.seek(Ticks::from_seconds_f64(t)),
            Step::Play => self.player_send(PlayerCommand::Play),
            Step::Pause => self.player_send(PlayerCommand::Pause),
            Step::Rate { value } => self.set_rate(value),
            Step::Wait { ms } => runner.wait_until = Some(Instant::now() + Duration::from_millis(ms)),
            Step::WaitFrame { ms } => {
                runner.waiting_frame = Some((self.frame_seen, Instant::now(), ms));
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
                self.store = Some(tv2_application::ProjectStore::from_user_path(&PathBuf::from(path)));
                let ok = self.save_project(false);
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
                    "revision": self.session.revision(),
                    "view": format!("{:?}", self.view),
                    "playhead_ms": self.playhead.as_millis_round(),
                    "player": {"position_ms": snap.position.as_millis_round(), "playing": snap.playing, "rate": snap.rate, "generation": snap.generation, "buffering": snap.buffering, "decode_ms": snap.decode_ms, "audio_available": snap.audio_available},
                    "last_frame": self.last_frame.as_ref().map(|f| serde_json::json!({"position_ms": f.position.as_millis_round(), "w": f.frame.width, "h": f.frame.height, "generation": f.generation})),
                    "clips": seq.as_ref().map(|s| s.clips.iter().map(|c| serde_json::json!({"id": c.id, "track": s.track(&c.track_id).map(|t| t.name.clone()), "position_ms": c.position.as_millis_round(), "source_start_ms": c.source.start.as_millis_round(), "source_end_ms": c.source.end.as_millis_round(), "enabled": c.enabled, "link": c.link_group})).collect::<Vec<_>>()),
                    "layers": self.project().layers.iter().map(|l| serde_json::json!({"id": l.layer_id, "name": l.name, "items": l.items.iter().map(|i| serde_json::json!({"id": i.item_id, "label": i.label, "state": i.state.as_str(), "ranges": i.ranges.iter().map(|r| [r.start.as_seconds_ms(), r.end.as_seconds_ms()]).collect::<Vec<_>>()})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
                    "toasts": self.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>(),
                    "undo": self.session.undo_label(),
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
