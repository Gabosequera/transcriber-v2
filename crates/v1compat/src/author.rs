//! Authoritative marcas.py sidecar. Header/quarantine are stored once, marks only as items.
use crate::{V1Result, invalid, secs_json, secs_to_ticks};
use serde_json::{Value, json};
use tv2_domain::{Asset, ItemState, LayerKind, SemanticItem, SemanticLayer, Ticks, TimeRange};

fn number(raw: &Value, key: &str) -> V1Result<f64> {
    raw[key].as_f64().filter(|n| n.is_finite()).ok_or_else(|| invalid(format!("marca: {key} no es un tiempo finito")))
}
fn mark(raw: &Value, duration: Ticks) -> V1Result<SemanticItem> {
    let id = raw["id"].as_str().ok_or_else(|| invalid("marca sin id"))?;
    let digits = id.strip_prefix('m').ok_or_else(|| invalid("id de marca inválido"))?;
    if digits.len() < 4 || !digits.bytes().all(|b| b.is_ascii_digit()) || digits.parse::<u64>().is_err() {
        return Err(invalid("id de marca inválido"));
    }
    let state = match raw.get("decision") {
        None | Some(Value::Null) => ItemState::Proposed,
        Some(v) if v == "incluir" => ItemState::Accepted,
        Some(v) if v == "excluir" => ItemState::Disabled,
        _ => return Err(invalid("decisión desconocida")),
    };
    let (a, b) = match raw["tipo"].as_str() {
        Some("punto") if state == ItemState::Proposed => {
            let t = number(raw, "t")?;
            (t, t)
        }
        Some("region") => {
            let a = number(raw, "t_ini")?;
            let b = number(raw, "t_fin")?;
            if b <= a {
                return Err(invalid("región vacía"));
            }
            (a, b)
        }
        _ => return Err(invalid("geometría de marca incompatible con su decisión")),
    };
    if a < 0.0 || b > duration.as_seconds_f64() || (state != ItemState::Proposed && b - a + 1e-9 < 0.2) {
        return Err(invalid("marca fuera del medio o decisión menor de 200 ms"));
    }
    let mut item = SemanticItem::new(TimeRange::new(secs_to_ticks(a), secs_to_ticks(b)), raw["label"].as_str().unwrap_or(""));
    item.item_id = id.into();
    item.state = state;
    item.edited = raw["tv2_edited"].as_bool().unwrap_or(true);
    item.origin = Some("author".into());
    item.comment = match raw.get("prompt") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        _ => return Err(invalid("prompt no es texto")),
    };
    item.extra = raw.as_object().cloned().ok_or_else(|| invalid("marca no es objeto"))?;
    for key in ["id", "tipo", "t", "t_ini", "t_fin", "decision", "prompt", "label", "tv2_edited"] {
        item.extra.remove(key);
    }
    // Null/absent optional values survive a read/write cycle without a second mark copy.
    item.extra.insert("tv2_author_optional".into(),json!({"label":raw.get("label"),"prompt":raw.get("prompt"),"label_present":raw.get("label").is_some(),"prompt_present":raw.get("prompt").is_some()}));
    Ok(item)
}

