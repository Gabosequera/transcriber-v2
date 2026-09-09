//! Project-owned, content-addressed copies of immutable auxiliary evidence.
//! Blobs are published before a checkpoint can reference them; interrupted copies
//! leave at most unreferenced verified blobs, never a partially published project.
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tv2_domain::{Project, error::DomainResult, evidence::SourceBundle};

use crate::{ProjectStore, session::DurableHistory};

/// Verify external master evidence from bytes rather than trusting its persisted
/// descriptor. Each distinct file/digest pair is parsed once per boundary.
pub(crate) fn verify_external_masters<'a>(store: &ProjectStore, projects: impl Iterator<Item = &'a Project>) -> DomainResult<()> {
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, Read};
    struct HashedReader {
        file: std::fs::File,
        hash: Sha256,
        bytes: u64,
    }
    impl Read for HashedReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let size = self.file.read(buffer)?;
            self.hash.update(&buffer[..size]);
            self.bytes += size as u64;
            Ok(size)
        }
    }
    let mut seen = HashSet::new();
    for master in projects.flat_map(|project| &project.masters) {
        let Some(bundle) = &master.source_bundle else { continue };
        if bundle.external_master_digest().is_none() {
            continue;
        }
        let file = bundle.files().get(bundle.master_path()).ok_or_else(|| tv2_domain::DomainError::invalid("master externo sin archivo"))?;
        let path = store.resolve_path(&file.source.to_string_lossy());
        let expected = master.document.full_digest();
        if !seen.insert((path.clone(), file.size, file.sha256.clone(), expected.to_string())) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&path)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != file.size {
            return Err(tv2_domain::DomainError::invalid("master externo no disponible o cambiado"));
        }
        let mut reader = std::io::BufReader::new(HashedReader { file: std::fs::File::open(path)?, hash: Sha256::new(), bytes: 0 });
        if reader.fill_buf()?.starts_with(&[0xef, 0xbb, 0xbf]) {
            reader.consume(3);
        }
        let parsed: serde_json::Value = serde_json::from_reader(&mut reader)?;
        let reader = reader.into_inner();
        if reader.bytes != file.size || hex::encode(reader.hash.finalize()) != file.sha256 || tv2_domain::digest::digest_json(&parsed) != expected {
            return Err(tv2_domain::DomainError::invalid("bytes o contenido canónico del master externo no coinciden con la evidencia"));
        }
    }
    Ok(())
}

pub(crate) fn verify_checkpoint(store: &ProjectStore, project: &Project, history: Option<&DurableHistory>) -> DomainResult<()> {
    verify_external_masters(
        store,
        std::iter::once(project).chain(history.into_iter().flat_map(|h| h.undo.iter().chain(&h.redo).flat_map(|e| [&e.before, &e.after]))),
    )
}

pub(crate) fn map_project(project: &mut Project, f: &impl Fn(&str) -> String, cache: &mut HashMap<usize, Arc<SourceBundle>>) {
    map_files(project, &|file| f(&file.source.to_string_lossy()).into(), cache);
}

fn map_files(
    project: &mut Project,
    f: &impl Fn(&tv2_domain::evidence::SourceFile) -> std::path::PathBuf,
    cache: &mut HashMap<usize, Arc<SourceBundle>>,
) {
    for master in &mut project.masters {
        let Some(bundle) = &master.source_bundle else { continue };
        let key = Arc::as_ptr(bundle) as usize;
        let mapped = cache.entry(key).or_insert_with(|| {
            let mut files = bundle.files().clone();
            for file in files.values_mut() {
                file.source = f(file);
            }
            if &files == bundle.files() { bundle.clone() } else { Arc::new(bundle.relocated(files)) }
        });
        master.source_bundle = Some(mapped.clone());
    }
}

