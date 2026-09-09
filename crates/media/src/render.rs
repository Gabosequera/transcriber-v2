//! Renderizador de timeline: dado un tiempo de secuencia compone el fotograma
//! con los clips de video activos. Mantiene decodificadores por clip y los
//! reutiliza cuando el destino avanza (lectura secuencial); reabre en saltos.
//! Usado por el visor (con latest-wins) y por la exportación (secuencial estricto).

use crate::compositor::{Canvas, Frame, dest_rect};
use crate::decoder::{VideoDecoder, decode_image};
use crate::ffmpeg::FfmpegTools;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tv2_domain::asset::AssetKind;
use tv2_domain::ids::{AssetId, ClipId};
use tv2_domain::resolve::{ResolvedClip, ResolvedTimeline};
use tv2_domain::time::Ticks;

/// Localización y forma de un asset para decodificar.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AssetSource {
    pub path: PathBuf,
    pub kind: AssetKind,
    /// Tamaño de presentación (tras rotación).
    pub width: u32,
    pub height: u32,
    /// `start_time` del contenedor como metadata. `-ss` usa tiempo relativo
    /// por defecto: no sumar este offset otra vez al solicitar la fuente.
    pub start_time: Ticks,
    pub missing: bool,
}

impl AssetSource {
    pub fn from_asset(asset: &tv2_domain::asset::Asset, path: PathBuf) -> AssetSource {
        let (w, h) = asset.probe.video.as_ref().map(|v| v.display_size()).unwrap_or((0, 0));
        AssetSource { path, kind: asset.kind, width: w, height: h, start_time: asset.probe.start_time, missing: asset.missing }
    }
}

struct ClipDecoder {
    decoder: VideoDecoder,
    last: Option<(Ticks, Frame)>,
    size: (u32, u32),
    keyframes_only: bool,
}

pub struct TimelineRenderer {
    tools: FfmpegTools,
    pub timeline: Arc<ResolvedTimeline>,
    pub assets: Arc<HashMap<AssetId, AssetSource>>,
    pub width: u32,
    pub height: u32,
    decoders: HashMap<ClipId, ClipDecoder>,
    images: HashMap<AssetId, Frame>,
    canvas: Canvas,
    pub keyframes_only: bool,
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
    /// Último tiempo compuesto, para saber si hace falta repintar.
    pub last_rendered: Option<Ticks>,
    pub last_source_times: HashMap<ClipId, Ticks>,
}

impl TimelineRenderer {
    pub fn new(
        tools: FfmpegTools,
        timeline: Arc<ResolvedTimeline>,
        assets: Arc<HashMap<AssetId, AssetSource>>,
        width: u32,
        height: u32,
    ) -> TimelineRenderer {
        let width = width.max(2) & !1;
        let height = height.max(2) & !1;
        TimelineRenderer {
            tools,
            timeline,
            assets,
            width,
            height,
            decoders: HashMap::new(),
            images: HashMap::new(),
            canvas: Canvas::new(width, height),
            keyframes_only: false,
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_rendered: None,
            last_source_times: HashMap::new(),
        }
    }

    pub fn set_timeline(&mut self, timeline: Arc<ResolvedTimeline>, assets: Arc<HashMap<AssetId, AssetSource>>) {
        self.timeline = timeline;
        self.assets = assets;
        self.decoders.clear();
        self.last_rendered = None;
    }

