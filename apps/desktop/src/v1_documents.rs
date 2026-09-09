//! Read-only V1 document watcher. Normalized three-way review and commands are
//! shared with project reconciliation; the immutable imported bundle is untouched.
use crate::app::{Severity, TranscriptorApp};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tv2_application::{Actor, CommandEnvelope, ProjectSession, reconcile::ExternalChange, session::PreparedCommand};
use tv2_domain::{AssetId, Command, DomainError, Project, SemanticLayer, Sequence, SequenceId, error::DomainResult};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentWatch {
    pub path: PathBuf,
    pub asset: AssetId,
    pub digest: String,
    pub layers: Vec<SemanticLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_sequence: Option<SequenceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<Sequence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<serde_json::Value>,
}
pub struct Review {
    watch: DocumentWatch,
    pub change: Option<ExternalChange>,
    revision: u64,
}
enum Completion {
    Loaded(Vec<DocumentWatch>),
    Scanned(Option<Box<Review>>, Vec<String>),
    Applied(Box<(DocumentWatch, PreparedCommand)>),
    OpenContract(Box<DocumentWatch>),
    Saved,
}
type Pending = crossbeam_channel::Receiver<DomainResult<Completion>>;
pub struct Documents {
    project: String,
    pub watches: Vec<DocumentWatch>,
    pub review: Option<Review>,
    pending: Option<Pending>,
    last_scan: Instant,
    pub error: Option<String>,
    pub open: bool,
    watchers: Vec<crate::watcher::Watcher>,
    persist: bool,
    dismissed: Option<String>,
}
impl Default for Documents {
    fn default() -> Self {
        Self {
            project: String::new(),
            watches: vec![],
            review: None,
            pending: None,
            last_scan: Instant::now(),
            error: None,
            open: false,
            watchers: vec![],
            persist: false,
            dismissed: None,
        }
    }
}
fn settings_path(project: &str) -> PathBuf {
    crate::paths::config_dir().join("v1-document-watches").join(format!("{}.json", tv2_domain::digest::digest_json(&serde_json::json!(project))))
}
fn background(work: impl FnOnce() -> DomainResult<Completion> + Send + 'static, ctx: &egui::Context) -> DomainResult<Pending> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let wake = ctx.clone();
    std::thread::Builder::new().name("v1-document-review".into()).spawn(move || {
        let _ = tx.send(work());
        wake.request_repaint();
    })?;
    Ok(rx)
}
fn read_bytes(path: &std::path::Path) -> DomainResult<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(DomainError::invalid("Documento vigilado supera 128 MiB; no se modifica ni se descarta"));
    }
    Ok(bytes)
}
fn read(watch: &DocumentWatch, project: &Project) -> DomainResult<DocumentWatch> {
    let asset = project.asset(&watch.asset).ok_or_else(|| DomainError::precondition("El medio vigilado ya no existe"))?;
    let bytes = read_bytes(&watch.path)?;
    let value: serde_json::Value = serde_json::from_slice(bytes.strip_prefix(&[239, 187, 191]).unwrap_or(&bytes))?;
    let digest = tv2_domain::digest::digest_json(&value);
    let schema = value.get("schema").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(
        schema,
        "editorial-layers-request/1"
            | "editorial-layers-proposal/1"
            | "editorial-topics-request/1"
            | "editorial-topics-proposal/1"
            | "editorial-trims-request/1"
            | "editorial-trims-proposal/1"
            | "editorial-montage-request/1"
            | "editorial-montage-proposal/1"
    ) {
        if !watch.layers.is_empty() || watch.sequence.is_some() {
            return Err(DomainError::precondition("Documento vinculado cambió de tipo; vincula el nuevo contrato explícitamente"));
        }
        let mut header = serde_json::Map::new();
        for key in
            ["schema", "request_id", "source_master_digest", "source_layers_digest", "pass", "pass_required", "previous_pass_digest", "complete"]
        {
            if let Some(v) = value.get(key) {
                header.insert(key.into(), v.clone());
            }
        }
        if read_bytes(&watch.path)? != bytes {
            return Err(DomainError::precondition("El pedido/propuesta V1 está cambiando"));
        }
        return Ok(DocumentWatch {
            path: watch.path.clone(),
            asset: watch.asset.clone(),
            digest,
            layers: vec![],
            target_sequence: None,
            sequence: None,
            contract: Some(header.into()),
        });
    }
    if schema == "editorial-montaje/1" {
        if !watch.layers.is_empty() || watch.contract.is_some() {
            return Err(DomainError::precondition("Documento vinculado cambió de tipo; vincula el montaje explícitamente"));
        }
        let montage = tv2_v1compat::montaje::V1Montaje::parse(value, Some(&asset.fingerprint))?;
        if watch
            .sequence
            .as_ref()
            .and_then(|s| s.extra.get("v1_original_montage"))
            .and_then(|m| m["revision"].as_u64())
            .is_some_and(|r| montage.revision < r)
        {
            return Err(DomainError::precondition("El montaje V1 retrocedió de revisión"));
        }
        let sequence = normalize_montage(watch, project, &montage)?;
        if read_bytes(&watch.path)? != bytes {
            return Err(DomainError::precondition("El montaje V1 está cambiando; esperando lectura estable"));
        }
        return Ok(DocumentWatch {
            path: watch.path.clone(),
            asset: watch.asset.clone(),
            digest,
            layers: vec![],
            target_sequence: Some(sequence.id.clone()),
            sequence: Some(sequence),
            contract: None,
        });
    }
    if watch.sequence.is_some() || watch.contract.is_some() {
        return Err(DomainError::precondition("Documento vinculado cambió de tipo"));
    }
    let mut layers = match value.get("schema").and_then(|v| v.as_str()) {
        Some("editorial-layer/1") => vec![tv2_v1compat::layers::layer_from_v1(&value, &asset.id, Some(&asset.fingerprint), asset.duration())?],
        Some("editorial-trims/1") => tv2_v1compat::trims::trims_from_v1(&value, &asset.id, Some(&asset.fingerprint))?.layers,
        _ if value.get("timebase").and_then(|v| v.as_str()) == Some("media_elapsed_v1") => vec![tv2_v1compat::author::from_v1(&value, asset)?],
        _ => {
            let master = project
                .masters
                .iter()
                .find(|m| m.asset_id == asset.id)
                .ok_or_else(|| DomainError::invalid("Documento no reconocido o sin master"))?;
            vec![tv2_v1compat::chunks::from_v1(&value, &asset.id, asset.duration(), &master.source_digest)?]
        }
    };
    for layer in &mut layers {
        if layer.kind == tv2_domain::LayerKind::Author
            && let Some(existing) = project.layers.iter().find(|l| l.asset_id == asset.id && l.kind == layer.kind && !l.deleted)
        {
            layer.layer_id = existing.layer_id.clone();
        }
        if let Some(previous) = watch.layers.iter().find(|l| l.layer_id == layer.layer_id)
            && layer.revision < previous.revision
        {
            return Err(DomainError::precondition("El documento V1 retrocedió de revisión; se conserva la base"));
        }
    }
    if read_bytes(&watch.path)? != bytes {
        return Err(DomainError::precondition("El documento V1 está cambiando; esperando una lectura estable"));
    }
    Ok(DocumentWatch {
        path: watch.path.clone(),
        asset: watch.asset.clone(),
        digest,
        layers,
        target_sequence: watch.target_sequence.clone(),
        sequence: None,
        contract: None,
    })
}

