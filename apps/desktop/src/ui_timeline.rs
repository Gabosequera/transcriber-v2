//! Timeline: regla, pistas multimedia, carriles semánticos, playhead, IN/OUT,
//! loop y gestos (selección, arrastre con previsualización y una sola
//! transacción al soltar, trim de bordes, corte con herramienta, scrub, zoom
//! anclado al cursor, pan). Escape cancela sin residuo.

use crate::app::{RenameTarget, Severity, Tool, TranscriptorApp, ViewMode, colors};
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use tv2_domain::ids::{ClipId, ItemId, LayerId, TrackId};
use tv2_domain::layers::ItemState;
use tv2_domain::time::{Ticks, TimeRange};
use tv2_domain::timeline::TrackKind;
use tv2_domain::{ClipEdge, Command, MovePolicy};

/// Previsualización de arrastre: (clips, delta, delta de pista, válido).
type MovePreview = (Vec<ClipId>, Ticks, i32, bool);
/// Previsualización de trim: (clip, borde, tiempo nuevo).
type TrimPreview = (ClipId, ClipEdge, Ticks);

pub const HEADER_W: f32 = 150.0;
pub const RULER_H: f32 = 24.0;
pub const DEFAULT_TRACK_H: f32 = 48.0;
pub const LAYER_H: f32 = 30.0;
pub const HANDLE_W: f32 = 7.0;
pub const DRAG_THRESHOLD: f32 = 4.0;
pub const SNAP_PX: f32 = 8.0;

#[derive(Clone, Debug)]
pub enum Gesture {
    None,
    /// Posible arrastre (press sin superar el umbral).
    Pending {
        clip: ClipId,
        origin: Pos2,
        edge: Option<ClipEdge>,
        additive: bool,
        base_revision: u64,
    },
    MoveClips {
        clips: Vec<ClipId>,
        origin: Pos2,
        delta_t: Ticks,
        track_delta: i32,
        valid: bool,
        base_revision: u64,
    },
    TrimClip {
        clip: ClipId,
        edge: ClipEdge,
        new_time: Ticks,
        origin: Pos2,
        base_revision: u64,
    },
    Scrub,
    Pan {
        last: Pos2,
    },
    /// Press en un carril vacío: clic = playhead; arrastre = selección por área.
    LanePress {
        origin: Pos2,
        additive: bool,
    },
    PendingItem {
        layer: LayerId,
        item: ItemId,
        origin: Pos2,
        additive: bool,
        range_index: usize,
        edge: Option<ClipEdge>,
        base_revision: u64,
    },
    EditItems {
        origin: Pos2,
        layer: LayerId,
        item: ItemId,
        range_index: usize,
        edge: Option<ClipEdge>,
        base_revision: u64,
    },
    BoxSelect {
        origin: Pos2,
        current: Pos2,
        additive: bool,
    },
    BoxEdit {
        origin: Pos2,
        layer: LayerId,
        subtract: bool,
        base_revision: u64,
    },
}

pub struct TimelineView {
    pub px_per_s: f32,
    pub scroll_t: Ticks,
    pub scroll_y: f32,
    pub gesture: Gesture,
    pub hover_time: Option<Ticks>,
    last_width: f32,
    /// Geometría del último frame (para pruebas y depuración): rect total, x0 del contenido y carriles (id, y, alto).
    pub last_rect: Rect,
    pub last_x0: f32,
    pub last_lanes: Vec<(String, f32, f32)>,
    pub index: crate::timeline_index::ClipIndex,
    pub semantic_index: crate::timeline_index::SemanticIndex,
}

impl TimelineView {
    pub fn new(px_per_s: f32) -> Self {
        TimelineView {
            px_per_s: px_per_s.clamp(2.0, 4000.0),
            scroll_t: Ticks::ZERO,
            scroll_y: 0.0,
            gesture: Gesture::None,
            hover_time: None,
            last_width: 800.0,
            last_rect: Rect::NOTHING,
            last_x0: 0.0,
            last_lanes: Vec::new(),
            index: Default::default(),
            semantic_index: Default::default(),
        }
    }

    pub fn cancel_gesture(&mut self) {
        self.gesture = Gesture::None;
    }

    pub fn t_to_x(&self, t: Ticks, x0: f32) -> f32 {
        x0 + (t - self.scroll_t).as_seconds_f64() as f32 * self.px_per_s
    }

    pub fn x_to_t(&self, x: f32, x0: f32) -> Ticks {
        self.scroll_t + Ticks::from_seconds_f64(((x - x0) / self.px_per_s) as f64)
    }

    pub fn visible_span(&self) -> Ticks {
        Ticks::from_seconds_f64((self.last_width / self.px_per_s) as f64)
    }

    pub fn zoom_by(&mut self, factor: f32, anchor: Option<(f32, f32)>) {
        let (ax, x0) = anchor.unwrap_or((self.last_width / 2.0, 0.0));
        let t_at = self.x_to_t(ax, x0);
        self.px_per_s = (self.px_per_s * factor).clamp(2.0, 4000.0);
        // mantener t_at bajo el cursor
        self.scroll_t = t_at - Ticks::from_seconds_f64(((ax - x0) / self.px_per_s) as f64);
        self.scroll_t = self.scroll_t.max(Ticks::ZERO);
    }

    pub fn fit(&mut self, duration: Ticks) {
        let secs = duration.as_seconds_f64().max(1.0) as f32;
        self.px_per_s = ((self.last_width - 20.0) / secs).clamp(2.0, 4000.0);
        self.scroll_t = Ticks::ZERO;
    }

    pub fn zoom_to(&mut self, r: TimeRange) {
        let secs = r.duration().as_seconds_f64().max(0.2) as f32;
        self.px_per_s = ((self.last_width - 40.0) / secs).clamp(2.0, 4000.0);
        self.scroll_t = (r.start - Ticks::from_seconds_f64((20.0 / self.px_per_s) as f64)).max(Ticks::ZERO);
    }

    pub fn center_on(&mut self, t: Ticks) {
        self.scroll_t = (t - Ticks::from_seconds_f64((self.last_width / 2.0 / self.px_per_s) as f64)).max(Ticks::ZERO);
    }

    pub fn ensure_visible(&mut self, t: Ticks) {
        let span = self.visible_span();
        if t < self.scroll_t || t > self.scroll_t + span {
            self.scroll_t = (t - Ticks::from_seconds_f64((20.0 / self.px_per_s) as f64)).max(Ticks::ZERO);
        }
    }

    pub fn snap(&self, t: Ticks, edges: &[Ticks], enabled: bool) -> Ticks {
        if !enabled {
            return t;
        }
        let radius = Ticks::from_seconds_f64((SNAP_PX / self.px_per_s) as f64);
        let mut best: Option<(Ticks, Ticks)> = None;
        for e in edges {
            let d = (*e - t).abs();
            if d <= radius && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, *e));
            }
        }
        best.map(|(_, e)| e).unwrap_or(t)
    }
}

struct Lane {
    kind: LaneKind,
    y: f32,
    h: f32,
    source_stream: Option<u32>,
}

#[derive(Clone)]
enum LaneKind {
    Track(TrackId),
    Layer(LayerId),
}

