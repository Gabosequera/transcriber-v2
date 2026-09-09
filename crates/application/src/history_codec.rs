//! History/2 stores one anchor and structural deltas. History/1 remains readable.
//! This bounds repeated *serialized* evidence without claiming bounded snapshot RAM.
use crate::session::{Actor, DurableHistory, HistoryEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tv2_domain::{DomainError, Project, error::DomainResult};

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Part {
    Key { key: String },
    Index { index: usize },
}
#[derive(Serialize, Deserialize)]
struct Patch {
    path: Vec<Part>,
    before: Option<Value>,
    after: Option<Value>,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    label: String,
    actor: Actor,
    command_id: String,
    expect: u64,
    before: Vec<Patch>,
    after: Vec<Patch>,
    before_digest: String,
    after_digest: String,
}
#[derive(Serialize, Deserialize)]
struct Compact {
    schema: String,
    project_id: String,
    revision: u64,
    undo_len: usize,
    anchor: Option<Value>,
    entries: Vec<Entry>,
}
fn hash(value: &Value) -> String {
    tv2_domain::digest::digest_json(value)
}
fn differences(before: &Value, after: &Value) -> Vec<Patch> {
    fn walk(before: Option<&Value>, after: Option<&Value>, path: &mut Vec<Part>, out: &mut Vec<Patch>) {
        if before == after {
            return;
        }
        match (before, after) {
            (Some(Value::Object(b)), Some(Value::Object(a))) => {
                for key in b.keys().chain(a.keys()).collect::<std::collections::BTreeSet<_>>() {
                    path.push(Part::Key { key: key.clone() });
                    walk(b.get(key), a.get(key), path, out);
                    path.pop();
                }
            }
            (Some(Value::Array(b)), Some(Value::Array(a))) if b.len() == a.len() => {
                for (index, (b, a)) in b.iter().zip(a).enumerate() {
                    path.push(Part::Index { index });
                    walk(Some(b), Some(a), path, out);
                    path.pop();
                }
            }
            _ => {
                out.push(Patch {
                    path: path
                        .iter()
                        .map(|p| match p {
                            Part::Key { key } => Part::Key { key: key.clone() },
                            Part::Index { index } => Part::Index { index: *index },
                        })
                        .collect(),
                    before: before.cloned(),
                    after: after.cloned(),
                });
            }
        }
    }
    let mut out = vec![];
    walk(Some(before), Some(after), &mut vec![], &mut out);
    out
}
fn apply(value: &mut Value, patches: Vec<Patch>) -> DomainResult<()> {
    for patch in patches {
        let mut target = &mut *value;
        let Some((last, parents)) = patch.path.split_last() else {
            if patch.before.as_ref() != Some(target) {
                return Err(DomainError::invalid("base de delta alterada"));
            }
            *target = patch.after.ok_or_else(|| DomainError::invalid("delta elimina raíz"))?;
            continue;
        };
        for part in parents {
            target = match part {
                Part::Key { key } => target.get_mut(key),
                Part::Index { index } => target.get_mut(*index),
            }
            .ok_or_else(|| DomainError::invalid("ruta de delta inexistente"))?;
        }
        match last {
            Part::Key { key } => {
                let map = target.as_object_mut().ok_or_else(|| DomainError::invalid("delta no apunta a objeto"))?;
                if map.get(key) != patch.before.as_ref() {
                    return Err(DomainError::invalid("base de delta alterada"));
                }
                match patch.after {
                    Some(after) => {
                        map.insert(key.clone(), after);
                    }
                    None => {
                        map.remove(key);
                    }
                }
            }
            Part::Index { index } => {
                let slot =
                    target.as_array_mut().and_then(|a| a.get_mut(*index)).ok_or_else(|| DomainError::invalid("índice de delta inexistente"))?;
                if patch.before.as_ref() != Some(slot) {
                    return Err(DomainError::invalid("base de delta alterada"));
                }
                *slot = patch.after.ok_or_else(|| DomainError::invalid("delta elimina índice"))?;
            }
        }
    }
    Ok(())
}
pub fn encode(history: &DurableHistory) -> DomainResult<Value> {
    let mut previous = Value::Null;
    let mut anchor = None;
    let mut entries = vec![];
    for entry in history.undo.iter().chain(&history.redo) {
        let before = serde_json::to_value(&entry.before)?;
        let after = serde_json::to_value(&entry.after)?;
        if anchor.is_none() {
            previous = before.clone();
            anchor = Some(before.clone());
        }
        entries.push(Entry {
            label: entry.label.clone(),
            actor: entry.actor.clone(),
            command_id: entry.command_id.clone(),
            expect: entry.expect,
            before: differences(&previous, &before),
            after: differences(&before, &after),
            before_digest: hash(&before),
            after_digest: hash(&after),
        });
        previous = after;
    }
    Ok(serde_json::to_value(Compact {
        schema: "transcriptor-history/2".into(),
        project_id: history.project_id.clone(),
        revision: history.revision,
        undo_len: history.undo.len(),
        anchor,
        entries,
    })?)
}
pub fn decode(raw: Value) -> DomainResult<DurableHistory> {
    if raw["schema"] == "transcriptor-history/1" {
        return Ok(serde_json::from_value(raw)?);
    }
    if raw["schema"] != "transcriptor-history/2" {
        return Err(DomainError::unsupported("schema de historial desconocido"));
    }
    let compact: Compact = serde_json::from_value(raw)?;
    if compact.undo_len > compact.entries.len()
        || compact.entries.len() > 2 * crate::session::HISTORY_DEPTH
        || compact.anchor.is_some() == compact.entries.is_empty()
    {
        return Err(DomainError::invalid("índice de historial inválido"));
    }
    let mut value = compact.anchor.unwrap_or(Value::Null);
    let mut entries = std::collections::VecDeque::new();
    for entry in compact.entries {
        apply(&mut value, entry.before)?;
        if hash(&value) != entry.before_digest {
            return Err(DomainError::invalid("digest anterior de historial inválido"));
        }
        let before: Project = serde_json::from_value(value.clone())?;
        apply(&mut value, entry.after)?;
        if hash(&value) != entry.after_digest {
            return Err(DomainError::invalid("digest posterior de historial inválido"));
        }
        let after: Project = serde_json::from_value(value.clone())?;
        entries.push_back(HistoryEntry { before, after, label: entry.label, actor: entry.actor, command_id: entry.command_id, expect: entry.expect });
    }
    let redo = entries.split_off(compact.undo_len);
    Ok(DurableHistory { schema: "transcriptor-history/1".into(), project_id: compact.project_id, revision: compact.revision, undo: entries, redo })
}
pub mod optional {
    use super::*;
    pub fn serialize<S: serde::Serializer>(history: &Option<DurableHistory>, serializer: S) -> Result<S::Ok, S::Error> {
        history.as_ref().map(encode).transpose().map_err(serde::ser::Error::custom)?.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<DurableHistory>, D::Error> {
        Option::<Value>::deserialize(deserializer)?.map(decode).transpose().map_err(serde::de::Error::custom)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deltas_roundtrip_undo_redo_and_reject_corrupt_base_with_shared_large_evidence() {
        let mut p = Project::new("history");
        p.extra.insert("evidence".into(), serde_json::json!("unchanged evidence ".repeat(10_000)));
        let mut session = crate::ProjectSession::new(p);
        for index in 0..20 {
            session.execute(crate::CommandEnvelope::human(tv2_domain::Command::RenameProject { name: format!("name {index}") })).unwrap();
        }
        session.undo(Actor::Human).unwrap();
        session.undo(Actor::Human).unwrap();
        let history = session.history_snapshot();
        let raw = encode(&history).unwrap();
        let decoded = decode(raw.clone()).unwrap();
        decoded.validate(session.project()).unwrap();
        assert_eq!(history, decoded);
        let legacy = serde_json::to_value(&history).unwrap();
        assert_eq!(decode(legacy.clone()).unwrap(), history);
        let compact_bytes = serde_json::to_vec(&raw).unwrap().len();
        let legacy_bytes = serde_json::to_vec(&legacy).unwrap().len();
        assert!(compact_bytes * 10 < legacy_bytes, "{compact_bytes} vs {legacy_bytes}");
        println!("history/2 compact_bytes={compact_bytes} legacy_bytes={legacy_bytes} entries=20 evidence_chars=190000");
        let mut bad = raw;
        bad["entries"][0]["after_digest"] = serde_json::json!("altered");
        assert!(decode(bad).is_err());
    }
}
