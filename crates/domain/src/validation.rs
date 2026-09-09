//! Validation at snapshot boundaries. Never inspect media files here: an offline
//! asset is still a valid project. Check references before doing time arithmetic.

use crate::error::DomainResult;
use crate::ids::is_valid_v1_id;
use crate::{AssetKind, DomainError, Project, Rational, Ticks, TrackKind};
use std::collections::{HashMap, HashSet};

/// Successful layer-validation certificates. No project/base is presumed valid;
/// every call still validates global references, assets, sequences and masters.
/// The latest complete immutable layer and duration are retained for at most 256
/// layer IDs. COW items keep a certificate unchanged when callers mutate a copy.
#[derive(Default)]
pub struct LayerValidationCache {
    layers: HashMap<crate::LayerId, (crate::SemanticLayer, Ticks)>,
}

impl LayerValidationCache {
    pub fn validate(&mut self, project: &Project) -> DomainResult<()> {
        project.validate_with_layers(|layer, duration| {
            if self.layers.get(&layer.layer_id).is_some_and(|(old, old_duration)| *old_duration == duration && old == layer) {
                return Ok(());
            }
            validate_layer(layer, duration)?;
            // This is acceleration only: dropping certificates never drops data.
            if self.layers.len() >= 256 && !self.layers.contains_key(&layer.layer_id) {
                self.layers.clear();
            }
            self.layers.insert(layer.layer_id.clone(), (layer.clone(), duration));
            Ok(())
        })
    }
}

fn validate_layer(layer: &crate::SemanticLayer, duration: Ticks) -> DomainResult<()> {
    crate::layers::validate_layer(layer, duration)?;
    unique(layer.deleted_item_ids.iter().map(|id| id.as_str()), "tombstone")
}

fn unique<'a>(ids: impl Iterator<Item = &'a str>, kind: &str) -> DomainResult<()> {
    let mut seen = HashSet::new();
    for id in ids {
        if !is_valid_v1_id(id) || !seen.insert(id) {
            return Err(DomainError::invalid(format!("{kind}: ID inválido o duplicado «{id}»")));
        }
    }
    Ok(())
}

fn rate(r: Rational) -> DomainResult<()> {
    if r.num <= 0 || r.den <= 0 {
        return Err(DomainError::invalid("base temporal debe ser positiva y representable"));
    }
    let ticks = r.den as i128 * crate::FLICKS_PER_SECOND as i128 / r.num as i128;
    if ticks < 1 || ticks > i64::MAX as i128 {
        return Err(DomainError::invalid("base temporal no representable"));
    }
    Ok(())
}

fn gain(v: f32) -> DomainResult<()> {
    if !v.is_finite() || !(-96.0..=24.0).contains(&v) {
        return Err(DomainError::invalid("ganancia fuera de rango (-96..24 dB)"));
    }
    Ok(())
}

impl Project {
    /// Shared by load, recovery, preview and commit. Unknown fields are kept.
    pub fn validate(&self) -> DomainResult<()> {
        self.validate_with_layers(validate_layer)
    }