pub fn draw(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    if app.view == ViewMode::Sequence {
        let mut index = std::mem::take(&mut app.timeline_view.index);
        if let Some(sequence) = app.sequence() {
            index.ensure_async(&app.project().project_id, app.session.revision(), sequence, ui.ctx());
        }
        if index.busy() {
            ui.label("Actualizando índices multimedia…");
        }
        if let Some(error) = index.error().map(str::to_owned) {
            ui.horizontal(|ui| {
                ui.colored_label(colors::WARN, error);
                if ui.button("Reintentar índices").clicked() {
                    index.retry();
                }
            });
        }
        app.timeline_view.index = index;
    }
    let mut semantic_index = std::mem::take(&mut app.timeline_view.semantic_index);
    semantic_index.ensure(app, ui.ctx());
    if semantic_index.busy() {
        ui.label("Actualizando índices editoriales…");
    }
    if let Some(error) = semantic_index.error().map(str::to_owned) {
        ui.horizontal(|ui| {
            ui.colored_label(colors::WARN, error);
            if ui.button("Reintentar índices").clicked() {
                semantic_index.retry();
            }
        });
    }
    app.timeline_view.semantic_index = semantic_index;
    let available = ui.available_rect_before_wrap();
    let rect = available;
    ui.expand_to_include_rect(rect);
    let painter = ui.painter_at(rect);
    app.timeline_view.last_width = (rect.width() - HEADER_W).max(50.0);
    let x0 = rect.left() + HEADER_W;
    app.timeline_view.last_rect = rect;
    app.timeline_view.last_x0 = x0;
    let content_rect = Rect::from_min_max(Pos2::new(x0, rect.top() + RULER_H), rect.max);
    painter.rect_filled(rect, 0.0, colors::LANE_BG);

    // ---- lanes ----
    let mut lanes: Vec<Lane> = Vec::new();
    let mut y = rect.top() + RULER_H - app.timeline_view.scroll_y;
    let layer_asset = app.current_layer_asset();
    if app.view == ViewMode::Sequence {
        if let Some(seq) = app.sequence() {
            // pistas de video de arriba (última = más alta) hacia abajo, luego audio
            let mut order: Vec<&tv2_domain::timeline::Track> = seq.tracks.iter().filter(|t| t.kind == TrackKind::Video).collect();
            order.reverse();
            order.extend(seq.tracks.iter().filter(|t| t.kind == TrackKind::Audio));
            for t in order {
                let h = app.ui.track_heights.get(t.id.as_str()).copied().or(t.height).unwrap_or(DEFAULT_TRACK_H);
                lanes.push(Lane { kind: LaneKind::Track(t.id.clone()), y, h, source_stream: None });
                y += h;
            }
        }
    } else if let Some(a) = &app.source_asset
        && let Some(asset) = app.project().asset(a)
    {
        // en Fuente: una banda por el medio (video + pistas de audio)
        let h = DEFAULT_TRACK_H;
        if asset.probe.video.is_some() {
            lanes.push(Lane { kind: LaneKind::Track(TrackId::new(format!("source:{}", asset.id))), y, h, source_stream: None });
            y += h;
        }
        for stream in 0..asset.probe.audio.len() {
            lanes.push(Lane {
                kind: LaneKind::Track(TrackId::new(format!("source:{}:audio:{stream}", asset.id))),
                y,
                h,
                source_stream: Some(stream as u32),
            });
            y += h;
        }
    }
    let layers: Vec<(LayerId, String, String, bool)> = layer_asset
        .as_ref()
        .map(|a| {
            app.project()
                .ordered_layers()
                .iter()
                .filter(|l| &l.asset_id == a && l.visible)
                .map(|l| (l.layer_id.clone(), l.name.clone(), l.color.clone(), l.locked))
                .collect()
        })
        .unwrap_or_default();
    for (id, _, _, _) in &layers {
        lanes.push(Lane { kind: LaneKind::Layer(id.clone()), y, h: LAYER_H, source_stream: None });
        y += LAYER_H;
    }
    let total_h = y - (rect.top() + RULER_H - app.timeline_view.scroll_y);
    app.timeline_view.last_lanes = lanes
        .iter()
        .map(|l| {
            let id = match &l.kind {
                LaneKind::Track(t) => t.to_string(),
                LaneKind::Layer(l) => l.to_string(),
            };
            (id, l.y, l.h)
        })
        .collect();

    // ---- input global del área ----
    let response = ui.interact(rect, ui.id().with("timeline"), Sense::click_and_drag());
    // Estado del puntero directo (press/down/release): un gesto se captura desde el press hasta el release,
    // sin depender del umbral interno de arrastre de egui.
    let (pointer, primary_pressed, primary_down, primary_released, press_origin, secondary_clicked) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            i.pointer.press_origin(),
            i.pointer.secondary_clicked(),
        )
    });
    let pointer_in = pointer.is_some_and(|p| rect.contains(p));
    let hover_in_content = pointer.is_some_and(|p| content_rect.contains(p)) && response.contains_pointer();
    let press_in_rect = primary_pressed && press_origin.is_some_and(|o| rect.contains(o)) && response.contains_pointer();
    let secondary = secondary_clicked && pointer_in && response.contains_pointer();
    let modifiers = ui.input(|i| i.modifiers);
    // zoom / scroll con rueda
    if response.hovered() {
        let (scroll, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        if let Some(p) = pointer {
            if modifiers.ctrl || (zoom_delta - 1.0).abs() > 1e-3 {
                let factor = if modifiers.ctrl && scroll.y.abs() > 0.0 { (1.0 + scroll.y / 200.0).clamp(0.5, 2.0) } else { zoom_delta };
                if (factor - 1.0).abs() > 1e-4 {
                    app.timeline_view.zoom_by(factor, Some((p.x, x0)));
                }
            } else if modifiers.shift || scroll.x.abs() > 0.0 {
                let dx = if scroll.x.abs() > 0.0 { scroll.x } else { scroll.y };
                let dt = Ticks::from_seconds_f64((-dx / app.timeline_view.px_per_s) as f64);
                app.timeline_view.scroll_t = (app.timeline_view.scroll_t + dt).max(Ticks::ZERO);
            } else if scroll.y.abs() > 0.0 {
                let max_scroll = (total_h - content_rect.height()).max(0.0);
                app.timeline_view.scroll_y = (app.timeline_view.scroll_y - scroll.y).clamp(0.0, max_scroll);
            }
        }
    }
    app.timeline_view.hover_time = if hover_in_content { pointer.map(|p| app.timeline_view.x_to_t(p.x, x0).max(Ticks::ZERO)) } else { None };

    let edges_for_snap: Vec<Ticks> = {
        let mut e = Vec::new();
        let margin = Ticks::from_seconds_f64((SNAP_PX / app.timeline_view.px_per_s) as f64);
        let window = TimeRange::new(app.timeline_view.scroll_t - margin, app.timeline_view.scroll_t + app.timeline_view.visible_span() + margin);
        if (primary_down || primary_released) && app.view == ViewMode::Sequence {
            let mut index = std::mem::take(&mut app.timeline_view.index);
            if let Some(seq) = app.sequence() {
                index.ensure_async(&app.project().project_id, app.session.revision(), seq, ui.ctx());
            }
            let margin = Ticks::from_seconds_f64((SNAP_PX / app.timeline_view.px_per_s) as f64);
            let a = app.timeline_view.scroll_t - margin;
            let b = app.timeline_view.scroll_t + app.timeline_view.visible_span() + margin;
            let lo = index.edges.partition_point(|t| *t < a);
            let hi = index.edges.partition_point(|t| *t <= b);
            e.extend_from_slice(&index.edges[lo..hi]);
            app.timeline_view.index = index;
        }
        if primary_down || primary_released {
            if app.view == ViewMode::Sequence {
                e.extend_from_slice(app.timeline_view.index.marker_edges(window));
            }
            if let Some(asset) = &layer_asset {
                e.extend_from_slice(app.timeline_view.semantic_index.edges(asset, window));
            }
        }
        e.push(app.playhead);
        if let Some(i) = app.in_point {
            e.push(i);
        }
        if let Some(o) = app.out_point {
            e.push(o);
        }
        e
    };

    // ---- gestos ----
    let ruler_rect = Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(rect.right(), rect.top() + RULER_H));
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let capture_lost = !ui.input(|i| i.focused) || (!primary_down && !primary_released && !matches!(app.timeline_view.gesture, Gesture::None));
    if escape || capture_lost {
        app.timeline_view.gesture = Gesture::None;
        app.gesture_preview = Default::default();
    }
    if (press_in_rect || secondary)
        && !app.gesture_preview.commit
        && let Some(p) = pointer
    {
        if ruler_rect.contains(p) {
            if secondary {
                let t = app.timeline_view.x_to_t(p.x, x0).max(Ticks::ZERO);
                app.playhead = t;
                if modifiers.shift {
                    app.dispatch("loop.clear");
                } else if modifiers.ctrl {
                    app.dispatch("loop.set_out");
                } else {
                    app.dispatch("loop.set_in");
                }
            } else {
                app.timeline_view.gesture = Gesture::Scrub;
                let t = app.timeline_view.x_to_t(p.x, x0).max(Ticks::ZERO);
                app.player_send(tv2_media::player::PlayerCommand::Pause);
                app.scrub(t);
            }
        } else if content_rect.contains(p)
            && !app.timeline_view.semantic_index.busy()
            && app.timeline_view.semantic_index.error().is_none()
            && (app.view != ViewMode::Sequence || !app.timeline_view.index.busy() && app.timeline_view.index.error().is_none())
        {
            let hit = hit_test(app, &lanes, p, x0);
            match hit {
                Hit::Clip { clip, edge } => {
                    if app.tool == Tool::Cut && edge.is_none() && !secondary {
                        let t = app.timeline_view.x_to_t(p.x, x0).floor_to_frame(app.frame_rate());
                        app.selection.clips = vec![clip];
                        app.selection.items.clear();
                        app.seek(t);
                        app.split_at_playhead();
                    } else if secondary {
                        if !app.selection.clips.contains(&clip) {
                            app.selection.clips = vec![clip];
                            app.selection.items.clear();
                        }
                    } else {
                        app.timeline_view.gesture = Gesture::Pending {
                            clip,
                            origin: p,
                            edge,
                            additive: modifiers.shift || modifiers.ctrl,
                            base_revision: app.session.revision(),
                        };
                    }
                }
                Hit::Item { layer, item, range_index, edge } => {
                    if secondary {
                        if !app.selection.items.iter().any(|(l, i)| l == &layer && i == &item) {
                            app.selection.items = vec![(layer.clone(), item)];
                            app.selection.clips.clear();
                        }
                        app.selection.layer = Some(layer);
                    } else if app.tool == Tool::Cut && edge.is_none() && !modifiers.ctrl {
                        app.timeline_view.gesture =
                            Gesture::BoxEdit { origin: p, layer, subtract: modifiers.shift, base_revision: app.session.revision() };
                    } else {
                        app.timeline_view.gesture = Gesture::PendingItem {
                            layer,
                            item,
                            origin: p,
                            additive: modifiers.shift || modifiers.ctrl,
                            range_index,
                            edge: if modifiers.ctrl { None } else { edge },
                            base_revision: app.session.revision(),
                        };
                    }
                }
                Hit::Lane(kind) => {
                    if let LaneKind::Layer(l) = &kind {
                        app.selection.layer = Some(l.clone());
                    }
                    if secondary {
                        // Right-click opens context; it must not start a pan on
                        // button release and then keep panning on mere hover.
                    } else if modifiers.alt {
                        app.timeline_view.gesture = Gesture::Pan { last: p };
                    } else if app.tool == Tool::Cut
                        && !modifiers.ctrl
                        && let LaneKind::Layer(layer) = kind
                    {
                        app.timeline_view.gesture =
                            Gesture::BoxEdit { origin: p, layer, subtract: modifiers.shift, base_revision: app.session.revision() };
                    } else {
                        let additive = modifiers.shift || modifiers.ctrl;
                        if !additive {
                            app.selection.clear();
                        }
                        if let LaneKind::Layer(layer) = kind {
                            app.selection.layer = Some(layer);
                        }
                        let t = app.timeline_view.x_to_t(p.x, x0).max(Ticks::ZERO);
                        app.player_send(tv2_media::player::PlayerCommand::Pause);
                        app.seek(t);
                        app.timeline_view.gesture = Gesture::LanePress { origin: p, additive };
                    }
                }
                Hit::Header(kind) => {
                    if let LaneKind::Layer(l) = kind {
                        app.selection.layer = Some(l);
                    }
                }
                Hit::None => {}
            }
        }
    }
    // progreso del gesto
    if let Some(p) = pointer {
        let dragging = primary_down;
        if dragging
            && matches!(
                app.timeline_view.gesture,
                Gesture::MoveClips { .. }
                    | Gesture::TrimClip { .. }
                    | Gesture::Scrub
                    | Gesture::EditItems { .. }
                    | Gesture::BoxEdit { .. }
                    | Gesture::BoxSelect { .. }
            )
        {
            let direction = if p.x > rect.right() - 20.0 {
                1.0
            } else if p.x < x0 + 20.0 {
                -1.0
            } else {
                0.0
            };
            if direction != 0.0 {
                let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.05);
                let before = app.timeline_view.scroll_t;
                app.timeline_view.scroll_t =
                    (before + Ticks::from_seconds_f64((direction * 300.0 * dt / app.timeline_view.px_per_s) as f64)).max(Ticks::ZERO);
                let px = (app.timeline_view.scroll_t - before).as_seconds_f64() as f32 * app.timeline_view.px_per_s;
                if let Gesture::MoveClips { origin, .. }
                | Gesture::EditItems { origin, .. }
                | Gesture::BoxEdit { origin, .. }
                | Gesture::BoxSelect { origin, .. } = &mut app.timeline_view.gesture
                {
                    origin.x -= px;
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        if dragging && matches!(app.timeline_view.gesture, Gesture::BoxSelect { .. } | Gesture::MoveClips { .. }) {
            let direction = if p.y > content_rect.bottom() - 20.0 {
                1.0
            } else if p.y < content_rect.top() + 20.0 {
                -1.0
            } else {
                0.0
            };
            let before = app.timeline_view.scroll_y;
            let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.05);
            app.timeline_view.scroll_y = (before + direction * 240.0 * dt).clamp(0.0, (total_h - content_rect.height()).max(0.0));
            let delta = app.timeline_view.scroll_y - before;
            if delta != 0.0 {
                // Keep the selection/drag anchor in document coordinates while the viewport moves.
                if let Gesture::BoxSelect { origin, .. } | Gesture::MoveClips { origin, .. } = &mut app.timeline_view.gesture {
                    origin.y -= delta;
                }
                for lane in &mut lanes {
                    lane.y -= delta;
                }
                for (_, y, _) in &mut app.timeline_view.last_lanes {
                    *y -= delta;
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        let mut next: Option<Gesture> = None;
        match app.timeline_view.gesture.clone() {
            Gesture::Pending { clip, origin, edge, additive, base_revision } => {
                if dragging && (p - origin).length() > DRAG_THRESHOLD {
                    if let Some(edge) = edge {
                        app.selection.clips = vec![clip.clone()];
                        app.selection.items.clear();
                        next = Some(Gesture::TrimClip { clip, edge, new_time: app.timeline_view.x_to_t(p.x, x0), origin, base_revision });
                    } else {
                        if !app.selection.clips.contains(&clip) {
                            if !additive {
                                app.selection.clips.clear();
                                app.selection.items.clear();
                            }
                            app.selection.clips.push(clip.clone());
                        }
                        let clips = app.linked_selection();
                        next = Some(Gesture::MoveClips { clips, origin, delta_t: Ticks::ZERO, track_delta: 0, valid: true, base_revision });
                    }
                } else if primary_released {
                    // clic simple: seleccionar
                    if additive {
                        if let Some(i) = app.selection.clips.iter().position(|c| c == &clip) {
                            app.selection.clips.remove(i);
                        } else {
                            app.selection.clips.push(clip);
                        }
                    } else {
                        app.selection.clips = vec![clip];
                        app.selection.items.clear();
                    }
                    next = Some(Gesture::None);
                }
            }
            Gesture::PendingItem { layer, item, origin, additive, range_index, edge, base_revision } => {
                if primary_released || (dragging && (p - origin).length() > DRAG_THRESHOLD) {
                    let moving = !primary_released;
                    if moving {
                        if !app.selection.items.contains(&(layer.clone(), item.clone())) {
                            if !additive {
                                app.selection.items.clear();
                            }
                            app.selection.items.push((layer.clone(), item.clone()));
                        }
                        app.selection.clips.clear();
                    } else if additive {
                        if let Some(i) = app.selection.items.iter().position(|(l, x)| l == &layer && x == &item) {
                            app.selection.items.remove(i);
                        } else {
                            app.selection.items.push((layer.clone(), item.clone()));
                        }
                    } else {
                        app.selection.items = vec![(layer.clone(), item.clone())];
                        app.selection.clips.clear();
                    }
                    app.selection.layer = Some(layer.clone());
                    next = Some(if moving { Gesture::EditItems { origin, layer, item, range_index, edge, base_revision } } else { Gesture::None });
                }
            }
            Gesture::EditItems { origin, layer, item, range_index, edge, base_revision } => {
                let raw = app.timeline_view.x_to_t(p.x, x0);
                let target = app.timeline_view.snap(raw, &edges_for_snap, app.ui.snapping);
                let origin_time = app.timeline_view.x_to_t(origin.x, x0);
                let command = if let Some(edge) = edge {
                    app.project()
                        .layer(&layer)
                        .and_then(|l| gesture_source_pair(app, &l.asset_id, l.item(&item)?.ranges.get(range_index).copied()?, origin_time, target))
                        .map(|(_, to)| to)
                        .map(|new_time| Command::TrimItem { layer_id: layer.clone(), item_id: item.clone(), range_index, edge, new_time })
                } else {
                    app.project().layer(&layer).and_then(|l| {
                        let (from, to) =
                            gesture_source_pair(app, &l.asset_id, l.item(&item)?.ranges.get(range_index).copied()?, origin_time, target)?;
                        Some(app.shift_items_command((to - from).round_to_frame(app.frame_rate())))
                    })
                };
                // The overlay is transient; only release validates and commits one command.
                let x = app.timeline_view.t_to_x(target, x0);
                ui.painter().line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(2.0, colors::ACCENT));
                ui.painter().text(Pos2::new(x + 5.0, p.y - 18.0), Align2::LEFT_BOTTOM, target.clock(), FontId::monospace(11.0), colors::TEXT);
                let stale = app.session.revision() != base_revision;
                if !stale && let Some(command) = command.clone() {
                    app.preview_gesture(command, primary_released, ui.ctx());
                } else if command.is_none() {
                    app.gesture_preview = Default::default();
                }
                if primary_released {
                    if stale {
                        app.toast(Severity::Warn, "El proyecto cambió durante el gesto; repite el arrastre");
                    } else if command.is_none() {
                        app.toast(Severity::Warn, "El gesto cruza otra ocurrencia o un mapping ambiguo; edita este tramo en Fuente");
                    }
                    next = Some(Gesture::None);
                }
            }
            Gesture::MoveClips { clips, origin, base_revision, .. } => {
                let raw_dt = Ticks::from_seconds_f64(((p.x - origin.x) / app.timeline_view.px_per_s) as f64);
                let moving_edges: std::collections::BTreeSet<_> =
                    clips.iter().filter_map(|id| app.sequence()?.clip(id)).flat_map(|clip| [clip.position, clip.end()]).collect();
                let targets: Vec<_> = edges_for_snap.iter().filter(|edge| !moving_edges.contains(edge)).copied().collect();
                let mut dt = raw_dt.round_to_frame(app.frame_rate());
                let mut nearest = Ticks::MAX;
                for edge in moving_edges {
                    let moved = edge + raw_dt;
                    let snapped = app.timeline_view.snap(moved, &targets, app.ui.snapping);
                    let distance = (snapped - moved).abs();
                    if snapped != moved && distance < nearest {
                        nearest = distance;
                        dt = snapped - edge;
                    }
                }
                let track_delta = lane_delta(&lanes, origin.y, p.y, app);
                let stale = app.session.revision() != base_revision;
                if !stale && (dt != Ticks::ZERO || track_delta != 0) {
                    app.preview_gesture(
                        Command::ShiftClips { clip_ids: clips.clone(), delta: dt, track_delta, policy: MovePolicy::Reject },
                        primary_released,
                        ui.ctx(),
                    );
                } else if !stale {
                    app.gesture_preview = Default::default();
                }
                let valid = !stale && app.gesture_preview.error.is_none();
                if primary_released {
                    if stale {
                        app.toast(Severity::Warn, "El proyecto cambió durante el gesto; repite el arrastre");
                    }
                    next = Some(Gesture::None);
                } else {
                    next = Some(Gesture::MoveClips { clips, origin, delta_t: dt, track_delta, valid, base_revision });
                }
            }
            Gesture::TrimClip { clip, edge, origin, base_revision, .. } => {
                let raw = app.timeline_view.x_to_t(p.x, x0);
                let t = app.timeline_view.snap(raw, &edges_for_snap, app.ui.snapping).floor_to_frame(app.frame_rate()).max(Ticks::ZERO);
                let stale = app.session.revision() != base_revision;
                if !stale {
                    let command = Command::Batch {
                        label: "Recortar clips enlazados".into(),
                        commands: app.linked_selection().into_iter().map(|clip_id| Command::TrimClip { clip_id, edge, new_time: t }).collect(),
                    };
                    app.preview_gesture(command, primary_released, ui.ctx());
                }
                if primary_released {
                    if stale {
                        app.toast(Severity::Warn, "El proyecto cambió durante el gesto; repite el recorte");
                    }
                    next = Some(Gesture::None);
                } else {
                    next = Some(Gesture::TrimClip { clip, edge, new_time: t, origin, base_revision });
                }
            }
            Gesture::Scrub => {
                if dragging {
                    let raw = app.timeline_view.x_to_t(p.x, x0).max(Ticks::ZERO);
                    let window = TimeRange::new(app.timeline_view.scroll_t, app.timeline_view.scroll_t + app.timeline_view.visible_span());
                    let markers = if app.view == ViewMode::Sequence { app.timeline_view.index.marker_edges(window) } else { &[] };
                    let t = app.timeline_view.snap(raw, markers, app.ui.snapping);
                    app.scrub(t);
                }
                if primary_released || !dragging {
                    let t = app.playhead;
                    app.seek(t);
                    next = Some(Gesture::None);
                }
            }
            Gesture::Pan { last } => {
                let dx = p.x - last.x;
                app.timeline_view.scroll_t =
                    (app.timeline_view.scroll_t - Ticks::from_seconds_f64((dx / app.timeline_view.px_per_s) as f64)).max(Ticks::ZERO);
                app.timeline_view.scroll_y = (app.timeline_view.scroll_y - (p.y - last.y)).max(0.0);
                next = Some(if primary_released || !primary_down { Gesture::None } else { Gesture::Pan { last: p } });
            }
            Gesture::LanePress { origin, additive } => {
                if primary_released {
                    next = Some(Gesture::None);
                } else if dragging && (p - origin).length() > DRAG_THRESHOLD {
                    next = Some(Gesture::BoxSelect { origin, current: p, additive });
                }
            }
            Gesture::BoxSelect { origin, additive, .. } => {
                if primary_released {
                    let r = Rect::from_two_pos(origin, p);
                    let t0 = app.timeline_view.x_to_t(r.left(), x0);
                    let t1 = app.timeline_view.x_to_t(r.right(), x0);
                    let sel_tracks: Vec<TrackId> = lanes
                        .iter()
                        .filter(|l| l.y < r.bottom() && l.y + l.h > r.top())
                        .filter_map(|l| if let LaneKind::Track(t) = &l.kind { Some(t.clone()) } else { None })
                        .collect();
                    let mut index = std::mem::take(&mut app.timeline_view.index);
                    let clip_indices = index.query(&sel_tracks, TimeRange::new(t0, t1));
                    app.timeline_view.index = index;
                    if app.view == ViewMode::Sequence
                        && let Some(seq) = app.sequence()
                    {
                        let ids: Vec<ClipId> = clip_indices.into_iter().filter_map(|i| seq.clips.get(i).map(|c| c.id.clone())).collect();
                        if additive {
                            for id in ids {
                                if !app.selection.clips.contains(&id) {
                                    app.selection.clips.push(id);
                                }
                            }
                        } else {
                            app.selection.clips = ids;
                        }
                    }
                    let mut items = Vec::new();
                    for lane in lanes.iter().filter(|lane| lane.y < r.bottom() && lane.y + lane.h > r.top()) {
                        if let LaneKind::Layer(layer_id) = &lane.kind
                            && let Some(layer) = app.project().layer(layer_id)
                        {
                            for occurrence in app.timeline_view.semantic_index.query(layer_id, TimeRange::new(t0, t1)) {
                                if let Some(item) = layer.items.get(occurrence.item) {
                                    items.push((layer_id.clone(), item.item_id.clone()));
                                }
                            }
                        }
                    }
                    items.sort();
                    items.dedup();
                    if additive {
                        for item in items {
                            if !app.selection.items.contains(&item) {
                                app.selection.items.push(item);
                            }
                        }
                    } else {
                        app.selection.items = items;
                    }
                    next = Some(Gesture::None);
                } else {
                    next = Some(Gesture::BoxSelect { origin, current: p, additive });
                }
            }
            Gesture::BoxEdit { origin, layer, subtract, base_revision } => {
                let a = app.timeline_view.x_to_t(origin.x, x0).max(Ticks::ZERO);
                let b = app.timeline_view.snap(app.timeline_view.x_to_t(p.x, x0).max(Ticks::ZERO), &edges_for_snap, app.ui.snapping);
                let color = if subtract { Color32::LIGHT_RED } else { colors::ACCENT };
                let preview = Rect::from_min_max(Pos2::new(origin.x.min(p.x), origin.y - 12.0), Pos2::new(origin.x.max(p.x), origin.y + 12.0));
                ui.painter().rect_filled(preview, 0.0, color.gamma_multiply(0.3));
                ui.painter().rect_stroke(preview, 0.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
                let stale = app.session.revision() != base_revision;
                if !stale && (p.x - origin.x).abs() >= DRAG_THRESHOLD {
                    let command = app.project().layer(&layer).and_then(|l| {
                        let (start, end) = match app.view {
                            ViewMode::Source => (app.view_time_to_source(a.min(b), &l.asset_id)?, app.view_time_to_source(a.max(b), &l.asset_id)?),
                            ViewMode::Sequence => app.sequence()?.clips.iter().filter(|c| c.enabled && c.asset_id == l.asset_id).find_map(|c| {
                                Some((c.seq_edge_to_source(a.min(b), ClipEdge::Start)?, c.seq_edge_to_source(a.max(b), ClipEdge::End)?))
                            })?,
                        };
                        Some(Command::BoxEdit { layer_id: layer.clone(), range: TimeRange::new(start, end), subtract })
                    });
                    if let Some(command) = command {
                        app.preview_gesture(command, primary_released, ui.ctx());
                    } else if primary_released {
                        app.toast(Severity::Warn, "La caja cruza un salto de fuente; dibújala en Fuente");
                    }
                }
                if primary_released {
                    if stale {
                        app.toast(Severity::Warn, "El proyecto cambió durante la caja; repite el gesto");
                    }
                    next = Some(Gesture::None);
                }
            }
            Gesture::None => {}
        }
        if let Some(n) = next {
            app.timeline_view.gesture = n;
        }
    } else if !matches!(app.timeline_view.gesture, Gesture::None) && !primary_down {
        // pérdida de captura / foco: sin residuo
        app.timeline_view.gesture = Gesture::None;
    }

    // ---- dibujo: lanes ----
    if matches!(app.timeline_view.gesture, Gesture::None) && !app.gesture_preview.commit {
        app.gesture_preview = Default::default();
    }
    let mut semantic_index = std::mem::take(&mut app.timeline_view.semantic_index);
    semantic_index.ensure(app, ui.ctx());
    app.timeline_view.semantic_index = semantic_index;
    let seq_clone = if app.view == ViewMode::Sequence {
        let mut index = std::mem::take(&mut app.timeline_view.index);
        let result = app.sequence().map(|seq| {
            index.ensure_async(&app.project().project_id, app.session.revision(), seq, ui.ctx());
            let tracks: Vec<TrackId> = lanes
                .iter()
                .filter(|l| l.y + l.h >= content_rect.top() && l.y < content_rect.bottom())
                .filter_map(|l| if let LaneKind::Track(id) = &l.kind { Some(id.clone()) } else { None })
                .collect();
            let range = TimeRange::new(app.timeline_view.scroll_t, app.timeline_view.scroll_t + app.timeline_view.visible_span());
            let indices = index.query(&tracks, range);
            let mut clips: Vec<_> = indices
                .into_iter()
                .map(|i| {
                    let clip = &seq.clips[i];
                    crate::timeline_index::paint_clip(app.gesture_preview.clip(&clip.id).unwrap_or(clip))
                })
                .collect();
            if let Gesture::MoveClips { clips: moving, .. } = &app.timeline_view.gesture {
                for id in moving {
                    if !clips.iter().any(|c| &c.id == id)
                        && let Some(c) = seq.clip(id)
                    {
                        clips.push(crate::timeline_index::paint_clip(app.gesture_preview.clip(&c.id).unwrap_or(c)));
                    }
                }
            }
            crate::timeline_index::PaintSequence { tracks: seq.tracks.clone(), clips }
        });
        app.timeline_view.index = index;
        result
    } else {
        None
    };
    let clip_rect_of = |view: &TimelineView, lane: &Lane, position: Ticks, end: Ticks| -> Rect {
        Rect::from_min_max(Pos2::new(view.t_to_x(position, x0), lane.y + 2.0), Pos2::new(view.t_to_x(end, x0), lane.y + lane.h - 2.0))
    };
    let (move_preview, trim_preview): (Option<MovePreview>, Option<TrimPreview>) = match &app.timeline_view.gesture {
        Gesture::MoveClips { clips, delta_t, track_delta, valid, .. } => (Some((clips.clone(), *delta_t, *track_delta, *valid)), None),
        Gesture::TrimClip { clip, edge, new_time, .. } => (None, Some((clip.clone(), *edge, *new_time))),
        _ => (None, None),
    };
    for (idx, lane) in lanes.iter().enumerate() {
        let lane_rect = Rect::from_min_max(Pos2::new(x0, lane.y), Pos2::new(rect.right(), lane.y + lane.h));
        if lane_rect.bottom() < content_rect.top() || lane_rect.top() > rect.bottom() {
            continue;
        }
        let clip_painter = ui.painter_at(lane_rect.intersect(content_rect));
        clip_painter.rect_filled(lane_rect, 0.0, if idx % 2 == 0 { colors::LANE_BG } else { colors::LANE_ALT });
        match &lane.kind {
            LaneKind::Track(track_id) => {
                if let Some(seq) = &seq_clone {
                    if let Some(track) = seq.track(track_id) {
                        for c in seq.clips.iter().filter(|c| &c.track_id == track_id) {
                            let mut pos = c.position;
                            let mut end = c.end();
                            let exact = app.gesture_preview.clip(&c.id).is_some();
                            let mut ghost = exact;
                            let mut valid = true;
                            if let Some((ids, dt, td, v)) = &move_preview
                                && ids.contains(&c.id)
                                && !exact
                            {
                                pos += *dt;
                                end += *dt;
                                ghost = true;
                                valid = *v;
                                if *td != 0 {
                                    // se dibuja en el carril destino si existe
                                    let target = lane_by_track_delta(&lanes, lane, *td, app);
                                    if let Some(tl) = target {
                                        let r = clip_rect_of(&app.timeline_view, tl, pos, end);
                                        draw_clip(&ui.painter_at(content_rect), r, c, track.kind, app, true, valid, None);
                                        continue;
                                    }
                                }
                            }
                            if let Some((id, edge, nt)) = &trim_preview
                                && id == &c.id
                                && !exact
                            {
                                match edge {
                                    ClipEdge::Start => pos = *nt,
                                    ClipEdge::End => end = *nt,
                                }
                                ghost = true;
                            }
                            let r = clip_rect_of(&app.timeline_view, lane, pos, end);
                            if r.right() < x0 || r.left() > rect.right() {
                                continue;
                            }
                            let items_overlay = if ghost { Vec::new() } else { semantic_overlay_for_clip(app, c) };
                            draw_clip(&clip_painter, r, c, track.kind, app, ghost, valid, Some(&items_overlay));
                        }
                    }
                } else if app.view == ViewMode::Source
                    && let Some(a) = app.source_asset.as_ref().and_then(|a| app.session.project().asset(a))
                {
                    let r = clip_rect_of(&app.timeline_view, lane, Ticks::ZERO, a.duration());
                    clip_painter.rect_filled(r, 4.0, colors::VIDEO_CLIP.gamma_multiply(0.7));
                    let path = app.resolve_asset_path(a);
                    if let Some(media) = &mut app.media_view {
                        let kind = if lane.source_stream.is_some() { TrackKind::Audio } else { TrackKind::Video };
                        media.draw(
                            &clip_painter,
                            r,
                            a,
                            &path,
                            TimeRange::new(Ticks::ZERO, a.duration()),
                            kind,
                            lane.source_stream.unwrap_or(0),
                            app.timeline_view.px_per_s,
                        );
                    }
                    clip_painter.rect_stroke(r, 4.0, Stroke::new(1.0, colors::VIDEO_CLIP), egui::StrokeKind::Inside);
                    clip_painter.text(
                        Pos2::new(r.left().max(x0) + 6.0, r.top() + 2.0),
                        Align2::LEFT_TOP,
                        format!("{} · fuente completa", a.name),
                        FontId::proportional(12.0),
                        colors::TEXT,
                    );
                }
            }
            LaneKind::Layer(layer_id) => {
                if let Some(layer) = app.project().layer(layer_id) {
                    let color = parse_color(&layer.color);
                    let margin = Ticks::from_seconds_f64((8.0 / app.timeline_view.px_per_s) as f64);
                    let visible =
                        TimeRange::new(app.timeline_view.scroll_t - margin, app.timeline_view.scroll_t + app.timeline_view.visible_span() + margin);
                    for occurrence in app.timeline_view.semantic_index.query(layer_id, visible) {
                        let Some(it) = layer.items.get(occurrence.item) else { continue };
                        let selected = app.selection.items.iter().any(|(l, i)| l == layer_id && i == &it.item_id);
                        let (a, b) = (occurrence.range.start, occurrence.range.end);
                        let x1 = app.timeline_view.t_to_x(a, x0);
                        let x2 = app.timeline_view.t_to_x(b, x0).max(x1 + 6.0);
                        if x2 < x0 || x1 > rect.right() {
                            continue;
                        }
                        let rr = Rect::from_min_max(Pos2::new(x1, lane.y + 4.0), Pos2::new(x2, lane.y + lane.h - 4.0));
                        draw_item(&clip_painter, rr, it, color, selected, app.timeline_view.px_per_s);
                    }
                }
            }
        }
        // cabecera
        for geometry in app.gesture_preview.geometry() {
            if !matches!(&lane.kind,LaneKind::Layer(id) if id==&geometry.layer) {
                continue;
            }
            let Some(layer) = app.project().layer(&geometry.layer) else { continue };
            for range in &geometry.ranges {
                for (a, b) in item_range_in_view(app, range, &layer.asset_id) {
                    let ghost = Rect::from_min_max(
                        Pos2::new(app.timeline_view.t_to_x(a, x0), lane.y + 3.0),
                        Pos2::new(app.timeline_view.t_to_x(b, x0).max(app.timeline_view.t_to_x(a, x0) + 6.0), lane.y + lane.h - 3.0),
                    );
                    let color = if geometry.removed { Color32::LIGHT_RED } else { colors::ACCENT };
                    clip_painter.rect_filled(ghost, 2.0, color.gamma_multiply(0.22));
                    clip_painter.rect_stroke(ghost, 2.0, Stroke::new(2.0, color), egui::StrokeKind::Inside);
                }
            }
        }
        let header_rect = Rect::from_min_max(Pos2::new(rect.left(), lane.y), Pos2::new(x0, lane.y + lane.h));
        draw_header(
            app,
            ui,
            header_rect.intersect(Rect::from_min_max(Pos2::new(rect.left(), content_rect.top()), Pos2::new(x0, rect.bottom()))),
            lane,
            &layers,
        );
    }
    // separador cabecera/contenido
    painter.line_segment([Pos2::new(x0, rect.top()), Pos2::new(x0, rect.bottom())], Stroke::new(1.0, Color32::from_gray(60)));

    // ---- regla ----
    draw_ruler(app, &painter, ruler_rect, x0);
    // IN/OUT y loop
    let overlay_painter = ui.painter_at(content_rect.union(ruler_rect));
    if let (Some(i), Some(o)) = (app.in_point, app.out_point)
        && i < o
    {
        let r = Rect::from_min_max(Pos2::new(app.timeline_view.t_to_x(i, x0), rect.top()), Pos2::new(app.timeline_view.t_to_x(o, x0), rect.bottom()));
        overlay_painter.rect_filled(r, 0.0, colors::INOUT.gamma_multiply(0.10));
    }
    for (t, label) in [(app.in_point, "IN"), (app.out_point, "OUT")] {
        if let Some(t) = t {
            let x = app.timeline_view.t_to_x(t, x0);
            overlay_painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, colors::INOUT));
            overlay_painter.text(
                Pos2::new(x + 2.0, rect.top() + RULER_H - 2.0),
                Align2::LEFT_BOTTOM,
                label,
                FontId::proportional(10.0),
                colors::INOUT,
            );
        }
    }
    if let Some(l) = app.loop_range {
        let r = Rect::from_min_max(
            Pos2::new(app.timeline_view.t_to_x(l.start, x0), rect.top()),
            Pos2::new(app.timeline_view.t_to_x(l.end, x0), rect.top() + 4.0),
        );
        overlay_painter.rect_filled(r, 0.0, colors::LOOP);
    }
    // caja de selección
    if let Gesture::BoxSelect { origin, current, .. } = &app.timeline_view.gesture {
        let r = Rect::from_two_pos(*origin, *current);
        overlay_painter.rect_filled(r, 0.0, colors::ACCENT.gamma_multiply(0.15));
        overlay_painter.rect_stroke(r, 0.0, Stroke::new(1.0, colors::ACCENT), egui::StrokeKind::Inside);
    }
    // playhead
    let px = app.timeline_view.t_to_x(app.playhead, x0);
    if px >= x0 - 1.0 && px <= rect.right() + 1.0 {
        overlay_painter.line_segment([Pos2::new(px, rect.top()), Pos2::new(px, rect.bottom())], Stroke::new(1.5, colors::PLAYHEAD));
        overlay_painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(px - 6.0, rect.top()), Pos2::new(px + 6.0, rect.top()), Pos2::new(px, rect.top() + 8.0)],
            colors::PLAYHEAD,
            Stroke::NONE,
        ));
    }
    // hover time
    if let Some(t) = app.timeline_view.hover_time
        && matches!(app.timeline_view.gesture, Gesture::None)
    {
        let x = app.timeline_view.t_to_x(t, x0);
        overlay_painter
            .line_segment([Pos2::new(x, content_rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, Color32::from_white_alpha(40)));
    }
    // cursor según herramienta / hit
    if hover_in_content && let Some(p) = pointer {
        let icon = match (app.tool, hit_test(app, &lanes, p, x0)) {
            (Tool::Cut, Hit::Clip { .. }) => egui::CursorIcon::Crosshair,
            (_, Hit::Clip { edge: Some(_), .. }) => egui::CursorIcon::ResizeHorizontal,
            (_, Hit::Clip { .. }) => egui::CursorIcon::Grab,
            _ => egui::CursorIcon::Default,
        };
        ui.ctx().set_cursor_icon(icon);
    }
    // menú contextual
    response.context_menu(|ui| context_menu(app, ui));
    // estado vacío
    if lanes.is_empty() {
        painter.text(
            content_rect.center(),
            Align2::CENTER_CENTER,
            "Importa un medio (Ctrl+I o arrástralo aquí) y pulsa «Insertar» en la biblioteca",
            FontId::proportional(14.0),
            colors::TEXT_DIM,
        );
    }
    if response.double_clicked()
        && app.tool == Tool::Select
        && app.view == ViewMode::Sequence
        && let Some(pointer) = pointer.filter(|p| content_rect.contains(*p))
        && let Hit::Clip { clip, .. } = hit_test(app, &lanes, pointer, x0)
    {
        let time = app.timeline_view.x_to_t(pointer.x, x0);
        if let Some(current) = app.sequence().and_then(|sequence| sequence.clip(&clip)) {
            app.playhead = time.clamp(current.position, current.end() - Ticks(1));
        }
        app.selection.clips = vec![clip];
        app.selection.items.clear();
        app.timeline_view.gesture = Gesture::None;
        app.gesture_preview = Default::default();
        app.dispatch("montage.reveal_source");
        ui.ctx().request_repaint();
    }
}

