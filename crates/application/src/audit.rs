//! Append-only audit segments. A small atomic index selects immutable, verified
//! blobs; retries publish the same content and never discard idempotency receipts.
use crate::session::JournalEvent;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, io::Read, path::Path};
use tv2_domain::{DomainError, error::DomainResult};

const INDEX: &str = "audit/index.json";
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const SEGMENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    sha256: String,
    bytes: u64,
    events: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    storage: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditIndex {
    schema: String,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditCommit {
    before: Option<String>,
    pub(crate) after: AuditIndex,
}

fn index_digest(index: Option<&AuditIndex>) -> DomainResult<Option<String>> {
    index.map(|value| serde_json::to_value(value).map(|value| tv2_domain::digest::digest_json(&value)).map_err(Into::into)).transpose()
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalPage {
    pub events: Vec<JournalEvent>,
    pub next_cursor: Option<String>,
    pub snapshot_digest: String,
}

fn bytes(path: &Path) -> DomainResult<Vec<u8>> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(DomainError::invalid("enlace en archivo de auditoría"));
    }
    let mut data = Vec::new();
    fs::File::open(path)?.take(MAX_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_BYTES {
        return Err(DomainError::invalid("segmento de auditoría supera 64 MiB"));
    }
    Ok(data)
}

pub(crate) fn index(root: &Path) -> DomainResult<Option<AuditIndex>> {
    let path = root.join(INDEX);
    if !path.exists() {
        return Ok(None);
    }
    let result: AuditIndex = serde_json::from_slice(&bytes(&path)?)?;
    if !matches!(result.schema.as_str(), "transcriptor-audit/1" | "transcriptor-audit/2") {
        return Err(DomainError::unsupported("schema de archivo de auditoría desconocido"));
    }
    for segment in &result.segments {
        if segment.sha256.len() != 64
            || !segment.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || segment.bytes > MAX_BYTES
            || segment.events == 0
        {
            return Err(DomainError::invalid("índice de auditoría inválido"));
        }
    }
    Ok(Some(result))
}

fn read_segment(root: &Path, segment: &Segment) -> DomainResult<Vec<JournalEvent>> {
    if segment.sha256.len() != 64 || !segment.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) || segment.bytes > MAX_BYTES {
        return Err(DomainError::invalid("localizador de archivo de auditoría inválido"));
    }
    let raw = bytes(&root.join("audit/segments").join(format!("{}.jsonl", segment.sha256)))?;
    if raw.len() as u64 != segment.bytes || hex::encode(Sha256::digest(&raw)) != segment.sha256 {
        return Err(DomainError::invalid("digest de segmento de auditoría inválido"));
    }
    let events = parse(&raw, false, segment.storage.then_some(root))?;
    if events.len() != segment.events {
        return Err(DomainError::invalid("contador de auditoría inválido"));
    }
    Ok(events)
}

fn parse(raw: &[u8], allow_partial_tail: bool, storage_root: Option<&Path>) -> DomainResult<Vec<JournalEvent>> {
    let mut events = Vec::new();
    let mut reader = storage_root.map(crate::storage_codec::Reader::new);
    for line in raw.split_inclusive(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(mut value) => {
                let storage = reader.is_some() && value["schema"] == crate::storage_codec::EVENT_STORAGE;
                if storage {
                    value = crate::storage_codec::unwrap_envelope(&mut value, "event")?;
                }
                let mut event = serde_json::from_value(value)?;
                if storage {
                    reader.as_mut().unwrap().event(&mut event)?;
                }
                events.push(event);
            }
            Err(_) if allow_partial_tail && !line.ends_with(b"\n") => break,
            Err(error) => return Err(DomainError::invalid(format!("auditoría corrupta: {error}"))),
        }
    }
    Ok(events)
}

