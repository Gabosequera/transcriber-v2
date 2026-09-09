//! Dominio de Transcriptor V2.
//!
//! Reglas:
//! - No importa GUI, GStreamer/FFmpeg ni procesos Python.
//! - Los tipos con estabilidad de contrato (tiempo, IDs, assets, clips, tracks,
//!   items semánticos, revisión, comandos, errores) viven aquí.
//! - Toda mutación del proyecto pasa por [`commands::Command::apply`].

pub mod asset;
pub mod commands;
pub mod digest;
pub mod error;
pub mod evidence;
pub mod ids;
pub mod layers;
pub mod project;
pub mod resolve;
pub mod review;
mod semantic_edit;
pub mod time;
pub mod timeline;
pub mod validation;

pub use asset::{Asset, AssetKind, AudioStreamInfo, Fingerprint, MediaProbe, VideoStreamInfo};
pub use commands::{ClipEdge, Command, CommandEffect, MovePolicy};
pub use error::{DomainError, ErrorCode};
pub use ids::{AssetId, ClipId, ItemId, LayerId, MarkerId, SequenceId, TrackId};
pub use layers::{ItemState, LayerKind, SemanticItem, SemanticLayer};
pub use project::{PROJECT_SCHEMA, Project, Revision};
pub use resolve::{ResolvedClip, ResolvedPiece, ResolvedTimeline};
pub use time::{FLICKS_PER_SECOND, Rational, Ticks, TimeRange};
pub use timeline::{Clip, Marker, Provenance, Sequence, Track, TrackKind, Transform};