enum Hit {
    Clip { clip: ClipId, edge: Option<ClipEdge> },
    Item { layer: LayerId, item: ItemId, range_index: usize, edge: Option<ClipEdge> },
    Lane(LaneKind),
    Header(LaneKind),
    None,
}

fn hit_test(app: &TranscriptorApp, lanes: &[Lane], p: Pos2, x0: f32) -> Hit {
    let Some(lane) = lanes.iter().find(|l| p.y >= l.y && p.y < l.y + l.h) else { return Hit::None };
    if p.x < x0 {
        return Hit::Header(lane.kind.clone());
    }
    let t = app.timeline_view.x_to_t(p.x, x0);
    match &lane.kind {
        LaneKind::Track(track_id) => {
            if let Some(seq) = app.sequence() {
                let near = TimeRange::new(t - Ticks(1), t + Ticks(1));
                for i in app.timeline_view.index.query_track(track_id, near) {
                    let Some(c) = seq.clips.get(i) else { continue };
                    let x1 = app.timeline_view.t_to_x(c.position, x0);
                    let x2 = app.timeline_view.t_to_x(c.end(), x0);
                    if p.x >= x1 && p.x <= x2 {
                        let edge = if (p.x - x1) <= HANDLE_W && (x2 - x1) > HANDLE_W * 3.0 {
                            Some(ClipEdge::Start)
                        } else if (x2 - p.x) <= HANDLE_W && (x2 - x1) > HANDLE_W * 3.0 {
                            Some(ClipEdge::End)
                        } else {
                            None
                        };
                        return Hit::Clip { clip: c.id.clone(), edge };
                    }
                }
            }
            Hit::Lane(lane.kind.clone())
        }
        LaneKind::Layer(layer_id) => {
            if let Some(layer) = app.project().layer(layer_id) {
                let margin = Ticks::from_seconds_f64((8.0 / app.timeline_view.px_per_s) as f64);
                for occurrence in app.timeline_view.semantic_index.query(layer_id, TimeRange::new(t - margin, t + margin)).into_iter().rev() {
                    let Some(it) = layer.items.get(occurrence.item) else { continue };
                    let range_index = occurrence.range_index;
                    let Some(r) = it.ranges.get(range_index) else { continue };
                    let (a, b) = (occurrence.range.start, occurrence.range.end);
                    let x1 = app.timeline_view.t_to_x(a, x0);
                    let x2 = app.timeline_view.t_to_x(b, x0).max(x1 + 6.0);
                    if p.x >= x1 && p.x <= x2 {
                        let handle = HANDLE_W.min((x2 - x1) / 3.0);
                        let edge = if !r.is_point() && p.x - x1 <= handle {
                            Some(ClipEdge::Start)
                        } else if !r.is_point() && x2 - p.x <= handle {
                            Some(ClipEdge::End)
                        } else {
                            None
                        };
                        return Hit::Item { layer: layer_id.clone(), item: it.item_id.clone(), range_index, edge };
                    }
                }
            }
            let _ = t;
            Hit::Lane(lane.kind.clone())
        }
    }
}

