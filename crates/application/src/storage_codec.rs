//! Storage-only shadows externalize large immutable evidence. These Projects
//! never enter the session: readers hydrate and verify before domain validation.
use crate::session::{DurableHistory, JournalEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tv2_domain::{Command, DomainError, Project, SemanticLayer, error::DomainResult, evidence::EvidenceDocument};

pub(crate) const PROJECT_STORAGE: &str = "transcriptor-project-storage/1";
pub(crate) const HISTORY_STORAGE: &str = "transcriptor-history-storage/1";
pub(crate) const EVENT_STORAGE: &str = "transcriptor-audit-event/1";
const REFERENCE: &str = "transcriptor-evidence-ref/1";
const LAYER_REFERENCE: &str = "transcriptor-layer-ref/1";
const LAYER_MARKER: &str = "__tv2_storage_layer";
const INLINE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    schema: String,
    sha256: String,
    bytes: u64,
    canonical_digest: String,
}

fn fingerprint(value: &impl Serialize) -> DomainResult<(String, u64)> {
    struct Sink {
        hash: Sha256,
        bytes: u64,
    }
    impl Write for Sink {
        fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
            self.hash.update(value);
            self.bytes += value.len() as u64;
            Ok(value.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Sink { hash: Sha256::new(), bytes: 0 };
    serde_json::to_writer(&mut sink, value)?;
    Ok((hex::encode(sink.hash.finalize()), sink.bytes))
}

fn blob_path(root: &Path, sha256: &str) -> DomainResult<PathBuf> {
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err(DomainError::invalid("SHA-256 de evidencia almacenada inválido"));
    }
    let path = root.join("evidence").join(format!("{sha256}.json"));
    for candidate in [root.to_path_buf(), root.join("evidence"), path.clone()] {
        if fs::symlink_metadata(candidate).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(DomainError::invalid("enlace en almacenamiento de evidencia"));
        }
    }
    Ok(path)
}

pub(crate) fn unwrap_envelope(raw: &mut serde_json::Value, payload: &str) -> DomainResult<serde_json::Value> {
    let object = raw.as_object_mut().ok_or_else(|| DomainError::invalid("envoltorio de almacenamiento inválido"))?;
    if object.keys().any(|key| key != "schema" && key != payload) {
        return Err(DomainError::unsupported("campos desconocidos en envoltorio de almacenamiento"));
    }
    object.get_mut(payload).map(serde_json::Value::take).ok_or_else(|| DomainError::invalid("envoltorio de almacenamiento sin contenido"))
}

fn read_value(path: &Path, reference: &Reference) -> DomainResult<serde_json::Value> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() != reference.bytes {
        return Err(DomainError::invalid("tamaño de evidencia almacenada inválido"));
    }
    struct Hashed {
        source: std::io::Take<fs::File>,
        hash: Sha256,
        bytes: u64,
    }
    impl Read for Hashed {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let n = self.source.read(buffer)?;
            self.hash.update(&buffer[..n]);
            self.bytes += n as u64;
            Ok(n)
        }
    }
    let mut reader =
        std::io::BufReader::new(Hashed { source: fs::File::open(path)?.take(reference.bytes.saturating_add(1)), hash: Sha256::new(), bytes: 0 });
    let value = serde_json::from_reader(&mut reader)?;
    let hashed = reader.into_inner();
    if hashed.bytes != reference.bytes || hex::encode(hashed.hash.finalize()) != reference.sha256 {
        return Err(DomainError::invalid("SHA-256 de evidencia almacenada inválido"));
    }
    Ok(value)
}

