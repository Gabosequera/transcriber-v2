//! Resolución del timeline: la misma estructura alimenta preview y export.
//!
//! Para una secuencia produce tramos consecutivos en tiempo de secuencia. En
//! cada tramo se sabe qué clips de video están activos (de abajo hacia arriba
//! en orden de composición) y qué clips de audio suenan (con su ganancia
//! efectiva). Las pistas ocultas/mute/solo/bloqueadas se aplican aquí, de modo
//! que visor y salida coinciden por construcción.

use crate::ids::{AssetId, ClipId, TrackId};
use crate::project::Project;
use crate::time::{Rational, Ticks, TimeRange};
use crate::timeline::{Sequence, TrackKind, Transform};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ResolvedClip {
    pub clip_id: ClipId,
    pub track_id: TrackId,
    pub asset_id: AssetId,
    /// Rango en secuencia cubierto por este clip dentro del tramo.
    pub range: TimeRange,
    /// Tiempo fuente en `range.start`.
    pub source_start: Ticks,
    pub transform: Transform,
    /// Ganancia lineal efectiva (clip × pista). Solo audio.
    pub gain: f32,
    pub audio_stream: u32,
    /// Índice de composición (0 = abajo).
    pub z: usize,
    pub is_image: bool,
}

impl ResolvedClip {
    pub fn source_at(&self, seq_t: Ticks) -> Ticks {
        self.source_start + (seq_t - self.range.start)
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ResolvedPiece {
    pub range: TimeRange,
    /// De abajo hacia arriba.
    pub video: Vec<ResolvedClip>,
    pub audio: Vec<ResolvedClip>,
}

impl ResolvedPiece {
    pub fn is_gap(&self) -> bool {
        self.video.is_empty() && self.audio.is_empty()
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ResolvedTimeline {
    pub frame_rate: Rational,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    pub duration: Ticks,
    pub pieces: Vec<ResolvedPiece>,
}

impl ResolvedTimeline {
    /// Extrae la unión ordenada de rangos de secuencia y concatena su contenido.
    /// Conserva composición, mezcla y tiempo fuente; no muta el proyecto.
    pub fn extract_ranges(&self, ranges: &[TimeRange]) -> crate::error::DomainResult<Self> {
        let mut ranges = ranges.to_vec();
        if ranges.is_empty() || ranges.iter().any(|r| r.start < Ticks::ZERO || r.end > self.duration || r.end <= r.start) {
            return Err(crate::error::DomainError::out_of_range("selecciona rangos no vacíos dentro de la secuencia"));
        }
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<TimeRange> = Vec::new();
        for range in ranges {
            if let Some(last) = merged.last_mut()
                && range.start <= last.end
            {
                last.end = last.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        let mut result = self.clone();
        result.pieces.clear();
        let mut position = Ticks::ZERO;
        for range in merged {
            for piece in &self.pieces {
                let Some(overlap) = piece.range.intersection(&range) else { continue };
                let destination = TimeRange::new(position + overlap.start - range.start, position + overlap.end - range.start);
                let map = |clips: &[ResolvedClip]| {
                    clips
                        .iter()
                        .map(|clip| {
                            let mut mapped = clip.clone();
                            mapped.source_start = clip.source_at(overlap.start);
                            mapped.range = destination;
                            mapped
                        })
                        .collect()
                };
                result.pieces.push(ResolvedPiece { range: destination, video: map(&piece.video), audio: map(&piece.audio) });
            }
            position += range.duration();
        }
        result.duration = position;
        Ok(result)
    }

    pub fn empty(seq: &Sequence) -> Self {
        ResolvedTimeline {
            frame_rate: seq.frame_rate,
            width: seq.width,
            height: seq.height,
            sample_rate: seq.sample_rate,
            duration: Ticks::ZERO,
            pieces: Vec::new(),
        }
    }

    /// Barrido de bordes: O(n log n + tamaño de salida).
    pub fn resolve(project: &Project, seq: &Sequence) -> Self {
        use std::collections::{BTreeSet, HashMap};
        let tracks: HashMap<_, _> = seq.tracks.iter().enumerate().map(|(z, t)| (&t.id, (z, t))).collect();
        let images: HashMap<_, _> = project.assets.iter().map(|a| (&a.id, a.kind == crate::asset::AssetKind::Image)).collect();
        let solo_audio = seq.tracks.iter().any(|t| t.kind == TrackKind::Audio && t.solo);
        let solo_video = seq.tracks.iter().any(|t| t.kind == TrackKind::Video && t.solo);
        let mut points = vec![Ticks::ZERO];
        let mut events = Vec::with_capacity(seq.clips.len() * 2);
        for (index, clip) in seq.clips.iter().enumerate().filter(|(_, c)| c.enabled) {
            points.extend([clip.position, clip.end()]);
            if let Some((z, _)) = tracks.get(&clip.track_id) {
                events.push((clip.position, true, *z, index));
                events.push((clip.end(), false, *z, index));
            }
        }
        points.sort_unstable();
        points.dedup();
        events.sort_unstable();
        let mut event = 0;
        let mut active = BTreeSet::new();
        let mut pieces = Vec::with_capacity(points.len().saturating_sub(1));
        for bounds in points.windows(2) {
            let range = TimeRange::new(bounds[0], bounds[1]);
            while event < events.len() && events[event].0 <= range.start {
                let (_, starts, z, index) = events[event];
                if starts {
                    active.insert((z, index));
                } else {
                    active.remove(&(z, index));
                }
                event += 1;
            }
            let mut video = Vec::new();
            let mut audio = Vec::new();
            for &(z, index) in &active {
                let clip = &seq.clips[index];
                let track = &seq.tracks[z];
                let enabled = match track.kind {
                    TrackKind::Video => track.visible && (!solo_video || track.solo),
                    TrackKind::Audio => !track.muted && (!solo_audio || track.solo),
                };
                if !enabled {
                    continue;
                }
                let resolved = ResolvedClip {
                    clip_id: clip.id.clone(),
                    track_id: track.id.clone(),
                    asset_id: clip.asset_id.clone(),
                    range,
                    source_start: clip.source.start + range.start - clip.position,
                    transform: clip.transform,
                    gain: db_to_linear(clip.gain_db + track.gain_db),
                    audio_stream: clip.audio_stream.unwrap_or(0),
                    z,
                    is_image: images.get(&clip.asset_id).copied().unwrap_or(false),
                };
                match track.kind {
                    TrackKind::Video => video.push(resolved),
                    TrackKind::Audio => audio.push(resolved),
                }
            }
            pieces.push(ResolvedPiece { range, video, audio });
        }
        ResolvedTimeline {
            frame_rate: seq.frame_rate,
            width: seq.width,
            height: seq.height,
            sample_rate: seq.sample_rate,
            duration: seq.extent(),
            pieces,
        }
    }

    #[cfg(test)]
    fn resolve_reference(project: &Project, seq: &Sequence) -> Self {
        let mut points: Vec<Ticks> = vec![Ticks::ZERO];
        for c in seq.clips.iter().filter(|c| c.enabled) {
            points.push(c.position);
            points.push(c.end());
        }
        points.sort();
        points.dedup();
        let duration = seq.extent();
        let any_solo_audio = seq.tracks.iter().any(|t| t.kind == TrackKind::Audio && t.solo);
        let any_solo_video = seq.tracks.iter().any(|t| t.kind == TrackKind::Video && t.solo);
        let mut pieces = Vec::new();
        for w in points.windows(2) {
            let range = TimeRange::new(w[0], w[1]);
            if range.duration().0 <= 0 {
                continue;
            }
            let mid = Ticks((range.start.0 + range.end.0) / 2);
            let mut video = Vec::new();
            let mut audio = Vec::new();
            for (z, track) in seq.tracks.iter().enumerate() {
                let audible = match track.kind {
                    TrackKind::Audio => !track.muted && (!any_solo_audio || track.solo),
                    TrackKind::Video => false,
                };
                let visible = match track.kind {
                    TrackKind::Video => track.visible && (!any_solo_video || track.solo),
                    TrackKind::Audio => false,
                };
                if !audible && !visible {
                    continue;
                }
                for c in seq.clips.iter().filter(|c| c.enabled && c.track_id == track.id && c.range().contains(mid)) {
                    let asset = project.asset(&c.asset_id);
                    let is_image = asset.map(|a| a.kind == crate::asset::AssetKind::Image).unwrap_or(false);
                    let rc = ResolvedClip {
                        clip_id: c.id.clone(),
                        track_id: track.id.clone(),
                        asset_id: c.asset_id.clone(),
                        range,
                        source_start: c.source.start + (range.start - c.position),
                        transform: c.transform,
                        gain: db_to_linear(c.gain_db + track.gain_db),
                        audio_stream: c.audio_stream.unwrap_or(0),
                        z,
                        is_image,
                    };
                    match track.kind {
                        TrackKind::Video => video.push(rc),
                        TrackKind::Audio => audio.push(rc),
                    }
                }
            }
            pieces.push(ResolvedPiece { range, video, audio });
        }
        ResolvedTimeline { frame_rate: seq.frame_rate, width: seq.width, height: seq.height, sample_rate: seq.sample_rate, duration, pieces }
    }

    pub fn piece_at(&self, t: Ticks) -> Option<&ResolvedPiece> {
        if t.is_negative() {
            return None;
        }
        let idx = self.pieces.partition_point(|p| p.range.end <= t);
        self.pieces.get(idx).filter(|p| p.range.contains(t))
    }

    /// Clip de video visible más alto en `t` (el que «manda» sin composición).
    pub fn top_video_at(&self, t: Ticks) -> Option<&ResolvedClip> {
        self.piece_at(t).and_then(|p| p.video.last())
    }

    pub fn frame_count(&self) -> i64 {
        // fotogramas completos que cubren la duración (ceil)
        let fd = self.frame_rate.frame_duration();
        if fd.0 == 0 {
            return 0;
        }
        (self.duration.0 + fd.0 - 1) / fd.0
    }

    /// Tramos no vacíos en tiempo de secuencia.
    pub fn content_pieces(&self) -> impl Iterator<Item = &ResolvedPiece> {
        self.pieces.iter().filter(|p| !p.is_gap())
    }
}

pub fn db_to_linear(db: f32) -> f32 {
    if db <= -96.0 { 0.0 } else { 10f32.powf(db / 20.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Command, MovePolicy};
    use crate::ids::AssetId;

    fn s(n: i64) -> Ticks {
        Ticks::from_seconds(n)
    }

    #[test]
    fn upper_video_track_covers_lower_and_audio_mixes() {
        let mut p = Project::new("t");
        Command::ImportAsset { asset: crate::commands::tests_support::fake_video("a", 100) }.apply(&mut p).unwrap();
        let v1 = TrackId::new(
            Command::AddTrack { sequence_id: None, kind: TrackKind::Video, name: "V1".into(), index: None }.apply(&mut p).unwrap().created[0].clone(),
        );
        let v2 = TrackId::new(
            Command::AddTrack { sequence_id: None, kind: TrackKind::Video, name: "V2".into(), index: None }.apply(&mut p).unwrap().created[0].clone(),
        );
        let a1 = TrackId::new(
            Command::AddTrack { sequence_id: None, kind: TrackKind::Audio, name: "A1".into(), index: None }.apply(&mut p).unwrap().created[0].clone(),
        );
        let a2 = TrackId::new(
            Command::AddTrack { sequence_id: None, kind: TrackKind::Audio, name: "A2".into(), index: None }.apply(&mut p).unwrap().created[0].clone(),
        );
        let add = |p: &mut Project, t: &TrackId, a: i64, b: i64, pos: i64| {
            Command::AddClip {
                track_id: t.clone(),
                asset_id: AssetId::new("a"),
                source: TimeRange::new(s(a), s(b)),
                position: s(pos),
                policy: MovePolicy::Reject,
                clip_id: None,
                link_group: None,
                audio_stream: Some(0),
                provenance: None,
            }
            .apply(p)
            .unwrap();
        };
        add(&mut p, &v1, 10, 30, 0); // 0–20
        add(&mut p, &v2, 60, 62, 8); // tapa 8–10
        add(&mut p, &a1, 10, 30, 0);
        add(&mut p, &a2, 40, 45, 5); // 5–10 se mezcla
        let seq = p.active().unwrap();
        let r = ResolvedTimeline::resolve(&p, seq);
        assert_eq!(r, ResolvedTimeline::resolve_reference(&p, seq));
        assert_eq!(r.duration, s(20));
        // Rangos desordenados/solapados se unen; se conserva el mapping y la mezcla.
        let extracted = r.extract_ranges(&[TimeRange::new(s(8), s(10)), TimeRange::new(s(1), s(3)), TimeRange::new(s(2), s(4))]).unwrap();
        assert_eq!(extracted.duration, s(5));
        assert_eq!(extracted.top_video_at(s(0)).unwrap().source_at(s(0)), s(11));
        assert_eq!(extracted.top_video_at(s(3)).unwrap().source_at(s(3)), s(60));
        assert_eq!(extracted.piece_at(s(3)).unwrap().audio.len(), 2);
        assert_eq!(extracted.piece_at(s(3)).unwrap().audio[0].source_at(s(3)), s(18));
        assert!(r.extract_ranges(&[]).is_err());
        assert!(r.extract_ranges(&[TimeRange::new(s(19), s(21))]).is_err());
        let ranges: Vec<(i64, i64)> =
            r.pieces.iter().map(|x| (x.range.start.as_millis_round() / 1000, x.range.end.as_millis_round() / 1000)).collect();
        assert_eq!(ranges, vec![(0, 5), (5, 8), (8, 10), (10, 20)]);
        let top = r.top_video_at(s(9)).unwrap();
        assert_eq!(top.track_id, v2);
        assert_eq!(top.source_at(s(9)), s(61));
        assert_eq!(r.piece_at(s(9)).unwrap().video.len(), 2);
        assert_eq!(r.piece_at(s(6)).unwrap().audio.len(), 2);
        assert_eq!(r.piece_at(s(15)).unwrap().audio.len(), 1);
        assert_eq!(r.top_video_at(s(15)).unwrap().source_at(s(15)), s(25));
        assert!(r.piece_at(s(25)).is_none());
        // mute A1 → solo A2 en 5–10; solo V2 → V1 desaparece
        let mut p2 = p.clone();
        Command::SetTrackProps {
            track_id: a1.clone(),
            name: None,
            muted: Some(true),
            solo: None,
            locked: None,
            visible: None,
            gain_db: None,
            height: None,
        }
        .apply(&mut p2)
        .unwrap();
        Command::SetTrackProps {
            track_id: v2.clone(),
            name: None,
            muted: None,
            solo: Some(true),
            locked: None,
            visible: None,
            gain_db: None,
            height: None,
        }
        .apply(&mut p2)
        .unwrap();
        let r2 = ResolvedTimeline::resolve(&p2, p2.active().unwrap());
        assert_eq!(r2, ResolvedTimeline::resolve_reference(&p2, p2.active().unwrap()));
        // Oráculo anterior por búsqueda exhaustiva contra un conjunto denso de bordes.
        let mut dense = p.clone();
        let seeds = dense.sequences[0].clips.clone();
        dense.sequences[0].clips = (0..120)
            .map(|i| {
                let mut clip = seeds[i % seeds.len()].clone();
                clip.id = ClipId::new(format!("clip-sweep-{i}"));
                clip.enabled = i % 7 != 0;
                clip.position = Ticks::from_millis((i * 7919 % 9000) as i64);
                clip.source = TimeRange::new(Ticks::from_millis((i * 131 % 1000) as i64), s(3));
                clip
            })
            .collect();
        assert_eq!(ResolvedTimeline::resolve(&dense, dense.active().unwrap()), ResolvedTimeline::resolve_reference(&dense, dense.active().unwrap()));
        assert_eq!(r2.piece_at(s(6)).unwrap().audio.len(), 1);
        assert!(r2.piece_at(s(15)).unwrap().audio.is_empty());
        assert!(r2.top_video_at(s(15)).is_none());
        assert_eq!(r2.top_video_at(s(9)).unwrap().track_id, v2);
    }

    #[test]
    fn gain_is_linear_product() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_linear(-6.0) - 0.5012).abs() < 1e-3);
        assert_eq!(db_to_linear(-96.0), 0.0);
    }
}