/// Rango fuente → rangos de vista (uno por clip que lo contenga en Secuencia).
pub(crate) fn item_range_in_view(app: &TranscriptorApp, r: &TimeRange, asset: &tv2_domain::ids::AssetId) -> Vec<(Ticks, Ticks)> {
    match app.view {
        ViewMode::Source => {
            if app.source_asset.as_ref() == Some(asset) {
                vec![(r.start, r.end)]
            } else {
                vec![]
            }
        }
        ViewMode::Sequence => app.timeline_view.semantic_index.project_range(asset, *r),
    }
}

fn semantic_overlay_for_clip(app: &TranscriptorApp, c: &tv2_domain::timeline::Clip) -> Vec<(Ticks, Ticks, Color32)> {
    let mut out = Vec::new();
    let view = TimeRange::new(app.timeline_view.scroll_t, app.timeline_view.scroll_t + app.timeline_view.visible_span());
    let Some(visible) = c.range().intersection(&view) else { return out };
    let source = TimeRange::new(c.source.start + (visible.start - c.position), c.source.start + (visible.end - c.position));
    for (layer_id, color) in app.timeline_view.semantic_index.visible_layers(&c.asset_id) {
        let Some(layer) = app.project().layer(layer_id) else { continue };
        let color = parse_color(color);
        for occurrence in app.timeline_view.semantic_index.query_source(layer_id, source) {
            let Some(it) = layer.items.get(occurrence.item).filter(|item| item.state != ItemState::Disabled) else { continue };
            if let Some(r) = it.ranges.get(occurrence.range_index)
                && let Some(i) = c.source.intersection(r)
            {
                out.push((c.position + (i.start - c.source.start), c.position + (i.end - c.source.start), color));
            }
        }
    }
    out
}

