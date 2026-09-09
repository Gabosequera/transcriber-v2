//! Safe boundaries use all tracks, never just the selected speaker.
use crate::error::DomainResult;
use crate::{CommandEffect, DomainError, SemanticLayer, Ticks, TimeRange};
use serde_json::{Value, json};

#[derive(Clone)]
struct Interval {
    a: i64,
    b: i64,
    kind: &'static str,
    track: String,
}
fn ms(v: &Value, key: &str) -> DomainResult<i64> {
    v[key]
        .as_f64()
        .filter(|x| x.is_finite() && *x >= 0.0 && *x < i64::MAX as f64 / 1000.0)
        .map(|x| (x * 1000.0).round() as i64)
        .ok_or_else(|| DomainError::invalid(format!("evidencia de borde con {key} inválido")))
}
fn intervals(master: &Value) -> DomainResult<(Vec<Interval>, Vec<Interval>)> {
    let mut hard = Vec::new();
    let mut utterances = Vec::new();
    if let Some(tracks) = master["tracks"].as_object() {
        for (track, data) in tracks {
            for (key, pad, kind) in [("words", 40, "word"), ("laughter", 120, "laughter")] {
                for event in data[key].as_array().into_iter().flatten() {
                    hard.push(Interval { a: (ms(event, "t_ini")? - pad).max(0), b: ms(event, "t_fin")? + pad, kind, track: track.clone() });
                }
            }
        }
    }
    for event in master.pointer("/conversation/utterances").and_then(Value::as_array).into_iter().flatten() {
        utterances.push(Interval {
            a: ms(event, "t_ini")?,
            b: ms(event, "t_fin")?,
            kind: "utterance",
            track: event["track_id"].as_str().unwrap_or("").into(),
        });
    }
    hard.sort_by_key(|i| (i.a, i.b));
    Ok((hard, utterances))
}
fn safety(hard: &[&Interval], utterances: &[&Interval], t: i64) -> (usize, usize, i64, Value) {
    let mut words = 0;
    let mut laughter = 0;
    let mut speech = 0;
    let mut clearance = 60_000;
    let mut tracks = std::collections::BTreeSet::new();
    for i in hard {
        if i.a < t && t < i.b {
            if i.kind == "word" {
                words += 1;
            } else {
                laughter += 1;
            }
            tracks.insert(&i.track);
        } else {
            clearance = clearance.min((t - i.a).abs().min((t - i.b).abs()));
        }
    }
    for i in utterances {
        if i.a < t && t < i.b {
            speech += 1;
            tracks.insert(&i.track);
        }
    }
    (
        words + laughter,
        speech,
        clearance,
        json!({"word_conflicts":words,"laughter_conflicts":laughter,"utterance_conflicts":speech,"active_tracks":tracks,"hard_clearance_seconds":clearance as f64/1000.0}),
    )
}

