//! Presentación de cachés: consultas por píxeles visibles y texturas acotadas.
use egui::{Color32, Pos2, Rect, Stroke};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tv2_domain::{
    asset::Asset,
    ids::AssetId,
    time::{Ticks, TimeRange},
    timeline::TrackKind,
};
use tv2_media::{FfmpegTools, MediaCaches};

type TextureKey = (AssetId, usize, usize);
pub struct MediaView {
    pub caches: MediaCaches,
    textures: HashMap<TextureKey, (egui::TextureHandle, u64)>,
    visible: HashSet<AssetId>,
    frame: u64,
    pub wave_columns: usize,
    pub thumb_tiles: usize,
}

impl MediaView {
    pub fn new(tools: FfmpegTools, ctx: egui::Context) -> Self {
        Self {
            caches: MediaCaches::new(tools, Some(crate::paths::cache_dir()), Arc::new(move || ctx.request_repaint())),
            textures: HashMap::new(),
            visible: HashSet::new(),
            frame: 0,
            wave_columns: 0,
            thumb_tiles: 0,
        }
    }

    pub fn clear(&mut self) {
        self.caches.clear();
        self.textures.clear();
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.visible.clear();
        self.wave_columns = 0;
        self.thumb_tiles = 0;
    }

    pub fn end_frame(&mut self) {
        // No se mantienen buffers CPU de medios fuera de la ventana visible.
        self.caches.retain_assets(&self.visible);
        self.textures.retain(|_, (_, used)| *used == self.frame);
        self.caches.tick();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        asset: &Asset,
        path: &Path,
        source: TimeRange,
        kind: TrackKind,
        stream: u32,
        px_per_s: f32,
    ) {
        let body = Rect::from_min_max(Pos2::new(rect.left() + 4.0, rect.top() + 17.0), rect.max - egui::vec2(4.0, 5.0));
        let visible = body.intersect(painter.clip_rect());
        if !visible.is_positive() || body.height() < 3.0 {
            return;
        }
        self.visible.insert(asset.id.clone());
        let start = source.start + Ticks::from_seconds_f64(((visible.left() - rect.left()) / px_per_s) as f64);
        let end = (source.start + Ticks::from_seconds_f64(((visible.right() - rect.left()) / px_per_s) as f64)).min(source.end);
        let p = painter.with_clip_rect(visible);
        if kind == TrackKind::Audio {
            let handle = self.caches.waveform(asset, stream, path);
            let data = handle.read();
            if let Some(error) = &data.error {
                p.text(visible.left_center(), egui::Align2::LEFT_CENTER, error, egui::FontId::proportional(10.0), Color32::LIGHT_RED);
                return;
            }
            let columns = visible.width().ceil().clamp(1.0, 8192.0) as usize;
            let peaks = data.peaks(TimeRange::new(start, end), columns);
            for (i, peak) in peaks.into_iter().enumerate() {
                if let Some(peak) = peak {
                    let x = visible.left() + i as f32 * visible.width() / columns as f32;
                    let h = (peak as f32 / 255.0 * body.height() * 0.5).max(0.5);
                    p.line_segment(
                        [Pos2::new(x, body.center().y - h), Pos2::new(x, body.center().y + h)],
                        Stroke::new(1.0, Color32::from_rgb(178, 241, 209)),
                    );
                    self.wave_columns += 1;
                }
            }
        } else {
            let handle = self.caches.thumbnails(asset, path);
            let data = handle.read();
            if let Some(error) = &data.error {
                p.text(visible.left_center(), egui::Align2::LEFT_CENTER, error, egui::FontId::proportional(10.0), Color32::LIGHT_RED);
                return;
            }
            let tile_w = (body.height() * 16.0 / 9.0).max(24.0);
            let interval = Ticks::from_seconds_f64((tile_w / px_per_s) as f64);
            let Some(preferred) = data.best_level(interval) else {
                return;
            };
            let first = ((visible.left() - body.left()) / tile_w).floor() as usize;
            let last = ((visible.right() - body.left()) / tile_w).ceil() as usize;
            for tile in first..last {
                let x = body.left() + tile as f32 * tile_w;
                let time = source.start + Ticks::from_seconds_f64(((x - rect.left()) / px_per_s) as f64);
                // El nivel fino puede no haber alcanzado este tiempo: usar grueso disponible.
                let Some((li, index)) = (0..=preferred).rev().find_map(|li| data.levels[li].index_at(time).map(|i| (li, i))) else {
                    continue;
                };
                let level = &data.levels[li];
                let key = (asset.id.clone(), li, index);
                if !self.textures.contains_key(&key) {
                    // Máximo 256 miniaturas residentes (≤12 MiB RGBA con 96×128).
                    if self.textures.len() >= 256 {
                        if let Some(oldest) = self
                            .textures
                            .iter()
                            .filter(|(_, (_, used))| *used != self.frame)
                            .min_by_key(|(_, (_, used))| *used)
                            .map(|(k, _)| k.clone())
                        {
                            self.textures.remove(&oldest);
                        } else {
                            continue;
                        }
                    }
                    let len = (level.width * level.height * 4) as usize;
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [level.width as usize, level.height as usize],
                        &level.rgba[index * len..(index + 1) * len],
                    );
                    let texture = painter.ctx().load_texture(format!("thumb-{}-{li}-{index}", asset.id), image, egui::TextureOptions::LINEAR);
                    self.textures.insert(key.clone(), (texture, self.frame));
                }
                if let Some((texture, used)) = self.textures.get_mut(&key) {
                    *used = self.frame;
                    p.image(
                        texture.id(),
                        Rect::from_min_size(Pos2::new(x, body.top()), egui::vec2(tile_w - 1.0, body.height())),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    self.thumb_tiles += 1;
                }
            }
        }
    }
}