fn gesture_source_pair(
    app: &TranscriptorApp,
    asset: &tv2_domain::AssetId,
    source_range: TimeRange,
    origin: Ticks,
    target: Ticks,
) -> Option<(Ticks, Ticks)> {
    if app.view == ViewMode::Source {
        return (app.source_asset.as_ref() == Some(asset)).then_some((origin, target));
    }
    let tolerance = Ticks::from_seconds_f64((HANDLE_W / app.timeline_view.px_per_s) as f64);
    app.timeline_view.semantic_index.gesture_source_pair(asset, source_range, origin, target, tolerance)
}

fn lane_delta(lanes: &[Lane], y_from: f32, y_to: f32, app: &TranscriptorApp) -> i32 {
    let idx = |y: f32| lanes.iter().position(|l| y >= l.y && y < l.y + l.h);
    let (Some(a), Some(b)) = (idx(y_from), idx(y_to)) else { return 0 };
    // solo entre pistas del mismo tipo; el orden visual es inverso al índice de composición para video
    let kind_of = |i: usize| -> Option<TrackKind> {
        if let LaneKind::Track(t) = &lanes[i].kind { app.sequence().and_then(|s| s.track(t)).map(|t| t.kind) } else { None }
    };
    match (kind_of(a), kind_of(b)) {
        (Some(ka), Some(kb)) if ka == kb => {
            let visual = b as i32 - a as i32;
            match ka {
                TrackKind::Video => -visual, // arriba en pantalla = índice mayor
                TrackKind::Audio => visual,
            }
        }
        _ => 0,
    }
}

