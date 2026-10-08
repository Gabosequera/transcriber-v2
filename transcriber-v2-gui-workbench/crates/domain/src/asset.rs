//! Medios importados: identidad, sondeo y localización.

use crate::ids::AssetId;
use crate::time::{Rational, Ticks};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Video,
    Audio,
    Image,
}

/// Identidad del medio, compatible con `medios.fingerprint` de V1
/// (`size`, `mtime_ns`, `hash_muestreado`, `inventario_sha256`). La identidad
/// para comparar es `size + hash_muestreado + inventario_sha256`; `mtime_ns`
/// es informativo. `hash_muestreado` es SHA-256 de tres bloques de 8 MiB
/// (inicio/medio/fin) más el tamaño: **no** es un hash completo del contenido.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_ns: Option<i128>,
    pub hash_muestreado: String,
    pub inventario_sha256: String,
}

impl Fingerprint {
    pub fn same_identity(&self, other: &Fingerprint) -> bool {
        self.size == other.size && self.hash_muestreado == other.hash_muestreado && self.inventario_sha256 == other.inventario_sha256
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct VideoStreamInfo {
    pub stream_index: u32,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Rotación declarada en metadata (0/90/180/270). Las dimensiones de
    /// presentación intercambian ancho/alto para 90/270.
    pub rotation: i32,
    pub frame_rate: Rational,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_frame_rate: Option<Rational>,
    pub time_base: Rational,
    pub start_time: Ticks,
    #[serde(default)]
    pub pix_fmt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nb_frames: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Ticks>,
    /// `true` si `r_frame_rate` y `avg_frame_rate` difieren (indicio de VFR).
    #[serde(default)]
    pub variable_frame_rate: bool,
}

impl VideoStreamInfo {
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation == 90 || self.rotation == 270 { (self.height, self.width) } else { (self.width, self.height) }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    pub stream_index: u32,
    /// Índice relativo entre pistas de audio (`0:a:N`), como `pistas[].idx` en V1.
    pub audio_index: u32,
    pub codec: String,
    pub channels: u32,
    #[serde(default)]
    pub channel_layout: String,
    pub sample_rate: u32,
    pub start_time: Ticks,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Ticks>,
    #[serde(default)]
    pub title: String,
}

/// Inventario del contenedor (resultado de ffprobe normalizado).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct MediaProbe {
    pub container: String,
    pub duration: Ticks,
    /// `start_time` del contenedor (T0 en V1: el del primer stream de video si existe).
    pub start_time: Ticks,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoStreamInfo>,
    #[serde(default)]
    pub audio: Vec<AudioStreamInfo>,
    #[serde(default)]
    pub size: u64,
}

impl MediaProbe {
    pub fn kind(&self) -> AssetKind {
        match (&self.video, self.audio.is_empty()) {
            (Some(v), _) if v.nb_frames == Some(1) || is_image_codec(&v.codec) => AssetKind::Image,
            (Some(_), _) => AssetKind::Video,
            (None, false) => AssetKind::Audio,
            (None, true) => AssetKind::Image,
        }
    }
}

pub fn is_image_codec(codec: &str) -> bool {
    matches!(codec, "png" | "mjpeg" | "jpeg" | "bmp" | "webp" | "tiff" | "gif" | "apng")
}

/// Medio del proyecto. `path` es solo un localizador; la identidad es `fingerprint`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub kind: AssetKind,
    pub name: String,
    /// Ruta portable (separadores `/`). Relativa al proyecto cuando es posible.
    pub path: String,
    pub probe: MediaProbe,
    pub fingerprint: Fingerprint,
    /// Duración por defecto de un clip de imagen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_duration: Option<Ticks>,
    #[serde(default)]
    pub missing: bool,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Asset {
    pub fn duration(&self) -> Ticks {
        match self.kind {
            AssetKind::Image => self.image_duration.unwrap_or(Ticks::from_seconds(5)),
            _ => self.probe.duration,
        }
    }

    pub fn has_video(&self) -> bool {
        self.probe.video.is_some()
    }

    pub fn has_audio(&self) -> bool {
        !self.probe.audio.is_empty()
    }

    pub fn frame_rate(&self) -> Option<Rational> {
        self.probe.video.as_ref().map(|v| v.frame_rate)
    }
}
