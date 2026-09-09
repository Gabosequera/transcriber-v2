//! Content/montage proposal adapters; all publication and commit is external.
use crate::{
    V1Result,
    contracts::{PreparedReview, ReviewKind, required},
    invalid,
    master::V1Master,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use tv2_domain::{
    AssetId, Command, LayerKind, Project, Ticks, TimeRange,
    digest::{digest_json, digest_object_without},
};

fn montage(project: &Project, asset: &AssetId) -> V1Result<Value> {
    let source = project.asset(asset).ok_or_else(|| invalid("medio inexistente"))?;
    if let Some(sequence) = project.active()
        && !sequence.clips.is_empty()
    {
        return crate::export::montage_document(project, &sequence.id);
    }
    Ok(
        json!({"schema":"editorial-montaje/1","media":source.fingerprint,"duration_source":source.duration().as_seconds_ms(),"revision":0,"next_id":1,"tracks":[{"track_id":"V1","name":"V1"}],"clips":[]}),
    )
}
fn trims(project: &Project, asset: &AssetId) -> V1Result<Value> {
    if project.layers.iter().any(|l| &l.asset_id == asset && l.kind == LayerKind::Trims) {
        return crate::export::trims_document(project, asset);
    }
    let source = project.asset(asset).ok_or_else(|| invalid("medio inexistente"))?;
    Ok(
        json!({"schema":"editorial-trims/1","media":source.fingerprint,"duration":source.duration().as_seconds_ms(),"revision":0,"next_id":1,"lanes":[{"lane_id":"main","name":"Recortes","color":"#728bd0"},{"lane_id":"ai","name":"Cortes sugeridos (AI)","color":"#8ab06b"}],"cuts":[]}),
    )
}
pub(crate) fn extend_request(project: &Project, asset: &AssetId, kind: ReviewKind, request: &mut Value) -> V1Result<()> {
    match kind {
        ReviewKind::Trims => {
            request["trims_digest"] = json!(digest_object_without(&trims(project, asset)?, &["revision", "updated_at"]));
            request["mode"] = json!("content");
            request["lane"] = json!("ai");
        }
        ReviewKind::Montage => {
            let current = montage(project, asset)?;
            request["montage_digest"] = if current["clips"].as_array().is_some_and(|c| !c.is_empty()) {
                json!(digest_object_without(&current, &["revision", "updated_at"]))
            } else {
                Value::Null
            };
            request["target_seconds"] = current.get("target_seconds").cloned().unwrap_or(json!(600));
            request["tolerance"] = json!(0.15);
            request["min_clip_seconds"] = json!(3);
            request["max_clip_seconds"] = json!(120);
            request["allow_reorder"] = json!(true);
            request["pass_required"] = json!(
                current
                    .pointer("/analysis/pass")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| invalid("contador de pasadas agotado"))?
            );
            request["media_duration"] = current["duration_source"].clone();
        }
        _ => {}
    }
    Ok(())
}
fn seconds(v: &Value, key: &str) -> V1Result<f64> {
    v[key].as_f64().filter(|n| n.is_finite() && *n >= 0.0 && *n < Ticks::MAX.as_seconds_f64()).ok_or_else(|| invalid(format!("{key} inválido")))
}
fn guidance(request: &Value, key: &str, default: f64) -> V1Result<f64> {
    if request.get(key).is_none_or(Value::is_null) {
        return Ok(default);
    }
    let value = seconds(request, key)?;
    Ok(if value == 0.0 { default } else { value })
}
fn clip_warnings(entry: &Value, range: TimeRange, used: &[TimeRange], minimum: f64, maximum: f64, kept: bool) -> V1Result<Vec<String>> {
    let mut warnings = match entry.get("warnings").filter(|value| !value.is_null()) {
        Some(Value::Array(values)) => values.iter().map(|value| value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())).collect(),
        None => Vec::new(),
        _ => return Err(invalid("warnings del clip debe ser una lista")),
    };
    let length = range.duration().as_seconds_f64();
    if minimum > 0.0 && length < minimum * 0.8 {
        warnings.push(format!("clip corto ({length:.0} s < {minimum:.0} s)"));
    }
    if maximum > 0.0 && length > maximum * 1.2 {
        warnings.push(format!("clip largo ({length:.0} s > {maximum:.0} s)"));
    }
    let reason = entry["reason"].as_str().unwrap_or("").trim();
    for (index, prior) in used.iter().enumerate() {
        // V1 EPS is one millisecond: touching edges and rounding-sized overlap
        // are not deliberate source repetition.
        if range.start < prior.end - Ticks::from_millis(1) && range.end > prior.start + Ticks::from_millis(1) {
            if entry["repeat"] != true || reason.is_empty() {
                return Err(invalid(format!("clip {} repite el tramo de clip {}; requiere repeat:true y motivo", used.len() + 1, index + 1)));
            }
            warnings.push(format!("repite el tramo de clip {} (callback deliberado)", index + 1));
        }
    }
    if !kept && reason.is_empty() {
        warnings.push("la AI no explicó este clip".into());
    }
    if entry
        .get("topic_ids")
        .is_some_and(|topics| !topics.is_null() && topics.as_array().is_none_or(|topics| topics.iter().any(|topic| !topic.is_string())))
    {
        return Err(invalid("topic_ids debe ser una lista de textos"));
    }
    Ok(warnings)
}
fn duration_warnings(total: Ticks, target: f64, tolerance: f64) -> Vec<String> {
    if target > 0.0 && (total.as_seconds_f64() - target).abs() > target * tolerance {
        vec![format!(
            "duración total {:.1} min fuera de la tolerancia ({:.1} min ±{:.0} %); se importa igual: tú decides",
            total.as_seconds_f64() / 60.0,
            target / 60.0,
            tolerance * 100.0
        )]
    } else {
        Vec::new()
    }
}

