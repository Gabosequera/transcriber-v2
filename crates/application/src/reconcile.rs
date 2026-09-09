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
    pub fields: Vec<FieldChange>,
    pub conflicts: Vec<FieldConflict>,
    pub choices: std::collections::BTreeMap<String, ConflictChoice>,
    pub human_review: bool,
    inputs: Option<Box<(Project, Project, Project)>>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FieldChange {
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FieldConflict {
    pub path: String,
    pub base: Option<Value>,
    pub local: Option<Value>,
    pub external: Option<Value>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    Local,
    External,
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Detailed changes are computed once per review, not per GUI frame.
pub fn field_diff(before: &Value, after: &Value) -> Vec<FieldChange> {
    fn walk(before: Option<&Value>, after: Option<&Value>, path: &str, out: &mut Vec<FieldChange>) {
        if before == after {
            return;
        }
        if let (Some(Value::Object(b)), Some(Value::Object(a))) = (before, after) {
            for key in b.keys().chain(a.keys()).collect::<std::collections::BTreeSet<_>>() {
                walk(b.get(key), a.get(key), &format!("{path}/{}", escape(key)), out);
            }
        } else if let (Some(Value::Array(b)), Some(Value::Array(a))) = (before, after)
            && let Some(key) = id_key(path)
        {
            let bm: Map<String, Value> = b.iter().filter_map(|v| Some((v[key].as_str()?.into(), v.clone()))).collect();
            let am: Map<String, Value> = a.iter().filter_map(|v| Some((v[key].as_str()?.into(), v.clone()))).collect();
            walk(Some(&Value::Object(bm)), Some(&Value::Object(am)), path, out);
            let bi: Vec<_> = b.iter().map(|v| &v[key]).collect();
            let ai: Vec<_> = a.iter().map(|v| &v[key]).collect();
            if bi != ai {
                out.push(FieldChange { path: format!("{path}/@order"), before: Some(serde_json::json!(bi)), after: Some(serde_json::json!(ai)) });
            }
        } else {
            out.push(FieldChange { path: path.into(), before: before.cloned(), after: after.cloned() });
        }
    }
    let mut out = Vec::new();
    walk(Some(before), Some(after), "", &mut out);
    out
}

fn id_key(path: &str) -> Option<&'static str> {
    match path.rsplit('/').next().unwrap_or("") {
        "layers" => Some("layer_id"),
        "items" => Some("item_id"),
        "assets" | "sequences" | "tracks" | "clips" | "markers" => Some("id"),
        _ => None,
    }
}

#[derive(Default)]
struct Resolution<'a> {
    collect: bool,
    conflicts: Vec<FieldConflict>,
    choices: Option<&'a std::collections::BTreeMap<String, ConflictChoice>>,
}
impl Resolution<'_> {
    fn choose(&mut self, path: &str, base: Option<&Value>, local: Option<&Value>, external: Option<&Value>) -> DomainResult<Option<Value>> {
        if let Some(choice) = self.choices.and_then(|c| c.get(path)) {
            return Ok(match choice {
                ConflictChoice::Local => local.cloned(),
                ConflictChoice::External => external.cloned(),
            });
        }
        if !self.collect {
            return Err(conflict(path));
        }
        self.conflicts.push(FieldConflict { path: path.into(), base: base.cloned(), local: local.cloned(), external: external.cloned() });
        Ok(local.cloned())
    }
}

pub(crate) fn digest(project: &Project) -> DomainResult<String> {
    Ok(tv2_domain::digest::digest_json(&serde_json::to_value(project)?))
}

#[cfg(test)]
pub(crate) fn prepare(base: &Project, local: &Project, external: &Project) -> DomainResult<ExternalChange> {
    prepare_with(base, local, external, false, None, false)
}

/// Inspect changes only. This does not authorize or apply edits; callers must
/// prepare and commit the resolved result through the normal application gate.
pub fn inspect(base: &Project, local: &Project, external: &Project) -> DomainResult<ExternalChange> {
    prepare_with(base, local, external, true, None, false)
}

impl ExternalChange {
    pub fn resolved(&self) -> DomainResult<Self> {
        let Some(inputs) = &self.inputs else {
            return Ok(self.clone());
        };
        let (base, local, external) = inputs.as_ref();
        let mut result = prepare_with(base, local, external, false, Some(&self.choices), self.human_review)?;
        result.external_digest.clone_from(&self.external_digest);
        Ok(result)
    }
}