    pub fn set_size(&mut self, width: u32, height: u32) {
        let width = width.max(2) & !1;
        let height = height.max(2) & !1;
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.canvas = Canvas::new(width, height);
            self.decoders.clear();
            self.last_rendered = None;
        }
    }

    pub fn drop_decoders(&mut self) {
        self.decoders.clear();
    }

    /// Tamaño al que se pide la decodificación de un clip: el rectángulo destino,
    /// acotado al lienzo (el compositor blitea 1:1 cuando coincide).
    fn decode_size(&self, src: &AssetSource, rc: &ResolvedClip) -> (u32, u32) {
        let (_, _, w, h) = dest_rect(src.width.max(1), src.height.max(1), self.width, self.height, &rc.transform);
        (w.min(self.width * 2).max(2), h.min(self.height * 2).max(2))
    }

    /// Compone el fotograma de `t`. `deadline`: tiempo máximo de espera por
    /// decodificador (visor: corto; export: largo). Devuelve el fotograma y si
    /// todos los clips estaban listos (false = algún clip va atrasado y se
    /// usó su último fotograma o negro).
    pub fn render(&mut self, t: Ticks, deadline: Duration) -> (Frame, bool) {
        let mut complete = true;
        self.canvas.clear();
        let piece = self.timeline.piece_at(t).cloned();
        let Some(piece) = piece else {
            self.decoders.clear();
            self.last_rendered = Some(t);
            return (self.canvas.to_frame(), true);
        };
        let active: Vec<ClipId> = piece.video.iter().map(|c| c.clip_id.clone()).collect();
        self.decoders.retain(|id, _| active.contains(id));
        self.last_source_times.clear();
        let started = Instant::now();
        for rc in &piece.video {
            let Some(src) = self.assets.get(&rc.asset_id).cloned() else {
                complete = false;
                continue;
            };
            if src.missing {
                complete = false;
                continue;
            }
            let source_t = rc.source_at(t);
            self.last_source_times.insert(rc.clip_id.clone(), source_t);
            if rc.is_image || src.kind == AssetKind::Image {
                let frame = match self.images.get(&rc.asset_id) {
                    Some(f) => f.clone(),
                    None => match decode_image(&src.path) {
                        Ok(f) => {
                            self.images.insert(rc.asset_id.clone(), f.clone());
                            f
                        }
                        Err(e) => {
                            tracing::warn!("imagen: {e}");
                            complete = false;
                            continue;
                        }
                    },
                };
                self.canvas.draw(&frame, &rc.transform);
                continue;
            }
            let size = self.decode_size(&src, rc);
            let frame_dur = self.timeline.frame_rate.frame_duration();
            let target_index_time = source_t;
            let remaining = deadline.saturating_sub(started.elapsed());
            let need_reopen = match self.decoders.get(&rc.clip_id) {
                None => true,
                Some(cd) => {
                    let next = cd.decoder.next_time();
                    cd.size != size
                        || cd.keyframes_only != self.keyframes_only
                        || target_index_time < next - frame_dur
                        || target_index_time > next + Ticks::from_seconds(3)
                        || (cd.decoder.is_finished() && cd.last.as_ref().is_none_or(|(lt, _)| target_index_time > *lt + frame_dur * 2))
                }
            };
            if need_reopen {
                match VideoDecoder::open_cancellable(
                    &self.tools,
                    &src.path,
                    source_t,
                    size.0,
                    size.1,
                    self.timeline.frame_rate,
                    self.keyframes_only,
                    self.cancel.clone(),
                ) {
                    Ok(d) => {
                        self.decoders.insert(rc.clip_id.clone(), ClipDecoder { decoder: d, last: None, size, keyframes_only: self.keyframes_only });
                    }
                    Err(e) => {
                        tracing::warn!(clip = %rc.clip_id, "decoder: {e}");
                        complete = false;
                        continue;
                    }
                }
            }
            let cd = self.decoders.get_mut(&rc.clip_id).unwrap();
            // avanzar hasta el fotograma cuyo tiempo fuente ≤ target < siguiente
            loop {
                let next = cd.decoder.next_time();
                if next > target_index_time && cd.last.is_some() {
                    break;
                }
                if cd.decoder.is_finished() {
                    break;
                }
                let wait = deadline.saturating_sub(started.elapsed()).max(Duration::from_millis(1));
                match cd.decoder.next(wait) {
                    Ok(Some((tt, f))) => {
                        cd.last = Some((tt, f));
                        if tt + frame_dur > target_index_time {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(crate::decoder::DecodeTimeout) => {
                        complete = false;
                        break;
                    }
                }
                if remaining.is_zero() {
                    complete = false;
                    break;
                }
            }
            match &cd.last {
                Some((tt, f)) => {
                    if target_index_time >= *tt + frame_dur {
                        complete = false;
                    }
                    self.canvas.draw(f, &rc.transform);
                }
                None => complete = false,
            }
        }
        self.last_rendered = Some(t);
        (self.canvas.to_frame(), complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonzero_container_start_is_not_added_twice_by_the_renderer() {
        use tv2_domain::{Command, project::Project, time::Rational};
        let tools = FfmpegTools::locate().unwrap();
        let path = fixture("fixture-start5.ts");
        let asset = tools.import(&path, path.to_string_lossy().into_owned()).unwrap();
        assert!(asset.probe.start_time > Ticks::from_seconds(4));
        let id = asset.id.clone();
        let source = AssetSource::from_asset(&asset, path.clone());
        let mut project = Project::new("Offset");
        Command::ImportAsset { asset }.apply(&mut project).unwrap();
        Command::InsertAssetLinked { asset_id: id.clone(), position: Ticks::ZERO, video_track: None, source: None }.apply(&mut project).unwrap();
        let resolved = Arc::new(ResolvedTimeline::resolve(&project, project.active().unwrap()));
        let mut renderer = TimelineRenderer::new(tools.clone(), resolved, Arc::new(HashMap::from([(id, source)])), 320, 180);
        let t = Ticks::from_seconds(2);
        let (rendered, complete) = renderer.render(t, Duration::from_secs(5));
        assert!(complete);
        // Contrato -ss relativo al comienzo del archivo, sin pasar por el compositor.
        let mut direct = VideoDecoder::open(&tools, &path, t, 320, 180, Rational::new(30, 1), false).unwrap();
        let (_, expected) = direct.next(Duration::from_secs(5)).unwrap().unwrap();
        let mae: f64 =
            rendered.rgba.iter().zip(expected.rgba.iter()).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / expected.rgba.len() as f64;
        assert!(mae < 2.0, "offset del contenedor aplicado dos veces: MAE={mae:.3}");
    }
    use tv2_domain::commands::Command;
    use tv2_domain::ids::TrackId;
    use tv2_domain::project::Project;
    use tv2_domain::time::{Rational, TimeRange};
    use tv2_domain::timeline::TrackKind;
    use tv2_domain::{MovePolicy, Transform};

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media").join(name)
    }

    #[test]
    fn renders_split_clip_with_overlay_from_real_media() {
        let tools = FfmpegTools::locate().unwrap();
        let mut p = Project::new("r");
        p.sequences[0].width = 640;
        p.sequences[0].height = 360;
        let a = tools.import(&fixture("fixture-a.mp4"), fixture("fixture-a.mp4").to_string_lossy().to_string()).unwrap();
        let png = tools.import(&fixture("overlay-alpha.png"), fixture("overlay-alpha.png").to_string_lossy().to_string()).unwrap();
        let (aid, pid) = (a.id.clone(), png.id.clone());
        Command::ImportAsset { asset: a }.apply(&mut p).unwrap();
        Command::ImportAsset { asset: png }.apply(&mut p).unwrap();
        let v1 = TrackId::new(
            Command::AddTrack { sequence_id: None, kind: TrackKind::Video, name: "V1".into(), index: None }.apply(&mut p).unwrap().created[0].clone(),
        );
        let v2 = TrackId::new(
            Command::AddTrack { sequence_id: None, kind: TrackKind::Video, name: "V2".into(), index: None }.apply(&mut p).unwrap().created[0].clone(),
        );
        // secuencia: 0–2 s ← fuente 5–7 s ; 2–4 s ← fuente 1–3 s ; overlay 1–3 s
        for (src_a, src_b, pos) in [(5, 7, 0), (1, 3, 2)] {
            Command::AddClip {
                track_id: v1.clone(),
                asset_id: aid.clone(),
                source: TimeRange::new(Ticks::from_seconds(src_a), Ticks::from_seconds(src_b)),
                position: Ticks::from_seconds(pos),
                policy: MovePolicy::Reject,
                clip_id: None,
                link_group: None,
                audio_stream: None,
                provenance: None,
            }
            .apply(&mut p)
            .unwrap();
        }
        Command::AddClip {
            track_id: v2,
            asset_id: pid.clone(),
            source: TimeRange::new(Ticks::ZERO, Ticks::from_seconds(2)),
            position: Ticks::from_seconds(1),
            policy: MovePolicy::Reject,
            clip_id: None,
            link_group: None,
            audio_stream: None,
            provenance: None,
        }
        .apply(&mut p)
        .unwrap();
        let seq = p.active().unwrap();
        let tl = Arc::new(ResolvedTimeline::resolve(&p, seq));
        let assets: HashMap<AssetId, AssetSource> =
            p.assets.iter().map(|a| (a.id.clone(), AssetSource::from_asset(a, PathBuf::from(&a.path)))).collect();
        let mut r = TimelineRenderer::new(tools, tl, Arc::new(assets), 640, 360);
        let fps = Rational::new(30, 1);
        // t = 2,5 s → fuente 1,5 s (fotograma A 45)
        let (f, ok) = r.render(Ticks::from_millis(2500), Duration::from_secs(10));
        assert!(ok);
        assert_eq!(*r.last_source_times.values().next().unwrap(), Ticks::from_millis(1500));
        assert_eq!((f.width, f.height), (640, 360));
        // el overlay (rojo semitransparente en 40..280×40..140 del PNG 320×180 ajustado al lienzo) tiñe la zona
        let px = f.pixel(320, 180);
        let dump = std::env::temp_dir().join("tv2-render-test.png");
        image::save_buffer(&dump, &f.rgba, f.width, f.height, image::ColorType::Rgba8).unwrap();
        assert!(px[0] > 90, "{px:?} (volcado en {})", dump.display());
        // t = 0,5 s → fuente 5,5 s: contenido y sin overlay; no bloquea la secuencia hacia atrás/adelante
        let (f2, ok2) = r.render(Ticks::from_millis(500), Duration::from_secs(10));
        assert!(ok2);
        assert_eq!(*r.last_source_times.values().next().unwrap(), Ticks::from_millis(5500));
        assert!(f2.rgba.iter().any(|b| *b > 0));
        // lectura secuencial fotograma a fotograma no reabre (misma decodificación)
        let mut t = Ticks::from_millis(500);
        for _ in 0..10 {
            t += fps.frame_duration();
            let (_, ok) = r.render(t, Duration::from_secs(10));
            assert!(ok);
        }
        let _ = Transform::default();
    }
}