pub(crate) fn visit(root: &Path, snapshot: Option<&AuditIndex>, mut f: impl FnMut(JournalEvent) -> DomainResult<()>) -> DomainResult<()> {
    if let Some(index) = snapshot {
        if !matches!(index.schema.as_str(), "transcriptor-audit/1" | "transcriptor-audit/2") {
            return Err(DomainError::unsupported("schema de archivo de auditoría desconocido"));
        }
        for segment in &index.segments {
            for event in read_segment(root, segment)? {
                f(event)?;
            }
        }
    } else {
        let path = root.join(crate::JOURNAL_FILE);
        if path.exists() {
            for event in parse(&bytes(&path)?, true, None)? {
                f(event)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn additions(root: &Path, events: &[JournalEvent]) -> DomainResult<Vec<JournalEvent>> {
    let mut pending = HashMap::new();
    for event in events {
        if let Some(previous) = pending.insert(event.command_id.as_str(), event)
            && previous != event
        {
            return Err(DomainError::precondition("command_id de auditoría reutilizado con otro contenido"));
        }
    }
    let snapshot = index(root)?;
    visit(root, snapshot.as_ref(), |event| {
        if let Some(candidate) = pending.remove(event.command_id.as_str())
            && candidate != &event
        {
            return Err(DomainError::precondition("command_id de auditoría reutilizado con otro contenido"));
        }
        Ok(())
    })?;
    let mut result = Vec::new();
    for event in events {
        if pending.remove(event.command_id.as_str()).is_some() {
            result.push(event.clone());
        }
    }
    Ok(result)
}

fn append_segments(root: &Path, index: &mut AuditIndex, events: &[JournalEvent]) -> DomainResult<()> {
    let mut segment = Vec::new();
    let mut count = 0;
    let mut writer = crate::storage_codec::Writer::new(root);
    let flush = |segment: &mut Vec<u8>, count: &mut usize, index: &mut AuditIndex, storage: bool| -> DomainResult<()> {
        if segment.is_empty() {
            return Ok(());
        }
        let sha256 = hex::encode(Sha256::digest(&*segment));
        let path = root.join("audit/segments").join(format!("{sha256}.jsonl"));
        if path.exists() {
            if bytes(&path)? != *segment {
                return Err(DomainError::precondition("segmento de auditoría modificado externamente"));
            }
        } else {
            crate::store::atomic_write(&path, segment)?;
        }
        index.segments.push(Segment { sha256, bytes: segment.len() as u64, events: *count, storage });
        if storage {
            index.schema = "transcriptor-audit/2".into();
        }
        segment.clear();
        *count = 0;
        Ok(())
    };
    for event in events {
        let shadow = writer.event(event)?;
        let mut raw = if writer.used {
            serde_json::to_vec(&serde_json::json!({"schema":crate::storage_codec::EVENT_STORAGE,"event":shadow}))?
        } else {
            serde_json::to_vec(&shadow)?
        };
        raw.push(b'\n');
        if raw.len() as u64 > MAX_BYTES {
            return Err(DomainError::invalid("evento de auditoría supera 64 MiB"));
        }
        if segment.len() + raw.len() > SEGMENT_BYTES {
            flush(&mut segment, &mut count, index, writer.used)?;
        }
        segment.extend(raw);
        count += 1;
    }
    flush(&mut segment, &mut count, index, writer.used)
}

pub(crate) fn prepare(root: &Path, events: &[JournalEvent]) -> DomainResult<AuditCommit> {
    for path in [root.join("audit"), root.join("audit/segments")] {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(DomainError::invalid("enlace en archivo de auditoría"));
        }
    }
    let additions = additions(root, events)?;
    let existing = index(root)?;
    let before = index_digest(existing.as_ref())?;
    let mut next = existing.unwrap_or_else(|| AuditIndex { schema: "transcriptor-audit/1".into(), segments: Vec::new() });
    if !root.join(INDEX).exists() {
        let mut legacy = Vec::new();
        visit(root, None, |event| {
            legacy.push(event);
            Ok(())
        })?;
        append_segments(root, &mut next, &legacy)?;
    }
    append_segments(root, &mut next, &additions)?;
    let raw = serde_json::to_vec(&next)?;
    if raw.len() as u64 > MAX_BYTES {
        return Err(DomainError::invalid("índice de auditoría supera 64 MiB"));
    }
    if index_digest(index(root)?.as_ref())? != before {
        return Err(DomainError::precondition("el índice de auditoría cambió durante publicación"));
    }
    Ok(AuditCommit { before, after: next })
}

pub(crate) fn validate_commit(root: &Path, plan: &AuditCommit) -> DomainResult<()> {
    let current = index_digest(index(root)?.as_ref())?;
    if current != plan.before && current != index_digest(Some(&plan.after))? {
        return Err(DomainError::precondition("el índice de auditoría cambió durante un commit pendiente"));
    }
    visit(root, Some(&plan.after), |_| Ok(()))
}

pub(crate) fn commit(root: &Path, plan: &AuditCommit) -> DomainResult<()> {
    validate_commit(root, plan)?;
    if index_digest(index(root)?.as_ref())? != index_digest(Some(&plan.after))? {
        crate::store::atomic_write(&root.join(INDEX), &serde_json::to_vec(&plan.after)?)?;
    }
    Ok(())
}

pub(crate) fn publish(root: &Path, events: &[JournalEvent]) -> DomainResult<()> {
    commit(root, &prepare(root, events)?)
}

/// Copy immutable segments before publishing the destination index. The caller
/// holds both project locks and has checked project identity/revision.
pub(crate) fn copy_to(source: &Path, destination: &Path) -> DomainResult<()> {
    let Some(snapshot) = index(source)? else {
        let mut legacy = Vec::new();
        visit(source, None, |event| {
            legacy.push(event);
            Ok(())
        })?;
        return publish(destination, &legacy);
    };
    if let Some(existing) = index(destination)?
        && serde_json::to_value(&existing)? != serde_json::to_value(&snapshot)?
    {
        return Err(DomainError::precondition("el destino ya contiene otra auditoría"));
    }
    for segment in &snapshot.segments {
        // Validate each blob's bytes and record count before copying it.
        let events = read_segment(source, segment)?;
        let mut writer = crate::storage_codec::Writer::new(destination);
        for event in &events {
            writer.event(event)?;
        }
        let relative = format!("audit/segments/{}.jsonl", segment.sha256);
        let target = destination.join(&relative);
        for path in [destination.to_path_buf(), destination.join("audit"), destination.join("audit/segments"), target.clone()] {
            if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(DomainError::invalid("enlace en destino de archivo de auditoría"));
            }
        }
        let file = tv2_domain::evidence::SourceFile { source: source.join(relative), size: segment.bytes, sha256: segment.sha256.clone() };
        if target.exists() {
            let mut expected = file.clone();
            expected.source = target;
            crate::documents::verify_source(&expected)?;
        } else {
            crate::documents::copy_verified(&file, &target, &None)?;
        }
    }
    crate::store::atomic_write(&destination.join(INDEX), &serde_json::to_vec(&snapshot)?)
}

pub(crate) fn page(root: &Path, cursor: Option<&str>, limit: usize) -> DomainResult<JournalPage> {
    if !(1..=500).contains(&limit) {
        return Err(DomainError::out_of_range("página de auditoría requiere entre 1 y 500 eventos"));
    }
    let snapshot = index(root)?;
    let digest = if let Some(index) = &snapshot {
        tv2_domain::digest::digest_json(&serde_json::to_value(index)?)
    } else {
        let path = root.join(crate::JOURNAL_FILE);
        hex::encode(Sha256::digest(if path.exists() { bytes(&path)? } else { Vec::new() }))
    };
    let offset = if let Some(cursor) = cursor {
        let (base, offset) = cursor.rsplit_once(':').ok_or_else(|| DomainError::invalid("cursor de auditoría inválido"))?;
        if base != digest {
            return Err(DomainError::precondition("la auditoría cambió; reinicia la paginación"));
        }
        offset.parse::<usize>().map_err(|_| DomainError::invalid("cursor de auditoría inválido"))?
    } else {
        0
    };
    let mut events = Vec::new();
    let total = if let Some(index) = &snapshot {
        let mut total = 0usize;
        for segment in &index.segments {
            let end = total.checked_add(segment.events).ok_or_else(|| DomainError::invalid("contador de auditoría desbordado"))?;
            if end > offset && events.len() < limit {
                events.extend(read_segment(root, segment)?.into_iter().skip(offset.saturating_sub(total)).take(limit - events.len()));
            }
            total = end;
        }
        total
    } else {
        let mut legacy = Vec::new();
        visit(root, None, |event| {
            legacy.push(event);
            Ok(())
        })?;
        let total = legacy.len();
        events.extend(legacy.into_iter().skip(offset).take(limit));
        total
    };
    if offset > total {
        return Err(DomainError::out_of_range("cursor fuera de auditoría"));
    }
    let next = offset + events.len();
    Ok(JournalPage { events, next_cursor: (next < total).then(|| format!("{digest}:{next}")), snapshot_digest: digest })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, CommandEnvelope, ProjectSession, ProjectStore};
    use tv2_domain::{Command, Project};

    fn event(revision: u64, large: bool) -> JournalEvent {
        JournalEvent {
            at: "now".into(),
            command_id: format!("command-{revision}"),
            actor: Actor::Human,
            kind: "command".into(),
            label: if large { "audit evidence ".repeat(22000) } else { "change".into() },
            base_revision: revision - 1,
            new_revision: revision,
            idempotency_key: None,
            diff: Default::default(),
            command: None,
            receipt: None,
        }
    }

    #[test]
    fn archive_paginates_without_rewriting_segments_and_rejects_stale_cursors_or_corruption() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        let mut project = Project::new("archive");
        project.revision = 9;
        let events: Vec<_> = (1..=9).map(|n| event(n, true)).collect();
        store.save_with_journal(&project, &events).unwrap();
        let first_index = index(temp.path()).unwrap().unwrap();
        assert!(first_index.segments.len() >= 3);
        let raw_index = fs::read(temp.path().join(INDEX)).unwrap();
        store.save_with_journal(&project, &events).unwrap();
        assert_eq!(fs::read(temp.path().join(INDEX)).unwrap(), raw_index);
        let first = store.journal_page(None, 3).unwrap();
        assert_eq!(first.events, events[..3]);
        let cursor = first.next_cursor.clone().unwrap();
        let second = store.journal_page(Some(&cursor), 3).unwrap();
        let third = store.journal_page(second.next_cursor.as_deref(), 3).unwrap();
        assert_eq!(second.events, events[3..6]);
        assert_eq!(third.events, events[6..]);
        assert!(third.next_cursor.is_none());
        project.revision = 10;
        store.save_with_journal(&project, &[event(10, false)]).unwrap();
        assert!(store.journal_page(Some(&cursor), 3).is_err());
        assert_eq!(store.read_journal().unwrap().len(), 10);
        let path = temp.path().join("audit/segments").join(format!("{}.jsonl", first_index.segments[0].sha256));
        fs::write(path, b"corrupt").unwrap();
        assert!(store.load_session().is_err());
        assert!(store.journal_page(None, 3).is_err());
    }

    #[test]
    fn archive_publication_retries_orphan_blobs_and_autosave_references_history() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        let mut project = Project::new("archive");
        project.revision = 8;
        let events: Vec<_> = (1..=8).map(|n| event(n, true)).collect();
        store.save_with_journal(&project, &events).unwrap();
        let mut pending_index = index(temp.path()).unwrap().unwrap();
        append_segments(temp.path(), &mut pending_index, &[event(9, false)]).unwrap();
        assert_eq!(store.read_journal().unwrap().len(), 8); // uncommitted blobs are invisible
        let mut live = store.load_session().unwrap();
        live.execute(CommandEnvelope::human(Command::RenameProject { name: "autosaved".into() }).with_idempotency("autosaved")).unwrap();
        store.save_autosave(live.project(), live.pending_journal()).unwrap();
        assert!(fs::metadata(temp.path().join("autosave.json")).unwrap().len() < 10000);
        let recovery = store.recovery_checkpoint(&project).unwrap().unwrap();
        assert_eq!(recovery.events.len(), 9);
        assert!(ProjectSession::with_audit(recovery.project, &recovery.events).unwrap().command_receipt("autosaved").unwrap().is_some());
        publish(temp.path(), &[event(9, false)]).unwrap();
        publish(temp.path(), &[event(9, false)]).unwrap();
        assert_eq!(store.read_journal().unwrap().len(), 9);
    }

    #[test]
    fn save_as_copies_archived_audit_without_source_dependency() {
        let temp = tempfile::tempdir().unwrap();
        let source = ProjectStore::at(temp.path().join("source.transcriptor"));
        let destination = ProjectStore::at(temp.path().join("copy.transcriptor"));
        let mut project = Project::new("source");
        project.revision = 4;
        let events: Vec<_> = (1..=4).map(|n| event(n, true)).collect();
        source.save_with_journal(&project, &events).unwrap();
        destination.copy_audit_from(&source, &project.project_id, project.revision).unwrap();
        destination.copy_audit_from(&source, &project.project_id, project.revision).unwrap();
        destination.save(&project).unwrap();
        fs::rename(&source.root, temp.path().join("unavailable-source")).unwrap();
        assert_eq!(destination.read_journal().unwrap(), events);
        assert_eq!(destination.load_session().unwrap().project(), &project);
    }

    #[test]
    fn more_than_ten_thousand_receipts_remain_replayable_after_archive_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path());
        let mut session = ProjectSession::new(Project::new("receipts"));
        let mut retry = None;
        for n in 0..10002 {
            let request = CommandEnvelope::human(Command::RenameProject { name: format!("change-{n}") }).with_idempotency(format!("key-{n}"));
            if n == 10001 {
                retry = Some(request.clone());
            }
            session.execute(request).unwrap();
            if n % 512 == 511 {
                store.save_with_journal(session.project(), session.pending_journal()).unwrap();
                session.acknowledge_save(session.revision(), session.pending_journal().len());
            }
        }
        store.save_with_journal(session.project(), session.pending_journal()).unwrap();
        let revision = session.revision();
        let retry = retry.unwrap();
        assert!(session.execute(retry.clone()).unwrap().replayed);
        let mut changed = retry.clone();
        changed.command = Command::RenameProject { name: "different-intent".into() };
        assert!(session.execute(changed).is_err());
        drop(session);
        let mut restored = store.load_session().unwrap();
        assert!(restored.command_receipt("key-1024").unwrap().is_some());
        assert!(restored.execute(retry).unwrap().replayed);
        assert_eq!(restored.revision(), revision);
        restored.execute(CommandEnvelope::human(Command::RenameProject { name: "after-limit".into() }).with_idempotency("key-next")).unwrap();
    }
}