fn prepare_with(
    base: &Project,
    local: &Project,
    external: &Project,
    collect: bool,
    choices: Option<&std::collections::BTreeMap<String, ConflictChoice>>,
    human: bool,
) -> DomainResult<ExternalChange> {
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
    let mut resolution = Resolution { collect, choices, ..Default::default() };
    let mut merged: Project = serde_json::from_value(merge_inner(Some(&b), Some(&l), Some(&e), "", &mut resolution)?.unwrap())?;
    merged.revision = local.revision.max(external.revision);
    if !collect {
        merged.validate()?;
        for old in &local.layers {
            let next = merged.layer(&old.layer_id);
            if old.deleted && next.is_some_and(|l| !l.deleted)
                || old.deleted_item_ids.iter().any(|id| next.is_none_or(|l| !l.deleted_item_ids.contains(id)))
            {
                return Err(DomainError::precondition("La resolución no puede borrar tombstones ni resucitar una capa"));
            }
        }
        crate::protection::check_transition(local, &merged, &if human { Actor::Human } else { Actor::External { source: "project.json".into() } })?;
    }
    Ok(ExternalChange {
        base_revision: local.revision,
        local_digest: digest(local)?,
        external_digest: digest(external)?,
        diff: DiffSummary::compute(local, &merged),
        merged,
        fields: field_diff(&l, &e),
        conflicts: resolution.conflicts,
        choices: choices.cloned().unwrap_or_default(),
        human_review: human,
        inputs: collect.then(|| Box::new((base.clone(), local.clone(), external.clone()))),
    })
}

#[cfg(test)]
fn merge(base: Option<&Value>, local: Option<&Value>, external: Option<&Value>, path: &str) -> DomainResult<Option<Value>> {
    merge_inner(base, local, external, path, &mut Resolution::default())
}

fn merge_inner(
    base: Option<&Value>,
    local: Option<&Value>,
    external: Option<&Value>,
    path: &str,
    resolution: &mut Resolution<'_>,
) -> DomainResult<Option<Value>> {
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
            if let Some(v) = merge_inner(b.get(key), l.get(key), e.get(key), &format!("{path}/{}", escape(key)), resolution)? {
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
        if let Some(key) = id_key(path) {
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
            let combined = merge_inner(Some(&bm), Some(&lm), Some(&em), path, resolution)?.unwrap();
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
            let prefer_external = if lo != bo && eo != bo && lo != eo {
                resolution.choose(
                    &format!("{path}/@order"),
                    Some(&serde_json::json!(bi)),
                    Some(&serde_json::json!(li)),
                    Some(&serde_json::json!(ei)),
                )? == Some(serde_json::json!(ei))
            } else {
                eo != bo
            };
            let (primary, secondary) = if prefer_external { (&ei, &li) } else { (&li, &ei) };
            let mut order = primary.clone();
            for id in secondary {
                if !order.contains(id) {
                    order.push(id.clone());
                }
            }
            return Ok(Some(Value::Array(order.iter().filter_map(|id| combined.get(id).cloned()).collect())));
        }
    }
    resolution.choose(path, base, local, external)
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
    fn detailed_review_collects_all_conflicts_and_requires_each_choice() {
        let base = Project::new("base");
        let mut local = base.clone();
        local.name = "local".into();
        local.extra.insert("a/b~c".into(), serde_json::json!(1));
        local.revision = 1;
        let mut external = base.clone();
        external.name = "external".into();
        external.extra.insert("a/b~c".into(), serde_json::json!(2));
        external.revision = 2;
        let mut review = inspect(&base, &local, &external).unwrap();
        assert_eq!(review.conflicts.len(), 2);
        assert!(review.fields.iter().any(|f| f.path == "/a~1b~0c"));
        assert!(review.resolved().is_err());
        review.choices.insert("/name".into(), ConflictChoice::External);
        assert!(review.resolved().is_err());
        review.choices.insert("/a~1b~0c".into(), ConflictChoice::Local);
        let resolved = review.resolved().unwrap();
        assert_eq!(resolved.merged.name, "external");
        assert_eq!(resolved.merged.extra["a/b~c"], 1);
        assert_eq!(resolved.merged.revision, 2);
        assert!(resolved.conflicts.is_empty());
    }
    #[test]
    fn explicit_human_resolution_is_required_for_editorial_decisions() {
        let mut base = Project::new("review");
        let asset = tv2_domain::commands::tests_support::fake_video("a", 10);
        base.assets.push(asset);
        let mut layer = tv2_domain::SemanticLayer::new("a".into(), tv2_domain::LayerKind::User, "notes");
        layer
            .items
            .push(tv2_domain::SemanticItem::new(tv2_domain::TimeRange::new(tv2_domain::Ticks::ZERO, tv2_domain::Ticks::from_seconds(1)), "human"));
        base.layers.push(layer);
        let mut external = base.clone();
        external.layers[0].items[0].label = "outside".into();
        external.revision = 1;
        let mut review = inspect(&base, &base, &external).unwrap();
        assert!(review.resolved().is_err());
        review.human_review = true;
        assert_eq!(review.resolved().unwrap().merged.layers[0].items[0].label, "outside");
        base.layers[0].deleted_item_ids.push("deleted-mark".into());
        let mut review = inspect(&base, &base, &external).unwrap();
        review.human_review = true;
        assert!(review.resolved().is_err(), "explicit review cannot erase deletion evidence");
    }
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