pub(crate) struct Writer {
    root: PathBuf,
    references: HashMap<String, Reference>,
    layer_shadows: HashMap<tv2_domain::LayerId, (SemanticLayer, SemanticLayer)>,
    pub(crate) used: bool,
}
impl Writer {
    pub(crate) fn new(root: &Path) -> Self {
        Self { root: root.into(), references: HashMap::new(), layer_shadows: HashMap::new(), used: false }
    }
    fn document(&mut self, document: &EvidenceDocument) -> DomainResult<EvidenceDocument> {
        let fingerprint = document.serialized_fingerprint();
        if fingerprint.bytes <= INLINE_BYTES && document["schema"] != REFERENCE {
            return Ok(document.clone());
        }
        self.used = true;
        let reference = if let Some(reference) = self.references.get(&fingerprint.sha256) {
            reference.clone()
        } else {
            let path = blob_path(&self.root, &fingerprint.sha256)?;
            let reference = Reference {
                schema: REFERENCE.into(),
                sha256: fingerprint.sha256.clone(),
                bytes: fingerprint.bytes,
                canonical_digest: document.full_digest().into(),
            };
            if path.exists() {
                let file = tv2_domain::evidence::SourceFile { source: path, size: reference.bytes, sha256: reference.sha256.clone() };
                crate::documents::verify_source(&file)?;
            } else {
                let parent = path.parent().ok_or_else(|| DomainError::invalid("evidencia sin carpeta"))?;
                fs::create_dir_all(parent)?;
                let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
                serde_json::to_writer(&mut temporary, document)?;
                temporary.flush()?;
                temporary.as_file().sync_all()?;
                let expected =
                    tv2_domain::evidence::SourceFile { source: temporary.path().into(), size: reference.bytes, sha256: reference.sha256.clone() };
                crate::documents::verify_source(&expected)?;
                temporary.persist_noclobber(&path).map_err(|e| DomainError::io(e.to_string()))?;
            }
            self.references.insert(reference.sha256.clone(), reference.clone());
            reference
        };
        Ok(serde_json::to_value(reference)?.into())
    }
    fn layer(&mut self, layer: &SemanticLayer) -> DomainResult<SemanticLayer> {
        // Adjacent history snapshots share unchanged projections. Equality uses
        // the items' Arc identity before content, avoiding a full serialization
        // for every snapshot. Retain only the latest version of each layer.
        if let Some((source, shadow)) = self.layer_shadows.get(&layer.layer_id)
            && source == layer
        {
            return Ok(shadow.clone());
        }
        let (sha256, bytes) = fingerprint(layer)?;
        if bytes <= INLINE_BYTES && !layer.extra.contains_key(LAYER_MARKER) {
            return Ok(layer.clone());
        }
        self.used = true;
        let reference = if let Some(reference) = self.references.get(&sha256) {
            reference.clone()
        } else {
            let path = blob_path(&self.root, &sha256)?;
            let reference = Reference {
                schema: LAYER_REFERENCE.into(),
                canonical_digest: tv2_domain::digest::digest_json(&serde_json::to_value(layer)?),
                sha256,
                bytes,
            };
            if path.exists() {
                crate::documents::verify_source(&tv2_domain::evidence::SourceFile { source: path, size: bytes, sha256: reference.sha256.clone() })?;
            } else {
                let parent = path.parent().ok_or_else(|| DomainError::invalid("capa sin carpeta"))?;
                fs::create_dir_all(parent)?;
                let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
                serde_json::to_writer(&mut temporary, layer)?;
                temporary.flush()?;
                temporary.as_file().sync_all()?;
                crate::documents::verify_source(&tv2_domain::evidence::SourceFile {
                    source: temporary.path().into(),
                    size: bytes,
                    sha256: reference.sha256.clone(),
                })?;
                temporary.persist_noclobber(&path).map_err(|e| DomainError::io(e.to_string()))?;
            }
            self.references.insert(reference.sha256.clone(), reference.clone());
            reference
        };
        let shadow = layer_shadow(layer, &reference)?;
        self.layer_shadows.insert(layer.layer_id.clone(), (layer.clone(), shadow.clone()));
        Ok(shadow)
    }
    pub(crate) fn project(&mut self, project: &Project) -> DomainResult<Project> {
        let mut shadow = project.clone();
        for master in &mut shadow.masters {
            master.document = self.document(&master.document)?;
        }
        for layer in &mut shadow.layers {
            *layer = self.layer(layer)?;
        }
        Ok(shadow)
    }
    pub(crate) fn history(&mut self, history: &DurableHistory) -> DomainResult<DurableHistory> {
        let mut shadow = history.clone();
        for entry in shadow.undo.iter_mut().chain(&mut shadow.redo) {
            entry.before = self.project(&entry.before)?;
            entry.after = self.project(&entry.after)?;
        }
        Ok(shadow)
    }
    pub(crate) fn command(&mut self, command: &Command) -> DomainResult<Command> {
        let mut shadow = command.clone();
        match &mut shadow {
            Command::AttachMaster { master } => master.document = self.document(&master.document)?,
            Command::ReconcileProject { project } => **project = self.project(project)?,
            Command::ReplaceLayer { layer } => *layer = self.layer(layer)?,
            Command::Batch { commands, .. } => {
                for command in commands {
                    *command = self.command(command)?;
                }
            }
            _ => {}
        }
        Ok(shadow)
    }
    pub(crate) fn event(&mut self, event: &JournalEvent) -> DomainResult<JournalEvent> {
        let mut shadow = event.clone();
        if let Some(command) = &shadow.command {
            shadow.command = Some(self.command(command)?);
        }
        if let Some(receipt) = &mut shadow.receipt {
            receipt.request.command = self.command(&receipt.request.command)?;
        }
        Ok(shadow)
    }
}