/// Safe edits use every parent's word/laughter interval. Padded hard boundaries
/// and nearest safe candidates are deterministic; impossible ranges reject all.
pub(crate) fn snap(master: &V1Master, a: f64, b: f64) -> V1Result<(TimeRange, Value)> {
    if a < 0.0 || b > master.duration.as_seconds_f64() || b <= a {
        return Err(invalid("rango propuesto fuera del medio"));
    }
    let mut blocked = Vec::new();
    for track in master.raw["tracks"].as_object().into_iter().flat_map(|m| m.values()) {
        for (key, pad) in [("words", 40), ("laughter", 120)] {
            for event in track[key].as_array().into_iter().flatten() {
                blocked.push((
                    crate::secs_to_ticks(seconds(event, "t_ini")?) - Ticks::from_millis(pad),
                    crate::secs_to_ticks(seconds(event, "t_fin")?) + Ticks::from_millis(pad),
                ));
            }
        }
    }
    let choose = |target: Ticks, lo: Ticks, hi: Ticks| -> V1Result<Ticks> {
        if lo > hi {
            return Err(invalid("no queda espacio para ajuste seguro"));
        }
        let mut candidates = vec![target.clamp(lo, hi), lo, hi];
        for (a, b) in &blocked {
            if *a >= lo && *a <= hi {
                candidates.push(*a);
            }
            if *b >= lo && *b <= hi {
                candidates.push(*b);
            }
        }
        candidates.retain(|t| !blocked.iter().any(|(a, b)| *a < *t && *t < *b));
        candidates.sort_by_key(|t| ((t.0 - target.0).abs(), t.0));
        candidates.first().copied().ok_or_else(|| invalid("no existe borde libre de palabras/risas dentro de 1.5 s"))
    };
    let a = crate::secs_to_ticks(a);
    let b = crate::secs_to_ticks(b);
    let radius = Ticks::from_millis(1500);
    let start = choose(a, (a - radius).max(Ticks::ZERO), (a + radius).min(b - Ticks::from_millis(1)))?;
    let end = choose(b, (b - radius).max(start + Ticks::from_millis(1)), (b + radius).min(master.duration))?;
    Ok((
        TimeRange::new(start, end),
        json!({"algorithm":"tv2-editorial-safe/1","proposed":[crate::secs_json(a),crate::secs_json(b)],"final":[crate::secs_json(start),crate::secs_json(end)],"word_padding_ms":40,"laughter_padding_ms":120,"radius_ms":1500}),
    ))
}
fn references(master: &V1Master, entry: &Value) -> V1Result<()> {
    for key in ["first_utterance_id", "last_utterance_id"] {
        if let Some(id) = entry.get(key).filter(|v| !v.is_null())
            && !master.raw.pointer("/conversation/utterances").and_then(Value::as_array).into_iter().flatten().any(|u| u["utterance_id"] == *id)
        {
            return Err(invalid(format!("{key} desconocido")));
        }
    }
    Ok(())
}
fn increment(next: &mut u64, prefix: &str) -> V1Result<String> {
    let id = format!("{prefix}-{:06}", *next);
    *next = next.checked_add(1).ok_or_else(|| invalid("contador IDs agotado"))?;
    Ok(id)
}
fn next_id(document: &Value, items: &str, key: &str, prefix: &str) -> V1Result<u64> {
    let mut next = document["next_id"].as_u64().unwrap_or(1);
    for value in document[items].as_array().into_iter().flatten() {
        if let Some(n) = value[key].as_str().and_then(|s| s.strip_prefix(prefix)).and_then(|s| s.parse::<u64>().ok()) {
            next = next.max(n.checked_add(1).ok_or_else(|| invalid("contador IDs agotado"))?);
        }
    }
    Ok(next)
}