fn lane_by_track_delta<'a>(lanes: &'a [Lane], from: &Lane, delta: i32, app: &TranscriptorApp) -> Option<&'a Lane> {
    let LaneKind::Track(track) = &from.kind else { return None };
    let sequence = app.sequence()?;
    let kind = sequence.track(track)?.kind;
    let compatible: Vec<_> = sequence.tracks.iter().filter(|track| track.kind == kind).collect();
    let index = compatible.iter().position(|candidate| &candidate.id == track)? as i32;
    let target = usize::try_from(index.checked_add(delta)?).ok().and_then(|index| compatible.get(index))?;
    lanes.iter().find(|lane| matches!(&lane.kind,LaneKind::Track(id) if id==&target.id))
}

#[allow(clippy::too_many_arguments)]
fn draw_clip(
    painter: &egui::Painter,
    r: Rect,
    c: &tv2_domain::timeline::Clip,
    kind: TrackKind,
    app: &mut TranscriptorApp,
    ghost: bool,
    valid: bool,
    overlays: Option<&Vec<(Ticks, Ticks, Color32)>>,
) {
    let is_image = app.project().asset(&c.asset_id).is_some_and(|a| a.kind == tv2_domain::asset::AssetKind::Image);
    let base = if !c.enabled {
        colors::DISABLED
    } else if is_image {
        colors::IMAGE_CLIP
    } else {
        match kind {
            TrackKind::Video => colors::VIDEO_CLIP,
            TrackKind::Audio => colors::AUDIO_CLIP,
        }
    };
    let selected = app.selection.clips.contains(&c.id);
    let fill = if ghost { base.gamma_multiply(0.5) } else { base };
    let fill = if !valid { colors::ERR.gamma_multiply(0.6) } else { fill };
    painter.rect_filled(r, 4.0, fill);
    if !ghost && let Some(asset) = app.session.project().asset(&c.asset_id) {
        let path = app.resolve_asset_path(asset);
        if let Some(media) = &mut app.media_view {
            media.draw(painter, r, asset, &path, c.source, kind, c.audio_stream.unwrap_or(0), app.timeline_view.px_per_s);
        }
    }
    if !c.enabled {
        // patrón rayado para desactivado (no solo color)
        let visible = r.intersect(painter.clip_rect());
        let mut x = visible.left() - r.height();
        while x < visible.right() {
            painter.line_segment(
                [Pos2::new(x, r.bottom()), Pos2::new((x + r.height()).min(r.right()), r.top())],
                Stroke::new(1.0, Color32::from_white_alpha(30)),
            );
            x += 10.0;
        }
    }
    if let Some(ov) = overlays {
        for (a, b, color) in ov {
            let x1 = app
                .timeline_view
                .t_to_x(*a, r.left() - (c.position - app.timeline_view.scroll_t).as_seconds_f64() as f32 * app.timeline_view.px_per_s);
            let x2 = app
                .timeline_view
                .t_to_x(*b, r.left() - (c.position - app.timeline_view.scroll_t).as_seconds_f64() as f32 * app.timeline_view.px_per_s);
            let rr = Rect::from_min_max(Pos2::new(x1.max(r.left()), r.bottom() - 5.0), Pos2::new(x2.min(r.right()), r.bottom() - 2.0));
            painter.rect_filled(rr, 0.0, *color);
        }
    }
    let stroke = if selected { Stroke::new(2.0, colors::ACCENT) } else { Stroke::new(1.0, Color32::from_black_alpha(120)) };
    painter.rect_stroke(r, 4.0, stroke, egui::StrokeKind::Inside);
    // asas
    if r.width() > HANDLE_W * 3.0 {
        painter.rect_filled(Rect::from_min_max(r.min, Pos2::new(r.left() + 3.0, r.bottom())), 0.0, Color32::from_white_alpha(50));
        painter.rect_filled(Rect::from_min_max(Pos2::new(r.right() - 3.0, r.top()), r.max), 0.0, Color32::from_white_alpha(50));
    }
    let label = if c.name.is_empty() { c.id.to_string() } else { c.name.clone() };
    let label = format!("{label}  {}", c.source.start.clock());
    let text_rect = r.shrink2(Vec2::new(6.0, 0.0));
    if text_rect.width() > 20.0 {
        painter.with_clip_rect(text_rect).text(
            Pos2::new(text_rect.left().max(painter.clip_rect().left() + 4.0), r.top() + 2.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(12.0),
            colors::TEXT,
        );
    }
}

fn draw_item(painter: &egui::Painter, r: Rect, it: &tv2_domain::layers::SemanticItem, color: Color32, selected: bool, _px_per_s: f32) {
    let fill = match it.state {
        ItemState::Accepted => color,
        ItemState::Proposed => color.gamma_multiply(0.6),
        ItemState::Disabled => colors::DISABLED,
    };
    painter.rect_filled(r, 3.0, fill);
    match it.state {
        ItemState::Proposed => {
            // trama diagonal = propuesto
            let visible = r.intersect(painter.clip_rect());
            let mut x = visible.left() - r.height();
            while x < visible.right() {
                painter.line_segment(
                    [Pos2::new(x, r.bottom()), Pos2::new((x + r.height()).min(r.right()), r.top())],
                    Stroke::new(1.0, Color32::from_black_alpha(60)),
                );
                x += 8.0;
            }
        }
        ItemState::Accepted => {
            // check
            let cx = r.left() + 4.0;
            let cy = r.center().y;
            painter.line_segment([Pos2::new(cx, cy), Pos2::new(cx + 3.0, cy + 3.0)], Stroke::new(2.0, Color32::WHITE));
            painter.line_segment([Pos2::new(cx + 3.0, cy + 3.0), Pos2::new(cx + 9.0, cy - 4.0)], Stroke::new(2.0, Color32::WHITE));
        }
        ItemState::Disabled => {}
    }
    if it.is_point() {
        painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(r.left() - 5.0, r.top()), Pos2::new(r.left() + 5.0, r.top()), Pos2::new(r.left(), r.bottom())],
            fill,
            Stroke::NONE,
        ));
    }
    let stroke = if selected { Stroke::new(2.0, colors::ACCENT) } else { Stroke::new(1.0, Color32::from_black_alpha(100)) };
    painter.rect_stroke(r, 3.0, stroke, egui::StrokeKind::Inside);
    let text_x = r.left() + if it.state == ItemState::Accepted { 16.0 } else { 5.0 };
    let text_rect = Rect::from_min_max(Pos2::new(text_x, r.top()), Pos2::new(r.right() - 3.0, r.bottom()));
    if text_rect.width() > 14.0 {
        let label = if it.label.is_empty() { it.item_id.to_string() } else { it.label.clone() };
        painter.with_clip_rect(text_rect).text(
            Pos2::new(text_x, r.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            if it.state == ItemState::Disabled { colors::TEXT_DIM } else { colors::TEXT },
        );
    }
}

