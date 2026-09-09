//! Estado de la aplicación y bucle de UI. La GUI presenta estado y emite
//! intenciones: todo cambio de proyecto pasa por `ProjectSession::execute`.

use crate::console::{ConsoleLine, ConsoleSink};
use crate::keymap::{Keymap, chord_from_egui};
use crossbeam_channel::Receiver;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tv2_application::{Actor, CommandEnvelope, ProjectSession, ProjectStore};
use tv2_domain::asset::AssetKind;
use tv2_domain::error::DomainError;
use tv2_domain::ids::{AssetId, ClipId, ItemId, LayerId, TrackId};
use tv2_domain::layers::{ItemState, LayerKind, SemanticItem};
use tv2_domain::project::Project;
use tv2_domain::resolve::ResolvedTimeline;
use tv2_domain::time::{Rational, Ticks, TimeRange};
use tv2_domain::timeline::{Sequence, Track, TrackKind};
use tv2_domain::{ClipEdge, Command, MovePolicy};
use tv2_media::export::{ExportPreset, ExportProgress, ExportRequest, ExportResult};
use tv2_media::ffmpeg::FfmpegTools;
use tv2_media::player::{PlayerCommand, PlayerHandle, PlayerSnapshot, PresentedFrame, SPEEDS};
use tv2_media::render::AssetSource;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViewMode {
    Source,
    #[default]
    Sequence,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tool {
    #[default]
    Select,
    Cut,
}

#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub clips: Vec<ClipId>,
    pub items: Vec<(LayerId, ItemId)>,
    pub layer: Option<LayerId>,
    pub asset: Option<AssetId>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.clips.clear();
        self.items.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub at: Instant,
    pub severity: Severity,
    pub text: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub library_width: f32,
    pub inspector_width: f32,
    pub timeline_height: f32,
    pub console_open: bool,
    pub console_height: f32,
    pub last_project: Option<String>,
    pub last_media_dir: Option<String>,
    pub last_export_dir: Option<String>,
    pub snapping: bool,
    pub follow_playhead: bool,
    pub track_heights: HashMap<String, f32>,
    pub zoom_px_per_s: f32,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            library_width: 260.0,
            inspector_width: 300.0,
            timeline_height: 320.0,
            console_open: false,
            console_height: 160.0,
            last_project: None,
            last_media_dir: None,
            last_export_dir: None,
            snapping: true,
            follow_playhead: true,
            track_heights: HashMap::new(),
            zoom_px_per_s: 40.0,
        }
    }
}

pub struct ExportState {
    pub open: bool,
    pub preset_idx: usize,
    pub destination: String,
    pub range_mode: usize, // 0 todo, 1 IN/OUT, 2 clips, 3 items/bloques
    pub running: Option<RunningExport>,
    pub last_result: Option<Result<ExportResult, DomainError>>,
    pub pending: VecDeque<ExportRequest>,
    pub history: Vec<(PathBuf, u64, Result<ExportResult, DomainError>)>,
}

