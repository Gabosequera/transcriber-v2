//! `editorial-montaje/1` (`views/montaje.json`): secuencia V1 con pistas de video
//! apiladas (`V1` abajo… la superior tapa video **y** audio) y aplanado sin
//! huecos (`flatten`). Se porta el algoritmo de `editorial_montaje.flatten` con
//! su aritmética en segundos (redondeo a 3 decimales, `EPS = 1e-3`) para que los
//! tramos coincidan exactamente con V1; después se convierten a `Ticks`.
//!
//! Conversión explícita a V2 (`to_sequence`): los tramos aplanados se colocan en
//! una pista de video y una de audio; cada clip conserva `v1_clip_id`, estado,
//! origen y temas en `extra`. Es un perfil «montaje V1», no una composición V2.

use crate::master::parse_fingerprint;
use crate::{V1Result, invalid, secs_to_ticks};
use serde_json::{Map, Value, json};
use tv2_domain::asset::Fingerprint;
use tv2_domain::ids::{AssetId, ClipId};
use tv2_domain::time::{Rational, TimeRange};
use tv2_domain::timeline::{Clip, Provenance, Sequence, Track, TrackKind};

pub const SCHEMA_MONTAJE: &str = "editorial-montaje/1";
const EPS: f64 = 1e-3;

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[derive(Clone, Debug, PartialEq)]
pub struct V1Clip {
    pub clip_id: String,
    pub track_id: String,
    pub source_ini: f64,
    pub source_fin: f64,
    pub seq_ini: f64,
    pub state: String,
    pub origin: String,
    pub label: String,
    pub topic_ids: Vec<String>,
    pub edited: bool,
    pub raw: Map<String, Value>,
}

impl V1Clip {
    pub fn seq_fin(&self) -> f64 {
        round3(self.seq_ini + self.source_fin - self.source_ini)
    }
}

#[derive(Clone, Debug)]
pub struct V1Montaje {
    pub duration_source: f64,
    pub revision: u64,
    pub tracks: Vec<String>,
    pub clips: Vec<V1Clip>,
    pub raw: Value,
}

/// Tramo aplanado (`flatten`): `gap` cuando no hay clip activo.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub gap: bool,
    pub seq_ini: f64,
    pub seq_fin: f64,
    pub source_ini: f64,
    pub source_fin: f64,
    pub clip_id: String,
    pub track_id: String,
}

fn track_number(t: &str) -> u32 {
    t[1..].parse().unwrap_or(0)
}

