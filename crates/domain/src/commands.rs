//! Comandos tipados: la única vía de mutación del proyecto. GUI, teclado,
//! archivos externos y agentes construyen los mismos valores.
//!
//! Serialización: objeto plano discriminado por `"type"` (nombre de la variante).
//! Un `Batch` es todo-o-nada: se aplica sobre una copia y se confirma entera.
//!
//! Políticas explícitas:
//! - **Colisiones**: en una misma pista los clips no se solapan. `MovePolicy::Reject`
//!   falla con `E_OVERLAP`; `MovePolicy::Overwrite` recorta/borra/parte lo que tapa;
//!   `MovePolicy::Insert` desplaza los clips posteriores de la pista destino y
//!   cierra el hueco de la pista origen (comportamiento V1 `move(mode="insert")`).
//! - **Ripple**: solo cuando el comando lo pide (`RemoveClip { ripple: true }`).
//! - **Pistas bloqueadas**: cualquier mutación de sus clips falla con `E_PRECONDITION`.

use crate::asset::Asset;
use crate::error::{DomainError, DomainResult};
use crate::ids::{AssetId, ClipId, ItemId, LayerId, MarkerId, SequenceId, TrackId};
use crate::layers::{ItemState, LayerKind, SemanticItem, SemanticLayer, validate_items, validate_layer};
use crate::project::Project;
use crate::time::{Ticks, TimeRange};
use crate::timeline::{Clip, Marker, Provenance, Sequence, Track, TrackKind, Transform};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovePolicy {
    #[default]
    Reject,
    Overwrite,
    Insert,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipEdge {
    Start,
    End,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    // ---- proyecto / assets ----
    RenameProject {
        name: String,
    },
    SetSkipTrims {
        enabled: bool,
    },
    /// Validated external snapshot, routed through the same session/history.
    ReconcileProject {
        project: Box<Project>,
    },
    AttachMaster {
        master: crate::evidence::MasterEvidence,
    },
    ImportAsset {
        asset: Asset,
    },
    RemoveAsset {
        asset_id: AssetId,
    },
    RelinkAsset {
        asset_id: AssetId,
        path: String,
        asset: Option<Asset>,
    },
    SetImageDuration {
        asset_id: AssetId,
        duration: Ticks,
    },

    // ---- pistas ----
    AddTrack {
        sequence_id: Option<SequenceId>,
        kind: TrackKind,
        name: String,
        index: Option<usize>,
    },
    RemoveTrack {
        track_id: TrackId,
    },
    SetTrackProps {
        track_id: TrackId,
        name: Option<String>,
        muted: Option<bool>,
        solo: Option<bool>,
        locked: Option<bool>,
        visible: Option<bool>,
        gain_db: Option<f32>,
        height: Option<f32>,
    },
    MoveTrack {
        track_id: TrackId,
        new_index: usize,
    },

    // ---- clips ----
    AddClip {
        track_id: TrackId,
        asset_id: AssetId,
        source: TimeRange,
        position: Ticks,
        #[serde(default)]
        policy: MovePolicy,
        #[serde(default)]
        clip_id: Option<ClipId>,
        #[serde(default)]
        link_group: Option<String>,
        #[serde(default)]
        audio_stream: Option<u32>,
        #[serde(default)]
        provenance: Option<Provenance>,
    },
    /// Inserta un asset con su video y todas sus pistas de audio enlazadas.
    /// Crea las pistas que falten (una de video, una de audio por stream).
    InsertAssetLinked {
        asset_id: AssetId,
        position: Ticks,
        video_track: Option<TrackId>,
        source: Option<TimeRange>,
    },
    MoveClip {
        clip_id: ClipId,
        position: Ticks,
        track_id: Option<TrackId>,
        #[serde(default)]
        policy: MovePolicy,
    },
    /// Mueve un conjunto (selección múltiple / enlazados) el mismo delta.
    /// `policy`: `Reject` falla si colisiona; `Overwrite` recorta/parte lo que tapa;
    /// `Insert` cierra el hueco de origen y desplaza los clips posteriores de cada pista
    /// destino (el ajuste al borde más cercano se decide una vez, con el primer clip,
    /// para que video y audio enlazados no se desincronicen).
    ShiftClips {
        clip_ids: Vec<ClipId>,
        delta: Ticks,
        track_delta: i32,
        #[serde(default)]
        policy: MovePolicy,
    },
    SplitClip {
        clip_id: ClipId,
        at: Ticks,
    },
    TrimClip {
        clip_id: ClipId,
        edge: ClipEdge,
        new_time: Ticks,
    },
    RemoveClips {
        clip_ids: Vec<ClipId>,
        ripple: bool,
    },
    DuplicateClip {
        clip_id: ClipId,
        position: Ticks,
        track_id: Option<TrackId>,
    },
    PasteClips {
        clips: Vec<Clip>,
        position: Ticks,
    },
    SetClipEnabled {
        clip_ids: Vec<ClipId>,
        enabled: bool,
    },
    SetClipProps {
        clip_id: ClipId,
        name: Option<String>,
        gain_db: Option<f32>,
        transform: Option<Transform>,
    },
    LinkClips {
        clip_ids: Vec<ClipId>,
    },
    UnlinkClips {
        clip_ids: Vec<ClipId>,
    },

    // ---- marcadores ----
    AddMarker {
        range: TimeRange,
        label: String,
        color: String,
        marker_id: Option<MarkerId>,
    },
    RemoveMarker {
        marker_id: MarkerId,
    },
    SetMarker {
        marker_id: MarkerId,
        range: Option<TimeRange>,
        label: Option<String>,
        comment: Option<String>,
        #[serde(default)]
        color: Option<String>,
    },

    // ---- capas semánticas ----
    CreateLayer {
        asset_id: AssetId,
        kind: LayerKind,
        name: String,
        color: Option<String>,
        layer_id: Option<LayerId>,
    },
    DeleteLayer {
        layer_id: LayerId,
    },
    SetLayerProps {
        layer_id: LayerId,
        name: Option<String>,
        color: Option<String>,
        visible: Option<bool>,
        locked: Option<bool>,
    },
    SetLayerOrder {
        layer_ids: Vec<LayerId>,
    },
    AddItem {
        layer_id: LayerId,
        item: SemanticItem,
    },
    PasteItems {
        layer_id: LayerId,
        items: Vec<SemanticItem>,
        position: Ticks,
    },
    SetItemState {
        layer_id: LayerId,
        item_ids: Vec<ItemId>,
        state: ItemState,
    },
    SetItemProps {
        layer_id: LayerId,
        item_id: ItemId,
        label: Option<String>,
        comment: Option<String>,
        ranges: Option<Vec<TimeRange>>,
    },
    SetItemStructure {
        layer_id: LayerId,
        item_id: ItemId,
        parent_id: Option<ItemId>,
        ranges: Vec<TimeRange>,
    },
    DeleteItems {
        layer_id: LayerId,
        item_ids: Vec<ItemId>,
    },
    /// Reemplaza el contenido validado de una capa (import/merge). Respeta tombstones.
    ReplaceLayer {
        layer: SemanticLayer,
    },

    // ---- secuencias ----
    /// Añade una secuencia completa (importación). Falla si el ID ya existe.
    AddSequence {
        sequence: Sequence,
        activate: bool,
    },
    SetActiveSequence {
        sequence_id: SequenceId,
    },

    // ---- composición ----
    Batch {
        label: String,
        commands: Vec<Command>,
    },
}

/// Resultado de aplicar un comando: IDs afectados y descripción humana.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct CommandEffect {
    pub label: String,
    pub affected: Vec<String>,
    #[serde(default)]
    pub created: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl CommandEffect {
    fn new(label: impl Into<String>) -> Self {
        CommandEffect { label: label.into(), ..Default::default() }
    }
    fn touch(mut self, id: impl ToString) -> Self {
        self.affected.push(id.to_string());
        self
    }
    fn create(mut self, id: impl ToString) -> Self {
        self.created.push(id.to_string());
        self.affected.push(id.to_string());
        self
    }
}

impl Command {
    /// Etiqueta corta para historial/menú.
    pub fn label(&self) -> String {
        match self {
            Command::RenameProject { .. } => "Renombrar proyecto".into(),
            Command::SetSkipTrims { .. } => "Saltar recortes al reproducir".into(),
            Command::ReconcileProject { .. } => "Reconciliar archivo externo".into(),
            Command::AttachMaster { .. } => "Incorporar master original".into(),
            Command::ImportAsset { asset } => format!("Importar {}", asset.name),
            Command::RemoveAsset { .. } => "Quitar medio".into(),
            Command::RelinkAsset { .. } => "Reenlazar medio".into(),
            Command::SetImageDuration { .. } => "Duración de imagen".into(),
            Command::AddTrack { name, .. } => format!("Añadir pista {name}"),
            Command::RemoveTrack { .. } => "Quitar pista".into(),
            Command::SetTrackProps { .. } => "Propiedades de pista".into(),
            Command::MoveTrack { .. } => "Mover pista".into(),
            Command::AddClip { .. } => "Añadir clip".into(),
            Command::InsertAssetLinked { .. } => "Insertar medio".into(),
            Command::MoveClip { .. } => "Mover clip".into(),
            Command::ShiftClips { policy, .. } => match policy {
                MovePolicy::Insert => "Insertar clips".into(),
                MovePolicy::Overwrite => "Mover clips (sobrescribir)".into(),
                MovePolicy::Reject => "Mover clips".into(),
            },
            Command::SplitClip { .. } => "Dividir clip".into(),
            Command::TrimClip { .. } => "Recortar clip".into(),
            Command::RemoveClips { ripple, .. } => {
                if *ripple {
                    "Eliminar con ripple".into()
                } else {
                    "Eliminar clip".into()
                }
            }
            Command::DuplicateClip { .. } => "Duplicar clip".into(),
            Command::PasteClips { .. } => "Pegar clips".into(),
            Command::SetClipEnabled { enabled, .. } => {
                if *enabled {
                    "Activar clip".into()
                } else {
                    "Desactivar clip".into()
                }
            }
            Command::SetClipProps { .. } => "Propiedades de clip".into(),
            Command::LinkClips { .. } => "Vincular clips".into(),
            Command::UnlinkClips { .. } => "Desvincular clips".into(),
            Command::AddMarker { .. } => "Añadir marcador".into(),
            Command::RemoveMarker { .. } => "Quitar marcador".into(),
            Command::SetMarker { .. } => "Editar marcador".into(),
            Command::CreateLayer { name, .. } => format!("Crear capa {name}"),
            Command::DeleteLayer { .. } => "Borrar capa".into(),
            Command::SetLayerProps { .. } => "Propiedades de capa".into(),
            Command::SetLayerOrder { .. } => "Ordenar carriles".into(),
            Command::AddItem { .. } => "Añadir tramo".into(),
            Command::PasteItems { .. } => "Pegar items editoriales".into(),
            Command::SetItemState { state, .. } => match state {
                ItemState::Accepted => "Aceptar".into(),
                ItemState::Disabled => "Desactivar".into(),
                ItemState::Proposed => "Activar".into(),
            },
            Command::SetItemProps { .. } => "Editar tramo".into(),
            Command::SetItemStructure { .. } => "Editar rangos y jerarquía".into(),
            Command::DeleteItems { .. } => "Borrar tramos".into(),
            Command::ReplaceLayer { .. } => "Reemplazar capa".into(),
            Command::AddSequence { sequence, .. } => format!("Añadir secuencia {}", sequence.name),
            Command::SetActiveSequence { .. } => "Cambiar de secuencia".into(),
            Command::Batch { label, .. } => label.clone(),
        }
    }

    /// `true` si el comando no cambia el proyecto de forma persistente (ninguno por ahora:
    /// transporte y selección no son comandos de dominio).
    pub fn is_persistent(&self) -> bool {
        true
    }

    /// Aplica el comando sobre `project`. En caso de error el proyecto puede
    /// quedar parcialmente modificado: el llamador (sesión) aplica sobre una
    /// copia y descarta si falla.
    pub fn apply(&self, project: &mut Project) -> DomainResult<CommandEffect> {
        match self {
            Command::RenameProject { name } => {
                if name.trim().is_empty() {
                    return Err(DomainError::invalid("el nombre no puede estar vacío"));
                }
                project.name = name.trim().to_string();
                Ok(CommandEffect::new(self.label()))
            }
            Command::SetSkipTrims { enabled } => {
                project.settings.skip_trims_on_play = *enabled;
                Ok(CommandEffect::new(self.label()))
            }
            Command::ReconcileProject { project: candidate } => {
                if project.project_id != candidate.project_id {
                    return Err(DomainError::precondition("el archivo externo pertenece a otro proyecto"));
                }
                candidate.validate()?;
                *project = candidate.as_ref().clone();
                Ok(CommandEffect::new(self.label()))
            }
            Command::AttachMaster { master } => {
                master.validate(project)?;
                if let Some(existing) = project.masters.iter().find(|m| m.asset_id == master.asset_id) {
                    if existing != master {
                        return Err(DomainError::precondition("el master original está protegido; importa el nuevo análisis como otra versión"));
                    }
                } else {
                    project.masters.push(master.clone());
                }
                Ok(CommandEffect::new(self.label()).touch(&master.asset_id))
            }
            Command::ImportAsset { asset } => {
                if project.assets.iter().any(|a| a.id == asset.id) {
                    return Err(DomainError::invalid(format!("el asset {} ya existe", asset.id)));
                }
                project.assets.push(asset.clone());
                Ok(CommandEffect::new(self.label()).create(&asset.id))
            }
            Command::RemoveAsset { asset_id } => {
                let used = project.sequences.iter().flat_map(|s| s.clips.iter()).any(|c| &c.asset_id == asset_id);
                if used {
                    return Err(DomainError::precondition("el medio está en uso en la secuencia; quita sus clips antes"));
                }
                let before = project.assets.len();
                project.assets.retain(|a| &a.id != asset_id);
                if project.assets.len() == before {
                    return Err(DomainError::not_found("asset", asset_id));
                }
                let removed: Vec<_> = project.layers.iter().filter(|l| &l.asset_id == asset_id).map(|l| l.layer_id.clone()).collect();
                project.layers.retain(|l| &l.asset_id != asset_id);
                project.layer_order.retain(|id| !removed.contains(id));
                Ok(CommandEffect::new(self.label()).touch(asset_id))
            }
            Command::RelinkAsset { asset_id, path, asset } => {
                if asset.is_none() {
                    return Err(DomainError::precondition("reenlazar requiere el probe y fingerprint del archivo candidato"));
                }
                let a = project.asset_mut(asset_id).ok_or_else(|| DomainError::not_found("asset", asset_id))?;
                if let Some(new) = asset {
                    if !new.fingerprint.same_identity(&a.fingerprint) {
                        return Err(DomainError::precondition("el archivo elegido no coincide con la identidad del medio original")
                            .with("expected", a.fingerprint.hash_muestreado.clone())
                            .with("actual", new.fingerprint.hash_muestreado.clone()));
                    }
                    a.probe = new.probe.clone();
                    a.fingerprint = new.fingerprint.clone();
                }
                a.path = path.clone();
                a.missing = false;
                Ok(CommandEffect::new(self.label()).touch(asset_id))
            }
            Command::SetImageDuration { asset_id, duration } => {
                let a = project.asset_mut(asset_id).ok_or_else(|| DomainError::not_found("asset", asset_id))?;
                if duration.0 <= 0 {
                    return Err(DomainError::invalid("la duración debe ser positiva"));
                }
                a.image_duration = Some(*duration);
                Ok(CommandEffect::new(self.label()).touch(asset_id))
            }
            Command::AddTrack { sequence_id, kind, name, index } => {
                let seq = match sequence_id {
                    Some(id) => project.sequence_mut(id).ok_or_else(|| DomainError::not_found("secuencia", id))?,
                    None => project.active_mut().ok_or_else(|| DomainError::invalid("no hay secuencia activa"))?,
                };
                let track = Track::new(*kind, name.clone());
                let id = track.id.clone();
                match index {
                    Some(i) => seq.tracks.insert((*i).min(seq.tracks.len()), track),
                    None => match kind {
                        // video nuevo arriba de los videos (después del último de video); audio al final
                        TrackKind::Video => {
                            let pos = seq.tracks.iter().rposition(|t| t.kind == TrackKind::Video).map(|p| p + 1).unwrap_or(0);
                            seq.tracks.insert(pos, track);
                        }
                        TrackKind::Audio => seq.tracks.push(track),
                    },
                }
                Ok(CommandEffect::new(self.label()).create(id))
            }
            Command::RemoveTrack { track_id } => {
                let seq = seq_of_track_mut(project, track_id)?;
                if seq.clips.iter().any(|c| &c.track_id == track_id) {
                    return Err(DomainError::precondition("la pista tiene clips; elimínalos antes"));
                }
                seq.tracks.retain(|t| &t.id != track_id);
                Ok(CommandEffect::new(self.label()).touch(track_id))
            }
            Command::SetTrackProps { track_id, name, muted, solo, locked, visible, gain_db, height } => {
                let seq = seq_of_track_mut(project, track_id)?;
                let t = seq.track_mut(track_id).ok_or_else(|| DomainError::not_found("pista", track_id))?;
                if let Some(v) = name {
                    t.name = v.clone();
                }
                if let Some(v) = muted {
                    t.muted = *v;
                }
                if let Some(v) = solo {
                    t.solo = *v;
                }
                if let Some(v) = locked {
                    t.locked = *v;
                }
                if let Some(v) = visible {
                    t.visible = *v;
                }
                if let Some(v) = gain_db {
                    if !v.is_finite() || *v < -96.0 || *v > 24.0 {
                        return Err(DomainError::invalid("ganancia fuera de rango (-96..24 dB)"));
                    }
                    t.gain_db = *v;
                }
                if let Some(v) = height {
                    t.height = Some(v.clamp(24.0, 400.0));
                }
                Ok(CommandEffect::new(self.label()).touch(track_id))
            }
            Command::MoveTrack { track_id, new_index } => {
                let seq = seq_of_track_mut(project, track_id)?;
                let from = seq.track_index(track_id).ok_or_else(|| DomainError::not_found("pista", track_id))?;
                let kind = seq.tracks[from].kind;
                let t = seq.tracks.remove(from);
                let to = (*new_index).min(seq.tracks.len());
                seq.tracks.insert(to, t);
                // las pistas de un mismo tipo permanecen contiguas: validación simple
                let kinds: Vec<TrackKind> = seq.tracks.iter().map(|t| t.kind).collect();
                let first_audio = kinds.iter().position(|k| *k == TrackKind::Audio);
                let last_video = kinds.iter().rposition(|k| *k == TrackKind::Video);
                if let (Some(fa), Some(lv)) = (first_audio, last_video)
                    && fa < lv
                {
                    let t = seq.tracks.remove(to);
                    seq.tracks.insert(from, t);
                    return Err(DomainError::precondition(format!(
                        "una pista de {} no puede colocarse entre pistas del otro tipo",
                        if kind == TrackKind::Video { "video" } else { "audio" }
                    )));
                }
                Ok(CommandEffect::new(self.label()).touch(track_id))
            }
            Command::AddClip { track_id, asset_id, source, position, policy, clip_id, link_group, audio_stream, provenance } => {
                let asset = project.asset(asset_id).ok_or_else(|| DomainError::not_found("asset", asset_id))?.clone();
                validate_source(&asset, source)?;
                let seq = seq_of_track_mut(project, track_id)?;
                let track = seq.track(track_id).ok_or_else(|| DomainError::not_found("pista", track_id))?;
                ensure_unlocked(track)?;
                check_track_compat(track, &asset, *audio_stream)?;
                if position.is_negative() {
                    return Err(DomainError::out_of_range("la posición no puede ser negativa"));
                }
                let mut clip = Clip::new(track_id.clone(), asset_id.clone(), *source, *position);
                if let Some(id) = clip_id {
                    if seq.clip(id).is_some() {
                        return Err(DomainError::invalid(format!("el clip {id} ya existe")));
                    }
                    clip.id = id.clone();
                }
                clip.name = asset.name.clone();
                clip.link_group = link_group.clone();
                clip.audio_stream = *audio_stream;
                if let Some(p) = provenance {
                    clip.provenance = p.clone();
                }
                place_clip(seq, clip.clone(), *policy)?;
                Ok(CommandEffect::new(self.label()).create(&clip.id))
            }
            Command::InsertAssetLinked { asset_id, position, video_track, source } => {
                let asset = project.asset(asset_id).ok_or_else(|| DomainError::not_found("asset", asset_id))?.clone();
                let source = source.unwrap_or(TimeRange::new(Ticks::ZERO, asset.duration()));
                validate_source(&asset, &source)?;
                let seq = project.active_mut().ok_or_else(|| DomainError::invalid("no hay secuencia activa"))?;
                let mut effect = CommandEffect::new(self.label());
                let group = if asset.has_video() && asset.has_audio() { Some(format!("link-{}", crate::ids::random_hex12())) } else { None };
                let provenance = Provenance { origin: "user".into(), created_at: Some(crate::project::now_iso()), ..Default::default() };
                // video (o imagen)
                if asset.has_video() {
                    let vt = match video_track {
                        Some(t) => t.clone(),
                        None => first_free_track(seq, TrackKind::Video, TimeRange::from_start_duration(*position, source.duration())).unwrap_or_else(
                            || {
                                let n = seq.tracks_of_kind(TrackKind::Video).len() + 1;
                                let t = Track::new(TrackKind::Video, format!("V{n}"));
                                let id = t.id.clone();
                                let pos = seq.tracks.iter().rposition(|t| t.kind == TrackKind::Video).map(|p| p + 1).unwrap_or(0);
                                seq.tracks.insert(pos, t);
                                id
                            },
                        ),
                    };
                    let mut clip = Clip::new(vt, asset.id.clone(), source, *position);
                    clip.name = asset.name.clone();
                    clip.link_group = group.clone();
                    clip.provenance = provenance.clone();
                    let id = clip.id.clone();
                    place_clip(seq, clip, MovePolicy::Reject)?;
                    effect = effect.create(id);
                }
                for (i, _stream) in asset.probe.audio.iter().enumerate() {
                    let range = TimeRange::from_start_duration(*position, source.duration());
                    let at = first_free_track_skipping(seq, TrackKind::Audio, range, i).unwrap_or_else(|| {
                        let n = seq.tracks_of_kind(TrackKind::Audio).len() + 1;
                        let t = Track::new(TrackKind::Audio, format!("A{n}"));
                        let id = t.id.clone();
                        seq.tracks.push(t);
                        id
                    });
                    let mut clip = Clip::new(at, asset.id.clone(), source, *position);
                    clip.name = if asset.probe.audio.len() > 1 { format!("{} · A{}", asset.name, i + 1) } else { asset.name.clone() };
                    clip.link_group = group.clone();
                    clip.audio_stream = Some(i as u32);
                    clip.provenance = provenance.clone();
                    let id = clip.id.clone();
                    place_clip(seq, clip, MovePolicy::Reject)?;
                    effect = effect.create(id);
                }
                if effect.created.is_empty() {
                    return Err(DomainError::precondition("el medio no tiene video ni audio"));
                }
                Ok(effect)
            }
            Command::MoveClip { clip_id, position, track_id, policy } => {
                let seq = seq_of_clip_mut(project, clip_id)?;
                let mut clip = seq.clip(clip_id).cloned().ok_or_else(|| DomainError::not_found("clip", clip_id))?;
                let origin_track = seq.track(&clip.track_id).cloned().ok_or_else(|| DomainError::not_found("pista", &clip.track_id))?;
                ensure_unlocked(&origin_track)?;
                let target_id = track_id.clone().unwrap_or(clip.track_id.clone());
                let target = seq.track(&target_id).cloned().ok_or_else(|| DomainError::not_found("pista", &target_id))?;
                ensure_unlocked(&target)?;
                if target.kind != origin_track.kind {
                    return Err(DomainError::precondition("no se puede mover un clip entre pistas de distinto tipo"));
                }
                if position.is_negative() {
                    return Err(DomainError::out_of_range("la posición no puede ser negativa"));
                }
                let origin_range = clip.range();
                let length = clip.duration();
                seq.clips.retain(|c| &c.id != clip_id);
                let mut target_pos = *position;
                if *policy == MovePolicy::Insert {
                    // la pista origen cierra el hueco
                    for c in seq.clips.iter_mut().filter(|c| c.track_id == origin_track.id) {
                        if c.position >= origin_range.end {
                            c.position -= length;
                        }
                    }
                    // en la pista destino: dentro de un clip → al borde más cercano; los posteriores se desplazan
                    let mut snapped = target_pos;
                    for c in seq.clips.iter().filter(|c| c.track_id == target_id) {
                        let r = c.range();
                        if r.start < target_pos && target_pos < r.end {
                            snapped = if target_pos - r.start < r.end - target_pos { r.start } else { r.end };
                            break;
                        }
                    }
                    target_pos = snapped;
                    for c in seq.clips.iter_mut().filter(|c| c.track_id == target_id) {
                        if c.position >= target_pos {
                            c.position += length;
                        }
                    }
                }
                clip.track_id = target_id;
                clip.position = target_pos;
                let policy = if *policy == MovePolicy::Insert { MovePolicy::Reject } else { *policy };
                place_clip(seq, clip, policy)?;
                Ok(CommandEffect::new(self.label()).touch(clip_id))
            }
            Command::ShiftClips { clip_ids, delta, track_delta, policy } => {
                if clip_ids.is_empty() {
                    return Err(DomainError::invalid("no hay clips que mover"));
                }
                let seq = seq_of_clip_mut(project, &clip_ids[0])?;
                let mut moving: Vec<Clip> = Vec::new();
                for id in clip_ids {
                    if moving.iter().any(|c| &c.id == id) {
                        continue;
                    }
                    let c = seq.clip(id).cloned().ok_or_else(|| DomainError::not_found("clip", id))?;
                    let t = seq.track(&c.track_id).ok_or_else(|| DomainError::not_found("pista", &c.track_id))?;
                    ensure_unlocked(t)?;
                    moving.push(c);
                }
                // acotar delta para que ninguno quede antes de 0
                let min_pos = moving.iter().map(|c| c.position).min().unwrap_or(Ticks::ZERO);
                let mut delta = (*delta).max(-min_pos);
                // cambio de pista: misma clase, dentro del rango de pistas de esa clase
                let mut new_tracks: Vec<TrackId> = Vec::new();
                for c in &moving {
                    let kind = seq.track(&c.track_id).map(|t| t.kind).unwrap();
                    let same_kind: Vec<&Track> = seq.tracks_of_kind(kind);
                    let idx = same_kind.iter().position(|t| t.id == c.track_id).unwrap() as i32;
                    let ni = idx + *track_delta;
                    if ni < 0 || ni as usize >= same_kind.len() {
                        return Err(DomainError::out_of_range("no hay pista destino en esa dirección"));
                    }
                    let target = same_kind[ni as usize];
                    ensure_unlocked(target)?;
                    new_tracks.push(target.id.clone());
                }
                let ids: Vec<ClipId> = moving.iter().map(|c| c.id.clone()).collect();
                let originals = moving.clone();
                seq.clips.retain(|c| !ids.contains(&c.id));
                if *policy == MovePolicy::Insert {
                    // 1) las pistas de origen cierran el hueco de cada clip movido (de atrás hacia delante)
                    let mut by_pos: Vec<&Clip> = originals.iter().collect();
                    by_pos.sort_by_key(|c| std::cmp::Reverse(c.position));
                    for r in by_pos {
                        let len = r.duration();
                        for c in seq.clips.iter_mut().filter(|c| c.track_id == r.track_id) {
                            if c.position >= r.end() {
                                c.position -= len;
                            }
                        }
                    }
                    // 2) punto de inserción: el primer clip (el agarrado) decide el ajuste al borde más cercano
                    let lead = &originals[0];
                    let lead_track = &new_tracks[0];
                    let wanted = lead.position + delta;
                    let mut snapped = wanted;
                    for c in seq.clips.iter().filter(|c| &c.track_id == lead_track) {
                        let r = c.range();
                        if r.start < wanted && wanted < r.end {
                            snapped = if wanted - r.start < r.end - wanted { r.start } else { r.end };
                            break;
                        }
                    }
                    delta = snapped - lead.position;
                    // 3) en cada pista destino, los clips a partir del bloque insertado se desplazan por su longitud
                    let mut tracks: Vec<TrackId> = new_tracks.clone();
                    tracks.sort();
                    tracks.dedup();
                    for t in tracks {
                        let block: Vec<(Ticks, Ticks)> = originals
                            .iter()
                            .zip(new_tracks.iter())
                            .filter(|(_, nt)| *nt == &t)
                            .map(|(c, _)| (c.position + delta, c.end() + delta))
                            .collect();
                        let Some(start) = block.iter().map(|b| b.0).min() else { continue };
                        let end = block.iter().map(|b| b.1).max().unwrap();
                        let len = end - start;
                        for c in seq.clips.iter_mut().filter(|c| c.track_id == t) {
                            if c.position >= start {
                                c.position += len;
                            }
                        }
                    }
                }
                for (c, t) in moving.iter_mut().zip(new_tracks) {
                    c.track_id = t;
                    c.position += delta;
                }
                // los clips movidos no pueden solaparse entre sí
                for (i, c) in moving.iter().enumerate() {
                    for other in moving.iter().skip(i + 1) {
                        if other.track_id == c.track_id && other.range().overlaps(&c.range()) {
                            return Err(DomainError::overlap("los clips movidos se solapan entre sí"));
                        }
                    }
                }
                let mut effect = CommandEffect::new(self.label());
                let place_policy = if *policy == MovePolicy::Overwrite { MovePolicy::Overwrite } else { MovePolicy::Reject };
                for c in moving {
                    effect = effect.touch(&c.id);
                    place_clip(seq, c, place_policy)?;
                }
                Ok(effect)
            }
            Command::SplitClip { clip_id, at } => {
                let frame = project.active().map(|s| s.frame_rate).unwrap_or_default().frame_duration();
                let seq = seq_of_clip_mut(project, clip_id)?;
                let clip = seq.clip_mut(clip_id).ok_or_else(|| DomainError::not_found("clip", clip_id))?;
                let r = clip.range();
                if *at < r.start + frame || *at > r.end - frame {
                    return Err(DomainError::out_of_range("el punto de corte debe caer dentro del clip (al menos un fotograma a cada lado)"));
                }
                let offset = *at - r.start;
                let mut right = clip.clone();
                right.id = ClipId::random();
                right.source.start = clip.source.start + offset;
                right.position = *at;
                // Las mitades derechas de un grupo enlazado dividido en el mismo instante
                // comparten un grupo nuevo (determinista): video y audio siguen unidos entre
                // sí, pero independientes de las mitades izquierdas.
                right.link_group = clip.link_group.as_ref().map(|g| format!("{g}@{}", at.0));
                right.provenance = Provenance {
                    origin: "split".into(),
                    parent_clip: Some(clip.id.clone()),
                    v1_clip_id: None,
                    created_at: Some(crate::project::now_iso()),
                };
                clip.source.end = clip.source.start + offset;
                let right_id = right.id.clone();
                seq.clips.push(right);
                Ok(CommandEffect::new(self.label()).touch(clip_id).create(right_id))
            }
            Command::TrimClip { clip_id, edge, new_time } => {
                let frame = project.active().map(|s| s.frame_rate).unwrap_or_default().frame_duration();
                let assets = project.assets.clone();
                let seq = seq_of_clip_mut(project, clip_id)?;
                let clip = seq.clip(clip_id).cloned().ok_or_else(|| DomainError::not_found("clip", clip_id))?;
                let track = seq.track(&clip.track_id).ok_or_else(|| DomainError::not_found("pista", &clip.track_id))?;
                ensure_unlocked(track)?;
                let asset = assets.iter().find(|a| a.id == clip.asset_id).ok_or_else(|| DomainError::not_found("asset", &clip.asset_id))?;
                let media_len = asset.duration();
                let mut new = clip.clone();
                match edge {
                    ClipEdge::Start => {
                        let delta = *new_time - clip.position;
                        let low = (-clip.source.start).max(Ticks::ZERO - clip.position);
                        let high = clip.duration() - frame;
                        let delta = delta.clamp(low, high);
                        new.position = clip.position + delta;
                        new.source.start = clip.source.start + delta;
                    }
                    ClipEdge::End => {
                        let delta = *new_time - clip.end();
                        let low = -(clip.duration() - frame);
                        let high = if asset.kind == crate::asset::AssetKind::Image { Ticks::from_seconds(3600) } else { media_len - clip.source.end };
                        let delta = delta.clamp(low, high);
                        new.source.end = clip.source.end + delta;
                    }
                }
                if let Some(o) = seq.find_overlap(&new.track_id, new.range(), Some(clip_id)) {
                    return Err(DomainError::overlap(format!("el recorte pisaría al clip {}", o.id)));
                }
                *seq.clip_mut(clip_id).unwrap() = new;
                Ok(CommandEffect::new(self.label()).touch(clip_id))
            }
            Command::RemoveClips { clip_ids, ripple } => {
                if clip_ids.is_empty() {
                    return Err(DomainError::invalid("no hay clips que eliminar"));
                }
                let seq = seq_of_clip_mut(project, &clip_ids[0])?;
                let mut removed: Vec<Clip> = Vec::new();
                for id in clip_ids {
                    let c = seq.clip(id).cloned().ok_or_else(|| DomainError::not_found("clip", id))?;
                    let t = seq.track(&c.track_id).ok_or_else(|| DomainError::not_found("pista", &c.track_id))?;
                    ensure_unlocked(t)?;
                    removed.push(c);
                }
                seq.clips.retain(|c| !clip_ids.contains(&c.id));
                if *ripple {
                    // ripple por pista: los clips posteriores de cada pista tocada cierran el hueco
                    removed.sort_by_key(|c| std::cmp::Reverse(c.position));
                    for r in &removed {
                        let len = r.duration();
                        for c in seq.clips.iter_mut().filter(|c| c.track_id == r.track_id) {
                            if c.position >= r.end() {
                                c.position -= len;
                            }
                        }
                    }
                }
                let mut effect = CommandEffect::new(self.label());
                for r in removed {
                    effect = effect.touch(&r.id);
                }
                Ok(effect)
            }
            Command::DuplicateClip { clip_id, position, track_id } => {
                let seq = seq_of_clip_mut(project, clip_id)?;
                let clip = seq.clip(clip_id).cloned().ok_or_else(|| DomainError::not_found("clip", clip_id))?;
                let mut dup = clip.clone();
                dup.id = ClipId::random();
                dup.position = *position;
                dup.link_group = None;
                if let Some(t) = track_id {
                    let target = seq.track(t).ok_or_else(|| DomainError::not_found("pista", t))?;
                    let origin_kind = seq.track(&clip.track_id).map(|t| t.kind).unwrap();
                    if target.kind != origin_kind {
                        return Err(DomainError::precondition("la pista destino es de otro tipo"));
                    }
                    dup.track_id = t.clone();
                }
                dup.provenance = Provenance {
                    origin: "duplicate".into(),
                    parent_clip: Some(clip.id.clone()),
                    v1_clip_id: None,
                    created_at: Some(crate::project::now_iso()),
                };
                let id = dup.id.clone();
                place_clip(seq, dup, MovePolicy::Reject)?;
                Ok(CommandEffect::new(self.label()).create(id))
            }
            Command::SetClipEnabled { clip_ids, enabled } => {
                let mut effect = CommandEffect::new(self.label());
                for id in clip_ids {
                    let seq = seq_of_clip_mut(project, id)?;
                    let c = seq.clip_mut(id).ok_or_else(|| DomainError::not_found("clip", id))?;
                    c.enabled = *enabled;
                    effect = effect.touch(id);
                }
                Ok(effect)
            }
            Command::SetClipProps { clip_id, name, gain_db, transform } => {
                let seq = seq_of_clip_mut(project, clip_id)?;
                let c = seq.clip_mut(clip_id).ok_or_else(|| DomainError::not_found("clip", clip_id))?;
                if let Some(v) = name {
                    c.name = v.clone();
                }
                if let Some(v) = gain_db {
                    if !v.is_finite() || *v < -96.0 || *v > 24.0 {
                        return Err(DomainError::invalid("ganancia fuera de rango (-96..24 dB)"));
                    }
                    c.gain_db = *v;
                }
                if let Some(t) = transform {
                    if !(t.opacity.is_finite() && (0.0..=1.0).contains(&t.opacity)) || !(t.scale.is_finite() && t.scale > 0.0 && t.scale <= 16.0) {
                        return Err(DomainError::invalid("transformación inválida (opacidad 0..1, escala 0..16)"));
                    }
                    c.transform = *t;
                }
                Ok(CommandEffect::new(self.label()).touch(clip_id))
            }
            Command::LinkClips { clip_ids } => {
                if clip_ids.len() < 2 {
                    return Err(DomainError::invalid("vincula al menos dos clips"));
                }
                let group = format!("link-{}", crate::ids::random_hex12());
                let mut effect = CommandEffect::new(self.label());
                for id in clip_ids {
                    let seq = seq_of_clip_mut(project, id)?;
                    let c = seq.clip_mut(id).ok_or_else(|| DomainError::not_found("clip", id))?;
                    c.link_group = Some(group.clone());
                    effect = effect.touch(id);
                }
                Ok(effect)
            }
            Command::UnlinkClips { clip_ids } => {
                let mut effect = CommandEffect::new(self.label());
                for id in clip_ids {
                    let seq = seq_of_clip_mut(project, id)?;
                    let c = seq.clip_mut(id).ok_or_else(|| DomainError::not_found("clip", id))?;
                    c.link_group = None;
                    effect = effect.touch(id);
                }
                Ok(effect)
            }
            Command::AddMarker { range, label, color, marker_id } => {
                if !range.is_ordered() || range.start.is_negative() {
                    return Err(DomainError::out_of_range("rango de marcador inválido"));
                }
                let seq = project.active_mut().ok_or_else(|| DomainError::invalid("no hay secuencia activa"))?;
                let m = Marker {
                    id: marker_id.clone().unwrap_or_else(MarkerId::random),
                    range: *range,
                    label: label.clone(),
                    color: color.clone(),
                    comment: String::new(),
                };
                if seq.markers.iter().any(|x| x.id == m.id) {
                    return Err(DomainError::invalid("el marcador ya existe"));
                }
                let id = m.id.clone();
                seq.markers.push(m);
                Ok(CommandEffect::new(self.label()).create(id))
            }
            Command::RemoveMarker { marker_id } => {
                let seq = project.active_mut().ok_or_else(|| DomainError::invalid("no hay secuencia activa"))?;
                let before = seq.markers.len();
                seq.markers.retain(|m| &m.id != marker_id);
                if seq.markers.len() == before {
                    return Err(DomainError::not_found("marcador", marker_id));
                }
                Ok(CommandEffect::new(self.label()).touch(marker_id))
            }
            Command::SetMarker { marker_id, range, label, comment, color } => {
                let seq = project.active_mut().ok_or_else(|| DomainError::invalid("no hay secuencia activa"))?;
                let m = seq.markers.iter_mut().find(|m| &m.id == marker_id).ok_or_else(|| DomainError::not_found("marcador", marker_id))?;
                if let Some(r) = range {
                    if !r.is_ordered() || r.start.is_negative() {
                        return Err(DomainError::out_of_range("rango de marcador inválido"));
                    }
                    m.range = *r;
                }
                if let Some(l) = label {
                    m.label = l.clone();
                }
                if let Some(c) = comment {
                    m.comment = c.clone();
                }
                if let Some(c) = color {
                    if !crate::layers::is_hex_color(c) {
                        return Err(DomainError::invalid("color inválido (#RRGGBB)"));
                    }
                    m.color = c.clone();
                }
                Ok(CommandEffect::new(self.label()).touch(marker_id))
            }
            Command::CreateLayer { asset_id, kind, name, color, layer_id } => {
                project.asset(asset_id).ok_or_else(|| DomainError::not_found("asset", asset_id))?;
                let mut layer = SemanticLayer::new(asset_id.clone(), kind.clone(), name.clone());
                if let Some(c) = color {
                    layer.color = c.clone();
                }
                if let Some(id) = layer_id {
                    layer.layer_id = id.clone();
                }
                if project.layer(&layer.layer_id).is_some() {
                    return Err(DomainError::invalid(format!("la capa {} ya existe", layer.layer_id)));
                }
                validate_layer(&layer, Ticks::MAX)?;
                let id = layer.layer_id.clone();
                project.layers.push(layer);
                project.layer_order.push(id.clone());
                Ok(CommandEffect::new(self.label()).create(id))
            }
            Command::DeleteLayer { layer_id } => {
                let l = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                ensure_layer_editable(l)?;
                if l.deleted {
                    return Err(DomainError::not_found("capa", layer_id));
                }
                l.deleted = true;
                l.revision = l.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                Ok(CommandEffect::new(self.label()).touch(layer_id))
            }
            Command::SetLayerProps { layer_id, name, color, visible, locked } => {
                let l = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                if let Some(n) = name {
                    if n.trim().is_empty() {
                        return Err(DomainError::invalid("falta el nombre de la capa"));
                    }
                    l.name = n.clone();
                }
                if let Some(c) = color {
                    if !crate::layers::is_hex_color(c) {
                        return Err(DomainError::invalid("color inválido (#RRGGBB)"));
                    }
                    l.color = c.clone();
                }
                if let Some(v) = visible {
                    l.visible = *v;
                }
                if let Some(v) = locked {
                    l.locked = *v;
                }
                l.revision = l.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                Ok(CommandEffect::new(self.label()).touch(layer_id))
            }
            Command::SetLayerOrder { layer_ids } => {
                for id in layer_ids {
                    project.layer(id).ok_or_else(|| DomainError::not_found("capa", id))?;
                }
                project.layer_order = layer_ids.clone();
                Ok(CommandEffect::new(self.label()))
            }
            Command::AddItem { layer_id, item } => {
                let duration = layer_asset_duration(project, layer_id)?;
                let l = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                ensure_layer_editable(l)?;
                if l.deleted_item_ids.contains(&item.item_id) {
                    return Err(DomainError::precondition(format!("el item {} fue borrado (tombstone)", item.item_id)));
                }
                let mut items = l.items.clone();
                items.push(item.clone());
                validate_items(&items, duration, l.kind.allows_points())?;
                l.items = items;
                l.revision = l.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                Ok(CommandEffect::new(self.label()).create(&item.item_id))
            }
            Command::SetItemState { layer_id, item_ids, state } => {
                let l = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                ensure_layer_editable(l)?;
                if *state == ItemState::Accepted && !l.kind.accepts_acceptance() {
                    return Err(DomainError::not_available(format!(
                        "los {} no admiten aceptación",
                        match l.kind {
                            LayerKind::Blocks => "bloques",
                            _ => "items de esta capa",
                        }
                    )));
                }
                let trims = l.kind == LayerKind::Trims;
                let mut effect = CommandEffect::new(self.label());
                for id in item_ids {
                    let it = l.item_mut(id).ok_or_else(|| DomainError::not_found("item", id))?;
                    let target = if trims && *state == ItemState::Proposed && it.is_human_accepted() { ItemState::Accepted } else { *state };
                    if it.state != target {
                        let accepted = it.is_human_accepted() || target == ItemState::Accepted;
                        it.state = target;
                        if trims {
                            it.extra.insert("accepted".into(), serde_json::json!(accepted));
                            it.extra.insert("enabled".into(), serde_json::json!(target.is_enabled()));
                        }
                        it.edited = true;
                        effect = effect.touch(id);
                    }
                }
                if !effect.affected.is_empty() {
                    l.revision = l.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                }
                Ok(effect)
            }
            Command::SetItemProps { layer_id, item_id, label, comment, ranges } => {
                let duration = layer_asset_duration(project, layer_id)?;
                let l = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                ensure_layer_editable(l)?;
                let allow_points = l.kind.allows_points();
                let mut items = l.items.clone();
                let it = items.iter_mut().find(|i| &i.item_id == item_id).ok_or_else(|| DomainError::not_found("item", item_id))?;
                if let Some(v) = label {
                    it.label = v.clone();
                }
                if let Some(v) = comment {
                    it.comment = v.clone();
                }
                if let Some(r) = ranges {
                    it.ranges = r.clone();
                }
                it.edited = true;
                validate_items(&items, duration, allow_points)?;
                l.items = items;
                l.revision = l.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                Ok(CommandEffect::new(self.label()).touch(item_id))
            }
            Command::PasteItems { layer_id, items, position } => {
                let duration = layer_asset_duration(project, layer_id)?;
                let layer = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                ensure_layer_editable(layer)?;
                let base = items.iter().map(SemanticItem::start).min().ok_or_else(|| DomainError::invalid("portapapeles vacío"))?;
                let mapping: std::collections::HashMap<_, _> = items.iter().map(|i| (i.item_id.clone(), ItemId::random())).collect();
                if mapping.len() != items.len() {
                    return Err(DomainError::invalid("item duplicado en portapapeles"));
                }
                let mut effect = CommandEffect::new(self.label());
                let mut combined = layer.items.clone();
                for source in items {
                    let mut item = source.clone();
                    item.item_id = mapping[&source.item_id].clone();
                    item.parent_id = source.parent_id.as_ref().and_then(|p| mapping.get(p)).cloned();
                    for range in &mut item.ranges {
                        for t in [&mut range.start, &mut range.end] {
                            t.0 =
                                t.0.checked_sub(base.0)
                                    .and_then(|d| position.0.checked_add(d))
                                    .ok_or_else(|| DomainError::out_of_range("rango pegado desbordado"))?;
                        }
                    }
                    item.edited = true;
                    item.extra.insert("copied_from_item_id".into(), serde_json::json!(source.item_id));
                    effect = effect.create(&item.item_id);
                    combined.push(item);
                }
                validate_items(&combined, duration, layer.kind.allows_points())?;
                layer.items = combined;
                layer.revision = layer.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
                Ok(effect)
            }
            Command::PasteClips { clips, position } => {
                let base = clips.iter().map(|c| c.position).min().ok_or_else(|| DomainError::invalid("portapapeles vacío"))?;
                let mut ids = std::collections::HashSet::new();
                let mut groups = std::collections::HashMap::new();
                let mut effect = CommandEffect::new(self.label());
                for source in clips {
                    if !ids.insert(&source.id) {
                        return Err(DomainError::invalid("clip duplicado en portapapeles"));
                    }
                    let offset = source
                        .position
                        .0
                        .checked_sub(base.0)
                        .and_then(|d| position.0.checked_add(d))
                        .ok_or_else(|| DomainError::out_of_range("posición de pegado desbordada"))?;
                    let id = ClipId::random();
                    let group = source
                        .link_group
                        .as_ref()
                        .map(|g| groups.entry(g.clone()).or_insert_with(|| format!("link-{}", crate::ids::random_hex12())).clone());
                    Command::AddClip {
                        track_id: source.track_id.clone(),
                        asset_id: source.asset_id.clone(),
                        source: source.source,
                        position: Ticks(offset),
                        policy: MovePolicy::Reject,
                        clip_id: Some(id.clone()),
                        link_group: group,
                        audio_stream: source.audio_stream,
                        provenance: Some(Provenance {
                            origin: "paste".into(),
                            parent_clip: Some(source.id.clone()),
                            v1_clip_id: source.provenance.v1_clip_id.clone(),
                            created_at: Some(crate::project::now_iso()),
                        }),
                    }
                    .apply(project)?;
                    let pasted = seq_of_clip_mut(project, &id)?.clip_mut(&id).unwrap();
                    pasted.name = source.name.clone();
                    pasted.enabled = source.enabled;
                    pasted.gain_db = source.gain_db;
                    pasted.transform = source.transform;
                    pasted.extra = source.extra.clone();
                    effect = effect.create(id);
                }
                project.validate()?;
                Ok(effect)
            }
            Command::SetItemStructure { layer_id, item_id, parent_id, ranges } => {
                let duration = layer_asset_duration(project, layer_id)?;
                let layer = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                ensure_layer_editable(layer)?;
                let mut items = layer.items.clone();
                let item = items.iter_mut().find(|i| &i.item_id == item_id).ok_or_else(|| DomainError::not_found("item", item_id))?;
                item.parent_id = parent_id.clone();
                item.ranges = ranges.clone();
                item.edited = true;
                validate_items(&items, duration, layer.kind.allows_points())?;
                layer.items = items;
                layer.revision = layer.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                Ok(CommandEffect::new(self.label()).touch(item_id))
            }
            Command::DeleteItems { layer_id, item_ids } => {
                let l = project.layer_mut(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
                ensure_layer_editable(l)?;
                let mut effect = CommandEffect::new(self.label());
                let mut to_delete: Vec<ItemId> = Vec::new();
                for id in item_ids {
                    l.item(id).ok_or_else(|| DomainError::not_found("item", id))?;
                    to_delete.push(id.clone());
                }
                // los hijos de un item borrado también se borran (tombstone para todos)
                let mut changed = true;
                while changed {
                    changed = false;
                    for it in &l.items {
                        if let Some(p) = &it.parent_id
                            && to_delete.contains(p)
                            && !to_delete.contains(&it.item_id)
                        {
                            to_delete.push(it.item_id.clone());
                            changed = true;
                        }
                    }
                }
                l.items.retain(|i| !to_delete.contains(&i.item_id));
                for id in to_delete {
                    if !l.deleted_item_ids.contains(&id) {
                        l.deleted_item_ids.push(id.clone());
                    }
                    effect = effect.touch(id);
                }
                l.revision = l.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                Ok(effect)
            }
            Command::ReplaceLayer { layer } => {
                let duration =
                    project.asset(&layer.asset_id).map(|a| a.duration()).ok_or_else(|| DomainError::not_found("asset", &layer.asset_id))?;
                let mut incoming = layer.clone();
                if let Some(existing) = project.layer(&layer.layer_id) {
                    if existing.deleted && !incoming.deleted {
                        return Err(DomainError::precondition("una capa borrada no puede resucitar por importación"));
                    }
                    if existing.asset_id != incoming.asset_id || existing.kind != incoming.kind {
                        return Err(DomainError::precondition("una capa existente no puede cambiar de medio o tipo"));
                    }
                    if (existing.locked || !existing.kind.is_editable()) && existing != &incoming {
                        return Err(DomainError::precondition("la capa está protegida contra reemplazo"));
                    }
                    // no resucitar tombstones
                    let mut dead: Vec<ItemId> = existing.deleted_item_ids.clone();
                    incoming.items.retain(|i| !dead.contains(&i.item_id));
                    let mut removed_children = true;
                    while removed_children {
                        let children: Vec<_> = incoming
                            .items
                            .iter()
                            .filter(|i| i.parent_id.as_ref().is_some_and(|p| dead.contains(p)))
                            .map(|i| i.item_id.clone())
                            .collect();
                        dead.extend(children);
                        let before = incoming.items.len();
                        incoming.items.retain(|i| !dead.contains(&i.item_id));
                        removed_children = incoming.items.len() != before;
                    }
                    for d in dead {
                        if !incoming.deleted_item_ids.contains(&d) {
                            incoming.deleted_item_ids.push(d);
                        }
                    }
                    incoming.revision = existing.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión de capa agotada"))?;
                }
                validate_layer(&incoming, duration)?;
                let id = incoming.layer_id.clone();
                match project.layers.iter_mut().find(|l| l.layer_id == id) {
                    Some(slot) => *slot = incoming,
                    None => {
                        project.layers.push(incoming);
                        project.layer_order.push(id.clone());
                    }
                }
                Ok(CommandEffect::new(self.label()).touch(id))
            }
            Command::AddSequence { sequence, activate } => {
                if project.sequence(&sequence.id).is_some() {
                    return Err(DomainError::invalid(format!("la secuencia {} ya existe", sequence.id)));
                }
                let id = sequence.id.clone();
                project.sequences.push(sequence.clone());
                if *activate {
                    project.active_sequence = Some(id.clone());
                }
                project.validate()?;
                Ok(CommandEffect::new(self.label()).create(id))
            }
            Command::SetActiveSequence { sequence_id } => {
                project.sequence(sequence_id).ok_or_else(|| DomainError::not_found("secuencia", sequence_id))?;
                project.active_sequence = Some(sequence_id.clone());
                Ok(CommandEffect::new(self.label()).touch(sequence_id))
            }
            Command::Batch { label, commands } => {
                if commands.is_empty() {
                    return Err(DomainError::invalid("batch vacío"));
                }
                let mut effect = CommandEffect::new(label.clone());
                for c in commands {
                    let e = c.apply(project)?;
                    effect.affected.extend(e.affected);
                    effect.created.extend(e.created);
                    effect.warnings.extend(e.warnings);
                }
                effect.affected.sort();
                effect.affected.dedup();
                Ok(effect)
            }
        }
    }
}

fn ensure_unlocked(track: &Track) -> DomainResult<()> {
    if track.locked {
        Err(DomainError::precondition(format!("la pista «{}» está bloqueada", track.name)).with("track", track.id.to_string()))
    } else {
        Ok(())
    }
}

fn ensure_layer_editable(layer: &SemanticLayer) -> DomainResult<()> {
    if layer.deleted || layer.locked || !layer.kind.is_editable() {
        Err(DomainError::precondition("la capa está borrada, bloqueada o es evidencia de solo lectura"))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_source(asset: &Asset, source: &TimeRange) -> DomainResult<()> {
    if !source.is_ordered() || source.is_point() || source.start.is_negative() {
        return Err(DomainError::out_of_range("rango fuente inválido"));
    }
    if asset.kind != crate::asset::AssetKind::Image && source.end.0 as i128 > asset.duration().0 as i128 + Ticks::from_millis(1).0 as i128 {
        return Err(DomainError::out_of_range(format!("el rango fuente termina en {} pero el medio dura {}", source.end, asset.duration())));
    }
    Ok(())
}

pub(crate) fn check_track_compat(track: &Track, asset: &Asset, audio_stream: Option<u32>) -> DomainResult<()> {
    match track.kind {
        TrackKind::Video if !asset.has_video() => Err(DomainError::precondition("el medio no tiene video para una pista de video")),
        TrackKind::Audio if !asset.has_audio() => Err(DomainError::precondition("el medio no tiene audio para una pista de audio")),
        TrackKind::Audio => {
            let idx = audio_stream.unwrap_or(0) as usize;
            if idx >= asset.probe.audio.len() {
                return Err(DomainError::precondition(format!("el medio no tiene pista de audio {idx}")));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn seq_of_track_mut<'a>(project: &'a mut Project, track_id: &TrackId) -> DomainResult<&'a mut crate::timeline::Sequence> {
    project.sequences.iter_mut().find(|s| s.tracks.iter().any(|t| &t.id == track_id)).ok_or_else(|| DomainError::not_found("pista", track_id))
}

fn seq_of_clip_mut<'a>(project: &'a mut Project, clip_id: &ClipId) -> DomainResult<&'a mut crate::timeline::Sequence> {
    project.sequences.iter_mut().find(|s| s.clips.iter().any(|c| &c.id == clip_id)).ok_or_else(|| DomainError::not_found("clip", clip_id))
}

fn layer_asset_duration(project: &Project, layer_id: &LayerId) -> DomainResult<Ticks> {
    let l = project.layer(layer_id).ok_or_else(|| DomainError::not_found("capa", layer_id))?;
    Ok(project.asset(&l.asset_id).map(|a| a.duration()).unwrap_or(Ticks::MAX))
}

fn first_free_track(seq: &crate::timeline::Sequence, kind: TrackKind, range: TimeRange) -> Option<TrackId> {
    first_free_track_skipping(seq, kind, range, 0)
}

fn first_free_track_skipping(seq: &crate::timeline::Sequence, kind: TrackKind, range: TimeRange, skip: usize) -> Option<TrackId> {
    seq.tracks
        .iter()
        .filter(|t| t.kind == kind && !t.locked)
        .skip(skip)
        .find(|t| seq.find_overlap(&t.id, range, None).is_none())
        .map(|t| t.id.clone())
}

/// Coloca un clip aplicando la política de colisión.
fn place_clip(seq: &mut crate::timeline::Sequence, clip: Clip, policy: MovePolicy) -> DomainResult<()> {
    match policy {
        MovePolicy::Reject | MovePolicy::Insert => {
            if let Some(o) = seq.find_overlap(&clip.track_id, clip.range(), Some(&clip.id)) {
                return Err(DomainError::overlap(format!("el clip se solaparía con «{}» ({})", o.name, o.id))
                    .with("other", o.id.to_string())
                    .with_action("elige otra posición, otra pista o usa sobrescribir"));
            }
            seq.clips.push(clip);
            Ok(())
        }
        MovePolicy::Overwrite => {
            let r = clip.range();
            let track = clip.track_id.clone();
            let mut additions: Vec<Clip> = Vec::new();
            let mut remove: Vec<ClipId> = Vec::new();
            for other in seq.clips.iter_mut().filter(|c| c.track_id == track && c.id != clip.id) {
                let o = other.range();
                if !o.overlaps(&r) {
                    continue;
                }
                if r.encloses(&o) {
                    remove.push(other.id.clone());
                } else if o.start < r.start && o.end > r.end {
                    // parte en dos
                    let mut tail = other.clone();
                    tail.id = ClipId::random();
                    tail.source.start = other.source.start + (r.end - o.start);
                    tail.position = r.end;
                    tail.provenance = Provenance { origin: "overwrite".into(), parent_clip: Some(other.id.clone()), ..Default::default() };
                    other.source.end = other.source.start + (r.start - o.start);
                    additions.push(tail);
                } else if o.start < r.start {
                    other.source.end = other.source.start + (r.start - o.start);
                } else {
                    let shift = r.end - o.start;
                    other.source.start += shift;
                    other.position = r.end;
                }
            }
            seq.clips.retain(|c| !remove.contains(&c.id));
            seq.clips.extend(additions);
            seq.clips.push(clip);
            Ok(())
        }
    }
}

/// Utilidades para tests de otros crates (assets sintéticos sin ffprobe).
pub mod tests_support {
    use crate::asset::{Asset, AssetKind, AudioStreamInfo, Fingerprint, MediaProbe, VideoStreamInfo};
    use crate::ids::AssetId;
    use crate::time::{Rational, Ticks};

    pub fn fake_video(id: &str, seconds: i64) -> Asset {
        Asset {
            id: AssetId::new(id),
            kind: AssetKind::Video,
            name: id.to_string(),
            path: format!("{id}.mp4"),
            probe: MediaProbe {
                container: "mp4".into(),
                duration: Ticks::from_seconds(seconds),
                start_time: Ticks::ZERO,
                video: Some(VideoStreamInfo {
                    stream_index: 0,
                    codec: "h264".into(),
                    width: 640,
                    height: 360,
                    rotation: 0,
                    frame_rate: Rational::new(30, 1),
                    avg_frame_rate: None,
                    time_base: Rational::new(1, 15360),
                    start_time: Ticks::ZERO,
                    pix_fmt: "yuv420p".into(),
                    nb_frames: Some((seconds * 30) as u64),
                    duration: Some(Ticks::from_seconds(seconds)),
                    variable_frame_rate: false,
                }),
                audio: vec![AudioStreamInfo {
                    stream_index: 1,
                    audio_index: 0,
                    codec: "aac".into(),
                    channels: 2,
                    channel_layout: "stereo".into(),
                    sample_rate: 48000,
                    start_time: Ticks::ZERO,
                    duration: Some(Ticks::from_seconds(seconds)),
                    title: String::new(),
                }],
                size: 1000,
            },
            fingerprint: Fingerprint { size: 1000, mtime_ns: None, hash_muestreado: "h".into(), inventario_sha256: "i".into() },
            image_duration: None,
            missing: false,
            extra: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::fake_video;
    use super::*;

    fn s(n: i64) -> Ticks {
        Ticks::from_seconds(n)
    }

    fn project_with_clips() -> (Project, TrackId, Vec<ClipId>) {
        let mut p = Project::new("t");
        Command::ImportAsset { asset: fake_video("a", 100) }.apply(&mut p).unwrap();
        let e = Command::AddTrack { sequence_id: None, kind: TrackKind::Video, name: "V1".into(), index: None }.apply(&mut p).unwrap();
        let track = TrackId::new(e.created[0].clone());
        let mut ids = Vec::new();
        for (a, b, pos) in [(10, 20, 0), (30, 35, 10), (40, 50, 15)] {
            let e = Command::AddClip {
                track_id: track.clone(),
                asset_id: AssetId::new("a"),
                source: TimeRange::new(s(a), s(b)),
                position: s(pos),
                policy: MovePolicy::Reject,
                clip_id: None,
                link_group: None,
                audio_stream: None,
                provenance: None,
            }
            .apply(&mut p)
            .unwrap();
            ids.push(ClipId::new(e.created[0].clone()));
        }
        (p, track, ids)
    }

    fn spans(p: &Project) -> Vec<(i64, i64)> {
        let seq = p.active().unwrap();
        let mut v: Vec<&Clip> = seq.clips.iter().collect();
        v.sort_by_key(|c| c.position);
        v.iter().map(|c| (c.position.as_millis_round() / 1000, c.end().as_millis_round() / 1000)).collect()
    }

    #[test]
    fn overlap_is_rejected_by_default() {
        let (mut p, track, _) = project_with_clips();
        let err = Command::AddClip {
            track_id: track,
            asset_id: AssetId::new("a"),
            source: TimeRange::new(s(0), s(3)),
            position: s(4),
            policy: MovePolicy::Reject,
            clip_id: None,
            link_group: None,
            audio_stream: None,
            provenance: None,
        }
        .apply(&mut p)
        .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::Overlap);
    }

    #[test]
    fn split_trim_move_remove_match_v1_semantics() {
        let (mut p, _track, ids) = project_with_clips();
        // split 0–10 en 4
        let e = Command::SplitClip { clip_id: ids[0].clone(), at: s(4) }.apply(&mut p).unwrap();
        let right = ClipId::new(e.created[0].clone());
        assert_eq!(spans(&p), vec![(0, 4), (4, 10), (10, 15), (15, 25)]);
        let seq = p.active().unwrap();
        assert_eq!(seq.clip(&right).unwrap().source.start, s(14));
        assert_eq!(seq.clip(&ids[0]).unwrap().source.end, s(14));
        // split en el borde falla
        assert!(Command::SplitClip { clip_id: ids[1].clone(), at: s(15) }.apply(&mut p).is_err());
        // trim start: seq y source se mueven juntos, el fin queda
        Command::TrimClip { clip_id: right.clone(), edge: ClipEdge::Start, new_time: Ticks::from_millis(5500) }.apply(&mut p).unwrap();
        let c = p.active().unwrap().clip(&right).unwrap().clone();
        assert_eq!((c.source.start, c.position, c.end()), (Ticks::from_millis(15500), Ticks::from_millis(5500), s(10)));
        // trim end no pisa al vecino: error explícito en vez de acotar silenciosamente
        assert!(Command::TrimClip { clip_id: right.clone(), edge: ClipEdge::End, new_time: s(13) }.apply(&mut p).is_err());
        Command::TrimClip { clip_id: right.clone(), edge: ClipEdge::End, new_time: s(8) }.apply(&mut p).unwrap();
        assert_eq!(p.active().unwrap().clip(&right).unwrap().source.end, s(18));
        // mover con insert: el tercero al principio; los demás se corren; su hueco se cierra
        Command::MoveClip { clip_id: ids[2].clone(), position: s(0), track_id: None, policy: MovePolicy::Insert }.apply(&mut p).unwrap();
        assert_eq!(spans(&p), vec![(0, 10), (10, 14), (15, 18), (20, 25)]);
        // insert dentro de un clip cae al borde más cercano
        Command::MoveClip { clip_id: ids[1].clone(), position: s(11), track_id: None, policy: MovePolicy::Insert }.apply(&mut p).unwrap();
        let c = p.active().unwrap().clip(&ids[1]).unwrap();
        assert_eq!(c.position, s(10));
        // remove con ripple cierra el hueco; sin ripple lo deja
        let before = p.active().unwrap().extent();
        Command::RemoveClips { clip_ids: vec![ids[1].clone()], ripple: true }.apply(&mut p).unwrap();
        assert_eq!(p.active().unwrap().extent(), before - s(5));
        let extent = p.active().unwrap().extent();
        Command::RemoveClips { clip_ids: vec![ids[2].clone()], ripple: false }.apply(&mut p).unwrap();
        assert_eq!(p.active().unwrap().extent(), extent);
    }

    #[test]
    fn split_keeps_halves_linked_but_independent_between_them() {
        let mut p = Project::new("t");
        Command::ImportAsset { asset: fake_video("a", 100) }.apply(&mut p).unwrap();
        let e =
            Command::InsertAssetLinked { asset_id: AssetId::new("a"), position: Ticks::ZERO, video_track: None, source: None }.apply(&mut p).unwrap();
        assert_eq!(e.created.len(), 2);
        let ids: Vec<ClipId> = e.created.iter().map(|s| ClipId::new(s.clone())).collect();
        let batch =
            Command::Batch { label: "split".into(), commands: ids.iter().map(|id| Command::SplitClip { clip_id: id.clone(), at: s(40) }).collect() };
        batch.apply(&mut p).unwrap();
        let seq = p.active().unwrap();
        let groups: std::collections::HashSet<String> = seq.clips.iter().filter_map(|c| c.link_group.clone()).collect();
        assert_eq!(groups.len(), 2, "izquierda y derecha en grupos distintos");
        let right: Vec<&Clip> = seq.clips.iter().filter(|c| c.position == s(40)).collect();
        assert_eq!(right.len(), 2);
        assert_eq!(right[0].link_group, right[1].link_group, "video y audio derechos siguen enlazados");
        let left: Vec<&Clip> = seq.clips.iter().filter(|c| c.position == Ticks::ZERO).collect();
        assert_ne!(left[0].link_group, right[0].link_group);
    }

    #[test]
    fn linked_group_insert_keeps_video_and_audio_in_sync_and_closes_gaps() {
        // dos pares enlazados consecutivos (V1/A1): [a 0–10][b 10–20]; insertar «a» tras «b»
        let mut p = Project::new("t");
        Command::ImportAsset { asset: fake_video("a", 100) }.apply(&mut p).unwrap();
        let ea = Command::InsertAssetLinked {
            asset_id: AssetId::new("a"),
            position: Ticks::ZERO,
            video_track: None,
            source: Some(TimeRange::new(s(0), s(10))),
        }
        .apply(&mut p)
        .unwrap();
        let eb = Command::InsertAssetLinked {
            asset_id: AssetId::new("a"),
            position: s(10),
            video_track: None,
            source: Some(TimeRange::new(s(50), s(60))),
        }
        .apply(&mut p)
        .unwrap();
        let a_ids: Vec<ClipId> = ea.created.iter().map(|x| ClipId::new(x.clone())).collect();
        let b_ids: Vec<ClipId> = eb.created.iter().map(|x| ClipId::new(x.clone())).collect();
        // con Reject el destino 15 s colisiona con «b»
        let err = Command::ShiftClips { clip_ids: a_ids.clone(), delta: s(15), track_delta: 0, policy: MovePolicy::Reject }
            .apply(&mut p.clone())
            .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::Overlap);
        // con Insert (semántica V1 `move(mode="insert")`): primero «b» cierra el hueco (→ 0–10); el destino 5 s
        // cae dentro de «b» y va al borde más cercano (equidistante → fin, 10); «a» queda en 10–20 en ambas pistas
        Command::ShiftClips { clip_ids: a_ids.clone(), delta: s(5), track_delta: 0, policy: MovePolicy::Insert }.apply(&mut p).unwrap();
        let seq = p.active().unwrap();
        for id in &a_ids {
            assert_eq!(seq.clip(id).unwrap().position, s(10), "video y audio de «a» juntos");
        }
        for id in &b_ids {
            assert_eq!(seq.clip(id).unwrap().position, Ticks::ZERO, "«b» cerró el hueco en ambas pistas");
        }
        assert_eq!(seq.extent(), s(20));
        // Overwrite: mover «b» (ahora 0–10) a 5 recorta «a» (10–20) a 15–20
        Command::ShiftClips { clip_ids: b_ids.clone(), delta: s(5), track_delta: 0, policy: MovePolicy::Overwrite }.apply(&mut p).unwrap();
        let seq = p.active().unwrap();
        for id in &a_ids {
            let c = seq.clip(id).unwrap();
            assert_eq!((c.position, c.end()), (s(15), s(20)));
            assert_eq!(c.source.start, s(5));
        }
    }

    #[test]
    fn overwrite_splits_covered_clip() {
        let mut p = Project::new("t");
        Command::ImportAsset { asset: fake_video("a", 100) }.apply(&mut p).unwrap();
        let e = Command::AddTrack { sequence_id: None, kind: TrackKind::Video, name: "V1".into(), index: None }.apply(&mut p).unwrap();
        let track = TrackId::new(e.created[0].clone());
        let add = |p: &mut Project, a: i64, b: i64, pos: i64, policy: MovePolicy| {
            Command::AddClip {
                track_id: track.clone(),
                asset_id: AssetId::new("a"),
                source: TimeRange::new(s(a), s(b)),
                position: s(pos),
                policy,
                clip_id: None,
                link_group: None,
                audio_stream: None,
                provenance: None,
            }
            .apply(p)
            .unwrap()
        };
        add(&mut p, 10, 30, 0, MovePolicy::Reject);
        add(&mut p, 50, 52, 8, MovePolicy::Overwrite);
        assert_eq!(spans(&p), vec![(0, 8), (8, 10), (10, 20)]);
        let seq = p.active().unwrap();
        let mut v: Vec<&Clip> = seq.clips.iter().collect();
        v.sort_by_key(|c| c.position);
        assert_eq!(v.iter().map(|c| (c.source.start, c.source.end)).collect::<Vec<_>>(), vec![(s(10), s(18)), (s(50), s(52)), (s(20), s(30))]);
    }

    #[test]
    fn locked_track_blocks_edits_and_batch_is_atomic() {
        let (mut p, track, ids) = project_with_clips();
        Command::SetTrackProps {
            track_id: track.clone(),
            name: None,
            muted: None,
            solo: None,
            locked: Some(true),
            visible: None,
            gain_db: None,
            height: None,
        }
        .apply(&mut p)
        .unwrap();
        let err =
            Command::MoveClip { clip_id: ids[0].clone(), position: s(50), track_id: None, policy: MovePolicy::Reject }.apply(&mut p).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::Precondition);
        Command::SetTrackProps {
            track_id: track,
            name: None,
            muted: None,
            solo: None,
            locked: Some(false),
            visible: None,
            gain_db: None,
            height: None,
        }
        .apply(&mut p)
        .unwrap();
        let snapshot = p.clone();
        let batch = Command::Batch {
            label: "dos".into(),
            commands: vec![
                Command::MoveClip { clip_id: ids[2].clone(), position: s(50), track_id: None, policy: MovePolicy::Reject },
                Command::MoveClip { clip_id: ids[1].clone(), position: s(52), track_id: None, policy: MovePolicy::Reject },
            ],
        };
        // el segundo colisiona: la sesión descarta la copia; aquí verificamos que falla
        let mut copy = p.clone();
        assert!(batch.apply(&mut copy).is_err());
        assert_eq!(p, snapshot);
    }

    #[test]
    fn layers_follow_v1_rules() {
        let mut p = Project::new("t");
        Command::ImportAsset { asset: fake_video("a", 100) }.apply(&mut p).unwrap();
        let e = Command::CreateLayer { asset_id: AssetId::new("a"), kind: LayerKind::User, name: "Notas".into(), color: None, layer_id: None }
            .apply(&mut p)
            .unwrap();
        let layer = LayerId::new(e.created[0].clone());
        let item = SemanticItem::new(TimeRange::new(s(1), s(3)), "uno");
        let item_id = item.item_id.clone();
        Command::AddItem { layer_id: layer.clone(), item }.apply(&mut p).unwrap();
        // fuera del medio
        let bad = SemanticItem::new(TimeRange::new(s(90), s(120)), "malo");
        assert!(Command::AddItem { layer_id: layer.clone(), item: bad }.apply(&mut p).is_err());
        // aceptar, aceptar de nuevo no vuelve a propuesto
        Command::SetItemState { layer_id: layer.clone(), item_ids: vec![item_id.clone()], state: ItemState::Accepted }.apply(&mut p).unwrap();
        let e = Command::SetItemState { layer_id: layer.clone(), item_ids: vec![item_id.clone()], state: ItemState::Accepted }.apply(&mut p).unwrap();
        assert!(e.affected.is_empty());
        assert_eq!(p.layer(&layer).unwrap().item(&item_id).unwrap().state, ItemState::Accepted);
        // borrar → tombstone → no se puede re-añadir ni resucitar por ReplaceLayer
        Command::DeleteItems { layer_id: layer.clone(), item_ids: vec![item_id.clone()] }.apply(&mut p).unwrap();
        assert!(p.layer(&layer).unwrap().deleted_item_ids.contains(&item_id));
        let mut zombie = SemanticItem::new(TimeRange::new(s(1), s(3)), "zombie");
        zombie.item_id = item_id.clone();
        assert!(Command::AddItem { layer_id: layer.clone(), item: zombie.clone() }.apply(&mut p).is_err());
        let mut replaced = p.layer(&layer).unwrap().clone();
        replaced.deleted_item_ids.clear();
        replaced.items = vec![zombie, SemanticItem::new(TimeRange::new(s(5), s(6)), "vivo")];
        Command::ReplaceLayer { layer: replaced }.apply(&mut p).unwrap();
        let l = p.layer(&layer).unwrap();
        assert_eq!(l.items.len(), 1);
        assert_eq!(l.items[0].label, "vivo");
        assert!(l.deleted_item_ids.contains(&item_id));
        // bloques no admiten aceptación
        let e = Command::CreateLayer { asset_id: AssetId::new("a"), kind: LayerKind::Blocks, name: "Bloques".into(), color: None, layer_id: None }
            .apply(&mut p)
            .unwrap();
        let blocks = LayerId::new(e.created[0].clone());
        let b = SemanticItem::new(TimeRange::new(s(0), s(10)), "b1");
        let bid = b.item_id.clone();
        Command::AddItem { layer_id: blocks.clone(), item: b }.apply(&mut p).unwrap();
        let before = p.clone();
        let err = Command::SetItemState { layer_id: blocks, item_ids: vec![bid], state: ItemState::Accepted }.apply(&mut p).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::NotAvailable);
        assert_eq!(p, before);
    }

    #[test]
    fn commands_serialize_with_type_tag() {
        let c = Command::SplitClip { clip_id: ClipId::new("clip-1"), at: s(4) };
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["type"], "split_clip");
        let back: Command = serde_json::from_value(json).unwrap();
        assert_eq!(back, c);
    }
}