pub struct RunningExport {
    pub progress: Arc<parking_lot::Mutex<ExportProgress>>,
    pub cancel: Arc<AtomicBool>,
    pub rx: Receiver<Result<ExportResult, DomainError>>,
    pub started: Instant,
    pub destination: PathBuf,
    pub revision: u64,
    pub thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for RunningExport {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct TranscriptorApp {
    pub session: ProjectSession,
    pub store: Option<ProjectStore>,
    pub tools: Result<FfmpegTools, String>,
    pub player: Option<PlayerHandle>,
    pub media_view: Option<crate::ui_media::MediaView>,
    pub keymap: Keymap,
    pub ui: UiState,
    pub view: ViewMode,
    pub tool: Tool,
    pub selection: Selection,
    pub playhead: Ticks,
    pub player_snapshot: PlayerSnapshot,
    pub texture: Option<egui::TextureHandle>,
    pub frame_seen: u64,
    pub last_frame: Option<PresentedFrame>,
    pub viewer_size: (u32, u32),
    pub in_point: Option<Ticks>,
    pub out_point: Option<Ticks>,
    pub loop_range: Option<TimeRange>,
    pub toasts: Vec<Toast>,
    pub console_lines: Vec<ConsoleLine>,
    pub console_rx: Receiver<ConsoleLine>,
    pub console_filter: tracing::Level,
    pub _console_sink: ConsoleSink,
    pub export: ExportState,
    pub imports: crate::import_jobs::ImportJobs,
    pub editorial_job: Option<crate::editorial_jobs::EditorialJob>,
    pub timeline_view: crate::ui_timeline::TimelineView,
    pub resolved: Arc<ResolvedTimeline>,
    pub resolved_revision: Option<(u64, ViewMode, Option<AssetId>)>,
    pub item_clipboard: Option<(LayerId, AssetId, Vec<SemanticItem>)>,
    pub clipboard: Vec<tv2_domain::timeline::Clip>,
    pub new_layer_dialog: Option<String>,
    pub new_layer_kind: LayerKind,
    pub rename_dialog: Option<(String, RenameTarget)>,
    pub goto_dialog: Option<String>,
    pub about_open: bool,
    pub shortcuts_open: bool,
    pub shortcut_search: String,
    pub shortcut_editor: Option<crate::keymap::ShortcutEditor>,
    pub pending_close: bool,
    pub close_after_save: bool,
    pub recovery: Option<Project>,
    pub recovery_audit: Vec<tv2_application::session::JournalEvent>,
    pub v1_export_job: Option<crossbeam_channel::Receiver<tv2_domain::error::DomainResult<PathBuf>>>,
    pub autosave_job: Option<crossbeam_channel::Receiver<tv2_domain::error::DomainResult<()>>>,
    pub persistence_job: Option<crate::persistence_jobs::PersistenceJob>,
    pub external: crate::external::ExternalMonitor,
    pub unsaved_recoveries: Vec<PathBuf>,
    pub(crate) unsaved_store: Option<ProjectStore>,
    pub source_asset: Option<AssetId>,
    pub last_autosave: Instant,
    pub dirty_title: bool,
    pub script: Option<crate::scripting::ScriptRunner>,
    pub frame_count: u64,
    pub markers_open: bool,
    pub item_editor: Option<crate::item_editor::ItemEditor>,
    pub marker_editor: Option<tv2_domain::timeline::Marker>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // `Project` se usa desde pruebas y desde el inspector en E3
pub enum RenameTarget {
    Clip(ClipId),
    Layer(LayerId),
    Track(TrackId),
    Project,
}

pub fn load_icon() -> Option<egui::IconData> {
    // icono procedural: cuadro naranja con barra de timeline
    let size = 64u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            let bar = (20..30).contains(&y) || (38..44).contains(&y) && x > 10 && x < 40;
            let (r, g, b) = if bar { (255, 255, 255) } else { (208, 153, 71) };
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }
    Some(egui::IconData { rgba, width: size, height: size })
}

impl TranscriptorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, console_sink: ConsoleSink, console_rx: Receiver<ConsoleLine>) -> Self {
        configure_style(&cc.egui_ctx);
        let ui: UiState = std::fs::read_to_string(crate::paths::ui_state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let tools = FfmpegTools::locate().map_err(|e| e.to_string());
        match &tools {
            Ok(t) => tracing::info!("FFmpeg: {} ({})", t.version, t.origin),
            Err(e) => tracing::error!("FFmpeg no disponible: {e}"),
        }
        let player = tools.as_ref().ok().map(|t| {
            let ctx = cc.egui_ctx.clone();
            tv2_media::player::spawn(t.clone(), Arc::new(move || ctx.request_repaint()))
        });
        let keymap = Keymap::load();
        let media_view = tools.as_ref().ok().map(|t| crate::ui_media::MediaView::new(t.clone(), cc.egui_ctx.clone()));
        for w in &keymap.warnings {
            tracing::warn!("keymap: {w}");
        }
        for (chord, winner, loser) in &keymap.conflicts {
            tracing::warn!("keymap: «{chord}» está en {winner} y {loser}; gana {winner}");
        }
        let project = Project::new("Sin título");
        let session = ProjectSession::new(project);
        let mut app = TranscriptorApp {
            markers_open: false,
            marker_editor: None,
            item_editor: None,
            session,
            store: None,
            tools,
            player,
            media_view,
            keymap,
            timeline_view: crate::ui_timeline::TimelineView::new(ui.zoom_px_per_s),
            ui,
            view: ViewMode::Sequence,
            tool: Tool::Select,
            selection: Selection::default(),
            playhead: Ticks::ZERO,
            player_snapshot: PlayerSnapshot { rate: 1.0, ..Default::default() },
            texture: None,
            frame_seen: 0,
            last_frame: None,
            viewer_size: (640, 360),
            in_point: None,
            out_point: None,
            loop_range: None,
            toasts: Vec::new(),
            console_lines: Vec::new(),
            console_rx,
            console_filter: tracing::Level::INFO,
            _console_sink: console_sink,
            imports: Default::default(),
            editorial_job: None,
            export: ExportState {
                open: false,
                preset_idx: 1,
                destination: String::new(),
                range_mode: 0,
                running: None,
                last_result: None,
                pending: VecDeque::new(),
                history: Vec::new(),
            },
            resolved: Arc::new(ResolvedTimeline {
                frame_rate: Rational::new(30, 1),
                width: 1920,
                height: 1080,
                sample_rate: 48000,
                duration: Ticks::ZERO,
                pieces: vec![],
            }),
            resolved_revision: None,
            clipboard: Vec::new(),
            item_clipboard: None,
            new_layer_dialog: None,
            new_layer_kind: LayerKind::User,
            rename_dialog: None,
            goto_dialog: None,
            about_open: false,
            shortcuts_open: false,
            shortcut_search: String::new(),
            shortcut_editor: None,
            pending_close: false,
            close_after_save: false,
            recovery: None,
            recovery_audit: Vec::new(),
            autosave_job: None,
            persistence_job: None,
            v1_export_job: None,
            external: Default::default(),
            unsaved_recoveries: Self::find_unsaved_recoveries(),
            unsaved_store: None,
            source_asset: None,
            last_autosave: Instant::now(),
            dirty_title: true,
            script: None,
            frame_count: 0,
        };
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--script" && i + 1 < args.len() {
                match crate::scripting::ScriptRunner::load(&args[i + 1]) {
                    Ok(r) => app.script = Some(r),
                    Err(e) => tracing::error!("guion: {e}"),
                }
                i += 2;
                continue;
            }
            let path = PathBuf::from(&args[i]);
            if path.exists() {
                app.open_project_path(&path);
            }
            i += 1;
        }
        app
    }

    // ---------- helpers de proyecto ----------

    pub fn project(&self) -> &Project {
        self.session.project()
    }

    fn find_unsaved_recoveries() -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(crate::paths::config_dir().join("recovery")) else {
            return vec![];
        };
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("project.json").is_file() && !p.join("resolved.json").exists())
            .collect();
        paths.sort();
        paths
    }

    pub fn recover_unsaved(&mut self, root: PathBuf) {
        if self.session.is_dirty() {
            self.toast(Severity::Warn, "Guarda el proyecto actual antes de recuperar otro");
            return;
        }
        let store = ProjectStore::at(root.clone());
        match store.load() {
            Ok(candidate) => {
                // Recovery starts with a new editable session and no normal save
                // destination. Its first Save opens the destination dialog.
                let mut empty = Project::new(candidate.name.clone());
                empty.project_id = candidate.project_id.clone();
                let mut session = ProjectSession::new(empty);
                match store.read_journal().and_then(|events| session.recover_with_audit(candidate, events)) {
                    Ok(()) => {
                        self.player_send(PlayerCommand::Pause);
                        self.session = session;
                        self.store = None;
                        self.unsaved_store = Some(store);
                        self.unsaved_recoveries.retain(|p| p != &root);
                        self.external = Default::default();
                        self.selection = Selection::default();
                        self.source_asset = self.project().assets.first().map(|a| a.id.clone());
                        self.resolved_revision = None;
                        self.after_change();
                        self.seek(Ticks::ZERO);
                    }
                    Err(e) => self.report(e),
                }
            }
            Err(e) => self.report(e),
        }
    }

    pub fn sequence(&self) -> Option<&Sequence> {
        self.project().active()
    }

    pub fn frame_rate(&self) -> Rational {
        self.sequence().map(|s| s.frame_rate).unwrap_or_default()
    }

    /// Duración del contenido visible (secuencia o fuente).
    pub fn duration(&self) -> Ticks {
        match self.view {
            ViewMode::Sequence => self.sequence().map(|s| s.extent()).unwrap_or(Ticks::ZERO),
            ViewMode::Source => self.source_asset.as_ref().and_then(|a| self.project().asset(a)).map(|a| a.duration()).unwrap_or(Ticks::ZERO),
        }
    }

    pub fn toast(&mut self, severity: Severity, text: impl Into<String>) {
        let text = text.into();
        match severity {
            Severity::Info => tracing::info!("{text}"),
            Severity::Warn => tracing::warn!("{text}"),
            Severity::Error => tracing::error!("{text}"),
        }
        self.toasts.push(Toast { at: Instant::now(), severity, text });
    }

    pub fn report(&mut self, err: DomainError) {
        let mut text = err.message.clone();
        if let Some(a) = &err.action {
            text.push_str(" · ");
            text.push_str(a);
        }
        let sev = match err.code {
            tv2_domain::ErrorCode::Overlap
            | tv2_domain::ErrorCode::NotAvailable
            | tv2_domain::ErrorCode::Precondition
            | tv2_domain::ErrorCode::OutOfRange
            | tv2_domain::ErrorCode::Empty => Severity::Warn,
            _ => Severity::Error,
        };
        tracing::debug!(code = ?err.code, context = ?err.context, "{}", err.message);
        self.toast(sev, text);
    }

    /// Ejecuta un comando humano. Devuelve `true` si se aplicó.
    pub fn exec(&mut self, command: Command) -> bool {
        self.exec_checked(command).is_ok()
    }

    /// Mismo recorrido humano, conservando el error tipado para los guiones.
    pub fn exec_checked(&mut self, command: Command) -> Result<(), DomainError> {
        match self.session.execute(CommandEnvelope::human(command)) {
            Ok(r) => {
                tracing::debug!(rev = r.new_revision, "{}: {}", r.effect.label, r.diff.human());
                self.after_change();
                Ok(())
            }
            Err(e) => {
                self.report(e.clone());
                Err(e)
            }
        }
    }

    pub fn undo(&mut self) {
        match self.session.undo(Actor::Human) {
            Ok(r) => {
                self.toast(Severity::Info, r.effect.label);
                self.after_change();
            }
            Err(e) => self.report(e),
        }
    }

    pub fn redo(&mut self) {
        match self.session.redo(Actor::Human) {
            Ok(r) => {
                self.toast(Severity::Info, r.effect.label);
                self.after_change();
            }
            Err(e) => self.report(e),
        }
    }

    pub(crate) fn after_change(&mut self) {
        // podar selección
        let clips: HashSet<ClipId> = self.project().sequences.iter().flat_map(|s| s.clips.iter().map(|c| c.id.clone())).collect();
        let items_ok: Vec<bool> = self.selection.items.iter().map(|(l, i)| self.project().layer(l).and_then(|l| l.item(i)).is_some()).collect();
        self.selection.clips.retain(|c| clips.contains(c));
        let mut idx = 0;
        self.selection.items.retain(|_| {
            let keep = items_ok[idx];
            idx += 1;
            keep
        });
        let layer_gone = self.selection.layer.as_ref().is_some_and(|l| self.project().layer(l).is_none_or(|l| l.deleted));
        if layer_gone {
            self.selection.layer = None;
        }
        self.dirty_title = true;
    }

    /// Timeline resuelta del modo actual; se recalcula solo si cambió la revisión o el modo.
    pub fn refresh_resolved(&mut self) {
        let key = (self.session.revision(), self.view, self.source_asset.clone());
        if self.resolved_revision.as_ref() == Some(&key) {
            return;
        }
        let project = self.project();
        let resolved = match self.view {
            ViewMode::Sequence => project.active().map(|s| ResolvedTimeline::resolve(project, s)),
            ViewMode::Source => self.source_asset.as_ref().and_then(|a| project.asset(a)).map(|asset| {
                // secuencia virtual: el medio completo con todas sus pistas
                let mut seq = Sequence::new(
                    "fuente",
                    asset.frame_rate().unwrap_or(Rational::new(30, 1)),
                    asset.probe.video.as_ref().map(|v| v.display_size().0).unwrap_or(1920),
                    asset.probe.video.as_ref().map(|v| v.display_size().1).unwrap_or(1080),
                    48000,
                );
                if asset.has_video() {
                    let t = Track::new(TrackKind::Video, "V");
                    let mut c =
                        tv2_domain::timeline::Clip::new(t.id.clone(), asset.id.clone(), TimeRange::new(Ticks::ZERO, asset.duration()), Ticks::ZERO);
                    c.name = asset.name.clone();
                    seq.tracks.push(t);
                    seq.clips.push(c);
                }
                for (i, _) in asset.probe.audio.iter().enumerate() {
                    let t = Track::new(TrackKind::Audio, format!("A{}", i + 1));
                    let mut c =
                        tv2_domain::timeline::Clip::new(t.id.clone(), asset.id.clone(), TimeRange::new(Ticks::ZERO, asset.duration()), Ticks::ZERO);
                    c.audio_stream = Some(i as u32);
                    seq.tracks.push(t);
                    seq.clips.push(c);
                }
                let mut p2 = Project::new("fuente");
                p2.assets = vec![asset.clone()];
                ResolvedTimeline::resolve(&p2, &seq)
            }),
        };
        let resolved = Arc::new(resolved.unwrap_or(ResolvedTimeline {
            frame_rate: self.frame_rate(),
            width: 1920,
            height: 1080,
            sample_rate: 48000,
            duration: Ticks::ZERO,
            pieces: vec![],
        }));
        self.resolved = resolved.clone();
        self.resolved_revision = Some(key);
        let assets = Arc::new(self.asset_sources());
        if let Some(pl) = &self.player {
            let skips = if self.project().settings.skip_trims_on_play { tv2_domain::review::skip_ranges(self.project(), &resolved) } else { vec![] };
            pl.send(PlayerCommand::SetTimeline { timeline: resolved, assets });
            pl.send(PlayerCommand::SetSkipRanges(skips));
        }
    }

    pub fn asset_sources(&self) -> HashMap<AssetId, AssetSource> {
        let p = self.project();
        p.assets.iter().map(|a| (a.id.clone(), AssetSource::from_asset(a, self.resolve_asset_path(a)))).collect()
    }

    pub fn resolve_asset_path(&self, asset: &tv2_domain::asset::Asset) -> PathBuf {
        match &self.store {
            Some(s) => s.resolve_path(&asset.path),
            None => PathBuf::from(&asset.path),
        }
    }

    // ---------- proyecto: abrir / guardar ----------

    pub fn open_project_path(&mut self, path: &Path) {
        if self.persistence_job.is_some() || self.autosave_job.is_some() {
            self.toast(Severity::Warn, "Espera a que termine la operación de archivos");
            return;
        }
        self.imports.cancel();
        if let Some(job) = &self.editorial_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        let store = ProjectStore::from_user_path(path);
        let (tx, result) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("project-open".into()).spawn(move || {
            let _ = tx.send(crate::persistence_jobs::load(store));
        }) {
            Ok(_) => {
                self.persistence_job = Some(crate::persistence_jobs::PersistenceJob {
                    project_id: self.project().project_id.clone(),
                    revision: self.session.revision(),
                    result,
                })
            }
            Err(e) => self.report(DomainError::process(format!("No se pudo iniciar apertura: {e}"))),
        }
    }
    pub fn save_project(&mut self, save_as: bool) -> bool {
        let store = if self.store.is_none() || save_as {
            let mut dlg =
                rfd::FileDialog::new().set_title("Guardar proyecto").set_file_name(format!("{}.transcriptor", tv2_slug(&self.project().name)));
            if let Some(d) = &self.ui.last_project
                && let Some(parent) = Path::new(d).parent()
            {
                dlg = dlg.set_directory(parent);
            }
            let Some(p) = dlg.save_file() else { return false };
            ProjectStore::from_user_path(&p)
        } else {
            self.store.clone().unwrap()
        };
        if self.persistence_job.is_some() || self.autosave_job.is_some() {
            self.toast(Severity::Warn, "Espera a que termine la operación de archivos");
            return false;
        }
        let project = self.project().clone();
        let history = self.session.history_snapshot();
        let pending = self.session.pending_journal().to_vec();
        let source = self.store.clone().or(self.unsaved_store.clone());
        let recovery_root = self.unsaved_store.as_ref().map(|s| s.root.clone());
        let (tx, result) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("project-save".into()).spawn(move || {
            let _ = tx.send(crate::persistence_jobs::save(store, source, project, history, pending, recovery_root));
        }) {
            Ok(_) => {
                self.persistence_job = Some(crate::persistence_jobs::PersistenceJob {
                    project_id: self.project().project_id.clone(),
                    revision: self.session.revision(),
                    result,
                });
                true
            }
            Err(e) => {
                self.report(DomainError::process(format!("No se pudo iniciar guardado: {e}")));
                false
            }
        }
    }
    pub fn accept_external(&mut self) {
        if let (Some(store), Some(change)) = (self.store.clone(), self.external.change.take()) {
            match store.accept_external(&mut self.session, change) {
                Ok(()) => {
                    self.external = Default::default();
                    self.after_change();
                    self.toast(Severity::Info, "Cambio externo aplicado; disponible en Deshacer");
                }
                Err(e) => self.report(e),
            }
        }
    }

    pub fn open_project_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("Abrir proyecto").add_filter("Proyecto Transcriptor", &["json"]);
        if let Some(d) = &self.ui.last_project
            && let Some(parent) = Path::new(d).parent()
        {
            dlg = dlg.set_directory(parent);
        }
        if let Some(p) = dlg.pick_file() {
            self.open_project_path(&p);
        }
    }

    pub fn new_project(&mut self) {
        self.imports.cancel();
        if let Some(job) = &self.editorial_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        self.marker_editor = None;
        if let Some(media) = &mut self.media_view {
            media.clear();
        }
        self.session = ProjectSession::new(Project::new("Sin título"));
        self.store = None;
        self.selection = Selection::default();
        self.source_asset = None;
        self.resolved_revision = None;
        self.seek(Ticks::ZERO);
        self.toast(Severity::Info, "Proyecto nuevo");
    }

    // ---------- medios ----------

    pub fn import_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("Importar medios").add_filter(
            "Medios",
            &["mp4", "mkv", "mov", "webm", "avi", "m4v", "wav", "mp3", "flac", "m4a", "aac", "ogg", "opus", "png", "jpg", "jpeg", "webp", "bmp"],
        );
        if let Some(d) = &self.ui.last_media_dir {
            dlg = dlg.set_directory(d);
        }
        if let Some(files) = dlg.pick_files() {
            self.import_paths(&files);
        }
    }

    pub fn import_paths(&mut self, files: &[PathBuf]) {
        if let Err(e) = &self.tools {
            self.toast(Severity::Error, format!("No se puede importar sin FFmpeg: {e}"));
            return;
        }
        for f in files {
            if self.imports.pending.len() >= 64 {
                self.toast(Severity::Warn, "Cola de importación llena (64 pendientes); espera antes de añadir más medios");
                break;
            }
            if let Some(parent) = f.parent() {
                self.ui.last_media_dir = Some(parent.to_string_lossy().into_owned());
            }
            let abs = std::path::absolute(f).unwrap_or_else(|_| f.clone());
            self.imports.pending.push_back((self.project().project_id.clone(), abs));
        }
        self.poll_import();
    }

    pub fn poll_import(&mut self) {
        let result = self.imports.running.as_ref().and_then(|job| match job.result.try_recv() {
            Ok(value) => Some(value),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(DomainError::process("el worker de importación terminó sin resultado"))),
        });
        if let Some(value) = result {
            let job = self.imports.running.take().unwrap();
            if job.project == self.project().project_id && !job.cancel.load(std::sync::atomic::Ordering::Acquire) {
                match value {
                    Ok(mut asset) => {
                        asset.path = job.path.to_string_lossy().replace('\\', "/");
                        if let Some(existing) = self.project().assets.iter().find(|a| a.fingerprint.same_identity(&asset.fingerprint)) {
                            self.toast(Severity::Warn, format!("«{}» ya está en la biblioteca como «{}»", asset.name, existing.name));
                        } else {
                            let id = asset.id.clone();
                            let message = format!("Importado {}: {}", asset.name, describe_asset(&asset));
                            if self.exec(Command::ImportAsset { asset }) {
                                self.toast(Severity::Info, message);
                                if self.source_asset.is_none() {
                                    self.source_asset = Some(id.clone());
                                }
                                self.selection.asset = Some(id);
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(script) = &mut self.script {
                            script.failed = true;
                        }
                        self.report(e);
                    }
                }
            }
        }
        if self.imports.running.is_none()
            && let Some((project, path)) = self.imports.pending.pop_front()
        {
            match crate::import_jobs::RunningImport::start(project, path, self.tools.as_ref().unwrap().clone()) {
                Ok(job) => self.imports.running = Some(job),
                Err(e) => self.report(DomainError::process(format!("no se pudo iniciar importación: {e}"))),
            }
        }
    }

    /// Inserta un asset (o su rango) en la secuencia en el playhead.
    pub fn insert_asset_at_playhead(&mut self, asset_id: AssetId, source: Option<TimeRange>) {
        let pos = if self.view == ViewMode::Sequence { self.playhead } else { self.sequence().map(|s| s.extent()).unwrap_or(Ticks::ZERO) };
        let pos = pos.floor_to_frame(self.frame_rate());
        if self.exec(Command::InsertAssetLinked { asset_id, position: pos, video_track: None, source }) {
            self.toast(Severity::Info, format!("Insertado en {}", pos.timecode_ms()));
        }
    }

    // ---------- importación V1 ----------

    /// Importa una carpeta `editorial/` de V1: capas, recortes y montaje (perfil V1
    /// aplanado). Si ningún medio del proyecto coincide con el master, intenta el
    /// `media.path` del master (absoluto o relativo a la carpeta) y una ruta sugerida.
    pub fn import_v1_folder(&mut self, root: &Path, media_hint: Option<PathBuf>) {
        if self.editorial_job.is_some() {
            self.toast(Severity::Warn, "Hay una importación editorial en curso; espera o cancélala");
            return;
        }
        match crate::editorial_jobs::EditorialJob::start(
            self.project().project_id.clone(),
            self.session.revision(),
            root.to_path_buf(),
            media_hint,
            self.project().assets.clone(),
            self.tools.as_ref().ok().cloned(),
        ) {
            Ok(job) => self.editorial_job = Some(job),
            Err(e) => self.report(DomainError::process(format!("No se pudo iniciar importación V1: {e}"))),
        }
    }

    pub fn poll_editorial_import(&mut self) {
        let result = self.editorial_job.as_ref().and_then(|job| match job.result.try_recv() {
            Ok(value) => Some(value),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(DomainError::process("El worker editorial terminó sin resultado"))),
        });
        let Some(result) = result else { return };
        let job = self.editorial_job.take().unwrap();
        if job.project != self.project().project_id || job.cancel.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let crate::editorial_jobs::EditorialImport { asset, import } = match result {
            Ok(value) => value,
            Err(e) => {
                if let Some(script) = &mut self.script {
                    script.failed = true;
                }
                self.report(e);
                return;
            }
        };
        if job.revision != self.session.revision() {
            self.toast(
                Severity::Warn,
                "El proyecto cambió durante la importación V1. Repite la importación sobre la revisión actual; no se aplicó nada.",
            );
            if let Some(script) = &mut self.script {
                script.failed = true;
            }
            return;
        }
        for w in &import.report.warnings {
            self.toast(Severity::Warn, format!("V1: {w}"));
        }
        let mut commands = Vec::new();
        if self.project().asset(&asset.id).is_none() {
            commands.push(Command::ImportAsset { asset: asset.clone() });
        }
        commands.extend(tv2_v1compat::import::import_commands(&import, self.project(), &asset.id));
        let n = commands.len();
        let envelope = CommandEnvelope::human(Command::Batch { label: "Importar proyecto V1".into(), commands })
            .with_base(job.revision)
            .with_actor(Actor::External { source: "import-v1".into() });
        if let Err(e) = self.session.execute(envelope) {
            self.report(e);
        } else {
            self.after_change();
            self.source_asset = Some(asset.id.clone());
            self.resolved_revision = None;
            let r = &import.report;
            self.toast(
                Severity::Info,
                format!(
                    "V1 importado: {} capas, {} carriles de recortes, {} tramos de montaje ({n} comandos)",
                    r.layers, r.trims_layers, r.montage_pieces
                ),
            );
            if import.sequence.is_some() {
                self.view = ViewMode::Sequence;
                self.refresh_resolved();
                let d = self.duration();
                self.timeline_view.fit(d);
                self.seek(Ticks::ZERO);
            }
        }
    }

    pub fn export_v1_dialog(&mut self, montage: bool) {
        if self.v1_export_job.is_some() {
            self.toast(Severity::Warn, "Hay una exportación V1 en curso");
            return;
        }
        let Some(directory) = rfd::FileDialog::new().set_title("Destino para una nueva carpeta de exportación V1").pick_folder() else { return };
        let project = self.project().clone();
        let layer = self.selection.layer.clone().or_else(|| self.selection.items.first().map(|(l, _)| l.clone()));
        let sequence = self.sequence().map(|s| s.id.clone());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let result = std::thread::Builder::new().name("v1-export".into()).spawn(move || {
            let work = || -> tv2_domain::error::DomainResult<PathBuf> {
                let (value, name) = if montage {
                    let id = sequence.ok_or_else(|| DomainError::invalid("Selecciona una secuencia"))?;
                    (tv2_v1compat::export::montage_document(&project, &id)?, "montaje.json".to_string())
                } else {
                    let id = layer.ok_or_else(|| DomainError::invalid("Selecciona una capa manual, de temas o AI"))?;
                    let kind = &project.layer(&id).ok_or_else(|| DomainError::invalid("Capa inexistente"))?.kind;
                    let filename = match kind {
                        LayerKind::Trims => "trims.json".into(),
                        LayerKind::Blocks => "chunks.selected.json".into(),
                        _ => format!("{id}.json"),
                    };
                    (tv2_v1compat::export::layer_document(&project, &id)?, filename)
                };
                let target = directory.join(format!("v1-export-r{}-{}", project.revision, tv2_domain::ids::random_hex12()));
                // A new directory prevents overwriting any imported V1 document.
                std::fs::create_dir(&target)?;
                tv2_application::store::atomic_write(&target.join(name), &serde_json::to_vec_pretty(&value)?)?;
                Ok(target)
            };
            let _ = tx.send(work());
        });
        match result {
            Ok(_) => self.v1_export_job = Some(rx),
            Err(e) => self.report(e.into()),
        }
    }

    fn poll_v1_export(&mut self) {
        let Some(job) = &self.v1_export_job else { return };
        let result = match job.try_recv() {
            Ok(value) => value,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Err(DomainError::process("Worker de exportación V1 terminó sin resultado")),
        };
        self.v1_export_job = None;
        match result {
            Ok(path) => self.toast(Severity::Info, format!("Documento V1 exportado en {}", path.display())),
            Err(e) => self.report(e),
        }
    }

    pub fn import_v1_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("Carpeta editorial de V1 (contiene *.editorial.master.json)");
        if let Some(d) = &self.ui.last_media_dir {
            dlg = dlg.set_directory(d);
        }
        if let Some(p) = dlg.pick_folder() {
            self.import_v1_folder(&p, None);
        }
    }

    // ---------- transporte ----------

    pub fn seek(&mut self, t: Ticks) {
        let t = t.clamp(Ticks::ZERO, self.duration().max(Ticks::ZERO));
        self.playhead = t;
        if let Some(p) = &self.player {
            p.send(PlayerCommand::Seek(t));
        }
    }

    pub fn scrub(&mut self, t: Ticks) {
        let t = t.clamp(Ticks::ZERO, self.duration().max(Ticks::ZERO));
        self.playhead = t;
        if let Some(p) = &self.player {
            p.send(PlayerCommand::Scrub(t));
        }
    }

    pub fn player_send(&self, c: PlayerCommand) {
        if let Some(p) = &self.player {
            p.send(c);
        }
    }

    pub fn set_rate(&mut self, rate: f64) {
        self.player_send(PlayerCommand::SetRate(rate));
        self.toast(Severity::Info, format!("×{} · {}", rate, tv2_media::player::audio_policy(rate)));
    }

    fn rate_step(&mut self, delta: i32) {
        let snap = &self.player_snapshot;
        let idx = SPEEDS.iter().position(|s| (*s - snap.rate).abs() < 1e-6).unwrap_or(0) as i32;
        if delta > 0 {
            if !snap.playing {
                self.player_send(PlayerCommand::SetRate(1.0));
                self.player_send(PlayerCommand::Play);
                return;
            }
            let ni = (idx + 1).min(SPEEDS.len() as i32 - 1);
            self.set_rate(SPEEDS[ni as usize]);
        } else {
            if idx == 0 {
                self.player_send(PlayerCommand::Pause);
                return;
            }
            self.set_rate(SPEEDS[(idx - 1) as usize]);
        }
    }

    // ---------- acciones (un solo registro) ----------

    pub fn dispatch(&mut self, action: &str) {
        let fr = self.frame_rate();
        let fd = fr.frame_duration();
        match action {
            "transport.play_pause" => self.player_send(PlayerCommand::TogglePlay),
            "transport.pause" => self.player_send(PlayerCommand::Pause),
            "transport.faster" => self.rate_step(1),
            "transport.slower" => self.rate_step(-1),
            "transport.rate_1" => self.set_rate(1.0),
            "transport.rate_2" => self.set_rate(2.0),
            "transport.rate_3" => self.set_rate(3.0),
            "transport.rate_4" => self.set_rate(4.0),
            "transport.skim" => {
                self.set_rate(8.0);
                if !self.player_snapshot.playing {
                    self.player_send(PlayerCommand::Play);
                }
            }
            "transport.play_from_item" => {
                let start = self.selected_item_range().map(|r| r.start).or(self.in_point);
                if let Some(s) = start {
                    self.seek(s);
                    self.player_send(PlayerCommand::Play);
                } else {
                    self.toast(Severity::Warn, "No hay item seleccionado ni IN");
                }
            }
            "nav.frame_prev" => self.player_send(PlayerCommand::StepFrames(-1)),
            "nav.frame_next" => self.player_send(PlayerCommand::StepFrames(1)),
            "nav.frame_prev_10" => self.player_send(PlayerCommand::StepFrames(-10)),
            "nav.frame_next_10" => self.player_send(PlayerCommand::StepFrames(10)),
            "nav.step_prev" => self.seek(self.playhead - Ticks::from_millis(500)),
            "nav.step_next" => self.seek(self.playhead + Ticks::from_millis(500)),
            "nav.step_prev_5" => self.seek(self.playhead - Ticks::from_seconds(5)),
            "nav.step_next_5" => self.seek(self.playhead + Ticks::from_seconds(5)),
            "nav.home" => self.seek(Ticks::ZERO),
            "nav.end" => self.seek(self.duration()),
            "nav.prev_edge" | "nav.next_edge" => {
                let edges = self.edges();
                let t = self.playhead;
                let target = if action == "nav.prev_edge" {
                    edges.iter().rev().find(|e| **e < t - fd / 2).copied()
                } else {
                    edges.iter().find(|e| **e > t + fd / 2).copied()
                };
                match target {
                    Some(e) => self.seek(e),
                    None => self.toast(Severity::Info, "No hay más bordes en esa dirección"),
                }
            }
            "nav.prev_silence" | "nav.next_silence" => self.navigate_silence(action == "nav.next_silence"),
            "nav.goto" => self.goto_dialog = Some(self.playhead.timecode_ms()),
            "nav.sel_start" => {
                if let Some(r) = self.selected_range() {
                    self.seek(r.start)
                }
            }
            "nav.sel_end" => {
                if let Some(r) = self.selected_range() {
                    self.seek(r.end)
                }
            }
            "edit.split" => self.split_at_playhead(),
            "edit.trim_start" | "edit.trim_end" => {
                let edge = if action == "edit.trim_start" { ClipEdge::Start } else { ClipEdge::End };
                self.trim_selection(edge);
            }
            "edit.nudge_prev" => self.nudge(-fd, 0),
            "edit.nudge_next" => self.nudge(fd, 0),
            "edit.nudge_prev_10" => self.nudge(fd * -10, 0),
            "edit.nudge_next_10" => self.nudge(fd * 10, 0),
            "edit.item_prev" | "edit.item_next" => self.select_neighbor(if action == "edit.item_next" { 1 } else { -1 }),
            "edit.accept" | "edit.accept_next" => {
                if self.set_selected_items_state(ItemState::Accepted) && action == "edit.accept_next" {
                    self.select_neighbor(1);
                }
            }
            "edit.toggle" => {
                if !self.selection.items.is_empty() {
                    let mut commands = Vec::new();
                    for (layer_id, item_id) in &self.selection.items {
                        commands.push(if self.project().layer(layer_id).is_some_and(|l| l.kind == LayerKind::Author) {
                            Command::CycleAuthorDecision { layer_id: layer_id.clone(), item_ids: vec![item_id.clone()] }
                        } else {
                            Command::SetItemState { layer_id: layer_id.clone(), item_ids: vec![item_id.clone()], state: ItemState::Disabled }
                        });
                    }
                    self.exec(Command::Batch { label: "Desactivar / decisión del autor".into(), commands });
                } else if !self.selection.clips.is_empty() {
                    let ids = self.selection.clips.clone();
                    self.exec(Command::SetClipEnabled { clip_ids: ids, enabled: false });
                } else {
                    self.toast(Severity::Warn, "Nada seleccionado");
                }
            }
            "edit.activate" => {
                if !self.selection.items.is_empty() {
                    self.set_selected_items_state(ItemState::Proposed);
                } else if !self.selection.clips.is_empty() {
                    let ids = self.selection.clips.clone();
                    self.exec(Command::SetClipEnabled { clip_ids: ids, enabled: true });
                } else {
                    self.toast(Severity::Warn, "Nada seleccionado");
                }
            }
            "edit.delete" => self.delete_selection(false),
            "edit.delete_ripple" => self.delete_selection(true),
            "edit.edit" => self.open_rename_for_selection(),
            "edit.deselect" => {
                self.selection.clear();
                self.timeline_view.cancel_gesture();
            }
            "edit.undo" => self.undo(),
            "edit.redo" => self.redo(),
            "edit.copy" => {
                self.copy_selection();
            }
            "edit.cut" => {
                if self.copy_selection() {
                    self.delete_selection(false);
                }
            }
            "edit.paste" => self.paste_at_playhead(),
            "edit.duplicate" => self.duplicate_selection(),
            "tools.select" => self.tool = Tool::Select,
            "tools.cut" => self.tool = Tool::Cut,
            "tools.select_all" => {
                if let Some(layer_id) = self.selection.layer.clone() {
                    if let Some(layer) = self.project().layer(&layer_id) {
                        self.selection.items = layer.items.iter().map(|i| (layer_id.clone(), i.item_id.clone())).collect();
                        self.selection.clips.clear();
                    }
                } else if let Some(seq) = self.sequence() {
                    let track = self.selection.clips.first().and_then(|c| seq.clip(c)).map(|c| c.track_id.clone());
                    self.selection.clips =
                        seq.clips.iter().filter(|c| track.as_ref().is_none_or(|t| &c.track_id == t)).map(|c| c.id.clone()).collect();
                }
            }
            "layers.new_lane" => self.new_layer_dialog = Some("Nueva capa".into()),
            "layers.add_range" => self.add_range_to_layer(),
            "view.zoom_in" => self.timeline_view.zoom_by(1.25, None),
            "view.zoom_out" => self.timeline_view.zoom_by(0.8, None),
            "view.fit" => self.timeline_view.fit(self.duration()),
            "view.zoom_sel" => {
                if let Some(r) = self.selected_range() {
                    self.timeline_view.zoom_to(r);
                } else {
                    self.timeline_view.fit(self.duration());
                }
            }
            "view.follow" => {
                self.ui.follow_playhead = !self.ui.follow_playhead;
                let on = self.ui.follow_playhead;
                self.toast(Severity::Info, if on { "Seguir al playhead: sí" } else { "Seguir al playhead: no" });
            }
            "view.center" => self.timeline_view.center_on(self.playhead),
            "view.skip_trims" => {
                let enabled = !self.project().settings.skip_trims_on_play;
                if self.exec(Command::SetSkipTrims { enabled }) {
                    self.toast(Severity::Info, if enabled { "Saltar recortes: activado" } else { "Saltar recortes: desactivado" });
                }
            }
            "loop.set_in" => self.set_loop_edge(true),
            "loop.set_out" => self.set_loop_edge(false),
            "loop.clear" => {
                self.loop_range = None;
                self.player_send(PlayerCommand::SetLoop(None));
            }
            "view.mode_montage" => self.toggle_view(),
            "montage.add_selection" => self.add_selection_to_sequence(),
            "montage.add_topic" => self.add_topic_to_sequence(),
            "montage.reveal_source" => self.reveal_in_source(),
            "montage.move_up" => self.nudge(Ticks::ZERO, 1),
            "montage.move_down" => self.nudge(Ticks::ZERO, -1),
            "montage.export" => self.open_export_dialog(),
            "sequence.markers" => self.markers_open = !self.markers_open,
            "sequence.marker_add" => {
                if self.view != ViewMode::Sequence {
                    self.toast(Severity::Warn, "Los marcadores pertenecen a la secuencia; cambia a Vista: Secuencia");
                } else {
                    self.exec(Command::AddMarker {
                        range: TimeRange::new(self.playhead, self.playhead),
                        label: format!("Marcador {}", self.sequence().map(|s| s.markers.len() + 1).unwrap_or(1)),
                        color: "#e8b55b".into(),
                        marker_id: None,
                    });
                    self.markers_open = true;
                }
            }
            "sequence.insert_selection" => {
                let ids = self.linked_selection();
                if self.view == ViewMode::Sequence
                    && let Some(lead) = ids.first().and_then(|id| self.sequence().and_then(|s| s.clip(id)))
                {
                    let delta = self.playhead - lead.position;
                    self.exec(Command::ShiftClips { clip_ids: ids, delta, track_delta: 0, policy: MovePolicy::Insert });
                } else {
                    self.toast(Severity::Warn, "Selecciona clips en la secuencia");
                }
            }
            "marks.point" => self.add_point_mark(),
            "marks.in" => {
                self.in_point = Some(self.playhead);
                if let Some(o) = self.out_point
                    && o <= self.playhead
                {
                    self.out_point = None;
                }
            }
            "marks.out" => {
                self.out_point = Some(self.playhead);
                if let Some(i) = self.in_point
                    && i >= self.playhead
                {
                    self.in_point = None;
                }
            }
            "file.open" => self.open_project_dialog(),
            "file.save" => {
                self.save_project(false);
            }
            "file.import" => self.import_dialog(),
            other => self.toast(Severity::Warn, format!("Acción sin implementar: {other}")),
        }
    }

    fn edges(&self) -> Vec<Ticks> {
        let mut edges: Vec<Ticks> = match self.view {
            ViewMode::Sequence => self.sequence().map(|s| s.clip_edges()).unwrap_or_default(),
            ViewMode::Source => Vec::new(),
        };
        // bordes de items semánticos visibles del asset fuente
        if let Some(asset) = self.current_layer_asset() {
            for l in self.project().ordered_layers().iter().filter(|l| l.asset_id == asset && l.visible) {
                for it in &l.items {
                    for r in &it.ranges {
                        let (a, b) = self.layer_range_to_view(r, &asset);
                        edges.push(a);
                        edges.push(b);
                    }
                }
            }
        }
        edges.push(Ticks::ZERO);
        edges.push(self.duration());
        edges.sort();
        edges.dedup();
        edges
    }

    /// Asset cuyas capas se muestran: en Fuente el asset fuente; en Secuencia,
    /// el asset del clip seleccionado o el primer video.
    pub fn current_layer_asset(&self) -> Option<AssetId> {
        match self.view {
            ViewMode::Source => self.source_asset.clone(),
            ViewMode::Sequence => self
                .selection
                .clips
                .first()
                .and_then(|c| self.sequence().and_then(|s| s.clip(c)).map(|c| c.asset_id.clone()))
                .or_else(|| self.source_asset.clone())
                .or_else(|| self.project().assets.first().map(|a| a.id.clone())),
        }
    }

    /// En Secuencia, un rango fuente se muestra en cada clip que lo contiene
    /// (aquí solo el primero, para bordes de navegación).
    fn layer_range_to_view(&self, r: &TimeRange, asset: &AssetId) -> (Ticks, Ticks) {
        match self.view {
            ViewMode::Source => (r.start, r.end),
            ViewMode::Sequence => {
                if let Some(seq) = self.sequence() {
                    for c in seq.clips.iter().filter(|c| &c.asset_id == asset) {
                        if let Some(i) = c.source.intersection(r) {
                            return (c.position + (i.start - c.source.start), c.position + (i.end - c.source.start));
                        }
                    }
                }
                (r.start, r.end)
            }
        }
    }

    pub fn selected_range(&self) -> Option<TimeRange> {
        if let (Some(i), Some(o)) = (self.in_point, self.out_point)
            && i < o
        {
            return Some(TimeRange::new(i, o));
        }
        if let Some(r) = self.selected_item_range() {
            return Some(r);
        }
        let seq = self.sequence()?;
        let clips: Vec<&tv2_domain::timeline::Clip> = self.selection.clips.iter().filter_map(|c| seq.clip(c)).collect();
        if clips.is_empty() {
            return None;
        }
        let start = clips.iter().map(|c| c.position).min()?;
        let end = clips.iter().map(|c| c.end()).max()?;
        Some(TimeRange::new(start, end))
    }

    fn selected_item_range(&self) -> Option<TimeRange> {
        let (l, i) = self.selection.items.first()?;
        let layer = self.project().layer(l)?;
        let item = layer.item(i)?;
        let (a, b) = self.layer_range_to_view(&TimeRange::new(item.start(), item.end()), &layer.asset_id);
        Some(TimeRange::new(a, b))
    }

    pub fn split_at_playhead(&mut self) {
        if !self.selection.items.is_empty() {
            let mut commands = Vec::new();
            for (layer_id, item_id) in &self.selection.items {
                let Some(layer) = self.project().layer(layer_id) else { return };
                // A selected descendant is already partitioned by its selected ancestor.
                let mut parent = layer.item(item_id).and_then(|i| i.parent_id.clone());
                let mut covered = false;
                while let Some(id) = parent {
                    if self.selection.items.contains(&(layer_id.clone(), id.clone())) {
                        covered = true;
                        break;
                    }
                    parent = layer.item(&id).and_then(|i| i.parent_id.clone());
                }
                if covered {
                    continue;
                }
                let Some(at) = self.view_time_to_source(self.playhead, &layer.asset_id) else {
                    self.toast(Severity::Warn, "El playhead no tiene correspondencia con el medio del item");
                    return;
                };
                commands.push(Command::SplitItem { layer_id: layer_id.clone(), item_id: item_id.clone(), at });
            }
            self.exec(Command::Batch { label: "Dividir items seleccionados".into(), commands });
            return;
        }
        if self.view != ViewMode::Sequence {
            self.toast(Severity::Warn, "Dividir actúa en la secuencia (Ctrl+M)");
            return;
        }
        let t = self.playhead.floor_to_frame(self.frame_rate());
        let linked = self.linked_selection();
        let Some(seq) = self.sequence() else { return };
        let mut targets: Vec<ClipId> = linked.iter().filter(|c| seq.clip(c).is_some_and(|c| c.range().contains(t))).cloned().collect();
        if targets.is_empty() {
            targets = seq.clips.iter().filter(|c| c.range().contains(t) && c.position < t && t < c.end()).map(|c| c.id.clone()).collect();
        }
        if targets.is_empty() {
            self.toast(Severity::Warn, "No hay clip bajo el playhead");
            return;
        }
        let commands: Vec<Command> = targets.iter().map(|id| Command::SplitClip { clip_id: id.clone(), at: t }).collect();
        let cmd = if commands.len() == 1 { commands.into_iter().next().unwrap() } else { Command::Batch { label: "Dividir clips".into(), commands } };
        self.exec(cmd);
    }

    fn nudge(&mut self, delta: Ticks, track_delta: i32) {
        if !self.selection.items.is_empty() {
            if track_delta != 0 {
                self.toast(Severity::Warn, "Usa el orden de carriles para mover una capa");
                return;
            }
            let command = self.shift_items_command(delta);
            self.exec(command);
            return;
        }
        let ids = self.linked_selection();
        if ids.is_empty() {
            self.toast(Severity::Warn, "Selecciona un clip");
            return;
        }
        self.exec(Command::ShiftClips { clip_ids: ids, delta, track_delta, policy: MovePolicy::Reject });
    }

    /// Selección ampliada con los clips enlazados (audio/video).
    pub fn linked_selection(&self) -> Vec<ClipId> {
        let Some(seq) = self.sequence() else { return Vec::new() };
        let mut out: Vec<ClipId> = self.selection.clips.clone();
        let groups: HashSet<String> = self.selection.clips.iter().filter_map(|c| seq.clip(c)).filter_map(|c| c.link_group.clone()).collect();
        for c in &seq.clips {
            if let Some(g) = &c.link_group
                && groups.contains(g)
                && !out.contains(&c.id)
            {
                out.push(c.id.clone());
            }
        }
        out
    }

    pub fn shift_items_command(&self, delta: Ticks) -> Command {
        let mut by_layer: std::collections::BTreeMap<LayerId, Vec<ItemId>> = Default::default();
        let mut lower = Ticks(i64::MIN);
        let mut upper = Ticks::MAX;
        for (l, i) in &self.selection.items {
            by_layer.entry(l.clone()).or_default().push(i.clone());
            if let Some(layer) = self.project().layer(l)
                && let Some(item) = layer.item(i)
                && let Some(asset) = self.project().asset(&layer.asset_id)
            {
                lower = lower.max(-item.start());
                upper = upper.min(asset.duration() - item.end());
            }
        }
        let delta = delta.clamp(lower, upper);
        Command::Batch {
            label: "Mover selección editorial".into(),
            commands: by_layer.into_iter().map(|(layer_id, item_ids)| Command::ShiftItems { layer_id, item_ids, delta }).collect(),
        }
    }

    fn trim_selection(&mut self, edge: ClipEdge) {
        let mut commands = Vec::new();
        if !self.selection.items.is_empty() {
            for (layer_id, item_id) in &self.selection.items {
                let Some(layer) = self.project().layer(layer_id) else { return };
                let Some(item) = layer.item(item_id) else { return };
                let Some(new_time) = self.view_edge_to_source(self.playhead, &layer.asset_id, edge) else {
                    self.toast(Severity::Warn, "El playhead no tiene correspondencia fuente");
                    return;
                };
                let range_index = item
                    .ranges
                    .iter()
                    .position(|r| r.start <= new_time && new_time <= r.end)
                    .unwrap_or_else(|| if edge == ClipEdge::Start { 0 } else { item.ranges.len() - 1 });
                commands.push(Command::TrimItem { layer_id: layer_id.clone(), item_id: item_id.clone(), range_index, edge, new_time });
            }
        } else {
            commands.extend(self.linked_selection().into_iter().map(|clip_id| Command::TrimClip { clip_id, edge, new_time: self.playhead }));
        }
        if commands.is_empty() {
            self.toast(Severity::Warn, "Selecciona un clip o item para recortar");
            return;
        }
        self.exec(Command::Batch { label: "Recortar selección".into(), commands });
    }

    pub fn move_layer(&mut self, id: &LayerId, delta: i32) {
        let mut order: Vec<_> = self.project().ordered_layers().iter().map(|l| l.layer_id.clone()).collect();
        let Some(index) = order.iter().position(|l| l == id) else { return };
        let target = (index as i32 + delta).clamp(0, order.len() as i32 - 1) as usize;
        order.swap(index, target);
        self.exec(Command::SetLayerOrder { layer_ids: order });
    }

    fn set_selected_items_state(&mut self, state: ItemState) -> bool {
        if self.selection.items.is_empty() {
            self.toast(Severity::Warn, "Selecciona un tramo de una capa");
            return false;
        }
        let mut by_layer: HashMap<LayerId, Vec<ItemId>> = HashMap::new();
        for (l, i) in &self.selection.items {
            by_layer.entry(l.clone()).or_default().push(i.clone());
        }
        let commands = by_layer.into_iter().map(|(layer_id, item_ids)| Command::SetItemState { layer_id, item_ids, state }).collect();
        self.exec(Command::Batch { label: "Estado de selección editorial".into(), commands })
    }

    fn select_neighbor(&mut self, dir: i32) {
        // items del carril seleccionado en orden temporal
        let Some(layer_id) = self.selection.items.first().map(|(l, _)| l.clone()).or(self.selection.layer.clone()) else {
            self.toast(Severity::Warn, "Selecciona una capa o un tramo");
            return;
        };
        let Some(layer) = self.project().layer(&layer_id).cloned() else { return };
        let mut items: Vec<&SemanticItem> = layer.items.iter().collect();
        items.sort_by_key(|i| (i.start(), i.item_id.clone()));
        if items.is_empty() {
            return;
        }
        let cur = self.selection.items.first().and_then(|(_, i)| items.iter().position(|x| &x.item_id == i));
        let next = match cur {
            Some(p) => (p as i32 + dir).clamp(0, items.len() as i32 - 1) as usize,
            None => {
                if dir > 0 {
                    0
                } else {
                    items.len() - 1
                }
            }
        };
        let it = items[next];
        let id = it.item_id.clone();
        let start = it.start();
        self.selection.items = vec![(layer_id.clone(), id)];
        self.selection.layer = Some(layer_id);
        let (a, _) = self.layer_range_to_view(&TimeRange::new(start, start), &layer.asset_id);
        self.seek(a);
    }

    fn delete_selection(&mut self, ripple: bool) {
        if !self.selection.items.is_empty() {
            let mut by_layer: HashMap<LayerId, Vec<ItemId>> = HashMap::new();
            for (l, i) in &self.selection.items {
                by_layer.entry(l.clone()).or_default().push(i.clone());
            }
            let commands = by_layer.into_iter().map(|(l, ids)| Command::DeleteItems { layer_id: l, item_ids: ids }).collect();
            if self.exec(Command::Batch { label: "Borrar items seleccionados".into(), commands }) {
                self.selection.items.clear();
            }
            return;
        }
        let ids = self.linked_selection();
        if ids.is_empty() {
            self.toast(Severity::Warn, "Nada seleccionado");
            return;
        }
        if self.exec(Command::RemoveClips { clip_ids: ids, ripple }) {
            self.selection.clips.clear();
        }
    }

    fn duplicate_selection(&mut self) {
        if !self.copy_selection() {
            return;
        }
        if let Some((layer_id, _, items)) = self.item_clipboard.clone() {
            let position = items.iter().map(SemanticItem::end).max().unwrap();
            match self.session.execute(CommandEnvelope::human(Command::PasteItems { layer_id: layer_id.clone(), items, position })) {
                Ok(r) => {
                    self.selection.items = r.effect.created.iter().map(|id| (layer_id.clone(), ItemId::new(id))).collect();
                    self.after_change();
                }
                Err(e) => self.report(e),
            }
        } else {
            let position = self.clipboard.iter().map(|c| c.end()).max().unwrap();
            match self.session.execute(CommandEnvelope::human(Command::PasteClips { clips: self.clipboard.clone(), position })) {
                Ok(r) => {
                    self.selection.clips = r.effect.created.iter().map(ClipId::new).collect();
                    self.after_change();
                }
                Err(e) => self.report(e),
            }
        }
    }

    fn copy_selection(&mut self) -> bool {
        if let Some((layer_id, _)) = self.selection.items.first() {
            if self.selection.items.iter().any(|(l, _)| l != layer_id) {
                self.toast(Severity::Warn, "Copia items de un solo carril cada vez");
                return false;
            }
            let Some(layer) = self.project().layer(layer_id) else { return false };
            if !layer.kind.is_editable() || layer.locked {
                self.toast(Severity::Warn, "Esta evidencia es de solo lectura");
                return false;
            }
            let mut ids: std::collections::HashSet<_> = self.selection.items.iter().map(|(_, i)| i.clone()).collect();
            loop {
                let old = ids.len();
                for item in &layer.items {
                    if item.parent_id.as_ref().is_some_and(|p| ids.contains(p)) {
                        ids.insert(item.item_id.clone());
                    }
                }
                if old == ids.len() {
                    break;
                }
            }
            let items: Vec<_> = layer.items.iter().filter(|i| ids.contains(&i.item_id)).cloned().collect();
            if items.is_empty() {
                return false;
            }
            self.item_clipboard = Some((layer.layer_id.clone(), layer.asset_id.clone(), items));
            self.clipboard.clear();
            self.toast(Severity::Info, "Items y descendientes copiados; al pegar se conservan sus rangos relativos");
            return true;
        }
        let Some(seq) = self.sequence() else { return false };
        let ids = self.linked_selection();
        let clips: Vec<_> = ids.iter().filter_map(|c| seq.clip(c)).cloned().collect();
        if clips.is_empty() {
            self.toast(Severity::Warn, "Nada que copiar");
            return false;
        }
        self.item_clipboard = None;
        self.clipboard = clips;
        self.toast(Severity::Info, format!("{} clip(s) copiados", self.clipboard.len()));
        true
    }

    fn paste_at_playhead(&mut self) {
        if let Some((source_layer, asset, items)) = self.item_clipboard.clone() {
            let layer_id = self.selection.layer.clone().unwrap_or(source_layer);
            if self.project().layer(&layer_id).is_none_or(|l| l.asset_id != asset) {
                self.toast(Severity::Warn, "El destino debe ser una capa del mismo medio");
                return;
            }
            let Some(position) = self.view_time_to_source(self.playhead, &asset) else {
                self.toast(Severity::Warn, "El playhead no corresponde al medio de los items");
                return;
            };
            match self.session.execute(CommandEnvelope::human(Command::PasteItems { layer_id: layer_id.clone(), items, position })) {
                Ok(r) => {
                    self.selection.items = r.effect.created.iter().map(|id| (layer_id.clone(), ItemId::new(id))).collect();
                    self.after_change();
                }
                Err(e) => self.report(e),
            }
            return;
        }
        if self.clipboard.is_empty() {
            self.toast(Severity::Warn, "Portapapeles vacío");
            return;
        }
        let at = self.playhead.floor_to_frame(self.frame_rate());
        match self.session.execute(CommandEnvelope::human(Command::PasteClips { clips: self.clipboard.clone(), position: at })) {
            Ok(r) => {
                self.selection.clips = r.effect.created.iter().map(|s| ClipId::new(s.clone())).collect();
                self.after_change();
            }
            Err(e) => self.report(e),
        }
    }

    fn open_rename_for_selection(&mut self) {
        if let Some((l, i)) = self.selection.items.first().cloned() {
            if let Some(layer) = self.project().layer(&l)
                && layer.kind.is_editable()
                && !layer.locked
                && !layer.deleted
                && let Some(item) = layer.item(&i)
            {
                self.item_editor =
                    Some(crate::item_editor::ItemEditor::new(self.project().project_id.clone(), self.session.revision(), l.clone(), item));
            }
        } else if let Some(c) = self.selection.clips.first().cloned() {
            let name = self.sequence().and_then(|s| s.clip(&c)).map(|c| c.name.clone()).unwrap_or_default();
            self.rename_dialog = Some((name, RenameTarget::Clip(c)));
        } else if let Some(l) = self.selection.layer.clone() {
            let name = self.project().layer(&l).map(|l| l.name.clone()).unwrap_or_default();
            self.rename_dialog = Some((name, RenameTarget::Layer(l)));
        } else {
            self.toast(Severity::Warn, "Nada seleccionado para editar");
        }
    }

    pub fn apply_rename(&mut self, text: String, target: RenameTarget) {
        match target {
            RenameTarget::Clip(c) => {
                self.exec(Command::SetClipProps { clip_id: c, name: Some(text), gain_db: None, transform: None });
            }
            RenameTarget::Layer(l) => {
                self.exec(Command::SetLayerProps { layer_id: l, name: Some(text), color: None, visible: None, locked: None });
            }
            RenameTarget::Track(t) => {
                self.exec(Command::SetTrackProps {
                    track_id: t,
                    name: Some(text),
                    muted: None,
                    solo: None,
                    locked: None,
                    visible: None,
                    gain_db: None,
                    height: None,
                });
            }
            RenameTarget::Project => {
                self.exec(Command::RenameProject { name: text });
            }
        }
    }

    fn set_loop_edge(&mut self, is_in: bool) {
        let t = self.playhead;
        let mut r = self.loop_range.unwrap_or(TimeRange::new(Ticks::ZERO, self.duration()));
        if is_in {
            r.start = t;
            if r.end <= t {
                r.end = self.duration();
            }
        } else {
            r.end = t;
            if r.start >= t {
                r.start = Ticks::ZERO;
            }
        }
        self.loop_range = Some(r);
        self.player_send(PlayerCommand::SetLoop(Some(r)));
    }

    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            ViewMode::Sequence => ViewMode::Source,
            ViewMode::Source => ViewMode::Sequence,
        };
        if self.view == ViewMode::Source && self.source_asset.is_none() {
            self.source_asset = self.current_layer_asset();
        }
        self.selection.clips.clear();
        self.player_send(PlayerCommand::Pause);
        self.resolved_revision = None;
        self.refresh_resolved();
        let d = self.duration();
        self.seek(self.playhead.min(d));
        self.timeline_view.fit(d);
    }

    pub fn set_source_asset(&mut self, id: AssetId) {
        self.source_asset = Some(id);
        if self.view == ViewMode::Source {
            self.resolved_revision = None;
            self.refresh_resolved();
            self.seek(Ticks::ZERO);
            self.timeline_view.fit(self.duration());
        }
    }

    /// «Revelar en fuente»: lleva a la posición fuente exacta del clip seleccionado bajo el playhead.
    fn reveal_in_source(&mut self) {
        let Some(seq) = self.sequence() else { return };
        let clip =
            self.selection.clips.first().and_then(|c| seq.clip(c)).or_else(|| seq.clips.iter().find(|c| c.range().contains(self.playhead))).cloned();
        let Some(clip) = clip else {
            self.toast(Severity::Warn, "Selecciona un clip");
            return;
        };
        let src_t = clip.seq_to_source(self.playhead).unwrap_or(clip.source.start);
        self.view = ViewMode::Source;
        self.source_asset = Some(clip.asset_id.clone());
        self.selection.clips.clear();
        self.player_send(PlayerCommand::Pause);
        self.resolved_revision = None;
        self.refresh_resolved();
        self.in_point = Some(clip.source.start);
        self.out_point = Some(clip.source.end);
        self.seek(src_t);
        self.timeline_view.center_on(src_t);
        self.toast(Severity::Info, format!("Fuente {} en {}", clip.name, src_t.timecode_ms()));
    }

    /// Ctrl+Shift+A: añade el rango IN/OUT (o el item seleccionado) de la fuente al final de la secuencia.
    fn add_selection_to_sequence(&mut self) {
        let Some(asset) = self.current_layer_asset() else {
            self.toast(Severity::Warn, "No hay medio fuente");
            return;
        };
        let ranges = if self.view == ViewMode::Source {
            match (self.in_point, self.out_point) {
                (Some(i), Some(o)) if i < o => Some(vec![TimeRange::new(i, o)]),
                _ => self.selection.items.first().and_then(|(l, i)| self.project().layer(l).and_then(|l| l.item(i)).map(|it| it.ranges.clone())),
            }
        } else {
            self.selection.items.first().and_then(|(l, i)| self.project().layer(l).and_then(|l| l.item(i)).map(|it| it.ranges.clone()))
        };
        let Some(ranges) = ranges else {
            self.toast(Severity::Warn, "Marca IN/OUT (I/O) o selecciona un tramo");
            return;
        };
        self.append_source_ranges(asset, ranges, "Añadir selección multirrango");
    }

    fn add_topic_to_sequence(&mut self) {
        let selected = self.selection.items.first().and_then(|(l, i)| self.project().layer(l).map(|l| (l, i)));
        let Some((layer, id)) = selected.filter(|(l, _)| l.kind == LayerKind::Topics) else {
            self.toast(Severity::Warn, "Selecciona un tema o subtema");
            return;
        };
        let Some(mut item) = layer.item(id) else {
            return;
        };
        for _ in 0..tv2_domain::layers::MAX_HIERARCHY_DEPTH {
            let Some(parent) = item.parent_id.as_ref().and_then(|id| layer.item(id)) else {
                break;
            };
            item = parent;
        }
        self.append_source_ranges(layer.asset_id.clone(), item.ranges.clone(), "Añadir tema completo");
    }

    fn append_source_ranges(&mut self, asset: AssetId, ranges: Vec<TimeRange>, label: &str) {
        let mut position = self.sequence().map(|s| s.extent()).unwrap_or(Ticks::ZERO);
        let mut commands = Vec::new();
        for range in ranges {
            if range.end <= range.start {
                self.toast(Severity::Warn, "Un punto no tiene duración para montaje");
                return;
            }
            commands.push(Command::InsertAssetLinked { asset_id: asset.clone(), position, video_track: None, source: Some(range) });
            let Some(next) = position.0.checked_add(range.duration().0) else {
                self.toast(Severity::Error, "Duración de secuencia agotada");
                return;
            };
            position = Ticks(next);
        }
        if self.exec(Command::Batch { label: label.into(), commands }) {
            self.toast(Severity::Info, "Rangos añadidos en una sola operación de Deshacer");
        }
    }

    pub fn reveal_occurrence(&mut self, clip: ClipId, time: Ticks) {
        self.view = ViewMode::Sequence;
        self.selection.clips = vec![clip];
        self.player_send(PlayerCommand::Pause);
        self.resolved_revision = None;
        self.refresh_resolved();
        self.seek(time);
        self.timeline_view.center_on(time);
    }

    fn navigate_silence(&mut self, next: bool) {
        let mut positions = Vec::new();
        for layer in self.project().layers.iter().filter(|l| !l.deleted && l.kind == LayerKind::Silence) {
            for range in layer.items.iter().flat_map(|i| &i.ranges) {
                match self.view {
                    ViewMode::Source if self.source_asset.as_ref() == Some(&layer.asset_id) => positions.push(range.start),
                    ViewMode::Sequence => {
                        if let Some(seq) = self.sequence() {
                            positions.extend(seq.range_occurrences(&layer.asset_id, *range).iter().map(|o| o.sequence.start));
                        }
                    }
                    _ => {}
                }
            }
        }
        positions.sort();
        positions.dedup();
        let target = if next { positions.into_iter().find(|t| *t > self.playhead) } else { positions.into_iter().rev().find(|t| *t < self.playhead) };
        match target {
            Some(t) => self.seek(t),
            None => self.toast(Severity::Info, "No hay otro silencio importado en esa dirección"),
        }
    }

    fn add_point_mark(&mut self) {
        let Some(layer_id) = self.ensure_author_layer() else { return };
        let asset = self.project().layer(&layer_id).map(|l| l.asset_id.clone()).unwrap();
        let Some(t) = self.view_time_to_source(self.playhead, &asset) else {
            self.toast(Severity::Warn, "El playhead no está sobre un clip de ese medio");
            return;
        };
        let mut item = SemanticItem::new(TimeRange::new(t, t), "Marca");
        item.item_id = LayerKind::Author.fresh_item_id();
        item.state = ItemState::Proposed;
        let id = item.item_id.clone();
        if self.exec(Command::AddItem { layer_id: layer_id.clone(), item }) {
            self.selection.items = vec![(layer_id, id)];
        }
    }

    /// Capa «Marcas del autor» del asset actual (se crea si falta).
    fn ensure_author_layer(&mut self) -> Option<LayerId> {
        let asset = self.current_layer_asset()?;
        if let Some(l) = self.project().layers.iter().find(|l| l.asset_id == asset && l.kind == LayerKind::Author && !l.deleted) {
            return Some(l.layer_id.clone());
        }
        match self.session.execute(CommandEnvelope::human(Command::CreateLayer {
            asset_id: asset,
            kind: LayerKind::Author,
            name: "Marcas del autor".into(),
            color: Some("#d09947".into()),
            layer_id: None,
        })) {
            Ok(r) => {
                self.after_change();
                Some(LayerId::new(r.effect.created[0].clone()))
            }
            Err(e) => {
                self.report(e);
                None
            }
        }
    }

    /// Convierte un tiempo de la vista actual a tiempo fuente del asset.
    pub fn view_time_to_source(&self, t: Ticks, asset: &AssetId) -> Option<Ticks> {
        match self.view {
            ViewMode::Source => (self.source_asset.as_ref() == Some(asset)).then_some(t),
            ViewMode::Sequence => self.sequence()?.clips.iter().filter(|c| &c.asset_id == asset && c.enabled).find_map(|c| c.seq_to_source(t)),
        }
    }

    pub fn view_edge_to_source(&self, t: Ticks, asset: &AssetId, edge: ClipEdge) -> Option<Ticks> {
        match self.view {
            ViewMode::Source => self.view_time_to_source(t, asset),
            ViewMode::Sequence => {
                self.sequence()?.clips.iter().filter(|c| &c.asset_id == asset && c.enabled).find_map(|c| c.seq_edge_to_source(t, edge))
            }
        }
    }

    pub fn add_range_to_layer(&mut self) {
        let Some(layer_id) = self.selection.layer.clone() else {
            self.toast(Severity::Warn, "Selecciona una capa (clic en su nombre) y marca IN/OUT");
            return;
        };
        let (Some(i), Some(o)) = (self.in_point, self.out_point) else {
            self.toast(Severity::Warn, "Marca IN (I) y OUT (O) antes de añadir el tramo");
            return;
        };
        if i >= o {
            self.toast(Severity::Warn, "IN debe ser anterior a OUT");
            return;
        }
        let asset = self.project().layer(&layer_id).map(|l| l.asset_id.clone()).unwrap();
        let (Some(a), Some(b)) = (self.view_time_to_source(i, &asset), self.view_time_to_source(o - Ticks(1), &asset).map(|t| t + Ticks(1))) else {
            self.toast(Severity::Warn, "IN/OUT deben caer sobre clips de ese medio");
            return;
        };
        let n = self.project().layer(&layer_id).map(|l| l.items.len() + 1).unwrap_or(1);
        let mut item = SemanticItem::new(TimeRange::new(a.min(b), a.max(b)), format!("Tramo {n}"));
        if let Some(layer) = self.project().layer(&layer_id) {
            item.item_id = layer.kind.fresh_item_id();
        }
        let id = item.item_id.clone();
        if self.exec(Command::AddItem { layer_id: layer_id.clone(), item }) {
            self.selection.items = vec![(layer_id, id)];
            self.in_point = None;
            self.out_point = None;
        }
    }

    pub fn create_layer(&mut self, name: String) {
        let Some(asset) = self.current_layer_asset() else {
            self.toast(Severity::Warn, "Importa un medio antes de crear capas");
            return;
        };
        match self.session.execute(CommandEnvelope::human(Command::CreateLayer {
            asset_id: asset,
            kind: self.new_layer_kind.clone(),
            name,
            color: None,
            layer_id: None,
        })) {
            Ok(r) => {
                self.selection.layer = Some(LayerId::new(r.effect.created[0].clone()));
                self.after_change();
            }
            Err(e) => self.report(e),
        }
    }

    // ---------- export ----------

    pub fn open_export_dialog(&mut self) {
        if self.export.destination.is_empty() {
            let dir = self
                .ui
                .last_export_dir
                .clone()
                .or_else(|| self.store.as_ref().and_then(|s| s.root.parent().map(|p| p.to_string_lossy().to_string())))
                .unwrap_or_else(|| std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into()));
            self.export.destination = format!("{}\\{}-export.mp4", dir.trim_end_matches(['\\', '/']), tv2_slug(&self.project().name));
        }
        self.export.open = true;
    }

    pub fn start_export(&mut self, preset: ExportPreset) {
        let Ok(_) = &self.tools else {
            self.toast(Severity::Error, "FFmpeg no disponible");
            return;
        };
        if self.view != ViewMode::Sequence {
            self.toast(Severity::Warn, "La exportación usa la secuencia; cambia a Secuencia (Ctrl+M)");
            return;
        }
        let Some(seq) = self.sequence() else { return };
        let mut resolved = Arc::new(ResolvedTimeline::resolve(self.project(), seq));
        if resolved.duration.0 <= 0 {
            self.toast(Severity::Warn, "La secuencia está vacía");
            return;
        }
        let range = if self.export.range_mode == 1 {
            match (self.in_point, self.out_point) {
                (Some(i), Some(o)) if i < o => Some(TimeRange::new(i, o)),
                _ => {
                    self.toast(Severity::Warn, "No hay IN/OUT válidos");
                    return;
                }
            }
        } else {
            None
        };
        if self.export.range_mode >= 2 {
            let ranges: Vec<TimeRange> = if self.export.range_mode == 2 {
                self.selection.clips.iter().filter_map(|id| seq.clip(id)).filter(|c| c.enabled).map(|c| c.range()).collect()
            } else {
                self.selection
                    .items
                    .iter()
                    .flat_map(|(layer_id, item_id)| {
                        let Some(layer) = self.project().layer(layer_id).filter(|l| !l.deleted) else { return Vec::new() };
                        let Some(item) = layer.item(item_id) else { return Vec::new() };
                        seq.clips
                            .iter()
                            .filter(|c| c.enabled && c.asset_id == layer.asset_id)
                            .flat_map(|clip| {
                                item.ranges
                                    .iter()
                                    .filter_map(|r| clip.source.intersection(r))
                                    .map(|r| TimeRange::new(clip.position + r.start - clip.source.start, clip.position + r.end - clip.source.start))
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };
            match resolved.extract_ranges(&ranges) {
                Ok(extracted) => resolved = Arc::new(extracted),
                Err(error) => {
                    self.toast(Severity::Warn, error.to_string());
                    return;
                }
            }
        }
        let dest = PathBuf::from(&self.export.destination);
        if self.export.pending.len() >= 16 {
            self.toast(Severity::Warn, "La cola está llena (16 pendientes). Espera o cancela un trabajo");
            return;
        }
        if self.export.pending.iter().any(|r| r.destination == dest) || self.export.running.as_ref().is_some_and(|r| r.destination == dest) {
            self.toast(Severity::Warn, "Ese destino ya está en la cola; elige otro nombre");
            return;
        }
        if let Some(parent) = dest.parent() {
            self.ui.last_export_dir = Some(parent.to_string_lossy().to_string());
        }
        let req = ExportRequest {
            timeline: resolved,
            assets: Arc::new(self.asset_sources()),
            preset,
            range,
            destination: dest.clone(),
            project_revision: self.session.revision(),
        };
        self.export.pending.push_back(req);
        self.launch_next_export();
        self.toast(Severity::Info, "Exportación añadida a la cola (revisión congelada)");
    }

    fn launch_next_export(&mut self) {
        if self.export.running.is_some() {
            return;
        }
        let Some(req) = self.export.pending.pop_front() else {
            return;
        };
        let destination = req.destination.clone();
        let revision = req.project_revision;
        let Ok(tools) = self.tools.clone() else {
            let error = Err(DomainError::unsupported("FFmpeg no disponible"));
            self.export.history.push((destination, revision, error.clone()));
            self.export.last_result = Some(error);
            return;
        };
        let progress = Arc::new(parking_lot::Mutex::new(ExportProgress::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (p2, c2) = (progress.clone(), cancel.clone());
        let spawned = std::thread::Builder::new().name("export".into()).spawn(move || {
            let r = tv2_media::export::ExportJob::run(&tools, &req, &c2, |p| *p2.lock() = p);
            let _ = tx.send(r);
        });
        match spawned {
            Ok(thread) => {
                self.export.running =
                    Some(RunningExport { progress, cancel, rx, started: Instant::now(), destination, revision, thread: Some(thread) })
            }
            Err(e) => {
                let error = Err(DomainError::process(format!("No se pudo iniciar exportación: {e}")));
                self.export.history.push((destination, revision, error.clone()));
                self.export.last_result = Some(error);
            }
        }
    }

    pub fn cancel_queued_export(&mut self, index: usize) {
        if let Some(req) = self.export.pending.remove(index) {
            self.export.history.push((
                req.destination,
                req.project_revision,
                Err(DomainError::new(tv2_domain::error::ErrorCode::Cancelled, "Cancelado antes de iniciar")),
            ));
        }
    }

    fn poll_export(&mut self) {
        let done = self.export.running.as_ref().and_then(|r| match r.rx.try_recv() {
            Ok(result) => Some(result),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(DomainError::process("El worker de exportación terminó sin resultado"))),
        });
        if let Some(result) = done {
            match &result {
                Ok(r) => {
                    let msg = format!(
                        "Exportado {} ({}, {} fotogramas, {:.1} s)",
                        r.path.display(),
                        r.duration.timecode_ms(),
                        r.frames_written,
                        r.elapsed_s
                    );
                    self.toast(Severity::Info, msg);
                }
                Err(e) => self.report(e.clone()),
            }
            if let Some(r) = self.export.running.take() {
                self.export.history.push((r.destination.clone(), r.revision, result.clone()));
            }
            self.export.last_result = Some(result);
        }
        if self.export.history.len() > 32 {
            self.export.history.drain(..self.export.history.len() - 32);
        }
        self.launch_next_export();
    }

    // ---------- teclado ----------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.shortcuts_open {
            if let Some(editor) = &mut self.shortcut_editor
                && editor.capturing
            {
                let captured = ctx.input(|i| {
                    i.events.iter().find_map(|event| match event {
                        egui::Event::Key { key, modifiers, pressed: true, repeat: false, .. } => chord_from_egui(*key, *modifiers),
                        _ => None,
                    })
                });
                if let Some(chord) = captured {
                    editor.text = chord;
                    editor.capturing = false;
                }
            }
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return; // un campo de texto tiene el foco: las teclas de edición no disparan
        }
        let events: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, modifiers) in events {
            let Some(chord) = chord_from_egui(key, modifiers) else { continue };
            if let Some(action) = self.keymap.resolve(&chord) {
                let action = action.to_string();
                self.dispatch(&action);
            }
        }
    }

    fn poll_player(&mut self) {
        let Some(p) = &self.player else { return };
        self.player_snapshot = p.snapshot();
        if self.player_snapshot.playing {
            self.playhead = self.player_snapshot.position;
            if self.ui.follow_playhead {
                self.timeline_view.ensure_visible(self.playhead);
            }
        } else if let Some(f) = &self.last_frame
            && self.player_snapshot.generation == f.generation
        {
            // en pausa el playhead lo manda la GUI
        }
        if let Some(e) = self.player_snapshot.last_error.take() {
            self.toast(Severity::Error, e);
        }
    }

    fn poll_console(&mut self) {
        while let Ok(line) = self.console_rx.try_recv() {
            self.console_lines.push(line);
        }
        if self.console_lines.len() > 2000 {
            let n = self.console_lines.len() - 2000;
            self.console_lines.drain(..n);
        }
    }

    fn autosave(&mut self) {
        if let Some(job) = &self.autosave_job {
            match job.try_recv() {
                Ok(result) => {
                    self.autosave_job = None;
                    if let Err(e) = result {
                        self.report(e.with_action("Falló el autosave; guarda el proyecto"));
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.autosave_job = None;
                    self.report(DomainError::process("Worker de autosave terminó sin resultado"));
                }
            }
        }
        if self.persistence_job.is_some() || self.last_autosave.elapsed().as_secs() < 60 || !self.session.is_dirty() {
            return;
        }
        self.last_autosave = Instant::now();
        let saved = self.store.is_some();
        if !saved && self.unsaved_store.is_none() {
            let name = format!("{}-{}.transcriptor", self.project().project_id, tv2_domain::ids::random_hex12());
            self.unsaved_store = Some(ProjectStore::at(crate::paths::config_dir().join("recovery").join(name)));
        }
        let store = self.store.as_ref().or(self.unsaved_store.as_ref()).unwrap().clone();
        let project = self.project().clone();
        let events = self.session.pending_journal().to_vec();
        let history = self.session.history_snapshot();
        let (tx, rx) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("project-autosave".into()).spawn(move || {
            let result = if saved {
                store.save_autosave_checkpoint(&project, &events, Some(&history))
            } else {
                store.save_checkpoint(&project, &events, Some(&history))
            };
            let _ = tx.send(result);
        }) {
            Ok(_) => self.autosave_job = Some(rx),
            Err(e) => self.report(DomainError::process(format!("No se pudo iniciar autosave: {e}"))),
        }
    }

    pub fn save_ui_state(&self) {
        let mut ui = self.ui.clone();
        ui.zoom_px_per_s = self.timeline_view.px_per_s;
        if let Ok(text) = serde_json::to_string_pretty(&ui) {
            let path = crate::paths::ui_state_file();
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(path, text);
        }
    }
}

impl eframe::App for TranscriptorApp {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        self.poll_console();
        self.poll_player();
        self.poll_export();
        self.poll_import();
        self.poll_editorial_import();
        self.poll_v1_export();
        self.poll_persistence();
        if self.close_after_save && self.persistence_job.is_none() && !self.session.is_dirty() {
            self.close_after_save = false;
            self.pending_close = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.external.poll(self.store.as_ref(), self.session.project(), ctx);
        self.refresh_resolved();
        // archivos soltados sobre la ventana
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.import_paths(&dropped);
        }
        self.handle_keys(ctx);
        // fotograma nuevo del reproductor
        if let Some(p) = &self.player
            && let Some(pf) = p.take_frame(&mut self.frame_seen)
        {
            let img = egui::ColorImage::from_rgba_unmultiplied([pf.frame.width as usize, pf.frame.height as usize], &pf.frame.rgba);
            match &mut self.texture {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("visor", img, egui::TextureOptions::LINEAR)),
            }
            if !self.player_snapshot.playing {
                self.playhead = pf.position;
            }
            self.last_frame = Some(pf);
        }
        if self.dirty_title {
            let title = format!("{}{} — Transcriptor V2", self.project().name, if self.session.is_dirty() { " *" } else { "" });
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
            self.dirty_title = false;
        }
        if let Some(media) = &mut self.media_view {
            media.begin_frame();
        }
        crate::ui_panels::draw(self, root);
        crate::item_editor::draw(self, ctx);
        if let Some(media) = &mut self.media_view {
            media.end_frame();
        }
        self.frame_count += 1;
        self.script_tick(ctx);
        self.autosave();
        if self.session.is_dirty() || self.autosave_job.is_some() || self.v1_export_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        // toasts caducan
        self.toasts.retain(|t| t.at.elapsed().as_secs_f32() < if t.severity == Severity::Error { 10.0 } else { 5.0 });
        if ctx.input(|i| i.viewport().close_requested()) && (self.persistence_job.is_some() || self.autosave_job.is_some()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.toast(Severity::Info, "Espera a que termine la escritura antes de cerrar");
        } else if ctx.input(|i| i.viewport().close_requested()) && self.session.is_dirty() && !self.pending_close {
            self.pending_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.imports.busy()
            || self.persistence_job.is_some()
            || self.editorial_job.is_some()
            || self.player_snapshot.playing
            || self.export.running.is_some()
            || !self.toasts.is_empty()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        let _ = frame;
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_ui_state();
    }

    fn on_exit(&mut self) {
        self.save_ui_state();
        self.shutdown_workers();
    }
}

impl TranscriptorApp {
    pub fn shutdown_workers(&mut self) {
        self.imports.cancel();
        if let Some(job) = &self.editorial_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        self.imports.running.take();
        self.export.pending.clear();
        if let Some(job) = &self.export.running {
            job.cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(media) = &mut self.media_view {
            media.clear();
        }
        self.export.running.take();
        self.media_view.take();
        self.player.take();
    }
}

impl Drop for TranscriptorApp {
    fn drop(&mut self) {
        self.shutdown_workers();
    }
}

pub fn describe_asset(a: &tv2_domain::asset::Asset) -> String {
    let mut parts = Vec::new();
    match a.kind {
        AssetKind::Video => parts.push("video".to_string()),
        AssetKind::Audio => parts.push("audio".to_string()),
        AssetKind::Image => parts.push("imagen".to_string()),
    }
    if let Some(v) = &a.probe.video {
        let (w, h) = v.display_size();
        parts.push(format!("{w}×{h}"));
        if a.kind == AssetKind::Video {
            parts.push(format!("{:.3} fps{}", v.frame_rate.as_f64(), if v.variable_frame_rate { " (VFR)" } else { "" }));
        }
        parts.push(v.codec.clone());
        if v.rotation != 0 {
            parts.push(format!("rot {}°", v.rotation));
        }
    }
    if !a.probe.audio.is_empty() {
        let s = &a.probe.audio[0];
        parts.push(format!("{} pista(s) de audio {} Hz {} can.", a.probe.audio.len(), s.sample_rate, s.channels));
    }
    if a.kind != AssetKind::Image {
        parts.push(a.probe.duration.timecode_ms());
    }
    if a.probe.start_time != Ticks::ZERO {
        parts.push(format!("start {}", a.probe.start_time.as_seconds_f64()));
    }
    parts.join(" · ")
}

pub fn tv2_slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let s = s.trim_matches('-').to_lowercase();
    if s.is_empty() { "proyecto".into() } else { s }
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(28, 29, 33);
    style.visuals.panel_fill = egui::Color32::from_rgb(28, 29, 33);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(18, 18, 21);
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(208, 153, 71).linear_multiply(0.35);
    style.visuals.selection.stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(240, 190, 90));
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    ctx.set_global_style(style);
}