fn draw_header(app: &mut TranscriptorApp, ui: &mut egui::Ui, rect: Rect, lane: &Lane, layers: &[(LayerId, String, String, bool)]) {
    if rect.height() <= 0.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, colors::HEADER_BG);
    painter.line_segment([Pos2::new(rect.left(), rect.bottom()), Pos2::new(rect.right(), rect.bottom())], Stroke::new(1.0, Color32::from_gray(50)));
    match &lane.kind {
        LaneKind::Track(track_id) => {
            let Some(track) = app.sequence().and_then(|s| s.track(track_id)).cloned() else {
                if app.view == ViewMode::Source {
                    painter.text(
                        Pos2::new(rect.left() + 8.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        lane.source_stream.map(|s| format!("Audio fuente {}", s + 1)).unwrap_or_else(|| "Video fuente".into()),
                        FontId::proportional(12.0),
                        colors::TEXT,
                    );
                }
                return;
            };
            let mut child = ui.new_child(
                egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.style_mut().spacing.item_spacing = Vec2::new(3.0, 2.0);
            let name_resp =
                child.add(egui::Label::new(egui::RichText::new(&track.name).size(12.0).color(colors::TEXT)).truncate().sense(Sense::click()));
            if name_resp.double_clicked() {
                app.rename_dialog = Some((track.name.clone(), RenameTarget::Track(track.id.clone())));
            }
            child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let small = |label: &str, on: bool, tip: &str| {
                    let mut b = egui::Button::new(egui::RichText::new(label).size(10.0)).min_size(Vec2::new(18.0, 16.0));
                    if on {
                        b = b.fill(colors::ACCENT.gamma_multiply(0.8));
                    }
                    (b, tip.to_string())
                };
                let (b, tip) = small("🔒", track.locked, "Bloquear pista");
                if ui.add(b).on_hover_text(tip).clicked() {
                    app.exec(Command::SetTrackProps {
                        track_id: track.id.clone(),
                        name: None,
                        muted: None,
                        solo: None,
                        locked: Some(!track.locked),
                        visible: None,
                        gain_db: None,
                        height: None,
                    });
                }
                if track.kind == TrackKind::Audio {
                    let (b, tip) = small("S", track.solo, "Solo");
                    if ui.add(b).on_hover_text(tip).clicked() {
                        app.exec(Command::SetTrackProps {
                            track_id: track.id.clone(),
                            name: None,
                            muted: None,
                            solo: Some(!track.solo),
                            locked: None,
                            visible: None,
                            gain_db: None,
                            height: None,
                        });
                    }
                    let (b, tip) = small("M", track.muted, "Silenciar");
                    if ui.add(b).on_hover_text(tip).clicked() {
                        app.exec(Command::SetTrackProps {
                            track_id: track.id.clone(),
                            name: None,
                            muted: Some(!track.muted),
                            solo: None,
                            locked: None,
                            visible: None,
                            gain_db: None,
                            height: None,
                        });
                    }
                } else {
                    let (b, tip) = small("👁", track.visible, "Visible");
                    if ui.add(b).on_hover_text(tip).clicked() {
                        app.exec(Command::SetTrackProps {
                            track_id: track.id.clone(),
                            name: None,
                            muted: None,
                            solo: None,
                            locked: None,
                            visible: Some(!track.visible),
                            gain_db: None,
                            height: None,
                        });
                    }
                }
            });
        }
        LaneKind::Layer(layer_id) => {
            let Some((_, name, color, locked)) = layers.iter().find(|(id, _, _, _)| id == layer_id) else { return };
            let selected = app.selection.layer.as_ref() == Some(layer_id);
            if selected {
                painter.rect_filled(rect, 0.0, colors::ACCENT.gamma_multiply(0.25));
            }
            painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(rect.left() + 4.0, rect.bottom())), 0.0, parse_color(color));
            let resp = ui.interact(rect, ui.id().with(("layer-header", layer_id.as_str())), Sense::click());
            if resp.clicked() {
                app.selection.layer = Some(layer_id.clone());
                app.selection.items.clear();
                app.selection.clips.clear();
            }
            if resp.double_clicked() {
                app.rename_dialog = Some((name.clone(), RenameTarget::Layer(layer_id.clone())));
            }
            let text = format!("{}{}", name, if *locked { " 🔒" } else { "" });
            painter.with_clip_rect(rect).text(
                Pos2::new(rect.left() + 10.0, rect.center().y),
                Align2::LEFT_CENTER,
                text,
                FontId::proportional(12.0),
                colors::TEXT,
            );
            resp.context_menu(|ui| {
                if app.project().layer(layer_id).is_some_and(|l| l.kind == tv2_domain::LayerKind::Blocks)
                    && ui.button("Ajustar bordes seguros (radio 15 s)").clicked()
                {
                    app.prepare_semantic(Command::SnapBlockBoundaries { layer_id: layer_id.clone(), radius: Ticks::from_seconds(15) });
                    ui.close();
                }
                if let Some(layer) = app.project().layer(layer_id).cloned()
                    && layer.kind == tv2_domain::LayerKind::Trims
                {
                    if ui.button("Unir recortes solapados").clicked() {
                        app.exec(Command::CoalesceTrims { layer_id: layer_id.clone(), actor_id: None });
                        ui.close();
                    }
                    let targets: Vec<_> = app
                        .project()
                        .ordered_layers()
                        .into_iter()
                        .filter(|l| l.layer_id != *layer_id && l.asset_id == layer.asset_id && l.kind == tv2_domain::LayerKind::Trims && !l.locked)
                        .map(|l| (l.layer_id.clone(), l.name.clone()))
                        .collect();
                    ui.menu_button("Mover selección a…", |ui| {
                        for (id, name) in &targets {
                            if ui.button(name).clicked() {
                                let item_ids = app.selection.items.iter().filter(|(l, _)| l == layer_id).map(|(_, i)| i.clone()).collect();
                                app.exec(Command::MoveTrimItems { layer_id: layer_id.clone(), target_layer_id: id.clone(), item_ids });
                                ui.close();
                            }
                        }
                    });
                    if !matches!(layer.lane.as_deref(), Some("main" | "ai")) {
                        ui.menu_button("Quitar carril…", |ui| {
                            for (id, name) in &targets {
                                if ui.button(format!("Conservar recortes en {name}")).clicked() {
                                    app.exec(Command::RemoveTrimLane { layer_id: layer_id.clone(), move_to: Some(id.clone()) });
                                    ui.close();
                                }
                            }
                            if ui.button("Borrar carril y sus recortes (con Deshacer)").clicked() {
                                app.exec(Command::RemoveTrimLane { layer_id: layer_id.clone(), move_to: None });
                                ui.close();
                            }
                        });
                    }
                }
                if ui.button("Subir carril").clicked() {
                    app.move_layer(layer_id, -1);
                    ui.close();
                }
                if ui.button("Bajar carril").clicked() {
                    app.move_layer(layer_id, 1);
                    ui.close();
                }
                if ui.button("Seleccionar todos los items").clicked() {
                    app.selection.layer = Some(layer_id.clone());
                    app.selection.clips.clear();
                    app.selection.items.clear();
                    app.dispatch("tools.select_all");
                    ui.close();
                }
                if ui.button(if *locked { "Desbloquear" } else { "Bloquear" }).clicked() {
                    app.exec(Command::SetLayerProps { layer_id: layer_id.clone(), name: None, color: None, visible: None, locked: Some(!locked) });
                    ui.close();
                }
            });
            resp.on_hover_text("Capa semántica · clic para seleccionar, doble clic para renombrar, menú derecho para ordenar");
        }
    }
}