pub fn from_v1(raw: &Value, asset: &Asset) -> V1Result<SemanticLayer> {
    if raw["schema"] != 1 || raw["timebase"] != "media_elapsed_v1" {
        return Err(invalid("schema/timebase de marcas desconocido"));
    }
    let revision = raw["revision"].as_u64().ok_or_else(|| invalid("revision de marcas inválida"))?;
    raw["next_id"].as_u64().ok_or_else(|| invalid("next_id de marcas inválido"))?;
    let fp = crate::master::parse_fingerprint(&raw["fuente"])?;
    if !fp.same_identity(&asset.fingerprint) {
        return Err(invalid("marcas de otro medio"));
    }
    let marks = raw["marcas"].as_array().ok_or_else(|| invalid("marcas debe ser una lista"))?;
    let mut layer = SemanticLayer::new(asset.id.clone(), LayerKind::Author, "Marcas del autor");
    layer.layer_id = tv2_domain::LayerId::new(format!("author-{}", &tv2_domain::digest::digest_json(&json!(asset.id))[..16]));
    layer.revision = revision;
    let mut header = raw.as_object().cloned().unwrap();
    header.remove("marcas");
    let mut quarantine = match header.remove("cuarentena") {
        None => Vec::new(),
        Some(Value::Array(a)) => a,
        _ => return Err(invalid("cuarentena no es una lista")),
    };
    let mut seen = std::collections::HashSet::new();
    for raw_mark in marks {
        match mark(raw_mark, asset.duration()) {
            Ok(item) if seen.insert(item.item_id.clone()) => layer.items.push(item),
            Ok(_) => quarantine.push(json!({"marca":raw_mark,"motivo":"id duplicado"})),
            Err(e) => quarantine.push(json!({"marca":raw_mark,"motivo":e.to_string()})),
        }
    }
    if !quarantine.is_empty() || raw.get("cuarentena").is_some() {
        header.insert("cuarentena".into(), json!(quarantine));
    }
    layer.extra.insert("v1_author_header".into(), Value::Object(header));
    tv2_domain::layers::validate_layer(&layer, asset.duration()).map_err(|e| invalid(e.to_string()))?;
    Ok(layer)
}

