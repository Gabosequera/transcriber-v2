//! Derived statistics and cross-track grouping, without inference.
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use unicode_normalization::UnicodeNormalization;

fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.0)
}
fn overlap(a: &Value, b: &Value) -> f64 {
    (number(a, "t_fin").min(number(b, "t_fin")) - number(a, "t_ini").max(number(b, "t_ini"))).max(0.0)
}
fn round4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}
fn average(events: &[&Value], range: &Value, key: &str) -> Value {
    let mut total = 0.0;
    let mut weighted = 0.0;
    for event in events {
        if let Some(value) = event[key].as_f64().filter(|v| v.is_finite()) {
            let amount = overlap(event, range);
            total += amount;
            weighted += value * amount;
        }
    }
    if total > 0.0 { json!(round4(weighted / total)) } else { Value::Null }
}
pub(crate) fn signals(track: &Value, utterance: &Value, words: &[&Value]) -> Value {
    let laughter: Vec<_> = track["laughter"].as_array().into_iter().flatten().filter(|e| overlap(e, utterance) > 0.0).collect();
    let arousal: Vec<_> = track["arousal"].as_array().into_iter().flatten().filter(|e| overlap(e, utterance) > 0.0).collect();
    let mut result = json!({"laughter_event_ids":laughter.iter().map(|e|&e["event_id"]).collect::<Vec<_>>(),
        "laughter_max":laughter.iter().filter_map(|e|e.get("max_conf").or_else(||e.get("conf")).and_then(Value::as_f64)).reduce(f64::max),
        "emphasis_max":words.iter().filter_map(|e|e["emphasis_score"].as_f64()).reduce(f64::max),
        "intensity_z_mean":average(words,utterance,"intensity_z")});
    for key in ["arousal_z", "valence", "dominance"] {
        result[format!("{key}_mean")] = average(&arousal, utterance, key);
    }
    result
}
fn normalized(text: &str) -> String {
    let folded: String = text.to_lowercase().nfkd().filter(|c| unicode_normalization::char::canonical_combining_class(*c) == 0).collect();
    folded.split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit()).filter(|token| token.len() >= 2).collect::<Vec<_>>().join(" ")
}
/// Ratcliff/Obershelp matching blocks, the SequenceMatcher ratio with autojunk
/// disabled used by V1. ASCII normalization makes byte and character indices equal.
fn similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut queue = vec![(0, a.len(), 0, b.len())];
    let mut matched = 0usize;
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let mut best = (alo, blo, 0);
        let mut previous = vec![0usize; bhi - blo + 1];
        for (i, av) in a.iter().enumerate().take(ahi).skip(alo) {
            let mut current = vec![0usize; bhi - blo + 1];
            for j in blo..bhi {
                if *av == b[j] {
                    let length = previous[j - blo] + 1;
                    current[j - blo + 1] = length;
                    if length > best.2 {
                        best = (i + 1 - length, j + 1 - length, length);
                    }
                }
            }
            previous = current;
        }
        let (i, j, size) = best;
        if size == 0 {
            continue;
        }
        matched += size;
        if alo < i && blo < j {
            queue.push((alo, i, blo, j));
        }
        if i + size < ahi && j + size < bhi {
            queue.push((i + size, ahi, j + size, bhi));
        }
    }
    2.0 * matched as f64 / (a.len() + b.len()) as f64
}
fn quality(u: &Value, words: &BTreeMap<String, Value>) -> (f64, usize) {
    let probabilities: Vec<_> = u["word_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().and_then(|id| words.get(id)).and_then(|w| w["asr_prob"].as_f64()))
        .collect();
    let average = if probabilities.is_empty() { 0.0 } else { probabilities.iter().sum::<f64>() / probabilities.len() as f64 };
    (average, u["text"].as_str().unwrap_or("").chars().count())
}
pub(crate) fn regroup(tracks: &mut Map<String, Value>) -> Value {
    let mut utterances: Vec<Value> = tracks
        .iter()
        .flat_map(|(id, t)| {
            t["utterances"].as_array().into_iter().flatten().map(move |u| {
                let mut u = u.clone();
                u["track_id"] = json!(id);
                u
            })
        })
        .collect();
    utterances.sort_by(|a, b| {
        number(a, "t_ini")
            .total_cmp(&number(b, "t_ini"))
            .then(a["track_id"].as_str().cmp(&b["track_id"].as_str()))
            .then(number(a, "t_fin").total_cmp(&number(b, "t_fin")))
    });
    let mut groups = Vec::new();
    let mut start = 0;
    while start < utterances.len() {
        let mut end = start + 1;
        let mut component_end = number(&utterances[start], "t_fin");
        while end < utterances.len() && number(&utterances[end], "t_ini") < component_end {
            component_end = component_end.max(number(&utterances[end], "t_fin"));
            end += 1;
        }
        let ids: BTreeSet<_> = utterances[start..end].iter().filter_map(|u| u["track_id"].as_str().map(str::to_owned)).collect();
        if ids.len() > 1 {
            let group = format!("overlap-{:05}", groups.len() + 1);
            let mut intersections = Vec::new();
            for i in start..end {
                for j in i + 1..end {
                    let a = &utterances[i];
                    let b = &utterances[j];
                    if a["track_id"] != b["track_id"] && overlap(a, b) > 0.0 {
                        intersections.push(json!({"t_ini":number(a,"t_ini").max(number(b,"t_ini")),"t_fin":number(a,"t_fin").min(number(b,"t_fin")),"utterance_ids":[a["utterance_id"],b["utterance_id"]]}));
                    }
                }
            }
            groups.push(json!({"overlap_group":group,"t_ini":number(&utterances[start],"t_ini"),"t_fin":component_end,"utterance_ids":utterances[start..end].iter().map(|u|&u["utterance_id"]).collect::<Vec<_>>(),"track_ids":ids,"intersections":intersections}));
            for u in &mut utterances[start..end] {
                u["overlap_group"] = json!(group);
            }
        }
        start = end;
    }
    let words: BTreeMap<String, Value> = tracks
        .values()
        .flat_map(|t| t["words"].as_array().into_iter().flatten())
        .filter_map(|w| w["word_id"].as_str().map(|id| (id.to_owned(), w.clone())))
        .collect();
    let texts: Vec<_> = utterances.iter().map(|u| normalized(u["text"].as_str().unwrap_or(""))).collect();
    let mut duplicates = Vec::new();
    let mut claimed = HashSet::new();
    let mut active: Vec<usize> = Vec::new();
    for index in 0..utterances.len() {
        active.retain(|i| number(&utterances[*i], "t_fin") > number(&utterances[index], "t_ini") - 0.45);
        if texts[index].len() >= 8 && !claimed.contains(&index) {
            for other in &active {
                if claimed.contains(other) || utterances[index]["track_id"] == utterances[*other]["track_id"] {
                    continue;
                }
                let shorter = (number(&utterances[index], "t_fin") - number(&utterances[index], "t_ini"))
                    .min(number(&utterances[*other], "t_fin") - number(&utterances[*other], "t_ini"));
                if shorter <= 0.0 || overlap(&utterances[index], &utterances[*other]) / shorter < 0.8 {
                    continue;
                }
                let score = similarity(&texts[index], &texts[*other]);
                if score < 0.92 {
                    continue;
                }
                let (primary, secondary) =
                    if quality(&utterances[index], &words) >= quality(&utterances[*other], &words) { (index, *other) } else { (*other, index) };
                let group = format!("duplicate-{:05}", duplicates.len() + 1);
                duplicates.push(json!({"duplicate_group":group,"similarity":round4(score),"primary_utterance_id":utterances[primary]["utterance_id"],"observations":[utterances[primary]["utterance_id"],utterances[secondary]["utterance_id"]],"reason":"coincidencia temporal y textual conservadora"}));
                utterances[primary]["duplicate_group"] = json!(group);
                utterances[secondary]["duplicate_group"] = json!(group);
                utterances[secondary]["duplicate_secondary"] = json!(true);
                claimed.insert(primary);
                claimed.insert(secondary);
                break;
            }
        }
        active.push(index);
    }
    for (id, track) in tracks {
        track["utterances"] = json!(utterances.iter().filter(|u| u["track_id"] == *id).collect::<Vec<_>>());
    }
    json!({"utterances":utterances,"clean_utterance_ids":utterances.iter().filter(|u|u["duplicate_secondary"]!=true).map(|u|&u["utterance_id"]).collect::<Vec<_>>(),"overlap_groups":groups,"duplicate_groups":duplicates})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_and_matching_use_v1_rules() {
        assert_eq!(normalized("¡Sí! Él está aquí, 12 veces."), "si el esta aqui 12 veces");
        assert_eq!(similarity("abcd", "bcde"), 0.75);
        assert_eq!(similarity("a", "b"), 0.0);
    }
}