fn layer_shadow(layer: &SemanticLayer, reference: &Reference) -> DomainResult<SemanticLayer> {
    let mut shadow = SemanticLayer::new(layer.asset_id.clone(), layer.kind.clone(), "storage");
    shadow.layer_id = layer.layer_id.clone();
    shadow.extra.insert(LAYER_MARKER.into(), serde_json::to_value(reference)?);
    Ok(shadow)
}

pub(crate) struct Reader {
    root: PathBuf,
    documents: HashMap<String, EvidenceDocument>,
    layers: HashMap<String, (String, u64, SemanticLayer)>,
    verified: HashSet<String>,
}
impl Reader {
    pub(crate) fn new(root: &Path) -> Self {
        Self { root: root.into(), documents: HashMap::new(), layers: HashMap::new(), verified: HashSet::new() }
    }
    pub(crate) fn seed_project(&mut self, project: &Project) {
        for master in &project.masters {
            let fingerprint = master.document.serialized_fingerprint();
            if fingerprint.bytes > INLINE_BYTES {
                self.documents.insert(fingerprint.sha256.clone(), master.document.clone());
            }
        }
        for layer in &project.layers {
            if let Ok((sha256, bytes)) = fingerprint(layer)
                && (bytes > INLINE_BYTES || layer.extra.contains_key(LAYER_MARKER))
                && let Ok(raw) = serde_json::to_value(layer)
            {
                self.layers.insert(sha256, (tv2_domain::digest::digest_json(&raw), bytes, layer.clone()));
            }
        }
    }
    fn document(&mut self, document: &EvidenceDocument) -> DomainResult<EvidenceDocument> {
        if document["schema"] != REFERENCE {
            return Ok(document.clone());
        }
        let reference: Reference = serde_json::from_value(serde_json::to_value(document)?)?;
        let path = blob_path(&self.root, &reference.sha256)?;
        if let Some(document) = self.documents.get(&reference.sha256) {
            if document.full_digest() != reference.canonical_digest || document.serialized_fingerprint().bytes != reference.bytes {
                return Err(DomainError::invalid("descriptor de evidencia no coincide con el documento verificado"));
            }
            if !self.verified.contains(&reference.sha256) {
                crate::documents::verify_source(&tv2_domain::evidence::SourceFile {
                    source: path,
                    size: reference.bytes,
                    sha256: reference.sha256.clone(),
                })?;
                self.verified.insert(reference.sha256.clone());
            }
            return Ok(document.clone());
        }
        let value = read_value(&path, &reference)?;
        let document: EvidenceDocument = value.into();
        if document.full_digest() != reference.canonical_digest {
            return Err(DomainError::invalid("digest de evidencia almacenada inválido"));
        }
        self.verified.insert(reference.sha256.clone());
        self.documents.insert(reference.sha256, document.clone());
        Ok(document)
    }
    fn layer(&mut self, layer: &mut SemanticLayer) -> DomainResult<()> {
        let Some(marker) = layer.extra.get(LAYER_MARKER).filter(|marker| marker["schema"] == LAYER_REFERENCE) else {
            return Ok(());
        };
        let reference: Reference = serde_json::from_value(marker.clone())?;
        if *layer != layer_shadow(layer, &reference)? {
            return Err(DomainError::unsupported("campos editados o desconocidos en referencia de capa; no se descartará su contenido"));
        }
        if let Some((canonical, bytes, hydrated)) = self.layers.get(&reference.sha256) {
            if canonical != &reference.canonical_digest || bytes != &reference.bytes || *layer != layer_shadow(hydrated, &reference)? {
                return Err(DomainError::invalid("descriptor de capa no coincide con el contenido verificado"));
            }
            if !self.verified.contains(&reference.sha256) {
                crate::documents::verify_source(&tv2_domain::evidence::SourceFile {
                    source: blob_path(&self.root, &reference.sha256)?,
                    size: reference.bytes,
                    sha256: reference.sha256.clone(),
                })?;
                self.verified.insert(reference.sha256.clone());
            }
            *layer = hydrated.clone();
            return Ok(());
        }
        let path = blob_path(&self.root, &reference.sha256)?;
        let raw = read_value(&path, &reference)?;
        if tv2_domain::digest::digest_json(&raw) != reference.canonical_digest {
            return Err(DomainError::invalid("digest canónico de capa almacenada inválido"));
        }
        let hydrated: SemanticLayer = serde_json::from_value(raw)?;
        if hydrated.layer_id != layer.layer_id || hydrated.asset_id != layer.asset_id || hydrated.kind != layer.kind {
            return Err(DomainError::invalid("capa almacenada de otra identidad"));
        }
        self.verified.insert(reference.sha256.clone());
        self.layers.insert(reference.sha256, (reference.canonical_digest, reference.bytes, hydrated.clone()));
        *layer = hydrated;
        Ok(())
    }
    pub(crate) fn project(&mut self, project: &mut Project) -> DomainResult<()> {
        for master in &mut project.masters {
            master.document = self.document(&master.document)?;
        }
        for layer in &mut project.layers {
            self.layer(layer)?;
        }
        Ok(())
    }
    pub(crate) fn history(&mut self, history: &mut DurableHistory) -> DomainResult<()> {
        for entry in history.undo.iter_mut().chain(&mut history.redo) {
            self.project(&mut entry.before)?;
            self.project(&mut entry.after)?;
        }
        Ok(())
    }
    pub(crate) fn command(&mut self, command: &mut Command) -> DomainResult<()> {
        match command {
            Command::AttachMaster { master } => master.document = self.document(&master.document)?,
            Command::ReconcileProject { project } => self.project(project)?,
            Command::ReplaceLayer { layer } => self.layer(layer)?,
            Command::Batch { commands, .. } => {
                for command in commands {
                    self.command(command)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn event(&mut self, event: &mut JournalEvent) -> DomainResult<()> {
        if let Some(command) = &mut event.command {
            self.command(command)?;
        }
        if let Some(receipt) = &mut event.receipt {
            self.command(&mut receipt.request.command)?;
        }
        Ok(())
    }
}

pub(crate) fn command_has_large_evidence(command: &Command) -> bool {
    match command {
        Command::AttachMaster { master } => master.document.serialized_fingerprint().bytes > INLINE_BYTES || master.document["schema"] == REFERENCE,
        Command::ReconcileProject { project } => {
            project.masters.iter().any(|master| master.document.serialized_fingerprint().bytes > INLINE_BYTES)
                || project
                    .layers
                    .iter()
                    .any(|layer| fingerprint(layer).map_or(true, |(_, bytes)| bytes > INLINE_BYTES) || layer.extra.contains_key(LAYER_MARKER))
        }
        Command::ReplaceLayer { layer } => {
            fingerprint(layer).map_or(true, |(_, bytes)| bytes > INLINE_BYTES) || layer.extra.contains_key(LAYER_MARKER)
        }
        Command::Batch { commands, .. } => commands.iter().any(command_has_large_evidence),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, CommandEnvelope, ProjectSession, ProjectStore};
    use tv2_domain::{evidence::MasterEvidence, layers::LayerKind};

    #[test]
    fn hundred_thousand_items_share_clone_history_undo_and_detach_only_on_mutation() {
        use tv2_domain::{SemanticItem, Ticks, TimeRange};
        let (mut project, _) = fixture(0);
        let mut layer = SemanticLayer::new(project.assets[0].id.clone(), LayerKind::Transcript, "words");
        layer.items = (0..100_000).map(|_| SemanticItem::new(TimeRange::new(Ticks::ZERO, Ticks::from_seconds(1)), "word")).collect();
        project.layers.push(layer);
        let baseline = project.layers[0].items.clone();
        let mut copied = project.clone();
        assert!(baseline.shares_storage(&copied.layers[0].items));
        copied.layers[0].items[0].label = "isolated".into();
        assert!(!baseline.shares_storage(&copied.layers[0].items));
        assert_eq!(baseline[0].label, "word");
        drop(copied);
        let mut session = ProjectSession::new(project);
        session
            .execute(CommandEnvelope::human(Command::RenameProject { name: "renamed".into() }).with_actor(Actor::Agent { name: "agent".into() }))
            .unwrap();
        assert!(baseline.shares_storage(&session.project().layers[0].items));
        let history = session.history_snapshot();
        assert!(baseline.shares_storage(&history.undo[0].before.layers[0].items));
        assert!(baseline.shares_storage(&history.undo[0].after.layers[0].items));
        session.undo(Actor::Human).unwrap();
        assert!(baseline.shares_storage(&session.project().layers[0].items));
        session.redo(Actor::Human).unwrap();
        assert!(baseline.shares_storage(&session.project().layers[0].items));
        let sample = vec![baseline[0].clone()];
        let shared: tv2_domain::SharedVec<_> = sample.clone().into();
        assert_eq!(serde_json::to_value(&shared).unwrap(), serde_json::to_value(&sample).unwrap());
        let decoded: tv2_domain::SharedVec<SemanticItem> = serde_json::from_value(serde_json::to_value(shared).unwrap()).unwrap();
        assert_eq!(decoded.as_slice(), sample.as_slice());
    }

    #[test]
    fn hydrated_project_and_history_share_verified_layer_items() {
        use tv2_domain::{SemanticItem, Ticks, TimeRange};
        let (mut project, _) = fixture(0);
        let mut layer = SemanticLayer::new(project.assets[0].id.clone(), LayerKind::Transcript, "words");
        layer.items = (0..1_000).map(|_| SemanticItem::new(TimeRange::new(Ticks::ZERO, Ticks::from_seconds(1)), "word")).collect();
        project.layers.push(layer);
        let mut session = ProjectSession::new(project);
        session.execute(CommandEnvelope::human(Command::RenameProject { name: "next".into() })).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        store.save_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
        drop(session);
        let restored = store.load_session().unwrap();
        let history = restored.history_snapshot();
        assert!(restored.project().layers[0].items.shares_storage(&history.undo[0].before.layers[0].items));
        assert!(restored.project().layers[0].items.shares_storage(&history.undo[0].after.layers[0].items));
    }

    fn project_digest(project: &Project) -> DomainResult<String> {
        Ok(tv2_domain::digest::digest_json(&serde_json::to_value(project)?))
    }
    fn fixture(bytes: usize) -> (Project, MasterEvidence) {
        let asset = tv2_domain::commands::tests_support::fake_video("a", 30);
        let document: EvidenceDocument = serde_json::json!({"schema":"editorial-master/1", "media":{"fingerprint":asset.fingerprint,"duration":30.0}, "tracks":{}, "opaque_analysis":"x".repeat(bytes)}).into();
        let master = MasterEvidence { asset_id: asset.id.clone(), source_digest: document.source_digest().into(), document, source_bundle: None };
        let mut project = Project::new("large");
        project.assets.push(asset);
        (project, master)
    }

    #[test]
    fn master_above_64mib_survives_checkpoint_history_autosave_and_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        let (project, master) = fixture(65 * 1024 * 1024);
        store.save(&project).unwrap();
        let mut session = ProjectSession::new(project);
        let request = CommandEnvelope::human(Command::AttachMaster { master }).with_idempotency("large-master");
        session.execute(request.clone()).unwrap();
        assert!(session.command_receipt("large-master").unwrap().is_some());
        store.save_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
        let digest = project_digest(session.project()).unwrap();
        for name in ["project.json", "history.json", "audit/index.json"] {
            assert!(fs::metadata(temp.path().join(name)).unwrap().len() < 64 * 1024);
        }
        assert!(fs::read_dir(temp.path().join("evidence")).unwrap().any(|entry| entry.unwrap().metadata().unwrap().len() > 64 * 1024 * 1024));
        session.execute(CommandEnvelope::human(Command::RenameProject { name: "autosaved".into() })).unwrap();
        store.save_autosave_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
        assert!(fs::metadata(temp.path().join("autosave.json")).unwrap().len() < 64 * 1024);
        drop(session);
        let mut restored = store.load_session().unwrap();
        assert_eq!(project_digest(restored.project()).unwrap(), digest);
        assert!(restored.execute(request).unwrap().replayed);
        let recovery = store.recovery_checkpoint(restored.project()).unwrap().unwrap();
        assert_eq!(recovery.project.name, "autosaved");
        recovery.history.unwrap().validate(&recovery.project).unwrap();
        restored.undo(Actor::Human).unwrap();
        assert!(restored.project().masters.is_empty());
        restored.redo(Actor::Human).unwrap();
    }

    #[test]
    fn projected_layer_above_64mib_is_external_and_digest_stable() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        let (mut project, _) = fixture(0);
        let mut layer = SemanticLayer::new(project.assets[0].id.clone(), LayerKind::Transcript, "words");
        layer.extra.insert("projection".into(), serde_json::Value::String("w".repeat(65 * 1024 * 1024)));
        project.layers.push(layer);
        let digest = project_digest(&project).unwrap();
        store.save(&project).unwrap();
        drop(project);
        assert!(fs::metadata(store.project_path()).unwrap().len() < 64 * 1024);
        let restored = store.load().unwrap();
        assert_eq!(project_digest(&restored).unwrap(), digest);
        assert_eq!(restored.layers[0].extra["projection"].as_str().unwrap().len(), 65 * 1024 * 1024);
    }

    #[test]
    fn storage_save_as_preserves_archived_commands_and_detects_blob_corruption() {
        let temp = tempfile::tempdir().unwrap();
        let source = ProjectStore::at(temp.path().join("source"));
        let destination = ProjectStore::at(temp.path().join("destination"));
        let (project, master) = fixture(100 * 1024);
        let mut session = ProjectSession::new(project);
        let request = CommandEnvelope::human(Command::AttachMaster { master }).with_idempotency("copy");
        session.execute(request.clone()).unwrap();
        source.save_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
        destination.copy_audit_from(&source, &session.project().project_id, session.revision()).unwrap();
        destination.save_checkpoint(session.project(), &[], Some(&session.history_snapshot())).unwrap();
        fs::rename(&source.root, temp.path().join("unavailable-source")).unwrap();
        let mut restored = destination.load_session().unwrap();
        assert!(restored.execute(request).unwrap().replayed);
        restored.undo(Actor::Human).unwrap();
        assert!(restored.project().masters.is_empty());
        let blob = fs::read_dir(destination.root.join("evidence")).unwrap().next().unwrap().unwrap().path();
        let original = fs::read(&blob).unwrap();
        let mut corrupt = original.clone();
        corrupt[10] ^= 1;
        fs::write(&blob, corrupt).unwrap();
        assert!(destination.load().is_err());
        fs::write(&blob, original).unwrap();
        let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(destination.project_path()).unwrap()).unwrap();
        raw["project"]["masters"][0]["document"]["canonical_digest"] = "forged".into();
        fs::write(destination.project_path(), raw.to_string()).unwrap();
        assert!(destination.load().is_err());
    }

    #[test]
    fn reserved_markers_are_escaped_and_unknown_envelope_fields_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        let (mut project, master) = fixture(100 * 1024);
        project.masters.push(master);
        let mut layer = SemanticLayer::new(project.assets[0].id.clone(), LayerKind::User, "literal");
        layer.extra.insert(LAYER_MARKER.into(), serde_json::json!({"schema":LAYER_REFERENCE,"legitimate":"metadata"}));
        project.layers.push(layer);
        store.save(&project).unwrap();
        assert_eq!(store.load().unwrap(), project);
        let mut shadow = Writer::new(temp.path()).project(&project).unwrap();
        shadow.layers[0].name = "external edit to stub".into();
        assert!(Reader::new(temp.path()).project(&mut shadow).is_err());
        let literal: EvidenceDocument = serde_json::json!({"schema":REFERENCE,"literal":"user document"}).into();
        let shadow = Writer::new(temp.path()).document(&literal).unwrap();
        assert_eq!(Reader::new(temp.path()).document(&shadow).unwrap(), literal);
        let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(store.project_path()).unwrap()).unwrap();
        raw["future_required"] = true.into();
        fs::write(store.project_path(), raw.to_string()).unwrap();
        assert!(store.load().is_err());
    }
}