pub(crate) fn prepare(project: &Project, asset: &AssetId, kind: ReviewKind, request: &Value, proposal: &Value) -> V1Result<PreparedReview> {
    let source = project.asset(asset).ok_or_else(|| invalid("medio inexistente"))?;
    let master = crate::contracts::master(project, asset)?;
    let digest = digest_json(proposal);
    let request_id = required(request, "request_id")?.to_string();
    let pass = request["pass_required"].as_u64().filter(|p| *p > 0).ok_or_else(|| invalid("pasada inválida"))?;
    let mut docs = BTreeMap::new();
    let mut warnings = Vec::new();
    let command = if kind == ReviewKind::Trims {
        let mut document = trims(project, asset)?;
        if request["trims_digest"] != digest_object_without(&document, &["revision", "updated_at"]) {
            return Err(invalid("recortes cambiaron durante el ciclo"));
        }
        let lane = proposal["lane"].as_str().unwrap_or(if proposal["mode"] == "deep" { "ai-deep" } else { "ai" });
        if !lane.starts_with("ai") || !tv2_domain::ids::is_valid_v1_id(lane) {
            return Err(invalid("propuesta solo puede escribir carriles ai*"));
        }
        let mode = proposal["mode"].as_str().unwrap_or("content");
        if !matches!(mode, "content" | "deep") {
            return Err(invalid("modo de recortes desconocido"));
        }
        let mut next = next_id(&document, "cuts", "cut_id", "cut-")?;
        for id in project.layers.iter().flat_map(|l| &l.deleted_item_ids) {
            if let Some(n) = id.as_str().strip_prefix("cut-").and_then(|s| s.parse::<u64>().ok()) {
                next = next.max(n.checked_add(1).ok_or_else(|| invalid("contador IDs agotado"))?);
            }
        }
        let mut cuts: Vec<Value> = document["cuts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["lane"] != lane || c["origin"] != "ai" || c["edited"] == true || c["accepted"] == true || c["enabled"] == false)
            .cloned()
            .collect();
        for proposed in proposal["cuts"].as_array().ok_or_else(|| invalid("propuesta sin cuts"))? {
            if !proposed.is_object() {
                return Err(invalid("cada recorte propuesto debe ser objeto"));
            }
            references(&master, proposed)?;
            let (range, diagnostics) = snap(&master, seconds(proposed, "t_ini")?, seconds(proposed, "t_fin")?)?;
            if range.duration() < Ticks::from_millis(50) {
                return Err(invalid("recorte menor de 50 ms"));
            }
            let mut cut = proposed.clone();
            cut["cut_id"] = json!(increment(&mut next, "cut")?);
            cut["t_ini"] = crate::secs_json(range.start);
            cut["t_fin"] = crate::secs_json(range.end);
            cut["lane"] = json!(lane);
            cut["origin"] = json!("ai");
            cut["edited"] = json!(false);
            cut["accepted"] = json!(false);
            cut["enabled"] = json!(true);
            let confidence = proposed.get("confidence").map(|_| seconds(proposed, "confidence")).transpose()?.unwrap_or(0.5).clamp(0.0, 1.0);
            cut["confidence"] = json!(confidence);
            cut["evidence"] =
                json!({"proposal_digest":digest,"request_id":request_id,"source_evidence":proposed.get("evidence"),"boundaries":diagnostics});
            cuts.push(cut);
        }
        let lanes = document["lanes"].as_array_mut().unwrap();
        if !lanes.iter().any(|l| l["lane_id"] == lane) {
            lanes.push(json!({"lane_id":lane,"name":lane,"color":"#8ab06b"}));
        }
        document["cuts"] = json!(cuts);
        document["next_id"] = json!(next);
        document["ai"] = json!({"planner":proposal["planner"],"proposal_digest":digest,"mode":mode,"lane":lane,"request_id":request_id});
        let mut imported = crate::trims::trims_from_v1(&document, asset, Some(&source.fingerprint))?;
        for layer in &mut imported.layers {
            if layer.lane.as_deref() != Some(lane) {
                continue;
            }
            let fresh = |item: &tv2_domain::SemanticItem| {
                item.extra.get("evidence").is_some_and(|e| e["proposal_digest"] == digest && e["request_id"] == request_id)
            };
            let mut proposed = layer.clone();
            proposed.items.retain(fresh);
            if proposed.items.is_empty() {
                continue;
            }
            let mut scratch = Project::new("Coalescencia de propuesta");
            scratch.assets.push(source.clone());
            scratch.layer_order.push(proposed.layer_id.clone());
            scratch.layers.push(proposed);
            Command::CoalesceTrims { layer_id: layer.layer_id.clone(), actor_id: None }.apply(&mut scratch).map_err(|e| invalid(e.to_string()))?;
            let mut proposed = scratch.layers.pop().unwrap();
            for item in &mut proposed.items {
                item.edited = false;
            }
            layer.items.retain(|item| !fresh(item));
            layer.items.extend(proposed.items);
            layer.deleted_item_ids.extend(proposed.deleted_item_ids);
        }
        document = crate::trims::trims_to_v1(&imported, &source.fingerprint);
        let mut commands = Vec::new();
        for mut layer in imported.layers {
            if let Some(old) = project.layer(&layer.layer_id) {
                if old.locked || old.deleted {
                    return Err(invalid("carril de recortes protegido"));
                }
                for id in &old.deleted_item_ids {
                    if !layer.deleted_item_ids.contains(id) {
                        layer.deleted_item_ids.push(id.clone());
                    }
                }
                layer.visible = old.visible;
                for (key, value) in &old.extra {
                    if key != "v1_trims_header" {
                        layer.extra.insert(key.clone(), value.clone());
                    }
                }
            }
            commands.push(Command::ReplaceLayer { layer });
        }
        docs.insert("views/trims.json".into(), serde_json::to_string_pretty(&document)?);
        Command::Batch { label: format!("Propuesta de recortes {request_id}"), commands }
    } else {
        if proposal["pass"].as_u64() != Some(pass) {
            return Err(invalid("pasada de montaje fuera de orden"));
        }
        let mut document = montage(project, asset)?;
        let current_digest = if document["clips"].as_array().unwrap().is_empty() {
            Value::Null
        } else {
            json!(digest_object_without(&document, &["revision", "updated_at"]))
        };
        if request["montage_digest"] != current_digest || proposal["montage_digest"] != current_digest {
            return Err(invalid("montaje cambió durante el ciclo"));
        }
        let originals = document["clips"].as_array().unwrap().clone();
        let mut clips: Vec<_> = originals
            .iter()
            .filter(|c| c["origin"] != "ai" || c["edited"] == true || c["state"] == "accepted" || c["state"] == "disabled")
            .cloned()
            .collect();
        let protected: HashSet<String> = clips.iter().filter_map(|c| c["clip_id"].as_str().map(str::to_owned)).collect();
        let mut busy: Vec<_> = clips
            .iter()
            .filter(|c| c["track_id"] == "V1")
            .map(|c| {
                Ok((
                    crate::secs_to_ticks(seconds(c, "seq_ini")?),
                    crate::secs_to_ticks(seconds(c, "seq_ini")? + seconds(c, "source_fin")? - seconds(c, "source_ini")?),
                ))
            })
            .collect::<V1Result<_>>()?;
        busy.sort();
        let mut cursor = Ticks::ZERO;
        let mut next = next_id(&document, "clips", "clip_id", "clip-")?;
        let mut used: Vec<TimeRange> = Vec::new();
        let minimum = guidance(request, "min_clip_seconds", 0.0)?;
        let maximum = guidance(request, "max_clip_seconds", 1e9)?;
        let target = guidance(request, "target_seconds", 0.0)?;
        let tolerance = guidance(request, "tolerance", 0.15)?;
        if minimum > maximum {
            return Err(invalid("min_clip_seconds no puede superar max_clip_seconds"));
        }
        let mut total = Ticks::ZERO;
        let mut validated_clips = Vec::new();
        let mut kept = HashSet::new();
        let entries = proposal["clips"].as_array().filter(|c| !c.is_empty()).ok_or_else(|| invalid("propuesta sin clips"))?;
        for entry in entries {
            if !entry.is_object() {
                return Err(invalid("cada clip propuesto debe ser objeto"));
            }
            references(&master, entry)?;
            let keep = entry["keep"].as_str();
            let mut clip = if let Some(id) = keep {
                if !kept.insert(id) {
                    return Err(invalid("keep repetido"));
                }
                originals.iter().find(|c| c["clip_id"] == id).cloned().ok_or_else(|| invalid("keep inexistente"))?
            } else {
                entry.clone()
            };
            let (range, diagnostics) = if keep.is_some() {
                (
                    TimeRange::new(crate::secs_to_ticks(seconds(&clip, "source_ini")?), crate::secs_to_ticks(seconds(&clip, "source_fin")?)),
                    Value::Null,
                )
            } else {
                snap(&master, seconds(entry, "source_ini")?, seconds(entry, "source_fin")?)?
            };
            if range.duration() < Ticks::from_millis(34) {
                return Err(invalid("clip de montaje demasiado corto"));
            }
            let clip_warnings = clip_warnings(entry, range, &used, minimum, maximum, keep.is_some())?;
            warnings.extend(clip_warnings.iter().map(|warning| format!("Clip {}: {warning}", used.len() + 1)));
            total = Ticks(total.0.checked_add(range.duration().0).ok_or_else(|| invalid("duración total de montaje desbordada"))?);
            validated_clips.push(json!({"index":used.len()+1,"keep":keep,"source_ini":crate::secs_json(range.start),"source_fin":crate::secs_json(range.end),"warnings":clip_warnings,"boundaries":diagnostics}));
            used.push(range);
            if keep.is_some_and(|id| protected.contains(id)) {
                continue;
            }
            if keep.is_none() {
                clip["clip_id"] = json!(increment(&mut next, "clip")?);
            }
            for (a, b) in &busy {
                if cursor < *b && cursor + range.duration() > *a {
                    cursor = *b;
                }
            }
            clip["track_id"] = json!("V1");
            clip["source_ini"] = crate::secs_json(range.start);
            clip["source_fin"] = crate::secs_json(range.end);
            clip["seq_ini"] = crate::secs_json(cursor);
            cursor += range.duration();
            clip["origin"] = json!("ai");
            clip["state"] = json!("proposed");
            clip["edited"] = json!(false);
            if keep.is_none() {
                clip["reason"] = json!(entry["reason"].as_str().unwrap_or("").trim());
                clip["confidence"] = json!(entry.get("confidence").map(|_| seconds(entry, "confidence")).transpose()?.unwrap_or(0.5).clamp(0.0, 1.0));
            } else if let Some(reason) = entry["reason"].as_str().filter(|reason| !reason.trim().is_empty()) {
                clip["reason"] = json!(reason.trim());
            }
            clip["warnings"] = json!(clip_warnings);
            clip["tv2_boundaries"] = diagnostics;
            clip["tv2_proposal_digest"] = json!(digest);
            clips.push(clip);
        }
        document["clips"] = json!(clips);
        document["next_id"] = json!(next);
        document["target_seconds"] = request["target_seconds"].clone();
        let global_warnings = duration_warnings(total, target, tolerance);
        warnings.extend(global_warnings.iter().cloned());
        document["analysis"] = json!({"pass":pass,"request_id":request_id,"proposal_digest":digest,"source_master_digest":request["source_master_digest"],"source_layers_digest":request["source_layers_digest"],"warnings":global_warnings,"proposed_total_seconds":crate::secs_json(total)});
        docs.insert(format!("views/montaje-pass{pass}.json"),serde_json::to_string_pretty(&json!({"schema":"editorial-montage-proposal/1","request_id":request_id,"pass":pass,"proposal_digest":digest,"clips":validated_clips,"total_seconds":crate::secs_json(total),"target_seconds":target,"warnings":global_warnings}))?);
        let parsed = crate::montaje::V1Montaje::parse(document.clone(), Some(&source.fingerprint))?;
        let video = source.probe.video.as_ref().ok_or_else(|| invalid("montaje V1 requiere medio con video"))?;
        let mut sequence = parsed.to_sequence(asset, video.frame_rate, video.width, video.height, source.probe.audio.len() as u32);
        sequence.name = format!("Montaje · pasada {pass}");
        // Preserve the current sequence as a reviewable predecessor. Global clip
        // IDs must remain unique when importing the derived sequence beside it.
        for clip in &mut sequence.clips {
            clip.id = tv2_domain::ClipId::new(format!("{}-{}", sequence.id, clip.id));
            if let Some(group) = &mut clip.link_group {
                *group = format!("{}-{group}", sequence.id);
            }
        }
        let mut projection = sequence.clone();
        projection.extra.remove("v1_original_montage");
        projection.extra.remove("v1_projection_digest");
        sequence.extra.insert("v1_projection_digest".into(), json!(digest_json(&serde_json::to_value(projection)?)));
        docs.insert("views/montaje.json".into(), serde_json::to_string_pretty(&document)?);
        Command::AddSequence { sequence, activate: true }
    };
    docs.insert(format!("views/{}.proposed.json", kind.stem()), serde_json::to_string_pretty(proposal)?);
    docs.insert(format!(".work/reviews/{request_id}/{digest}.json"),serde_json::to_string_pretty(&json!({"schema":"transcriptor-editorial-review/1","request":request,"proposal":proposal,"proposal_digest":digest,"base_revision":project.revision,"state":"prepared","warnings":warnings}))?);
    for path in docs.keys() {
        crate::folder::validate_path(path)?;
    }
    Ok(PreparedReview { command: Some(command), documents: docs, proposal_digest: digest, request_id, pass, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn montage_advisories_follow_v1_thresholds_without_rejecting_duration() {
        let range = |seconds| TimeRange::new(Ticks::ZERO, Ticks::from_seconds(seconds));
        let short = clip_warnings(&json!({}), range(7), &[], 10.0, 20.0, false).unwrap();
        assert!(short.iter().any(|warning| warning.contains("clip corto")));
        assert!(short.iter().any(|warning| warning.contains("no explicó")));
        assert!(clip_warnings(&json!({"reason":"idea"}), range(8), &[], 10.0, 20.0, false).unwrap().is_empty());
        assert!(clip_warnings(&json!({"reason":"idea"}), range(24), &[], 10.0, 20.0, false).unwrap().is_empty());
        assert!(clip_warnings(&json!({"reason":"idea"}), range(25), &[], 10.0, 20.0, false).unwrap()[0].contains("clip largo"));
        assert!(duration_warnings(Ticks::from_seconds(85), 100.0, 0.15).is_empty());
        assert!(duration_warnings(Ticks::from_seconds(115), 100.0, 0.15).is_empty());
        assert_eq!(duration_warnings(Ticks::from_seconds(116), 100.0, 0.15).len(), 1);
    }
    #[test]
    fn repetition_requires_reason_and_emits_warning_beyond_v1_epsilon() {
        let previous = TimeRange::new(Ticks::ZERO, Ticks::from_seconds(5));
        let repeated = TimeRange::new(Ticks::from_seconds(4), Ticks::from_seconds(7));
        assert!(clip_warnings(&json!({"repeat":true}), repeated, &[previous], 0.0, 100.0, false).is_err());
        let warnings = clip_warnings(&json!({"repeat":true,"reason":"callback"}), repeated, &[previous], 0.0, 100.0, false).unwrap();
        assert!(warnings.iter().any(|warning| warning.contains("callback deliberado")));
        let touching = TimeRange::new(Ticks::from_millis(4999), Ticks::from_seconds(7));
        assert!(clip_warnings(&json!({"reason":"next"}), touching, &[previous], 0.0, 100.0, false).unwrap().is_empty());
        assert!(clip_warnings(&json!({"topic_ids":[1]}), repeated, &[], 0.0, 100.0, true).is_err());
    }
}