fn draw_ruler(app: &TranscriptorApp, painter: &egui::Painter, ruler: Rect, x0: f32) {
    painter.rect_filled(ruler, 0.0, colors::RULER_BG);
    let px_per_s = app.timeline_view.px_per_s;
    // paso de etiquetas: elegir el mayor de la lista que dé ≥ 80 px
    let steps = [0.04, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0];
    let step = steps.iter().copied().find(|s| s * px_per_s as f64 >= 80.0).unwrap_or(3600.0);
    let minor = step / 5.0;
    let start = app.timeline_view.scroll_t.as_seconds_f64();
    let end = start + (ruler.width() / px_per_s) as f64;
    let mut t = (start / minor).floor() * minor;
    while t <= end {
        let x = x0 + ((t - start) * px_per_s as f64) as f32;
        let is_major = ((t / step).round() * step - t).abs() < minor * 0.1;
        let h = if is_major { 10.0 } else { 4.0 };
        painter.line_segment([Pos2::new(x, ruler.bottom()), Pos2::new(x, ruler.bottom() - h)], Stroke::new(1.0, Color32::from_gray(140)));
        if is_major && t >= 0.0 {
            let ticks = Ticks::from_seconds_f64(t);
            let label = if step < 1.0 { ticks.timecode_frames(app.frame_rate()).to_string() } else { ticks.clock() };
            painter.text(Pos2::new(x + 3.0, ruler.top() + 2.0), Align2::LEFT_TOP, label, FontId::monospace(10.0), colors::TEXT_DIM);
        }
        t += minor;
    }
    if app.view == ViewMode::Sequence
        && let Some(seq) = app.sequence()
    {
        let p = painter.with_clip_rect(ruler);
        let margin = Ticks::from_seconds_f64((5.0 / px_per_s) as f64);
        let window = TimeRange::new(app.timeline_view.scroll_t - margin, app.timeline_view.scroll_t + app.timeline_view.visible_span() + margin);
        for index in app.timeline_view.index.query_markers(window) {
            let Some(marker) = seq.markers.get(index) else { continue };
            let x = app.timeline_view.t_to_x(marker.range.start, x0);
            let color = parse_color(&marker.color);
            if x >= ruler.left() - 5.0 && x <= ruler.right() + 5.0 {
                p.add(egui::Shape::convex_polygon(
                    vec![Pos2::new(x - 5.0, ruler.bottom() - 8.0), Pos2::new(x + 5.0, ruler.bottom() - 8.0), Pos2::new(x, ruler.bottom())],
                    color,
                    Stroke::NONE,
                ));
            }
            if !marker.range.is_point() {
                let end = app.timeline_view.t_to_x(marker.range.end, x0);
                p.line_segment([Pos2::new(x, ruler.bottom() - 8.0), Pos2::new(end, ruler.bottom() - 8.0)], Stroke::new(2.0, color));
            }
        }
    }
}

fn context_menu(app: &mut TranscriptorApp, ui: &mut egui::Ui) {
    let has_clip = !app.selection.clips.is_empty();
    let has_item = !app.selection.items.is_empty();
    let km_label = |app: &TranscriptorApp, id: &str| -> String {
        let p = app.keymap.pretty(id);
        let l = app.keymap.label(id);
        if p.is_empty() { l.to_string() } else { format!("{l}    {p}") }
    };
    let mut actions: Vec<&str> = Vec::new();
    if has_clip {
        actions.extend([
            "sequence.insert_selection",
            "edit.split",
            "edit.trim_start",
            "edit.trim_end",
            "edit.duplicate",
            "edit.copy",
            "edit.cut",
            "edit.delete",
            "edit.delete_ripple",
            "edit.toggle",
            "edit.activate",
            "edit.edit",
            "montage.reveal_source",
            "montage.move_up",
            "montage.move_down",
        ]);
    } else if has_item {
        actions.extend([
            "edit.split",
            "edit.trim_start",
            "edit.trim_end",
            "edit.copy",
            "edit.cut",
            "edit.paste",
            "edit.duplicate",
            "edit.accept",
            "edit.accept_next",
            "edit.toggle",
            "edit.activate",
            "edit.edit",
            "edit.delete",
            "montage.add_selection",
        ]);
    } else {
        actions.extend([
            "edit.paste",
            "marks.in",
            "marks.out",
            "marks.point",
            "layers.new_lane",
            "layers.add_range",
            "montage.add_selection",
            "view.fit",
            "view.zoom_sel",
            "loop.set_in",
            "loop.set_out",
            "loop.clear",
        ]);
    }
    for a in actions {
        if ui.button(km_label(app, a)).clicked() {
            app.dispatch(a);
            ui.close();
        }
    }
    ui.separator();
    if ui.checkbox(&mut app.ui.snapping, "Imán (snapping)").changed() {
        ui.close();
    }
    if ui.checkbox(&mut app.ui.follow_playhead, "Seguir al playhead").changed() {
        ui.close();
    }
    let _ = MovePolicy::Reject;
    let _ = Severity::Info;
    let _ = CornerRadius::ZERO;
}

pub fn parse_color(hex: &str) -> Color32 {
    if hex.len() == 7 && hex.starts_with('#') {
        let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(200);
        let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(150);
        let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(70);
        Color32::from_rgb(r, g, b)
    } else {
        colors::ACCENT
    }
}