impl V1Montaje {
    pub fn parse(raw: Value, expected: Option<&Fingerprint>) -> V1Result<V1Montaje> {
        let obj = raw.as_object().ok_or_else(|| invalid("montaje no es un objeto"))?;
        if obj.get("schema").and_then(|x| x.as_str()) != Some(SCHEMA_MONTAJE) {
            return Err(invalid(format!("schema del montaje debe ser {SCHEMA_MONTAJE}")));
        }
        if let (Some(exp), Some(fp)) = (expected, obj.get("media").map(parse_fingerprint).transpose()?)
            && !exp.same_identity(&fp)
        {
            return Err(invalid("el montaje pertenece a otro video"));
        }
        let duration = obj.get("duration_source").and_then(|x| x.as_f64()).ok_or_else(|| invalid("montaje sin duration_source"))?;
        let mut clips = Vec::new();
        for (i, c) in obj.get("clips").and_then(|x| x.as_array()).ok_or_else(|| invalid("clips debe ser una lista"))?.iter().enumerate() {
            let co = c.as_object().ok_or_else(|| invalid(format!("clip {} no es un objeto", i + 1)))?;
            let clip_id = co.get("clip_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if !(clip_id.starts_with("clip-") && clip_id.len() >= 11 && clip_id[5..].chars().all(|c| c.is_ascii_digit())) {
                return Err(invalid(format!("clip_id inválido: {clip_id:?}")));
            }
            let track_id = co.get("track_id").and_then(|x| x.as_str()).unwrap_or("V1").to_string();
            if !(track_id.starts_with('V') && track_id[1..].parse::<u32>().is_ok_and(|n| n >= 1)) {
                return Err(invalid(format!("{clip_id}: pista inválida {track_id:?}")));
            }
            let a = co.get("source_ini").and_then(|x| x.as_f64()).ok_or_else(|| invalid(format!("{clip_id}.source_ini")))?;
            let b = co.get("source_fin").and_then(|x| x.as_f64()).ok_or_else(|| invalid(format!("{clip_id}.source_fin")))?;
            if a < 0.0 || b > duration + 0.001 || b - a < 1.0 / 30.0 - 1e-9 {
                return Err(invalid(format!("{clip_id}: rango fuente inválido {a:.3}..{b:.3}")));
            }
            let seq = co.get("seq_ini").and_then(|x| x.as_f64()).unwrap_or(0.0);
            if seq < 0.0 {
                return Err(invalid(format!("{clip_id}: seq_ini negativo")));
            }
            let state = co.get("state").and_then(|x| x.as_str()).unwrap_or("proposed").to_string();
            if !["proposed", "accepted", "disabled"].contains(&state.as_str()) {
                return Err(invalid(format!("{clip_id}: estado desconocido {state:?}")));
            }
            clips.push(V1Clip {
                clip_id,
                track_id,
                source_ini: round3(a),
                source_fin: round3(b.min(duration)),
                seq_ini: round3(seq),
                state,
                origin: co.get("origin").and_then(|x| x.as_str()).unwrap_or("user").to_string(),
                label: co.get("label").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                topic_ids: co
                    .get("topic_ids")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|t| t.as_str()).map(|s| s.to_string()).collect())
                    .unwrap_or_default(),
                edited: co.get("edited").and_then(|x| x.as_bool()).unwrap_or(false),
                raw: co.clone(),
            });
        }
        // pistas: las declaradas más las usadas, más V1, ordenadas por número
        let mut tracks: Vec<String> = obj
            .get("tracks")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|t| t.get("track_id").and_then(|x| x.as_str())).map(|s| s.to_string()).collect())
            .unwrap_or_default();
        for t in std::iter::once("V1".to_string()).chain(clips.iter().map(|c| c.track_id.clone())) {
            if !tracks.contains(&t) {
                tracks.push(t);
            }
        }
        tracks.sort_by_key(|t| track_number(t));
        let order: std::collections::HashMap<String, usize> = tracks.iter().enumerate().map(|(i, t)| (t.clone(), i)).collect();
        clips.sort_by(|a, b| {
            (order[&a.track_id], a.seq_ini, a.clip_id.clone()).partial_cmp(&(order[&b.track_id], b.seq_ini, b.clip_id.clone())).unwrap()
        });
        // sin solapes en la misma pista
        let mut previous: std::collections::HashMap<&str, &V1Clip> = std::collections::HashMap::new();
        for c in &clips {
            if let Some(last) = previous.get(c.track_id.as_str())
                && c.seq_ini < last.seq_fin() - EPS
            {
                return Err(invalid(format!("{} se solapa con {} en {}", c.clip_id, last.clip_id, c.track_id)));
            }
            previous.insert(c.track_id.as_str(), c);
        }
        Ok(V1Montaje { duration_source: round3(duration), revision: obj.get("revision").and_then(|x| x.as_u64()).unwrap_or(0), tracks, clips, raw })
    }

    /// Porte de `editorial_montaje.flatten(document, gaps)`.
    pub fn flatten(&self, gaps: bool) -> Vec<Piece> {
        let active: Vec<&V1Clip> = self.clips.iter().filter(|c| c.state != "disabled").collect();
        if active.is_empty() {
            return Vec::new();
        }
        let rank: std::collections::HashMap<&str, usize> = self.tracks.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();
        let mut points: Vec<f64> = active.iter().flat_map(|c| [c.seq_ini, c.seq_fin()]).collect();
        points.sort_by(|a, b| a.partial_cmp(b).unwrap());
        points.dedup();
        let mut pieces: Vec<Piece> = Vec::new();
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b - a <= EPS {
                continue;
            }
            let mid = (a + b) / 2.0;
            let covering: Vec<&&V1Clip> =
                active.iter().filter(|c| c.seq_ini - EPS <= mid && mid <= c.seq_fin() + EPS && c.seq_ini < b && c.seq_fin() > a).collect();
            if covering.is_empty() {
                if gaps {
                    pieces.push(Piece {
                        gap: true,
                        seq_ini: round3(a),
                        seq_fin: round3(b),
                        source_ini: 0.0,
                        source_fin: 0.0,
                        clip_id: String::new(),
                        track_id: String::new(),
                    });
                }
                continue;
            }
            let top = covering.iter().max_by_key(|c| rank.get(c.track_id.as_str()).copied().unwrap_or(0)).unwrap();
            let offset = a - top.seq_ini;
            let piece = Piece {
                gap: false,
                seq_ini: round3(a),
                seq_fin: round3(b),
                clip_id: top.clip_id.clone(),
                track_id: top.track_id.clone(),
                source_ini: round3(top.source_ini + offset),
                source_fin: round3(top.source_ini + offset + (b - a)),
            };
            if let Some(last) = pieces.last_mut()
                && !last.gap
                && last.clip_id == piece.clip_id
                && (last.source_fin - piece.source_ini).abs() <= EPS
                && (last.seq_fin - piece.seq_ini).abs() <= EPS
            {
                last.seq_fin = piece.seq_fin;
                last.source_fin = piece.source_fin;
            } else {
                pieces.push(piece);
            }
        }
        if gaps {
            return pieces;
        }
        let mut cursor = 0.0;
        let mut compact = Vec::new();
        for p in pieces.into_iter().filter(|p| !p.gap) {
            let length = p.source_fin - p.source_ini;
            compact.push(Piece { seq_ini: round3(cursor), seq_fin: round3(cursor + length), ..p });
            cursor += length;
        }
        compact
    }

    pub fn total_seconds(&self) -> f64 {
        round3(self.flatten(false).iter().map(|p| p.source_fin - p.source_ini).sum())
    }

    /// `source_to_seq`: tiempos de secuencia (con huecos) en los que suena el instante fuente `t`.
    pub fn source_to_seq(&self, t: f64) -> Vec<f64> {
        self.flatten(true)
            .iter()
            .filter(|p| !p.gap && p.source_ini - EPS <= t && t < p.source_fin + EPS)
            .map(|p| round3(p.seq_ini + (t - p.source_ini)))
            .collect()
    }

    /// Conversión explícita a una secuencia V2 (perfil «montaje V1»: aplanado, sin
    /// huecos, la pista superior sustituye video y audio). `audio_streams` es el
    /// número de pistas de audio del asset; cada una recibe su pista A.
    pub fn to_sequence(&self, asset_id: &AssetId, frame_rate: Rational, width: u32, height: u32, audio_streams: u32) -> Sequence {
        let mut seq = Sequence::new("Montaje V1", frame_rate, width, height, 48_000);
        seq.extra.insert(
            "v1_montage_profile".into(),
            json!({"schema": SCHEMA_MONTAJE, "revision": self.revision, "flattened": true, "top_track_replaces_audio": true, "gaps_compacted": true}),
        );
        let v = Track::new(TrackKind::Video, "V1");
        let vid = v.id.clone();
        seq.tracks.push(v);
        let mut audio_tracks = Vec::new();
        for i in 0..audio_streams.max(1) {
            let a = Track::new(TrackKind::Audio, format!("A{}", i + 1));
            audio_tracks.push(a.id.clone());
            seq.tracks.push(a);
        }
        for (n, p) in self.flatten(false).iter().enumerate() {
            let source = TimeRange::new(secs_to_ticks(p.source_ini), secs_to_ticks(p.source_fin));
            let position = secs_to_ticks(p.seq_ini);
            let origin_clip = self.clips.iter().find(|c| c.clip_id == p.clip_id);
            let group = format!("link-v1-{}-{n}", p.clip_id);
            let make = |track: &tv2_domain::ids::TrackId, audio_stream: Option<u32>| {
                let mut c = Clip::new(track.clone(), asset_id.clone(), source, position);
                c.id = ClipId::new(format!("{}-{n}{}", p.clip_id, audio_stream.map(|s| format!("-a{s}")).unwrap_or_default()));
                c.name =
                    origin_clip.map(|c| if c.label.is_empty() { c.clip_id.clone() } else { c.label.clone() }).unwrap_or_else(|| p.clip_id.clone());
                c.link_group = Some(group.clone());
                c.audio_stream = audio_stream;
                c.provenance = Provenance {
                    origin: "import-v1".into(),
                    parent_clip: None,
                    v1_clip_id: Some(p.clip_id.clone()),
                    created_at: Some(tv2_domain::project::now_iso()),
                };
                if let Some(oc) = origin_clip {
                    c.extra.insert("v1".into(), json!({"state": oc.state, "origin": oc.origin, "topic_ids": oc.topic_ids, "edited": oc.edited, "track_id": oc.track_id, "seq_ini_placed": oc.seq_ini}));
                }
                c
            };
            seq.clips.push(make(&vid, None));
            for (i, at) in audio_tracks.iter().enumerate() {
                seq.clips.push(make(at, Some(i as u32)));
            }
        }
        seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::time::Ticks;

    fn fp() -> Fingerprint {
        Fingerprint { size: 10, mtime_ns: None, hash_muestreado: "m".into(), inventario_sha256: "i".into() }
    }

    fn clip(id: u32, track: &str, a: f64, b: f64, seq: f64) -> Value {
        json!({"clip_id": format!("clip-{id:06}"), "track_id": track, "source_ini": a, "source_fin": b, "seq_ini": seq, "label": "", "topic_ids": [], "origin": "user", "state": "proposed", "edited": false})
    }

    fn doc(clips: Vec<Value>) -> Value {
        json!({"schema": "editorial-montaje/1", "media": {"size": 10, "hash_muestreado": "m", "inventario_sha256": "i"}, "duration_source": 100.0,
               "target_seconds": 900.0, "revision": 0, "next_id": 10, "tracks": [{"track_id": "V1", "name": "V1"}], "clips": clips, "analysis": {"request_id": null, "pass": 0}})
    }

    /// `test_montaje.py::test_flatten_top_track_covers_bottom_at_start_middle_and_end`.
    #[test]
    fn top_track_covers_bottom_at_start_middle_and_end() {
        let m = V1Montaje::parse(
            doc(vec![
                clip(1, "V1", 10.0, 30.0, 0.0),
                clip(2, "V2", 50.0, 53.0, 0.0),
                clip(3, "V2", 60.0, 62.0, 8.0),
                clip(4, "V2", 70.0, 74.0, 18.0),
            ]),
            Some(&fp()),
        )
        .unwrap();
        let pieces = m.flatten(false);
        let ids: Vec<(String, f64, f64)> = pieces.iter().map(|p| (p.clip_id.clone(), p.source_ini, p.source_fin)).collect();
        assert_eq!(
            ids,
            vec![
                ("clip-000002".into(), 50.0, 53.0),
                ("clip-000001".into(), 13.0, 18.0),
                ("clip-000003".into(), 60.0, 62.0),
                ("clip-000001".into(), 20.0, 28.0),
                ("clip-000004".into(), 70.0, 74.0)
            ]
        );
        let seqs: Vec<(f64, f64)> = pieces.iter().map(|p| (p.seq_ini, p.seq_fin)).collect();
        assert_eq!(seqs, vec![(0.0, 3.0), (3.0, 8.0), (8.0, 10.0), (10.0, 18.0), (18.0, 22.0)]);
        assert_eq!(m.total_seconds(), 22.0);
        // desactivar el del medio destapa el de abajo
        let mut raw = m.raw.clone();
        raw["clips"][2]["state"] = json!("disabled");
        let m2 = V1Montaje::parse(raw, Some(&fp())).unwrap();
        let p = m2.flatten(false);
        assert_eq!(p.len(), 3);
        assert_eq!((p[1].seq_ini, p[1].seq_fin, p[1].source_ini, p[1].source_fin, p[1].clip_id.as_str()), (3.0, 18.0, 13.0, 28.0, "clip-000001"));
    }

    /// `test_gaps_are_dropped_when_flattening_but_kept_for_the_timeline` y `test_seq_source_mapping_with_repeated_source`.
    #[test]
    fn gaps_and_repeated_source_mapping() {
        let m = V1Montaje::parse(doc(vec![clip(1, "V1", 10.0, 12.0, 0.0), clip(2, "V1", 20.0, 23.0, 5.0)]), Some(&fp())).unwrap();
        assert_eq!(m.flatten(false).iter().map(|p| (p.seq_ini, p.seq_fin)).collect::<Vec<_>>(), vec![(0.0, 2.0), (2.0, 5.0)]);
        assert_eq!(
            m.flatten(true).iter().map(|p| (p.gap, p.seq_ini, p.seq_fin)).collect::<Vec<_>>(),
            vec![(false, 0.0, 2.0), (true, 2.0, 5.0), (false, 5.0, 8.0)]
        );
        let r =
            V1Montaje::parse(doc(vec![clip(1, "V1", 10.0, 14.0, 0.0), clip(2, "V1", 10.0, 14.0, 4.0), clip(3, "V1", 30.0, 32.0, 8.0)]), Some(&fp()))
                .unwrap();
        assert_eq!(r.source_to_seq(11.0), vec![1.0, 5.0]);
        assert_eq!(r.source_to_seq(31.0), vec![9.0]);
        assert!(r.source_to_seq(50.0).is_empty());
    }

    #[test]
    fn conversion_to_v2_keeps_observable_result_and_provenance() {
        let m = V1Montaje::parse(doc(vec![clip(1, "V1", 10.0, 30.0, 0.0), clip(2, "V2", 50.0, 53.0, 8.0)]), Some(&fp())).unwrap();
        let seq = m.to_sequence(&AssetId::new("a"), Rational::new(30, 1), 1920, 1080, 2);
        assert_eq!(seq.tracks.len(), 3);
        let video: Vec<&Clip> = seq.clips.iter().filter(|c| c.audio_stream.is_none()).collect();
        assert_eq!(video.len(), 3);
        assert_eq!(video[1].source, TimeRange::new(Ticks::from_seconds(50), Ticks::from_seconds(53)));
        assert_eq!(video[1].position, Ticks::from_seconds(8));
        assert_eq!(video[1].provenance.v1_clip_id.as_deref(), Some("clip-000002"));
        assert_eq!(video[2].source.start, Ticks::from_seconds(21));
        assert_eq!(seq.clips.len(), 9);
        assert_eq!(seq.extent(), Ticks::from_seconds(20));
        assert!(seq.extra.contains_key("v1_montage_profile"));
        // el audio sigue al video del tramo superior (una capa de audio por stream)
        let audio_at_9: Vec<&Clip> = seq.clips.iter().filter(|c| c.audio_stream.is_some() && c.range().contains(Ticks::from_seconds(9))).collect();
        assert_eq!(audio_at_9.len(), 2);
        assert!(audio_at_9.iter().all(|c| c.provenance.v1_clip_id.as_deref() == Some("clip-000002")));
    }

    #[test]
    fn validation_rejects_overlap_in_same_track_and_other_media() {
        assert!(V1Montaje::parse(doc(vec![clip(1, "V1", 10.0, 20.0, 0.0), clip(9, "V1", 10.0, 20.0, 5.0)]), Some(&fp())).is_err());
        let other = Fingerprint { size: 1, ..fp() };
        assert!(V1Montaje::parse(doc(vec![clip(1, "V1", 10.0, 20.0, 0.0)]), Some(&other)).is_err());
        let legacy = json!({"schema": "editorial-montaje/1", "media": {"size": 10, "hash_muestreado": "m", "inventario_sha256": "i"}, "duration_source": 100.0,
                            "clips": [{"clip_id": "clip-000004", "source_ini": 1.0, "source_fin": 3.0, "seq_ini": 0.0, "track_id": "V2"}]});
        let m = V1Montaje::parse(legacy, Some(&fp())).unwrap();
        assert_eq!(m.tracks, vec!["V1".to_string(), "V2".to_string()]);
    }
}