pub mod colors {
    use egui::Color32;
    pub const ACCENT: Color32 = Color32::from_rgb(208, 153, 71);
    pub const VIDEO_CLIP: Color32 = Color32::from_rgb(66, 110, 170);
    pub const AUDIO_CLIP: Color32 = Color32::from_rgb(60, 140, 110);
    pub const IMAGE_CLIP: Color32 = Color32::from_rgb(140, 90, 170);
    pub const DISABLED: Color32 = Color32::from_rgb(80, 80, 86);
    pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 80, 80);
    pub const INOUT: Color32 = Color32::from_rgb(120, 200, 255);
    pub const LOOP: Color32 = Color32::from_rgb(255, 210, 100);
    pub const RULER_BG: Color32 = Color32::from_rgb(36, 37, 42);
    pub const LANE_BG: Color32 = Color32::from_rgb(32, 33, 38);
    pub const LANE_ALT: Color32 = Color32::from_rgb(38, 39, 45);
    pub const HEADER_BG: Color32 = Color32::from_rgb(44, 45, 52);
    pub const TEXT: Color32 = Color32::from_rgb(225, 225, 228);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 150, 158);
    pub const OK: Color32 = Color32::from_rgb(90, 200, 120);
    pub const WARN: Color32 = Color32::from_rgb(240, 180, 60);
    pub const ERR: Color32 = Color32::from_rgb(240, 90, 90);
}
