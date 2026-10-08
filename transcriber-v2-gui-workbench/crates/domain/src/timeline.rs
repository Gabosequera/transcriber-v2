//! Pistas multimedia, clips y marcadores. Los clips componen video/audio; las
//! capas semánticas viven en [`crate::layers`] y no renderizan por defecto.

use crate::ids::{AssetId, ClipId, MarkerId, SequenceId, TrackId};
use crate::time::{Rational, Ticks, TimeRange};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    pub name: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Ganancia de pista en dB (solo audio; en video se ignora).
    #[serde(default)]
    pub gain_db: f32,
    /// Altura preferida en la UI (px lógicos). Presentación, no semántica.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

fn default_true() -> bool {
    true
}

impl Track {
    pub fn new(kind: TrackKind, name: impl Into<String>) -> Self {
        Track { id: TrackId::random(), kind, name: name.into(), muted: false, solo: false, locked: false, visible: true, gain_db: 0.0, height: None }
    }
}

/// Transformación básica de composición (video/imagen). Coordenadas
/// normalizadas respecto al lienzo: `x`/`y` desplazan el centro en fracción del
/// ancho/alto; `scale` multiplica el tamaño ajustado; `opacity` 0..1.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Transform {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default = "one")]
    pub opacity: f32,
    /// `fit` (letterbox), `fill` (recorta), `stretch` o `native` (píxeles 1:1).
    #[serde(default)]
    pub fit: FitMode,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    #[default]
    Fit,
    Fill,
    Stretch,
    Native,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { x: 0.0, y: 0.0, scale: 1.0, opacity: 1.0, fit: FitMode::Fit }
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Provenance {
    /// `user`, `ai`, `import-v1`, `split`, `duplicate`…
    #[serde(default)]
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_clip: Option<ClipId>,
    /// Identificador V1 original (`clip-000001`) cuando proviene de un montaje importado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v1_clip_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Clip multimedia: una ventana de un asset colocada en la secuencia.
///
/// `source` es tiempo **fuente** (reloj del asset, con `start_time` del
/// contenedor ya descontado: 0 = primer fotograma presentable). `position` es
/// tiempo de **secuencia**. Sin efecto de velocidad, la duración en secuencia
/// es `source.duration()`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub track_id: TrackId,
    pub asset_id: AssetId,
    pub source: TimeRange,
    pub position: Ticks,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    /// Clips que se mueven juntos (audio/video enlazado). `None` = sin enlace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_group: Option<String>,
    #[serde(default)]
    pub transform: Transform,
    /// Ganancia del clip en dB (audio).
    #[serde(default)]
    pub gain_db: f32,
    /// Para un clip de video: qué pista de audio del asset lleva (índice relativo). Solo clips en pistas de audio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_stream: Option<u32>,
    #[serde(default)]
    pub provenance: Provenance,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Clip {
    pub fn new(track_id: TrackId, asset_id: AssetId, source: TimeRange, position: Ticks) -> Self {
        Clip {
            id: ClipId::random(),
            track_id,
            asset_id,
            source,
            position,
            enabled: true,
            name: String::new(),
            link_group: None,
            transform: Transform::default(),
            gain_db: 0.0,
            audio_stream: None,
            provenance: Provenance { origin: "user".into(), ..Default::default() },
            extra: Default::default(),
        }
    }

    pub fn duration(&self) -> Ticks {
        self.source.duration()
    }

    /// Rango en tiempo de secuencia.
    pub fn range(&self) -> TimeRange {
        TimeRange::from_start_duration(self.position, self.duration())
    }

    pub fn end(&self) -> Ticks {
        self.position + self.duration()
    }

    /// Tiempo fuente que corresponde a un tiempo de secuencia dentro del clip.
    pub fn seq_to_source(&self, t: Ticks) -> Option<Ticks> {
        self.range().contains(t).then(|| self.source.start + (t - self.position))
    }

    /// Tiempo de secuencia que corresponde a un tiempo fuente dentro del clip.
    pub fn source_to_seq(&self, s: Ticks) -> Option<Ticks> {
        self.source.contains(s).then(|| self.position + (s - self.source.start))
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub id: MarkerId,
    pub range: TimeRange,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub comment: String,
}

/// Secuencia: lienzo, pistas, clips y marcadores.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Sequence {
    pub id: SequenceId,
    pub name: String,
    pub frame_rate: Rational,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    /// Orden de composición: índice mayor = más arriba (tapa a las de abajo).
    /// En la UI las pistas de video se dibujan arriba de las de audio.
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Sequence {
    pub fn new(name: impl Into<String>, frame_rate: Rational, width: u32, height: u32, sample_rate: u32) -> Self {
        Sequence {
            id: SequenceId::random(),
            name: name.into(),
            frame_rate,
            width,
            height,
            sample_rate,
            tracks: Vec::new(),
            clips: Vec::new(),
            markers: Vec::new(),
            extra: Default::default(),
        }
    }

    pub fn track(&self, id: &TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| &t.id == id)
    }

    pub fn track_mut(&mut self, id: &TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| &t.id == id)
    }

    pub fn track_index(&self, id: &TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| &t.id == id)
    }

    pub fn clip(&self, id: &ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| &c.id == id)
    }

    pub fn clip_mut(&mut self, id: &ClipId) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| &c.id == id)
    }

    /// Clips de una pista ordenados por posición.
    pub fn track_clips(&self, track: &TrackId) -> Vec<&Clip> {
        let mut v: Vec<&Clip> = self.clips.iter().filter(|c| &c.track_id == track).collect();
        v.sort_by_key(|c| (c.position, c.id.clone()));
        v
    }

    pub fn tracks_of_kind(&self, kind: TrackKind) -> Vec<&Track> {
        self.tracks.iter().filter(|t| t.kind == kind).collect()
    }

    /// Fin de la secuencia (último clip).
    pub fn extent(&self) -> Ticks {
        self.clips.iter().map(|c| c.end()).max().unwrap_or(Ticks::ZERO)
    }

    /// Bordes de clips (para snapping y navegación).
    pub fn clip_edges(&self) -> Vec<Ticks> {
        let mut v: Vec<Ticks> = self.clips.iter().flat_map(|c| [c.position, c.end()]).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Primer clip de otra pista que se solaparía con `range` en `track` (excluyendo `exclude`).
    pub fn find_overlap(&self, track: &TrackId, range: TimeRange, exclude: Option<&ClipId>) -> Option<&Clip> {
        self.clips.iter().filter(|c| &c.track_id == track && exclude != Some(&c.id)).find(|c| c.range().overlaps(&range))
    }

    /// Tiempos de secuencia donde suena el instante fuente `s` del asset (varios si se repite).
    pub fn source_to_seq(&self, asset: &AssetId, s: Ticks) -> Vec<(ClipId, Ticks)> {
        let mut out: Vec<(ClipId, Ticks)> =
            self.clips.iter().filter(|c| &c.asset_id == asset && c.enabled).filter_map(|c| c.source_to_seq(s).map(|t| (c.id.clone(), t))).collect();
        out.sort_by_key(|(_, t)| *t);
        out
    }
}