    fn validate_with_layers(&self, mut layer_validator: impl FnMut(&crate::SemanticLayer, Ticks) -> DomainResult<()>) -> DomainResult<()> {
        if !matches!(self.schema.as_str(), crate::PROJECT_SCHEMA | crate::project::LEGACY_PROJECT_SCHEMA) {
            return Err(DomainError::unsupported(format!("schema de proyecto desconocido: {}", self.schema)));
        }
        if !is_valid_v1_id(&self.project_id) || self.name.trim().is_empty() {
            return Err(DomainError::invalid("identidad o nombre de proyecto inválidos"));
        }
        unique(self.assets.iter().map(|a| a.id.as_str()), "asset")?;
        unique(self.sequences.iter().map(|s| s.id.as_str()), "secuencia")?;
        unique(self.sequences.iter().flat_map(|s| s.tracks.iter().map(|t| t.id.as_str())), "pista")?;
        unique(self.sequences.iter().flat_map(|s| s.clips.iter().map(|c| c.id.as_str())), "clip")?;
        unique(self.sequences.iter().flat_map(|s| s.markers.iter().map(|m| m.id.as_str())), "marcador")?;
        unique(self.layers.iter().map(|l| l.layer_id.as_str()), "capa")?;
        unique(self.layer_order.iter().map(|id| id.as_str()), "orden de capas")?;
        if self.active_sequence.as_ref().is_some_and(|id| self.sequence(id).is_none()) {
            return Err(DomainError::invalid("secuencia activa inexistente"));
        }
        let assets: HashMap<_, _> = self.assets.iter().map(|a| (&a.id, a)).collect();
        for a in &self.assets {
            if a.path.is_empty() || a.probe.duration < Ticks::ZERO || a.duration() <= Ticks::ZERO {
                return Err(DomainError::invalid(format!("asset {}: ruta o duración inválida", a.id)));
            }
            if let Some(v) = &a.probe.video {
                rate(v.frame_rate)?;
                rate(v.time_base)?;
                if v.width == 0 || v.height == 0 || v.duration.is_some_and(|d| d < Ticks::ZERO) {
                    return Err(DomainError::invalid("dimensiones o duración de video inválidas"));
                }
            }
            let mut streams = HashSet::new();
            for (i, audio) in a.probe.audio.iter().enumerate() {
                if audio.audio_index as usize != i
                    || !streams.insert(audio.stream_index)
                    || audio.sample_rate == 0
                    || audio.channels == 0
                    || audio.duration.is_some_and(|d| d < Ticks::ZERO)
                {
                    return Err(DomainError::invalid("inventario de audio inválido"));
                }
            }
        }
        for s in &self.sequences {
            rate(s.frame_rate)?;
            if s.width == 0 || s.height == 0 || s.sample_rate == 0 {
                return Err(DomainError::invalid("dimensiones o sample rate de secuencia inválidos"));
            }
            let tracks: HashMap<_, _> = s.tracks.iter().map(|t| (&t.id, t)).collect();
            let mut intervals: HashMap<_, Vec<_>> = HashMap::new();
            for t in &s.tracks {
                gain(t.gain_db)?;
                if t.height.is_some_and(|h| !h.is_finite() || !(24.0..=400.0).contains(&h)) {
                    return Err(DomainError::invalid("altura de pista inválida"));
                }
            }
            for c in &s.clips {
                let a = assets.get(&c.asset_id).ok_or_else(|| DomainError::not_found("asset", &c.asset_id))?;
                let t = tracks.get(&c.track_id).ok_or_else(|| DomainError::not_found("pista", &c.track_id))?;
                crate::commands::validate_source(a, &c.source)?;
                crate::commands::check_track_compat(t, a, c.audio_stream)?;
                if c.position < Ticks::ZERO || c.position.0.checked_add(c.source.end.0 - c.source.start.0).is_none() {
                    return Err(DomainError::out_of_range("posición o fin de clip no representable"));
                }
                if t.kind == TrackKind::Video && c.audio_stream.is_some() {
                    return Err(DomainError::invalid("clip de video con selector de audio"));
                }
                gain(c.gain_db)?;
                let tr = c.transform;
                if !tr.x.is_finite()
                    || !tr.y.is_finite()
                    || !tr.scale.is_finite()
                    || tr.scale <= 0.0
                    || !tr.opacity.is_finite()
                    || !(0.0..=1.0).contains(&tr.opacity)
                {
                    return Err(DomainError::invalid("transformación de clip inválida"));
                }
                intervals.entry(&c.track_id).or_default().push((c.position, c.end()));
            }
            for spans in intervals.values_mut() {
                spans.sort_unstable();
                if spans.windows(2).any(|w| w[0].1 > w[1].0) {
                    return Err(DomainError::overlap("clips solapados en una misma pista"));
                }
            }
            for m in &s.markers {
                if m.range.start < Ticks::ZERO || m.range.end < m.range.start || !crate::layers::is_hex_color(&m.color) {
                    return Err(DomainError::invalid("rango o color de marcador inválido"));
                }
            }
        }
        for l in &self.layers {
            let a = assets.get(&l.asset_id).ok_or_else(|| DomainError::not_found("asset", &l.asset_id))?;
            layer_validator(l, if a.kind == AssetKind::Image { Ticks::MAX } else { a.duration() })?;
        }
        for id in &self.layer_order {
            self.layer(id).ok_or_else(|| DomainError::not_found("capa", id))?;
        }
        unique(self.masters.iter().map(|m| m.asset_id.as_str()), "master por asset")?;
        for master in &self.masters {
            master.validate(self)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests_support::fake_video;
    use crate::{Clip, TimeRange, Track};

    fn fixture() -> Project {
        let mut p = Project::new("validation");
        let asset = fake_video("asset-a", 10);
        let t = Track::new(TrackKind::Video, "V1");
        let c = Clip::new(t.id.clone(), asset.id.clone(), TimeRange::new(Ticks::ZERO, Ticks::from_seconds(2)), Ticks::ZERO);
        p.assets.push(asset);
        p.sequences[0].tracks.push(t);
        p.sequences[0].clips.push(c);
        p
    }

    #[test]
    fn malformed_snapshots_reject_references_ids_times_and_nan() {
        let p = fixture();
        p.validate().unwrap();
        let mutations: Vec<fn(&mut Project)> = vec![
            |p| p.assets.clear(),
            |p| p.sequences[0].tracks.clear(),
            |p| p.sequences[0].frame_rate.num = 0,
            |p| p.sequences[0].sample_rate = 0,
            |p| p.sequences[0].clips[0].position = Ticks::MAX,
            |p| p.sequences[0].clips[0].source.start = Ticks(-1),
            |p| p.sequences[0].clips[0].transform.scale = f32::NAN,
            |p| {
                let clip = p.sequences[0].clips[0].clone();
                p.sequences[0].clips.push(clip);
            },
            |p| p.active_sequence = Some("unknown".into()),
            |p| p.layer_order.push("unknown".into()),
        ];
        for mutate in mutations {
            let mut bad = p.clone();
            mutate(&mut bad);
            assert!(bad.validate().is_err(), "accepted malformed snapshot: {bad:?}");
        }
    }

    #[test]
    fn detects_overlap_and_global_ids_but_allows_offline_media() {
        let mut p = fixture();
        p.assets[0].missing = true;
        p.validate().unwrap();
        let mut other = p.sequences[0].clips[0].clone();
        other.id = "clip-other".into();
        p.sequences[0].clips.push(other);
        assert_eq!(p.validate().unwrap_err().code, crate::ErrorCode::Overlap);
        p.sequences[0].clips.pop();
        let mut sequence = p.sequences[0].clone();
        sequence.id = "seq-other".into();
        p.sequences.push(sequence);
        assert!(p.validate().is_err());
    }
}