pub fn to_v1(layer: &SemanticLayer, asset: &Asset) -> V1Result<Value> {
    tv2_domain::layers::validate_layer(layer, asset.duration()).map_err(|e| invalid(e.to_string()))?;
    let mut header = layer.extra.get("v1_author_header").and_then(Value::as_object).cloned().unwrap_or_else(|| {
        json!({"schema":1,"timebase":"media_elapsed_v1","revision":0,"next_id":1,"fuente":asset.fingerprint,"cuarentena":[]})
            .as_object()
            .unwrap()
            .clone()
    });
    let mut next = header["next_id"].as_u64().ok_or_else(|| invalid("contador autor inválido"))?;
    let mut marks = Vec::new();
    for item in &layer.items {
        let n = item.item_id.as_str().strip_prefix('m').and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| invalid("id de marca no representable"))?;
        next = next.max(n.checked_add(1).ok_or_else(|| invalid("contador agotado"))?);
        for t in [item.start(), item.end()] {
            if secs_to_ticks(crate::ticks_to_secs(t)) != t {
                return Err(invalid("marca con tiempo submilisegundo"));
            }
        }
        let mut m = item.extra.clone();
        let optional = m.remove("tv2_author_optional").unwrap_or(Value::Null);
        m.insert("id".into(), json!(item.item_id));
        m.insert("tipo".into(), json!(if item.is_point() { "punto" } else { "region" }));
        m.insert(
            "decision".into(),
            match item.state {
                ItemState::Proposed => Value::Null,
                ItemState::Accepted => json!("incluir"),
                ItemState::Disabled => json!("excluir"),
            },
        );
        if item.is_point() {
            m.insert("t".into(), secs_json(item.start()));
        } else {
            m.insert("t_ini".into(), secs_json(item.start()));
            m.insert("t_fin".into(), secs_json(item.end()));
        }
        for (key, text) in [("label", &item.label), ("prompt", &item.comment)] {
            if !text.is_empty() {
                m.insert(key.into(), json!(text));
            } else if optional[format!("{key}_present")].as_bool() == Some(true) {
                m.insert(key.into(), if optional[key].is_null() { Value::Null } else { json!("") });
            }
        }
        if !item.edited {
            m.insert("tv2_edited".into(), json!(false));
        }
        let value = Value::Object(m);
        mark(&value, asset.duration())?;
        marks.push(value);
    }
    // Quarantined/deleted IDs remain reserved too.
    for id in layer.deleted_item_ids.iter().map(|i| i.as_str()).chain(
        header.get("cuarentena").and_then(Value::as_array).into_iter().flatten().filter_map(|q| q.pointer("/marca/id").and_then(Value::as_str)),
    ) {
        if let Some(n) = id.strip_prefix('m').and_then(|n| n.parse::<u64>().ok()) {
            next = next.max(n.checked_add(1).ok_or_else(|| invalid("contador agotado"))?);
        }
    }
    marks.sort_by(|a, b| {
        a.get("t")
            .or_else(|| a.get("t_ini"))
            .and_then(Value::as_f64)
            .partial_cmp(&b.get("t").or_else(|| b.get("t_ini")).and_then(Value::as_f64))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    header.insert("marcas".into(), json!(marks));
    header.insert("next_id".into(), json!(next));
    header.insert("revision".into(), json!(layer.revision));
    Ok(Value::Object(header))
}

/// Explicit candidates only. Never load application configuration or mutate the V1 store.
pub fn select(candidates: &[Value], asset: &Asset) -> V1Result<Option<SemanticLayer>> {
    let highest = candidates.iter().filter_map(|v| v["revision"].as_u64()).max();
    let mut chosen: Option<(SemanticLayer, &Value)> = None;
    for raw in candidates {
        let layer = from_v1(raw, asset)?;
        if Some(layer.revision) != highest {
            continue;
        }
        if let Some((old, old_raw)) = &chosen {
            if old.revision == layer.revision && *old_raw != raw {
                return Err(invalid("conflicto de marcas: misma revisión con contenido distinto; elige un candidato explícitamente"));
            }
            if old.revision > layer.revision {
                continue;
            }
        }
        chosen = Some((layer, raw));
    }
    Ok(chosen.map(|(l, _)| l))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn author_roundtrip_decisions_quarantine_identity_and_revision_conflicts() {
        let asset = tv2_domain::commands::tests_support::fake_video("a", 30);
        let raw = json!({"schema":1,"timebase":"media_elapsed_v1","revision":4,"next_id":4,"fuente":asset.fingerprint,"future":{"keep":1},"cuarentena":[],"marcas":[
            {"id":"m0001","tipo":"punto","t":3.0,"decision":null,"prompt":null,"creado":"original","future":42},
            {"id":"m0003","tipo":"region","t_ini":5.0,"t_fin":8.0,"decision":"excluir","prompt":"no usar","label":"nota"}]});
        let layer = from_v1(&raw, &asset).unwrap();
        assert_eq!(to_v1(&layer, &asset).unwrap(), raw);
        let mut bad = raw.clone();
        bad["marcas"].as_array_mut().unwrap().push(json!({"id":"m0099","tipo":"punto","t":35.0,"decision":null}));
        let layer = from_v1(&bad, &asset).unwrap();
        assert_eq!(layer.items.len(), 2);
        let back = to_v1(&layer, &asset).unwrap();
        assert_eq!(back["cuarentena"][0]["marca"]["id"], "m0099");
        assert_eq!(back["next_id"], 100);
        assert!(select(&[raw.clone(), bad.clone()], &asset).is_err());
        bad["revision"] = json!(5);
        assert_eq!(select(&[raw.clone(), bad], &asset).unwrap().unwrap().revision, 5);
        let mut other = asset.clone();
        other.fingerprint.size += 1;
        assert!(from_v1(&raw, &other).is_err());
        let mut p = tv2_domain::Project::new("author");
        p.assets.push(asset.clone());
        p.layers.push(from_v1(&raw, &asset).unwrap());
        let id = p.layers[0].layer_id.clone();
        tv2_domain::Command::CycleAuthorDecision { layer_id: id.clone(), item_ids: vec!["m0001".into()] }.apply(&mut p).unwrap();
        let edited = to_v1(p.layer(&id).unwrap(), &asset).unwrap();
        assert_eq!(edited["marcas"][0]["tipo"], "region");
        assert_eq!(edited["marcas"][0]["decision"], "incluir");
        assert_eq!(edited["marcas"][0]["future"], 42);
        assert_eq!(edited["revision"], 5);
    }
}