/// Switch live evidence locators only for content present in the saved frozen
/// checkpoint. Later imports remain untouched while concurrent edits survive.
pub(crate) fn adopt_projects<'a>(projects: impl Iterator<Item = &'a mut Project>, store: &ProjectStore, saved: &Project, history: &DurableHistory) {
    let mut files = HashMap::new();
    for p in std::iter::once(saved).chain(history.undo.iter().chain(&history.redo).flat_map(|e| [&e.before, &e.after])) {
        for file in p.masters.iter().filter_map(|m| m.source_bundle.as_ref()).flat_map(|b| b.files().values()) {
            files.insert((file.sha256.clone(), file.size), store.resolve_path(&file.source.to_string_lossy()));
        }
    }
    let mut cache = HashMap::new();
    for project in projects {
        map_files(project, &|f| files.get(&(f.sha256.clone(), f.size)).cloned().unwrap_or_else(|| f.source.clone()), &mut cache);
        project.schema.clone_from(&saved.schema);
    }
}

/// Map both media and auxiliary paths. Source documents, digests and identities
/// remain immutable; only their storage locators change.
pub fn map_project_paths(project: &mut Project, f: impl Fn(&str) -> String) {
    for asset in &mut project.assets {
        asset.path = f(&asset.path);
    }
    map_project(project, &f, &mut HashMap::new());
}