fn normalize_montage(watch: &DocumentWatch, project: &Project, montage: &tv2_v1compat::montaje::V1Montaje) -> DomainResult<Sequence> {
    let asset = project.asset(&watch.asset).ok_or_else(|| DomainError::precondition("Medio vigilado inexistente"))?;
    let video = asset.probe.video.as_ref().ok_or_else(|| DomainError::precondition("El montaje V1 requiere video"))?;
    let target = watch.sequence.as_ref().map(|s| s.id.clone()).or_else(|| watch.target_sequence.clone()).unwrap_or_else(|| {
        SequenceId::new(format!("watch-{}", &tv2_domain::digest::digest_json(&serde_json::json!([project.project_id, watch.path]))[..24]))
    });
    let baseline = watch.sequence.as_ref().or_else(|| project.sequence(&target));
    if baseline.is_some_and(|s| s.clips.iter().any(|c| c.asset_id != asset.id)) {
        return Err(DomainError::precondition("La secuencia vinculada contiene otros medios; vigila el montaje en una secuencia del mismo medio"));
    }
    let (width, height) = video.display_size();
    let mut sequence = montage.to_sequence(&asset.id, video.frame_rate, width, height, asset.probe.audio.len() as u32);
    sequence.id = target;
    if let Some(base) = baseline {
        sequence.name = base.name.clone();
        sequence.markers = base.markers.clone();
        sequence.sample_rate = base.sample_rate;
    }
    let mut ids = std::collections::HashMap::new();
    for (index, track) in sequence.tracks.iter_mut().enumerate() {
        let old_id = track.id.clone();
        let prior = baseline.and_then(|s| {
            s.tracks.iter().filter(|t| t.kind == track.kind).nth(if track.kind == tv2_domain::TrackKind::Video { 0 } else { index - 1 })
        });
        track.id = prior.map(|t| t.id.clone()).unwrap_or_else(|| tv2_domain::TrackId::new(format!("{}-track-{index}", sequence.id)));
        ids.insert(old_id, track.id.clone());
        if let Some(prior) = prior {
            *track = prior.clone();
        }
    }
    let mut used = std::collections::HashSet::new();
    for clip in &mut sequence.clips {
        clip.track_id = ids[&clip.track_id].clone();
        let generated_id = clip.id.clone();
        let candidates: Vec<_> = baseline
            .into_iter()
            .flat_map(|s| &s.clips)
            .filter(|c| c.provenance.v1_clip_id == clip.provenance.v1_clip_id && c.audio_stream == clip.audio_stream && !used.contains(&c.id))
            .collect();
        let prior = candidates
            .iter()
            .find(|c| c.id == generated_id || c.extra.get("tv2_watch_piece_id").and_then(|v| v.as_str()) == Some(generated_id.as_str()))
            .copied()
            .or_else(|| (candidates.len() == 1).then(|| candidates[0]));
        clip.id = prior.map(|c| c.id.clone()).unwrap_or_else(|| tv2_domain::ClipId::new(format!("{}-{}", sequence.id, generated_id)));
        used.insert(clip.id.clone());
        clip.extra.insert("tv2_watch_piece_id".into(), serde_json::json!(generated_id));
        clip.provenance.created_at = prior.and_then(|c| c.provenance.created_at.clone());
        clip.link_group = clip.link_group.as_ref().map(|group| format!("{}-{group}", sequence.id));
    }
    // Recompute the profile checksum after stable IDs and metadata were assigned.
    let mut projection = sequence.clone();
    projection.extra.remove("v1_original_montage");
    projection.extra.remove("v1_projection_digest");
    // Native playback controls retained from the binding are not represented by
    // the source V1 JSON. Keep the checksum distinct so inverse export validates
    // these controls instead of returning the unchanged V1 document prematurely.
    for track in &mut projection.tracks {
        track.muted = false;
        track.solo = false;
        track.visible = true;
        track.gain_db = 0.0;
    }
    sequence.extra.insert("v1_projection_digest".into(), serde_json::json!(tv2_domain::digest::digest_json(&serde_json::to_value(projection)?)));
    Ok(sequence)
}
fn substitute_sequence(project: &mut Project, sequence: &Sequence) {
    if let Some(slot) = project.sequence_mut(&sequence.id) {
        *slot = sequence.clone();
    } else {
        project.sequences.push(sequence.clone());
    }
}
fn dismiss_key(watch: &DocumentWatch) -> String {
    tv2_domain::digest::digest_json(&serde_json::json!([watch.path, watch.digest]))
}
fn substitute(project: &mut Project, layers: &[SemanticLayer], old: &[SemanticLayer]) {
    for layer in layers {
        if let Some(found) = project.layers.iter_mut().find(|l| l.layer_id == layer.layer_id) {
            *found = layer.clone();
        } else {
            project.layer_order.push(layer.layer_id.clone());
            project.layers.push(layer.clone());
        }
    }
    for previous in old.iter().filter(|l| !layers.iter().any(|n| n.layer_id == l.layer_id)) {
        if let Some(found) = project.layers.iter_mut().find(|l| l.layer_id == previous.layer_id) {
            found.deleted_item_ids.extend(found.items.iter().map(|i| i.item_id.clone()));
            found.deleted_item_ids.sort();
            found.deleted_item_ids.dedup();
            found.items.clear();
            found.deleted = true;
        }
    }
}
fn inspect(watch: &DocumentWatch, project: &Project) -> DomainResult<Option<Box<Review>>> {
    let next = read(watch, project)?;
    if next.digest == watch.digest {
        return Ok(None);
    }
    if next.contract.is_some() {
        return Ok(Some(Box::new(Review { watch: next, change: None, revision: project.revision })));
    }
    let mut base = project.clone();
    let mut external = project.clone();
    // Initial binding compares explicitly against the current content. Subsequent
    // reads use only the last accepted source baseline, never the current edit.
    if !watch.layers.is_empty() {
        substitute(&mut base, &watch.layers, &[]);
    }
    if let Some(sequence) = &watch.sequence {
        substitute_sequence(&mut base, sequence);
    }
    substitute(&mut external, &next.layers, &watch.layers);
    if let Some(sequence) = &next.sequence {
        substitute_sequence(&mut external, sequence);
    }
    let change = tv2_application::reconcile::inspect(&base, project, &external)?;
    Ok(Some(Box::new(Review { watch: next, change: Some(change), revision: project.revision })))
}
impl TranscriptorApp {
    pub fn watch_v1_document(&mut self) {
        let Some(asset) = self.current_layer_asset() else {
            self.toast(Severity::Warn, "Selecciona un medio");
            return;
        };
        let Some(path) = rfd::FileDialog::new().set_title("Vigilar JSON V1 (solo lectura)").add_filter("JSON", &["json"]).pick_file() else { return };
        if !self.v1_documents.watches.iter().any(|w| w.path == path) {
            let target_sequence = self.project().active_sequence.clone();
            self.v1_documents.watches.push(DocumentWatch {
                path,
                asset,
                digest: String::new(),
                layers: vec![],
                target_sequence,
                sequence: None,
                contract: None,
            });
            self.v1_documents.persist = true;
            self.v1_documents.watchers.clear();
        }
        self.v1_documents.open = true;
    }
    pub fn poll_v1_documents(&mut self, ctx: &egui::Context) {
        let mut state = std::mem::take(&mut self.v1_documents);
        if state.project != self.project().project_id {
            state = Documents { project: self.project().project_id.clone(), ..Default::default() };
            let path = settings_path(&state.project);
            match background(
                move || match std::fs::read(path) {
                    Ok(bytes) => Ok(Completion::Loaded(serde_json::from_slice(&bytes)?)),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Completion::Loaded(vec![])),
                    Err(e) => Err(e.into()),
                },
                ctx,
            ) {
                Ok(rx) => state.pending = Some(rx),
                Err(e) => state.error = Some(e.to_string()),
            }
        }
        if let Some(rx) = &state.pending {
            let received = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(crossbeam_channel::TryRecvError::Empty) => None,
                Err(_) => Some(Err(DomainError::process("Lector V1 terminó sin resultado"))),
            };
            if let Some(result) = received {
                state.pending = None;
                match result {
                    Ok(Completion::Loaded(watches)) => {
                        for watch in watches {
                            if !state.watches.iter().any(|w| w.path == watch.path) {
                                state.watches.push(watch);
                            }
                        }
                    }
                    Ok(Completion::Scanned(review, errors)) => {
                        state.error = (!errors.is_empty()).then(|| errors.join("\n"));
                        state.review = review
                            .map(|r| *r)
                            .filter(|r| r.revision == self.session.revision() && state.dismissed.as_ref() != Some(&dismiss_key(&r.watch)));
                        if state.review.is_some() {
                            state.open = true;
                        }
                    }
                    Ok(Completion::Applied(prepared)) => {
                        let (watch, command) = *prepared;
                        match self.session.commit_prepared(command) {
                            Ok(_) => {
                                self.after_change();
                                if let Some(previous) = state.watches.iter_mut().find(|w| w.path == watch.path) {
                                    *previous = watch;
                                }
                                state.persist = true;
                                state.review = None;
                                state.error = None;
                            }
                            Err(e) => state.error = Some(e.to_string()),
                        }
                    }
                    Ok(Completion::OpenContract(watch)) => {
                        let watch = *watch;
                        match crate::editorial_review::open_watched(self, ctx, watch.path.clone(), watch.asset.clone()) {
                            Ok(()) => {
                                if let Some(previous) = state.watches.iter_mut().find(|w| w.path == watch.path) {
                                    *previous = watch;
                                }
                                state.persist = true;
                                state.review = None;
                                state.error = None;
                            }
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Ok(Completion::Saved) => {}
                    Err(e) => state.error = Some(e.to_string()),
                }
            }
        }
        if state.review.as_ref().is_some_and(|r| r.revision != self.session.revision()) {
            state.review = None;
        }
        let notified = state.watchers.iter().any(|w| w.changed().is_some());
        if state.pending.is_none() {
            if state.persist {
                let watches = state.watches.clone();
                let path = settings_path(&state.project);
                match background(
                    move || {
                        std::fs::create_dir_all(path.parent().unwrap())?;
                        tv2_application::store::atomic_write(&path, &serde_json::to_vec(&watches)?)?;
                        Ok(Completion::Saved)
                    },
                    ctx,
                ) {
                    Ok(rx) => {
                        state.pending = Some(rx);
                        state.persist = false
                    }
                    Err(e) => state.error = Some(e.to_string()),
                }
            } else if state.review.is_none() && (notified || state.last_scan.elapsed() > Duration::from_secs(2)) && !state.watches.is_empty() {
                state.last_scan = Instant::now();
                let watches = state.watches.clone();
                let project = self.project().clone();
                let dismissed = state.dismissed.clone();
                match background(
                    move || {
                        let mut errors = Vec::new();
                        for watch in watches {
                            match inspect(&watch, &project) {
                                Ok(Some(review)) if dismissed.as_ref() != Some(&dismiss_key(&review.watch)) => {
                                    return Ok(Completion::Scanned(Some(review), errors));
                                }
                                Ok(_) => {}
                                Err(error) => errors.push(format!("{}: {error}", watch.path.display())),
                            }
                        }
                        Ok(Completion::Scanned(None, errors))
                    },
                    ctx,
                ) {
                    Ok(rx) => state.pending = Some(rx),
                    Err(e) => state.error = Some(e.to_string()),
                }
            }
        }
        if state.watchers.is_empty() && !state.watches.is_empty() {
            let roots: std::collections::BTreeSet<_> = state.watches.iter().filter_map(|w| w.path.parent().map(|p| p.to_path_buf())).collect();
            for root in roots {
                if let Ok(watcher) = crate::watcher::Watcher::start(root, ctx.clone()) {
                    state.watchers.push(watcher);
                }
            }
        }
        if !state.watches.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        self.v1_documents = state;
    }
}
pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.v1_documents);
    let mut open_contract = false;
    if state.open || state.review.is_some() {
        let mut apply = false;
        let mut dismiss = false;
        let mut remove = None;
        egui::Window::new("Documentos V1 vigilados").open(&mut state.open).default_width(700.0).show(ctx, |ui| {
            ui.label("Lectura externa; las fuentes V1 no se modifican. La base y los localizadores se conservan en configuración V2.");
            for (i, w) in state.watches.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(w.path.display().to_string());
                    if ui.small_button("Dejar de vigilar").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(error) = &state.error {
                ui.colored_label(egui::Color32::YELLOW, error);
            }
            if let Some(review) = &mut state.review {
                ui.heading(review.watch.path.file_name().unwrap_or_default().to_string_lossy());
                if let Some(sequence) = &review.watch.sequence {
                    ui.label(format!("Secuencia vinculada: {} ({})", sequence.name, sequence.id));
                }
                if let Some(change) = &mut review.change {
                    ui.label(change.diff.human());
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for field in &change.fields {
                            ui.collapsing(&field.path, |ui| {
                                ui.label(format!("Local: {:?}", field.before));
                                ui.label(format!("Externo: {:?}", field.after));
                            });
                        }
                        for conflict in &change.conflicts {
                            ui.label(format!("Conflicto {}", conflict.path));
                            for (choice, label) in [
                                (tv2_application::reconcile::ConflictChoice::Local, "Conservar local"),
                                (tv2_application::reconcile::ConflictChoice::External, "Usar externo"),
                            ] {
                                if ui.selectable_label(change.choices.get(&conflict.path) == Some(&choice), label).clicked() {
                                    change.choices.insert(conflict.path.clone(), choice);
                                }
                            }
                        }
                    });
                    ui.checkbox(&mut change.human_review, "Autorizar cambios a mis decisiones editoriales");
                    apply = ui
                        .add_enabled(
                            state.pending.is_none() && change.conflicts.iter().all(|c| change.choices.contains_key(&c.path)),
                            egui::Button::new("Aplicar contenido revisado"),
                        )
                        .clicked();
                } else if let Some(contract) = &review.watch.contract {
                    ui.label(format!("Contrato: {}", contract["schema"].as_str().unwrap_or("")));
                    ui.label(format!("Pedido: {}", contract["request_id"].as_str().unwrap_or("sin identificador")));
                    if let Some(pass) = contract.get("pass").or_else(|| contract.get("pass_required")) {
                        ui.label(format!("Pase: {pass}"));
                    }
                    ui.label("El contrato se copiará a una carpeta V2 y se cargará para revisar su propuesta antes de aplicar cambios.");
                    open_contract = ui.add_enabled(state.pending.is_none(), egui::Button::new("Abrir este pedido/propuesta para revisión")).clicked();
                }
                dismiss = ui.button("Posponer esta versión").clicked();
            }
        });
        if let Some(i) = remove {
            state.watches.remove(i);
            state.persist = true;
            state.watchers.clear();
            state.review = None;
        }
        if dismiss {
            state.dismissed = state.review.take().map(|r| dismiss_key(&r.watch));
        }
        if apply && let Some(review) = state.review.take() {
            let project = app.project().clone();
            match background(
                move || {
                    let next = read(&review.watch, &project)?;
                    if next.digest != review.watch.digest {
                        return Err(DomainError::precondition("El documento cambió tras la revisión"));
                    }
                    let change = review.change.ok_or_else(|| DomainError::precondition("El contrato requiere revisión editorial"))?;
                    let merged = change.resolved()?;
                    let command = if next.sequence.is_some() {
                        Command::ReconcileProject { project: Box::new(merged.merged) }
                    } else {
                        let commands = merged
                            .merged
                            .layers
                            .iter()
                            .filter(|l| project.layer(&l.layer_id) != Some(l))
                            .map(|l| Command::ReplaceLayer { layer: l.clone() })
                            .collect();
                        Command::Batch { label: "Reconciliar documento V1".into(), commands }
                    };
                    let actor = if change.human_review { Actor::Human } else { Actor::External { source: review.watch.path.display().to_string() } };
                    let request = CommandEnvelope::human(command).with_base(change.base_revision).with_actor(actor);
                    let prepared = ProjectSession::new(project).prepare_command(request)?;
                    Ok(Completion::Applied(Box::new((next, prepared))))
                },
                ctx,
            ) {
                Ok(rx) => state.pending = Some(rx),
                Err(e) => state.error = Some(e.to_string()),
            }
        }
    }
    if open_contract && let Some(review) = state.review.take() {
        let project = app.project().clone();
        match background(
            move || {
                let next = read(&review.watch, &project)?;
                if next.digest != review.watch.digest {
                    return Err(DomainError::precondition("El contrato cambió tras la revisión"));
                }
                Ok(Completion::OpenContract(Box::new(next)))
            },
            ctx,
        ) {
            Ok(rx) => state.pending = Some(rx),
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    app.v1_documents = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::{LayerKind, SemanticItem, Ticks, TimeRange};
    #[test]
    fn watched_montage_keeps_ids_and_merges_local_audio_with_external_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("montaje.json");
        let asset = tv2_domain::commands::tests_support::fake_video("source", 30);
        let mut project = Project::new("watch montage");
        project.assets.push(asset.clone());
        let mut raw = serde_json::json!({"schema":"editorial-montaje/1","duration_source":30.0,"revision":4,"clips":[{"clip_id":"clip-000001","source_ini":1.0,"source_fin":5.0,"seq_ini":0.0,"origin":"ai","state":"proposed"}]});
        std::fs::write(&path, raw.to_string()).unwrap();
        let initial = DocumentWatch {
            path: path.clone(),
            asset: asset.id,
            digest: String::new(),
            layers: vec![],
            target_sequence: project.active_sequence.clone(),
            sequence: None,
            contract: None,
        };
        let baseline = read(&initial, &project).unwrap();
        let sequence = baseline.sequence.as_ref().unwrap();
        substitute_sequence(&mut project, sequence);
        let again = read(&baseline, &project).unwrap();
        assert_eq!(again.sequence.as_ref(), Some(sequence));
        let id = sequence.clips[0].id.clone();
        project.sequence_mut(&sequence.id).unwrap().clips[0].gain_db = -3.0;
        raw["revision"] = serde_json::json!(5);
        raw["clips"][0]["source_fin"] = serde_json::json!(6.0);
        std::fs::write(&path, raw.to_string()).unwrap();
        let mut review = inspect(&baseline, &project).unwrap().unwrap();
        let change = review.change.as_mut().unwrap();
        assert!(change.conflicts.is_empty());
        change.human_review = true;
        let merged = change.resolved().unwrap();
        let updated = merged.merged.sequence(&sequence.id).unwrap();
        let clip = updated.clips.iter().find(|c| c.id == id).unwrap();
        assert_eq!(clip.gain_db, -3.0);
        assert_eq!(clip.source.end, Ticks::from_seconds(6));
        let request = CommandEnvelope::human(Command::ReconcileProject { project: Box::new(merged.merged) }).with_base(project.revision);
        let mut session = ProjectSession::new(project.clone());
        let prepared = session.prepare_command(request).unwrap();
        session.commit_prepared(prepared).unwrap();
        raw["revision"] = serde_json::json!(3);
        std::fs::write(&path, raw.to_string()).unwrap();
        assert!(read(&baseline, &project).is_err());
    }
    #[test]
    fn watched_contract_exposes_type_without_a_direct_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("topics-proposal.json");
        let asset = tv2_domain::commands::tests_support::fake_video("source", 30);
        let mut project = Project::new("watch contract");
        project.assets.push(asset.clone());
        std::fs::write(&path, r#"{"schema":"editorial-topics-proposal/1","request_id":"review-42","pass":1,"complete":true}"#).unwrap();
        let initial = DocumentWatch {
            path: path.clone(),
            asset: asset.id,
            digest: String::new(),
            layers: vec![],
            target_sequence: None,
            sequence: None,
            contract: None,
        };
        let review = inspect(&initial, &project).unwrap().unwrap();
        assert!(review.change.is_none());
        assert_eq!(review.watch.contract.as_ref().unwrap()["request_id"], "review-42");
        let mut other = review.watch.clone();
        other.path = dir.path().join("other.json");
        assert_ne!(dismiss_key(&review.watch), dismiss_key(&other));
        let roundtrip: DocumentWatch = serde_json::from_slice(&serde_json::to_vec(&review.watch).unwrap()).unwrap();
        assert!(inspect(&roundtrip, &project).unwrap().is_none());
    }
    #[test]
    fn watched_layer_merges_independent_fields_and_rejects_invalid_or_older_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layer.json");
        let asset = tv2_domain::commands::tests_support::fake_video("source", 30);
        let mut project = Project::new("watch");
        project.assets.push(asset.clone());
        let mut layer = SemanticLayer::new(asset.id.clone(), LayerKind::User, "Notes");
        layer.items.push(SemanticItem::new(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(2)), "base"));
        layer.revision = 4;
        project.layer_order.push(layer.layer_id.clone());
        project.layers.push(layer.clone());
        let raw = tv2_v1compat::layers::layer_to_v1(&layer, &asset.fingerprint);
        std::fs::write(&path, raw.to_string()).unwrap();
        let initial = DocumentWatch {
            path: path.clone(),
            asset: asset.id,
            digest: String::new(),
            layers: vec![],
            target_sequence: None,
            sequence: None,
            contract: None,
        };
        let baseline = read(&initial, &project).unwrap();
        project.layers[0].items[0].comment = "local comment".into();
        let mut external = raw.clone();
        external["revision"] = serde_json::json!(5);
        external["items"][0]["label"] = serde_json::json!("external label");
        std::fs::write(&path, external.to_string()).unwrap();
        let mut review = inspect(&baseline, &project).unwrap().unwrap();
        let change = review.change.as_mut().unwrap();
        assert!(change.conflicts.is_empty());
        assert!(change.resolved().is_err(), "human edits require explicit review");
        change.human_review = true;
        let merged = change.resolved().unwrap();
        assert_eq!(merged.merged.layers[0].items[0].label, "external label");
        assert_eq!(merged.merged.layers[0].items[0].comment, "local comment");
        std::fs::write(&path, "{partial").unwrap();
        assert!(read(&baseline, &project).is_err());
        external["revision"] = serde_json::json!(3);
        std::fs::write(&path, external.to_string()).unwrap();
        assert!(read(&baseline, &project).is_err());
        assert_eq!(project.layers[0].items[0].label, "base");
    }
}
