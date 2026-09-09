//! Three-way external reconciliation. Arrays with stable identities merge by ID;
//! ordered ranges and other arrays are atomic. Conflicts never choose a winner.
use crate::session::{Actor, DiffSummary};
use serde_json::{Map, Value};
use tv2_domain::error::DomainResult;
use tv2_domain::{DomainError, Project};

#[derive(Clone, Debug)]
pub struct ExternalChange {
    pub base_revision: u64,
    pub local_digest: String,
    pub external_digest: String,
    pub merged: Project,
    pub diff: DiffSummary,
}

pub(crate) fn digest(project: &Project) -> DomainResult<String> {
    Ok(tv2_domain::digest::digest_json(&serde_json::to_value(project)?))
}

pub(crate) fn prepare(base: &Project, local: &Project, external: &Project) -> DomainResult<ExternalChange> {
    external.validate()?;
    if base.project_id != external.project_id || base.project_id != local.project_id {
        return Err(DomainError::precondition("el archivo externo pertenece a otro proyecto"));
    }
    if external.revision < base.revision {
        return Err(DomainError::stale(base.revision, external.revision));
    }
    let normalize = |p: &Project| -> DomainResult<Value> {
        let mut v = serde_json::to_value(p)?;
        v["revision"] = serde_json::json!(base.revision);
        v["updated_at"] = serde_json::json!(base.updated_at);
        Ok(v)
    };
    let b = normalize(base)?;
    let l = normalize(local)?;
    let e = normalize(external)?;
    let mut merged: Project = serde_json::from_value(merge(Some(&b), Some(&l), Some(&e), "")?.unwrap())?;
    merged.revision = local.revision.max(external.revision);
    merged.validate()?;
    crate::protection::check_transition(local, &merged, &Actor::External { source: "project.json".into() })?;
    Ok(ExternalChange {
        base_revision: local.revision,
        local_digest: digest(local)?,
        external_digest: digest(external)?,
        diff: DiffSummary::compute(local, &merged),
        merged,
    })
}

fn merge(base: Option<&Value>, local: Option<&Value>, external: Option<&Value>, path: &str) -> DomainResult<Option<Value>> {
    if local == external || external == base {
        return Ok(local.cloned());
    }
    if local == base {
        return Ok(external.cloned());
    }
    if let (Some(Value::Object(b)), Some(Value::Object(l)), Some(Value::Object(e))) = (base, local, external) {
        let keys: std::collections::BTreeSet<_> = b.keys().chain(l.keys()).chain(e.keys()).collect();
        let mut out = Map::new();
        for key in keys {
            if let Some(v) = merge(b.get(key), l.get(key), e.get(key), &format!("{path}/{key}"))? {
                out.insert(key.clone(), v);
            }
        }
        return Ok(Some(Value::Object(out)));
    }
    if let (Some(Value::Array(b)), Some(Value::Array(l)), Some(Value::Array(e))) = (base, local, external) {
        let field = path.rsplit('/').next().unwrap_or("");
        if field == "deleted_item_ids" {
            let mut out = l.clone();
            for v in e {
                if !out.contains(v) {
                    out.push(v.clone());
                }
            }
            return Ok(Some(Value::Array(out)));
        }
        let id_key = match field {
            "layers" => Some("layer_id"),
            "items" => Some("item_id"),
            "assets" | "sequences" | "tracks" | "clips" | "markers" => Some("id"),
            _ => None,
        };
        if let Some(key) = id_key {
            let index = |arr: &[Value]| -> DomainResult<Map<String, Value>> {
                let mut out = Map::new();
                for value in arr {
                    let id = value.get(key).and_then(Value::as_str).ok_or_else(|| DomainError::invalid("colección sin ID"))?;
                    if out.insert(id.into(), value.clone()).is_some() {
                        return Err(DomainError::invalid("ID duplicado"));
                    }
                }
                Ok(out)
            };
            let bm = Value::Object(index(b)?);
            let lm = Value::Object(index(l)?);
            let em = Value::Object(index(e)?);
            let combined = merge(Some(&bm), Some(&lm), Some(&em), path)?.unwrap();
            // Order is semantic for tracks/layers: incompatible concurrent
            // reordering is a conflict, while independent insertions are safe.
            let ids = |arr: &[Value]| -> Vec<String> { arr.iter().map(|v| v[key].as_str().unwrap().to_string()).collect() };
            let bi = ids(b);
            let li = ids(l);
            let ei = ids(e);
            let common = |order: &[String]| -> Vec<String> {
                order.iter().filter(|id| bi.contains(id) && li.contains(id) && ei.contains(id)).cloned().collect()
            };
            let bo = common(&bi);
            let lo = common(&li);
            let eo = common(&ei);
            if lo != bo && eo != bo && lo != eo {
                return Err(conflict(path));
            }
            let (primary, secondary) = if eo != bo { (&ei, &li) } else { (&li, &ei) };
            let mut order = primary.clone();
            for id in secondary {
                if !order.contains(id) {
                    order.push(id.clone());
                }
            }
            return Ok(Some(Value::Array(order.iter().filter_map(|id| combined.get(id).cloned()).collect())));
        }
    }
    Err(conflict(path))
}

fn conflict(path: &str) -> DomainError {
    DomainError::new(tv2_domain::ErrorCode::ExternalConflict, format!("ediciones concurrentes incompatibles en {path}"))
        .with("path", path.to_string())
        .with_action("conserva ambas versiones; resuelve el campo en conflicto o guarda en otra carpeta")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merges_independent_changes_and_rejects_same_field_or_wrong_identity() {
        let base = Project::new("base");
        let mut local = base.clone();
        local.name = "local".into();
        local.revision = 2;
        let mut external = base.clone();
        external.settings.snapping = false;
        external.revision = 1;
        let change = prepare(&base, &local, &external).unwrap();
        assert_eq!(change.merged.name, "local");
        assert!(!change.merged.settings.snapping);
        external.name = "external".into();
        assert!(prepare(&base, &local, &external).is_err());
        external.project_id = "other".into();
        assert!(prepare(&base, &base, &external).is_err());
    }
    #[test]
    fn stable_ids_merge_items_and_conflicting_order_is_rejected() {
        let b = serde_json::json!({"tracks":[{"id":"a","name":"A"},{"id":"b","name":"B"},{"id":"c","name":"C"}]});
        let mut l = b.clone();
        l["tracks"][0]["name"] = serde_json::json!("local");
        let mut e = b.clone();
        e["tracks"][1]["name"] = serde_json::json!("external");
        let merged = merge(Some(&b), Some(&l), Some(&e), "").unwrap().unwrap();
        assert_eq!(merged["tracks"][0]["name"], "local");
        assert_eq!(merged["tracks"][1]["name"], "external");
        l["tracks"].as_array_mut().unwrap().swap(0, 1);
        e["tracks"].as_array_mut().unwrap().swap(1, 2);
        assert!(merge(Some(&b), Some(&l), Some(&e), "").is_err());
    }
}