impl ProjectStore {
    /// Called on the IO worker before saving, including Save As. Inputs must
    /// resolve against their source store first. The returned checkpoint uses
    /// root-relative locators which survive moving or renaming its folder.
    pub fn materialize_source_bundles(&self, project: &mut Project, history: &mut DurableHistory) -> DomainResult<()> {
        project.validate()?;
        history.validate(project)?;
        let mut copies = HashSet::new();
        for p in std::iter::once(&*project).chain(history.undo.iter().chain(&history.redo).flat_map(|e| [&e.before, &e.after])) {
            for bundle in p.masters.iter().filter_map(|m| m.source_bundle.as_ref()) {
                for file in bundle.files().values() {
                    if !copies.insert((file.source.clone(), file.size, file.sha256.clone())) {
                        continue;
                    }
                    if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
                        return Err(tv2_domain::DomainError::invalid("SHA-256 inválido en evidencia auxiliar"));
                    }
                    let relative = format!("source-bundles/{}", file.sha256);
                    let target = self.root.join(&relative);
                    for path in [&self.root, &self.root.join("source-bundles"), &target] {
                        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                            return Err(tv2_domain::DomainError::invalid("enlace en almacén de evidencia auxiliar"));
                        }
                    }
                    let mut source = file.clone();
                    source.source = self.resolve_path(&source.source.to_string_lossy());
                    let mut stored = source.clone();
                    stored.source = target.clone();
                    if target.exists() {
                        crate::documents::verify_source(&stored)?;
                    } else {
                        crate::documents::verify_source(&source)?;
                        crate::documents::copy_verified(&source, &target, &None)?;
                    }
                }
            }
        }
        let mut cache = HashMap::<usize, Arc<SourceBundle>>::new();
        for p in std::iter::once(&mut *project).chain(history.undo.iter_mut().chain(&mut history.redo).flat_map(|e| [&mut e.before, &mut e.after])) {
            p.schema = tv2_domain::PROJECT_SCHEMA.into();
            for master in &mut p.masters {
                let Some(bundle) = &master.source_bundle else { continue };
                let mapped = cache.entry(Arc::as_ptr(bundle) as usize).or_insert_with(|| {
                    let mut files = bundle.files().clone();
                    for file in files.values_mut() {
                        file.source = format!("@project/source-bundles/{}", file.sha256).into();
                    }
                    if &files == bundle.files() { bundle.clone() } else { Arc::new(bundle.relocated(files)) }
                });
                master.source_bundle = Some(mapped.clone());
            }
        }
        history.validate(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, CommandEnvelope, ProjectSession};
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, fs};
    use tv2_domain::{
        Command,
        evidence::{EvidenceDocument, MasterEvidence, SourceFile},
    };

    fn fixture(source: &std::path::Path) -> ProjectSession {
        let bytes = b"\0\xff immutable auxiliary\r\n";
        fs::write(source, bytes).unwrap();
        let file = SourceFile { source: source.into(), size: bytes.len() as u64, sha256: hex::encode(Sha256::digest(bytes)) };
        let asset = tv2_domain::commands::tests_support::fake_video("a", 30);
        let raw = serde_json::json!({"schema":"editorial-master/1","media":{"fingerprint":asset.fingerprint,"duration":30.0},"tracks":{}});
        let document: EvidenceDocument = raw.clone().into();
        let bundle = SourceBundle::new("master.json".into(), BTreeMap::from([("master.json".into(), raw.to_string())]))
            .with_files(BTreeMap::from([("a.bin".into(), file.clone()), ("nested/copy.bin".into(), file)]))
            .with_directories(std::collections::BTreeSet::from([("empty/nested".into())]));
        let mut project = Project::new("source");
        project.assets.push(asset.clone());
        project.masters.push(MasterEvidence {
            asset_id: asset.id,
            source_digest: document.source_digest().into(),
            document,
            source_bundle: Some(Arc::new(bundle)),
        });
        let mut session = ProjectSession::new(project);
        session.execute(CommandEnvelope::human(Command::RenameProject { name: "saved".into() })).unwrap();
        session
    }

    #[test]
    fn legacy_master_gains_source_bundle_without_replacing_evidence_or_human_layers_and_survives_save_as() {
        let temp = tempfile::tempdir().unwrap();
        let auxiliary = temp.path().join("original.bin");
        let source_session = fixture(&auxiliary);
        let enriched = source_session.project().masters[0].clone();
        let mut legacy = source_session.project().clone();
        legacy.schema = tv2_domain::project::LEGACY_PROJECT_SCHEMA.into();
        legacy.masters[0].source_bundle = None;
        let original_document = legacy.masters[0].document.clone();
        let mut layer = tv2_domain::SemanticLayer::new(legacy.assets[0].id.clone(), tv2_domain::LayerKind::User, "Human review");
        layer.items.push(tv2_domain::SemanticItem::new(
            tv2_domain::TimeRange::new(tv2_domain::Ticks::ZERO, tv2_domain::Ticks::from_seconds(1)),
            "Keep my edit",
        ));
        legacy.layers.push(layer);
        let human_layers = legacy.layers.clone();
        let source = ProjectStore::at(temp.path().join("legacy.transcriptor"));
        source.save(&legacy).unwrap();
        let mut session = source.load_session().unwrap();
        let request = CommandEnvelope::human(Command::AttachMaster { master: enriched })
            .with_actor(Actor::External { source: "original folder".into() })
            .with_idempotency("legacy-folder");
        session.execute(request.clone()).unwrap();
        assert_eq!(session.project().layers, human_layers);
        assert_eq!(session.project().masters.len(), 1);
        assert_eq!(session.project().masters[0].document, original_document);
        assert!(session.execute(request.clone()).unwrap().replayed);
        source.save_autosave_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
        let recovery = source.recovery_checkpoint(&legacy).unwrap().unwrap();
        let mut recovered = ProjectSession::new(legacy.clone());
        recovered.recover_checkpoint(recovery.project, recovery.events, recovery.history).unwrap();
        assert!(recovered.project().masters[0].source_bundle.is_some());
        assert_eq!(recovered.project().layers, human_layers);
        session.undo(Actor::Human).unwrap();
        assert!(session.project().masters[0].source_bundle.is_none());
        session.redo(Actor::Human).unwrap();
        assert!(session.project().masters[0].source_bundle.is_some());
        let mut saved = session.project().clone();
        let mut history = session.history_snapshot();
        source.materialize_source_bundles(&mut saved, &mut history).unwrap();
        source.save_checkpoint(&saved, session.pending_journal(), Some(&history)).unwrap();
        let mut reopened = source.load_session().unwrap();
        reopened.resolve_paths(|path| source.resolve_path(path).to_string_lossy().into());
        let destination = ProjectStore::at(temp.path().join("copy.transcriptor"));
        destination.copy_audit_from(&source, &reopened.project().project_id, reopened.revision()).unwrap();
        let mut saved = reopened.project().clone();
        let mut history = reopened.history_snapshot();
        destination.materialize_source_bundles(&mut saved, &mut history).unwrap();
        destination.save_checkpoint(&saved, &[], Some(&history)).unwrap();
        fs::rename(&source.root, temp.path().join("moved-source")).unwrap();
        fs::remove_file(auxiliary).unwrap();
        let mut restored = destination.load_session().unwrap();
        restored.resolve_paths(|path| destination.resolve_path(path).to_string_lossy().into());
        assert_eq!(restored.project().layers, human_layers);
        assert_eq!(restored.project().masters[0].document, original_document);
        assert!(restored.execute(request).unwrap().replayed);
        restored.undo(Actor::Human).unwrap();
        assert!(restored.project().masters[0].source_bundle.is_none());
        restored.redo(Actor::Human).unwrap();
        for file in restored.project().masters[0].source_bundle.as_ref().unwrap().files().values() {
            crate::documents::verify_source(file).unwrap();
        }
    }

    #[test]
    fn source_enrichment_rejects_changed_master_wrong_bundle_and_existing_bundle_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(&temp.path().join("aux.bin"));
        let original = source.project().clone();
        let mut session = ProjectSession::new(original.clone());
        let mut no_bundle = original.clone();
        no_bundle.masters[0].source_bundle = None;
        assert!(session.execute(CommandEnvelope::human(Command::ReconcileProject { project: Box::new(no_bundle.clone()) })).is_err());
        let bundle = original.masters[0].source_bundle.as_ref().unwrap();
        let mut replaced = original.masters[0].clone();
        replaced.source_bundle = Some(Arc::new(
            SourceBundle::new(bundle.master_path().into(), bundle.documents().clone())
                .with_directories(std::collections::BTreeSet::from(["different".into()])),
        ));
        assert!(session.execute(CommandEnvelope::human(Command::AttachMaster { master: replaced })).is_err());
        let mut changed = original.masters[0].clone();
        let mut raw = serde_json::to_value(&changed.document).unwrap();
        raw["original_analysis_changed"] = true.into();
        changed.document = raw.into();
        changed.source_digest = changed.document.source_digest().into();
        changed.source_bundle = None;
        assert!(session.execute(CommandEnvelope::human(Command::AttachMaster { master: changed.clone() })).is_err());
        let mut legacy = ProjectSession::new(no_bundle);
        let before = legacy.project().clone();
        let mut wrong_bundle = before.masters[0].clone();
        wrong_bundle.source_bundle = Some(Arc::new(SourceBundle::new(
            bundle.master_path().into(),
            BTreeMap::from([(bundle.master_path().into(), serde_json::to_string(&changed.document).unwrap())]),
        )));
        assert!(legacy.execute(CommandEnvelope::human(Command::AttachMaster { master: wrong_bundle })).is_err());
        changed.source_bundle = original.masters[0].source_bundle.clone();
        assert!(legacy.execute(CommandEnvelope::human(Command::AttachMaster { master: changed })).is_err());
        assert_eq!(legacy.project(), &before);
        assert_eq!(session.project(), &original);
    }

    #[test]
    fn recovery_after_undo_enrichment_requires_validated_pure_redo_proof_and_restores_portable_redo() {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(&temp.path().join("aux.bin"));
        let master = source.project().masters[0].clone();
        let mut legacy = source.project().clone();
        legacy.masters[0].source_bundle = None;
        let store = ProjectStore::at(temp.path().join("owned.transcriptor"));
        store.save(&legacy).unwrap();
        let mut session = ProjectSession::new(legacy);
        session.execute(CommandEnvelope::human(Command::AttachMaster { master }).with_idempotency("undo-source")).unwrap();
        let mut saved = session.project().clone();
        let mut history = session.history_snapshot();
        store.materialize_source_bundles(&mut saved, &mut history).unwrap();
        store.save_checkpoint(&saved, session.pending_journal(), Some(&history)).unwrap();
        session.adopt_source_bundle_locations(&store, &saved, &history);
        session.undo(Actor::Human).unwrap();
        assert!(session.project().masters[0].source_bundle.is_none());
        store.save_autosave_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
        let mut reopened = store.load_session().unwrap();
        let recovery = store.recovery_checkpoint(reopened.project()).unwrap().unwrap();
        let baseline = reopened.project().clone();
        assert!(reopened.recover(recovery.project.clone()).is_err());
        assert_eq!(reopened.project(), &baseline);
        let mut forged = recovery.history.clone().unwrap();
        forged.redo.back_mut().unwrap().after.name = "not a pure enrichment".into();
        forged.validate(&recovery.project).unwrap();
        assert!(reopened.recover_checkpoint(recovery.project.clone(), recovery.events.clone(), Some(forged)).is_err());
        assert_eq!(reopened.project(), &baseline);
        reopened.recover_checkpoint(recovery.project, recovery.events, recovery.history).unwrap();
        assert!(reopened.project().masters[0].source_bundle.is_none());
        assert!(reopened.can_redo());
        reopened.redo(Actor::Human).unwrap();
        let bundle = reopened.project().masters[0].source_bundle.as_ref().unwrap();
        for file in bundle.files().values() {
            assert!(file.source.to_string_lossy().starts_with("@project/"));
            let mut located = file.clone();
            located.source = store.resolve_path(&file.source.to_string_lossy());
            crate::documents::verify_source(&located).unwrap();
        }
        store.save_checkpoint(reopened.project(), reopened.pending_journal(), Some(&reopened.history_snapshot())).unwrap();
        let final_session = store.load_session().unwrap();
        assert_eq!(final_session.project().masters, reopened.project().masters);
        let moved = temp.path().join("moved-owned.transcriptor");
        fs::rename(&store.root, &moved).unwrap();
        let moved = ProjectStore::at(moved);
        let restored = moved.load_session().unwrap();
        for file in restored.project().masters[0].source_bundle.as_ref().unwrap().files().values() {
            let mut located = file.clone();
            located.source = moved.resolve_path(&file.source.to_string_lossy());
            crate::documents::verify_source(&located).unwrap();
        }
    }

    #[test]
    fn save_as_auxiliaries_survive_source_loss_folder_move_undo_and_export() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("original.bin");
        let mut session = fixture(&source);
        let store = ProjectStore::at(temp.path().join("destination.transcriptor"));
        let mut project = session.project().clone();
        let mut history = session.history_snapshot();
        store.materialize_source_bundles(&mut project, &mut history).unwrap();
        store.save_checkpoint(&project, session.pending_journal(), Some(&history)).unwrap();
        assert_eq!(fs::read_dir(store.root.join("source-bundles")).unwrap().count(), 1);
        session.execute(CommandEnvelope::human(Command::RenameProject { name: "concurrent".into() })).unwrap();
        session.adopt_source_bundle_locations(&store, &project, &history);
        assert_eq!(session.project().name, "concurrent");
        session.history_snapshot().validate(session.project()).unwrap();
        fs::remove_file(&source).unwrap();
        for file in session.project().masters[0].source_bundle.as_ref().unwrap().files().values() {
            crate::documents::verify_source(file).unwrap();
        }
        let moved = temp.path().join("renamed.transcriptor");
        fs::rename(&store.root, &moved).unwrap();
        let reopened = ProjectStore::at(moved);
        let mut restored = reopened.load_session().unwrap();
        restored.resolve_paths(|path| reopened.resolve_path(path).to_string_lossy().into());
        restored.undo(Actor::Human).unwrap();
        assert_eq!(restored.project().name, "source");
        restored.redo(Actor::Human).unwrap();
        let bundle = restored.project().masters[0].source_bundle.as_ref().unwrap();
        let export = temp.path().join("export");
        crate::documents::create(&export).unwrap();
        crate::documents::publish_with_directories(&export, bundle.documents(), bundle.files(), bundle.directories()).unwrap();
        assert!(export.join("empty/nested").is_dir());
        assert_eq!(fs::read(export.join("a.bin")).unwrap(), b"\0\xff immutable auxiliary\r\n");
        let second = ProjectStore::at(temp.path().join("second.transcriptor"));
        let mut p = restored.project().clone();
        let mut h = restored.history_snapshot();
        second.materialize_source_bundles(&mut p, &mut h).unwrap();
        second.save_checkpoint(&p, &reopened.read_journal().unwrap(), Some(&h)).unwrap();
    }

    #[test]
    fn changed_auxiliary_aborts_before_checkpoint_and_corrupt_owned_blob_is_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("original.bin");
        let session = fixture(&source);
        let store = ProjectStore::at(temp.path().join("target.transcriptor"));
        let original = session.project().clone();
        let mut project = original.clone();
        let mut history = session.history_snapshot();
        fs::write(&source, b"changed").unwrap();
        assert!(store.materialize_source_bundles(&mut project, &mut history).is_err());
        assert_eq!(project, original);
        assert!(!store.project_path().exists());
        fs::write(&source, b"\0\xff immutable auxiliary\r\n").unwrap();
        store.materialize_source_bundles(&mut project, &mut history).unwrap();
        let file = project.masters[0].source_bundle.as_ref().unwrap().files()["a.bin"].clone();
        let target = store.resolve_path(&file.source.to_string_lossy());
        fs::write(&target, b"external edit").unwrap();
        assert!(store.materialize_source_bundles(&mut project, &mut history).is_err());
        assert_eq!(fs::read(target).unwrap(), b"external edit");
    }

    #[test]
    fn legacy_checkpoint_migrates_only_on_save_preserving_history_receipts_and_extras() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path().join("legacy.transcriptor"));
        let mut project = Project::new("legacy");
        project.schema = tv2_domain::project::LEGACY_PROJECT_SCHEMA.into();
        project.extra.insert("future_metadata".into(), serde_json::json!({"nested":[1,2,3]}));
        let mut session = ProjectSession::new(project);
        let request = CommandEnvelope::human(Command::RenameProject { name: "first".into() }).with_idempotency("legacy-receipt");
        session.execute(request.clone()).unwrap();
        session.execute(CommandEnvelope::human(Command::RenameProject { name: "second".into() })).unwrap();
        session.undo(Actor::Human).unwrap();
        store.save_checkpoint(session.project(), session.pending_journal(), Some(&session.history_snapshot())).unwrap();
        let raw_before = fs::read(store.project_path()).unwrap();
        let mut opened = store.load_session().unwrap();
        assert_eq!(opened.project().schema, "transcriptor-project/1");
        assert_eq!(fs::read(store.project_path()).unwrap(), raw_before);
        let mut saved = opened.project().clone();
        let mut history = opened.history_snapshot();
        store.materialize_source_bundles(&mut saved, &mut history).unwrap();
        store.save_checkpoint(&saved, &[], Some(&history)).unwrap();
        opened.adopt_source_bundle_locations(&store, &saved, &history);
        assert_eq!(opened.project().schema, "transcriptor-project/2");
        opened.history_snapshot().validate(opened.project()).unwrap();
        let mut reopened = store.load_session().unwrap();
        assert!(reopened.execute(request).unwrap().replayed);
        reopened.redo(Actor::Human).unwrap();
        assert_eq!(reopened.project().name, "second");
        reopened.undo(Actor::Human).unwrap();
        reopened.undo(Actor::Human).unwrap();
        assert_eq!(reopened.project().name, "legacy");
        assert_eq!(reopened.project().schema, "transcriptor-project/2");
        assert_eq!(reopened.project().extra["future_metadata"], serde_json::json!({"nested":[1,2,3]}));
    }

    #[test]
    fn future_schema_or_unknown_bundle_contract_is_rejected_without_rewriting() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        let session = fixture(&temp.path().join("original.bin"));
        let mut raw = serde_json::to_value(session.project()).unwrap();
        raw["schema"] = serde_json::json!("transcriptor-project/999");
        fs::write(store.project_path(), raw.to_string()).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read_to_string(store.project_path()).unwrap(), raw.to_string());
        raw["schema"] = serde_json::json!("transcriptor-project/2");
        raw["masters"][0]["source_bundle"]["future_storage"] = serde_json::json!({"must_preserve":true});
        fs::write(store.project_path(), raw.to_string()).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read_to_string(store.project_path()).unwrap(), raw.to_string());
    }

    #[test]
    fn external_master_survives_save_as_and_rejects_tampered_bytes_or_forged_descriptors() {
        let temp = tempfile::tempdir().unwrap();
        let session = fixture(&temp.path().join("aux.bin"));
        let mut project = session.project().clone();
        let raw = serde_json::to_string(&project.masters[0].document).unwrap();
        let original = temp.path().join("master-original.json");
        let original_bytes = format!("\u{feff}{raw}\r\n").into_bytes();
        fs::write(&original, &original_bytes).unwrap();
        let file = SourceFile { source: original.clone(), size: original_bytes.len() as u64, sha256: hex::encode(Sha256::digest(&original_bytes)) };
        project.masters[0].source_bundle = Some(Arc::new(
            SourceBundle::new("master.json".into(), BTreeMap::new())
                .with_files(BTreeMap::from([("master.json".into(), file)]))
                .with_external_master_digest(project.masters[0].document.full_digest().into()),
        ));
        let session = ProjectSession::new(project);
        let mut project = session.project().clone();
        let mut history = session.history_snapshot();
        let store = ProjectStore::at(temp.path().join("copy.transcriptor"));
        store.materialize_source_bundles(&mut project, &mut history).unwrap();
        store.save_checkpoint(&project, &[], Some(&history)).unwrap();
        fs::remove_file(original).unwrap();
        assert_eq!(store.load().unwrap(), project);
        let bundle = project.masters[0].source_bundle.as_ref().unwrap();
        let target = store.resolve_path(&bundle.files()["master.json"].source.to_string_lossy());
        fs::write(&target, original_bytes.iter().copied().map(|byte| if byte == b'{' { b'[' } else { byte }).collect::<Vec<_>>()).unwrap();
        assert!(store.load().is_err());
        fs::write(&target, &original_bytes).unwrap();
        let mut forged = serde_json::to_value(&project).unwrap();
        forged["masters"][0]["document"]["invented_evidence"] = serde_json::json!(true);
        let evidence: EvidenceDocument = forged["masters"][0]["document"].clone().into();
        forged["masters"][0]["source_digest"] = serde_json::json!(evidence.source_digest());
        forged["masters"][0]["source_bundle"]["external_master_digest"] = serde_json::json!(evidence.full_digest());
        assert!(serde_json::from_value::<Project>(forged.clone()).unwrap().validate().is_ok());
        fs::write(store.project_path(), forged.to_string()).unwrap();
        assert!(store.load().is_err()); // descriptor alone cannot establish original evidence
        assert_eq!(fs::read(target).unwrap(), original_bytes);
    }
}
