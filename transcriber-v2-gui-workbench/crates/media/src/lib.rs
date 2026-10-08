//! Servicios multimedia de Transcriptor V2.
//!
//! Decisión (D-0003 en `implementation/decisions.md`): FFmpeg/FFprobe como
//! procesos externos para sondeo, decodificación y codificación; compositor y
//! mezclador propios en Rust, usados **por igual** por el visor y por la
//! exportación (misma `ResolvedTimeline`, mismo `TimelineRenderer`, mismo
//! `AudioMixer`). GStreamer queda como alternativa detrás de las mismas
//! interfaces (`VideoDecoder`, `AudioDecoder`).

pub mod audio;
pub mod cache;
pub mod compositor;
pub mod decoder;
pub mod export;
pub mod ffmpeg;
pub mod player;
mod process;
pub mod render;

pub use audio::{AudioDecoder, AudioMixer, AudioOutput};
pub use cache::{MediaCaches, ThumbData, ThumbHandle, WaveformData, WaveformHandle};
pub use compositor::Frame;
pub use decoder::VideoDecoder;
pub use export::{ExportJob, ExportPreset, ExportProgress, ExportRequest, ExportResult, presets};
pub use ffmpeg::{FfmpegTools, ProbeError};
pub use player::{PlayerCommand, PlayerHandle, PlayerSnapshot, PresentedFrame, SPEEDS, audio_policy};
pub use render::{AssetSource, TimelineRenderer};