pub fn snap(layer: &mut SemanticLayer, master: &Value, duration: Ticks, radius: Ticks) -> DomainResult<CommandEffect> {
    if layer.kind != crate::LayerKind::Blocks || !layer.extra.contains_key("v1_chunks_header") {
        return Err(DomainError::invalid("se requiere un plan de bloques"));
    }
    crate::layers::validate_layer(layer, duration)?;
    if radius < Ticks::from_millis(1) || radius > Ticks::from_seconds(300) {
        return Err(DomainError::out_of_range("radio seguro entre 1 ms y 300 s"));
    }
    let (hard, utterances) = intervals(master)?;
    let mut order: Vec<_> = (0..layer.items.len()).collect();
    order.sort_by_key(|i| layer.items[*i].start());
    let to_ms = |t: Ticks| t.0 / (crate::FLICKS_PER_SECOND / 1000);
    let dur = to_ms(duration);
    let radius = to_ms(radius);
    let targets: Vec<_> = order.iter().take(order.len().saturating_sub(1)).map(|i| to_ms(layer.items[*i].end())).collect();
    let mut bounds = vec![0];
    let mut adjustments = Vec::new();
    for (n, target) in targets.iter().copied().enumerate() {
        let previous = *bounds.last().unwrap();
        let lower = (previous + 1).max(target - radius).max(dur - (targets.len() - n) as i64 * 3_000_000);
        let upper = (targets.get(n + 1).copied().unwrap_or(dur) - 1).min(target + radius).min(previous + 3_000_000);
        if lower > upper {
            return Err(DomainError::out_of_range("no hay espacio para el snap seguro"));
        }
        let near: Vec<_> = hard.iter().filter(|i| i.b >= lower - 1000 && i.a <= upper + 1000).collect();
        let speech: Vec<_> = utterances.iter().filter(|i| i.b >= lower && i.a <= upper).collect();
        let mut candidates = std::collections::BTreeSet::from([target.clamp(lower, upper), lower, upper]);
        for i in near.iter().chain(&speech) {
            for t in [i.a, i.b] {
                if lower <= t && t <= upper {
                    candidates.insert((t - 1).max(lower));
                    candidates.insert((t + 1).min(upper));
                }
            }
        }
        let mut cursor = lower;
        for i in &near {
            let a = i.a.max(lower);
            let b = i.b.min(upper);
            if b <= lower || a >= upper {
                continue;
            }
            if a > cursor {
                candidates.insert((cursor + a) / 2);
            }
            cursor = cursor.max(b);
        }
        if cursor < upper {
            candidates.insert((cursor + upper) / 2);
        }
        let mut scored: Vec<_> = candidates
            .into_iter()
            .map(|t| {
                let (h, s, c, diagnostic) = safety(&near, &speech, t);
                ((h, s, -c.min(750), (t - target).abs(), t), diagnostic)
            })
            .collect();
        scored.sort_by_key(|(score, _)| *score);
        let (score, diagnostic) = scored.remove(0);
        if score.0 > 0 {
            return Err(DomainError::precondition("no existe corte libre de palabras/risas dentro del radio"));
        }
        bounds.push(score.4);
        adjustments.push(json!({"boundary_index":n+1,"proposed":target as f64/1000.0,"final":score.4 as f64/1000.0,"delta_seconds":(score.4-target) as f64/1000.0,"search_radius_seconds":radius as f64/1000.0,"safety":diagnostic}));
    }
    bounds.push(dur);
    let clean: std::collections::HashSet<_> =
        master.pointer("/conversation/clean_utterance_ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    for (n, index) in order.into_iter().enumerate() {
        let item = &mut layer.items[index];
        let a = bounds[n];
        let b = bounds[n + 1];
        item.extra.insert("semantic_t_ini".into(), json!(item.start().as_seconds_ms()));
        item.extra.insert("semantic_t_fin".into(), json!(item.end().as_seconds_ms()));
        let inside: Vec<_> = master
            .pointer("/conversation/utterances")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|u| {
                u["utterance_id"].as_str().is_some_and(|id| clean.contains(id))
                    && ms(u, "t_ini").is_ok_and(|t| t < b)
                    && ms(u, "t_fin").is_ok_and(|t| t > a)
            })
            .collect();
        item.extra.insert("first_utterance_id".into(), inside.first().map(|u| u["utterance_id"].clone()).unwrap_or(Value::Null));
        item.extra.insert("last_utterance_id".into(), inside.last().map(|u| u["utterance_id"].clone()).unwrap_or(Value::Null));
        item.ranges = vec![TimeRange::new(Ticks::from_millis(a), Ticks::from_millis(b))];
        item.edited = true;
        item.extra.insert("tv2_original_range".into(), json!([item.start(), item.end()]));
        if [n.checked_sub(1), Some(n)]
            .into_iter()
            .flatten()
            .any(|i| adjustments.get(i).is_some_and(|a| a["safety"]["utterance_conflicts"].as_u64().unwrap_or(0) > 0))
        {
            let warning = json!("Borde dentro de intervención: no existe silencio global en el radio de búsqueda.");
            let values = item.extra.entry("warnings".to_string()).or_insert_with(|| json!([]));
            if let Some(values) = values.as_array_mut()
                && !values.contains(&warning)
            {
                values.push(warning);
            }
        }
    }
    let header =
        layer.extra.get_mut("v1_chunks_header").and_then(Value::as_object_mut).ok_or_else(|| DomainError::invalid("cabecera de plan inválida"))?;
    header.insert("boundary_snap".into(), json!("global-safe/2"));
    header.insert("boundary_adjustments".into(), json!(adjustments));
    layer.revision = layer.revision.checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
    crate::layers::validate_layer(layer, duration)?;
    Ok(CommandEffect {
        label: "Ajustar bloques a bordes seguros".into(),
        affected: layer.items.iter().map(|i| i.item_id.to_string()).collect(),
        ..Default::default()
    })
}
