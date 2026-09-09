//! Pure parent/child derivation. Publication is delegated to the recoverable
//! document transaction in application; no source file is ever modified here.
use crate::{V1Result, invalid, master::V1Master, secs_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use tv2_domain::{Asset, Ticks, TimeRange, digest::digest_json};

#[derive(Clone, Debug, Serialize)]
pub struct MappingSegment {
    pub source: TimeRange,
    pub child: TimeRange,
}

/// Exact, unit-rate concatenation. Repeated and reordered source occurrences
/// remain distinct. Construction validates overflow and source bounds.
#[derive(Clone, Debug, Serialize)]
pub struct TimeMapping {
    segments: Vec<MappingSegment>,
    source_duration: Ticks,
    duration: Ticks,
}

impl TimeMapping {
    pub fn new(ranges: Vec<TimeRange>, source_duration: Ticks, chronological: bool) -> V1Result<Self> {
        if ranges.is_empty() || source_duration <= Ticks::ZERO {
            return Err(invalid("un hijo requiere segmentos y duración de fuente positiva"));
        }
        let mut cursor = Ticks::ZERO;
        let mut previous = Ticks::ZERO;
        let mut segments = Vec::with_capacity(ranges.len());
        for source in ranges {
            if source.start < Ticks::ZERO || source.end <= source.start || source.end > source_duration {
                return Err(invalid("segmento fuera del medio padre"));
            }
            if chronological && source.start < previous {
                return Err(invalid("segmentos cronológicos desordenados o solapados"));
            }
            let end = Ticks(cursor.0.checked_add(source.end.0 - source.start.0).ok_or_else(|| invalid("mapping excede tiempo representable"))?);
            segments.push(MappingSegment { source, child: TimeRange::new(cursor, end) });
            cursor = end;
            previous = source.end;
        }
        Ok(Self { segments, source_duration, duration: cursor })
    }

    pub fn duration(&self) -> Ticks {
        self.duration
    }
    pub fn segments(&self) -> &[MappingSegment] {
        &self.segments
    }

    /// (occurrence, clipped source, child range), in child order.
    pub fn map_range(&self, range: TimeRange) -> Vec<(usize, TimeRange, TimeRange)> {
        self.segments
            .iter()
            .enumerate()
            .filter_map(|(index, segment)| {
                let start = range.start.max(segment.source.start);
                let end = range.end.min(segment.source.end);
                (start < end).then(|| {
                    (
                        index,
                        TimeRange::new(start, end),
                        TimeRange::new(segment.child.start + (start - segment.source.start), segment.child.start + (end - segment.source.start)),
                    )
                })
            })
            .collect()
    }

    pub fn to_v1(&self) -> V1Result<Value> {
        // V1 rounds at milliseconds. Reject lossy export rather than invent a
        // different map for frame-accurate V2 edits.
        for segment in &self.segments {
            for t in [segment.source.start, segment.source.end, segment.child.start, segment.child.end] {
                if crate::secs_to_ticks(t.as_seconds_ms()) != t {
                    return Err(invalid("mapping submilisegundo requiere contrato V2; no representable en V1"));
                }
            }
        }
        Ok(json!(
            self.segments
                .iter()
                .map(|s| json!({
                    "source_ini":secs_json(s.source.start),"source_fin":secs_json(s.source.end),
                    "child_ini":secs_json(s.child.start),"child_fin":secs_json(s.child.end)
                }))
                .collect::<Vec<_>>()
        ))
    }
}

/// Recover the actual single-parent concatenation from the frozen export
/// timeline. Unrelated overlays/audio clocks and gaps have no single V1 map and
/// are rejected. This must use the request timeline, never a subsequently edited
/// project. Composited exports can still be retained as media without deriving.
pub fn mapping_from_timeline(
    timeline: &tv2_domain::ResolvedTimeline,
    parent: &Asset,
    range: Option<TimeRange>,
    has_video: bool,
) -> V1Result<TimeMapping> {
    let range = range.unwrap_or(TimeRange::new(Ticks::ZERO, timeline.duration));
    if range.start < Ticks::ZERO || range.end > timeline.duration || range.end <= range.start {
        return Err(invalid("rango de exportación fuera de secuencia"));
    }
    let mut cursor = range.start;
    let mut ranges: Vec<TimeRange> = Vec::new();
    for piece in &timeline.pieces {
        let Some(hit) = piece.range.intersection(&range) else { continue };
        if hit.start != cursor {
            return Err(invalid("hueco en mapping de exportación"));
        }
        let active: Vec<_> = piece.audio.iter().chain(piece.video.iter().filter(|_| has_video)).collect();
        let first = active.first().ok_or_else(|| invalid("exportación contiene hueco sin fuente padre"))?;
        let at = |clip: &tv2_domain::ResolvedClip| -> V1Result<Ticks> {
            if clip.range.start > hit.start || clip.range.end < hit.end || clip.range.start < Ticks::ZERO {
                return Err(invalid("clip resuelto no cubre su segmento"));
            }
            Ok(Ticks(
                clip.source_start
                    .0
                    .checked_add(hit.start.0 - clip.range.start.0)
                    .ok_or_else(|| invalid("mapping fuente excede tiempo representable"))?,
            ))
        };
        let start = at(first)?;
        for clip in &active {
            if clip.asset_id != parent.id || clip.is_image || at(clip)? != start {
                return Err(invalid("exportación con varios padres o relojes simultáneos requiere mapping compuesto V2"));
            }
        }
        let source =
            TimeRange::new(start, Ticks(start.0.checked_add(hit.duration().0).ok_or_else(|| invalid("mapping fuente excede tiempo representable"))?));
        if let Some(last) = ranges.last_mut()
            && last.end == source.start
        {
            last.end = source.end;
        } else {
            ranges.push(source);
        }
        cursor = hit.end;
    }
    if cursor != range.end {
        return Err(invalid("mapping no cubre toda la exportación"));
    }
    TimeMapping::new(ranges, parent.duration(), false)
}

/// A preserved track requires a verified one-to-one child audio inventory.
/// The current compositor produces Mixed, even if several parents contributed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AudioDisposition {
    Preserved { tracks: BTreeMap<String, u32> },
    Mixed { output_audio_index: u32 },
    Silent,
}

fn range(v: &Value) -> V1Result<TimeRange> {
    let a = v["t_ini"].as_f64().ok_or_else(|| invalid("evidencia sin t_ini"))?;
    let b = v["t_fin"].as_f64().ok_or_else(|| invalid("evidencia sin t_fin"))?;
    if !a.is_finite() || !b.is_finite() || a < 0.0 || a >= b || b > Ticks::MAX.as_seconds_f64() {
        return Err(invalid("rango de evidencia inválido"));
    }
    Ok(TimeRange::new(crate::secs_to_ticks(a), crate::secs_to_ticks(b)))
}

fn slice(events: &[Value], mapping: &TimeMapping, key: &str) -> V1Result<Vec<Value>> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        let id = event[key].as_str().ok_or_else(|| invalid(format!("evidencia sin {key}")))?;
        if !seen.insert(id) {
            return Err(invalid(format!("ID de evidencia repetido: {id}")));
        }
        let event_range = range(event)?;
        if event_range.end > mapping.source_duration {
            return Err(invalid("evidencia excede padre"));
        }
        for (index, source, child) in mapping.map_range(event_range) {
            let mut part = event.clone();
            part[key] = json!(format!("{id}~s{}", index + 1));
            part["t_ini"] = secs_json(child.start);
            part["t_fin"] = secs_json(child.end);
            part["source_id"] = json!(id);
            part["source_range"] = json!([secs_json(source.start), secs_json(source.end)]);
            part["source_segment"] = json!(index);
            result.push(part);
        }
    }
    result.sort_by(|a, b| {
        a["t_ini"].as_f64().partial_cmp(&b["t_ini"].as_f64()).unwrap_or(std::cmp::Ordering::Equal).then(a[key].as_str().cmp(&b[key].as_str()))
    });
    Ok(result)
}

fn project_track(original: &Value, mapping: &TimeMapping) -> V1Result<Value> {
    if !original.is_object() {
        return Err(invalid("pista padre no es un objeto"));
    }
    let mut track = original.clone();
    for collection in ["words", "utterances", "laughter", "arousal", "emotions", "intensity"] {
        let events = match original.get(collection) {
            None => &[][..],
            Some(v) => v.as_array().ok_or_else(|| invalid(format!("{collection} no es lista")))?.as_slice(),
        };
        let key = match collection {
            "words" => "word_id",
            "utterances" => "utterance_id",
            _ => "event_id",
        };
        track[collection] = json!(slice(events, mapping, key)?);
    }
    let words = track["words"].as_array().cloned().unwrap_or_default();
    let utterances = track["utterances"].as_array().cloned().unwrap_or_default();
    let mut rebuilt = Vec::new();
    for mut utterance in utterances {
        let ids: HashSet<_> = utterance["word_ids"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let selected: Vec<_> = words
            .iter()
            .filter(|w| w["source_segment"] == utterance["source_segment"] && w["source_id"].as_str().is_some_and(|id| ids.contains(id)))
            .collect();
        if selected.is_empty() {
            continue;
        } // Partial text without words is not evidence.
        let text = selected.iter().filter_map(|w| w["text"].as_str()).collect::<Vec<_>>().join(" ");
        let word_ids = json!(selected.iter().map(|w| &w["word_id"]).collect::<Vec<_>>());
        for key in ["overlap_group", "duplicate_group", "duplicate_secondary", "signals"] {
            utterance.as_object_mut().unwrap().remove(key);
        }
        utterance["word_ids"] = word_ids;
        utterance["text"] = json!(text);
        utterance["t_ini"] = selected.first().unwrap()["t_ini"].clone();
        utterance["t_fin"] = json!(selected.iter().filter_map(|w| w["t_fin"].as_f64()).fold(0.0, f64::max));
        utterance["signals"] = crate::derived_conversation::signals(&track, &utterance, &selected);
        rebuilt.push(utterance);
    }
    track["utterances"] = json!(rebuilt);
    // Parent generated paths and stream offsets must not become child locators.
    for key in ["audio", "audio_path", "wav", "path", "offset"] {
        track.as_object_mut().unwrap().remove(key);
    }
    Ok(track)
}

pub fn derive_layers(parent_layers: &[Value], mapping: &TimeMapping, child: &Asset, child_digest: &str, parent_digest: &str) -> V1Result<Vec<Value>> {
    let mut result = Vec::new();
    for original in parent_layers {
        crate::layers::layer_from_v1(original, &child.id, None, mapping.source_duration)?;
        if original["deleted"] == true {
            continue;
        }
        let mut layer = original.clone();
        let mut items = Vec::new();
        let mut dropped = Vec::new();
        for item in original["items"].as_array().unwrap() {
            let mut ranges = Vec::new();
            for part in item["ranges"].as_array().ok_or_else(|| invalid("item sin ranges"))? {
                for (index, source, child_range) in mapping.map_range(range(part)?) {
                    let mut r = part.clone();
                    r["t_ini"] = secs_json(child_range.start);
                    r["t_fin"] = secs_json(child_range.end);
                    r["source_range"] = json!([secs_json(source.start), secs_json(source.end)]);
                    r["source_segment"] = json!(index);
                    ranges.push(r);
                }
            }
            ranges.sort_by(|a, b| a["t_ini"].as_f64().partial_cmp(&b["t_ini"].as_f64()).unwrap());
            if ranges.is_empty() {
                dropped.push(item["item_id"].clone());
                continue;
            }
            let mut item = item.clone();
            item["ranges"] = json!(ranges);
            item["source_item_id"] = item["item_id"].clone();
            items.push(item);
        }
        loop {
            let ids: HashSet<_> = items.iter().filter_map(|i| i["item_id"].as_str().map(str::to_owned)).collect();
            let before = items.len();
            items.retain(|item| {
                let keep = item["parent_id"].as_str().is_none_or(|id| ids.contains(id));
                if !keep {
                    dropped.push(item["item_id"].clone());
                }
                keep
            });
            if items.len() == before {
                break;
            }
        }
        layer["items"] = json!(items);
        layer["revision"] = json!(0);
        layer["media_fingerprint"] = crate::master::identity_json(&child.fingerprint);
        layer["source_master_digest"] = json!(child_digest);
        layer["derived_from"] = json!({"source_master_digest":parent_digest,"layer_revision":original["revision"],"dropped_item_ids":dropped});
        crate::layers::layer_from_v1(&layer, &child.id, Some(&child.fingerprint), mapping.duration)?;
        result.push(layer);
    }
    Ok(result)
}

/// Materialize a reviewable package after the exporter verified its output and
/// the caller probed/fingerprinted `child`. This function performs no media I/O.
pub fn derive_package(
    parent: &V1Master,
    child: &Asset,
    mapping: &TimeMapping,
    audio: &AudioDisposition,
    layers: &[Value],
) -> V1Result<BTreeMap<String, String>> {
    if mapping.source_duration != parent.duration {
        return Err(invalid("mapping pertenece a otra duración padre"));
    }
    if (child.duration() - mapping.duration).abs() > Ticks::from_millis(100) {
        return Err(invalid("duración de exportación no coincide con mapping"));
    }
    let segments = mapping.to_v1()?;
    let originals = parent.raw["tracks"].as_object().ok_or_else(|| invalid("padre sin tracks"))?;
    let mut projected = serde_json::Map::new();
    for (id, original) in originals {
        let mut original = original.clone();
        if original.is_object() && original.get("utterances").is_none() {
            original["utterances"] = json!(
                parent
                    .raw
                    .pointer("/conversation/utterances")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|u| u["track_id"] == *id)
                    .collect::<Vec<_>>()
            );
        }
        projected.insert(id.clone(), project_track(&original, mapping)?);
    }
    let mut tracks = serde_json::Map::new();
    let mut warnings = Vec::new();
    match audio {
        AudioDisposition::Preserved { tracks: bindings } => {
            let mut indices = HashSet::new();
            if bindings.len() != originals.len() || bindings.len() != child.probe.audio.len() {
                return Err(invalid("conservación requiere mapping explícito de todas las pistas"));
            }
            for (id, index) in bindings {
                let stream =
                    child.probe.audio.iter().find(|s| s.audio_index == *index).ok_or_else(|| invalid("pista conservada ausente del sondeo hijo"))?;
                if !indices.insert(*index) {
                    return Err(invalid("dos pistas originales no pueden atribuirse al mismo stream hijo"));
                }
                let mut track = projected.get(id).cloned().ok_or_else(|| invalid("pista padre desconocida"))?;
                track["stream_index"] = json!(index);
                track["offset"] = secs_json(stream.start_time - child.probe.start_time);
                tracks.insert(id.clone(), track);
            }
        }
        AudioDisposition::Mixed { output_audio_index } => {
            if child.probe.audio.len() != 1 || child.probe.audio[0].audio_index != *output_audio_index {
                return Err(invalid("salida mezclada debe declarar exactamente el stream observado"));
            }
            tracks.insert("mix".into(),json!({"label":"Mezcla exportada","stream_index":output_audio_index,"offset":crate::secs_json(child.probe.audio[0].start_time-child.probe.start_time),"words":[],"utterances":[],"laughter":[],"arousal":[],"emotions":[],"intensity":[],"provenance":{"kind":"mixed","parent_evidence_track_ids":originals.keys().collect::<Vec<_>>(),"evidence":"archive/mapped-parent-evidence.json"}}));
            warnings.push("Audio mezclado: evidencia original archivada con mapping; no se atribuye como análisis del stream hijo.");
        }
        AudioDisposition::Silent => {
            if !child.probe.audio.is_empty() {
                return Err(invalid("salida silent contiene audio"));
            }
            warnings.push("Sin audio conservado: evidencia original archivada con mapping.");
        }
    }
    let conversation = crate::derived_conversation::regroup(&mut tracks);
    let projected_conversation = crate::derived_conversation::regroup(&mut projected);
    let parent_digest = parent.source_master_digest();
    // Unknown parent fields remain byte-semantically in the source archive;
    // copying them to the new root could mislabel old times/locators as child.
    let mut master = json!({"schema":"editorial-master/1","transcription":parent.raw["transcription"]});
    master["project"] = json!({"name":child.name,"profile":"editorial_voz"});
    master["media"] =
        json!({"path":child.path,"duration":secs_json(mapping.duration),"t0":secs_json(child.probe.start_time),"fingerprint":child.fingerprint});
    master["tracks"] = json!(tracks);
    master["conversation"] = conversation;
    master["chunks"] = json!([]);
    master["provenance"] = json!({"method":"derived/no-inference","baselines":"parent","audio":audio,"warnings":warnings});
    master["derivation"] = json!({"schema":"editorial-derivation/1","source_master_digest":parent_digest,"source_fingerprint":parent.fingerprint,"segments":segments,"ancestor":parent.raw["derivation"],"audio":audio,"source_evidence":"archive/parent.editorial.master.json"});
    master.as_object_mut().unwrap().remove("generated_at");
    let derived = V1Master::parse(master.clone())?;
    let mut docs = BTreeMap::new();
    for (path, value) in [
        ("project.editorial.master.json", master),
        ("archive/parent.editorial.master.json", parent.raw.clone()),
        (
            "archive/mapped-parent-evidence.json",
            json!({"schema":"transcriptor-mapped-evidence/1","source_master_digest":parent_digest,"source_document_digest":digest_json(&parent.raw),"segments":segments,"audio":audio,"tracks":projected,"conversation":projected_conversation}),
        ),
    ] {
        docs.insert(path.to_string(), serde_json::to_string_pretty(&value)?);
    }
    for layer in derive_layers(layers, mapping, child, &derived.source_master_digest(), &parent_digest)? {
        let path = format!("layers/{}.json", layer["layer_id"].as_str().unwrap());
        crate::folder::validate_path(&path)?;
        if docs.insert(path, serde_json::to_string_pretty(&layer)?).is_some() {
            return Err(invalid("layer_id repetido en derivación"));
        }
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(a: i64, b: i64) -> TimeRange {
        TimeRange::new(Ticks::from_seconds(a), Ticks::from_seconds(b))
    }
    #[test]
    fn repeated_occurrences_have_distinct_evidence_and_mixed_audio_does_not_impersonate_tracks() {
        let parent = V1Master::parse(crate::master::fixtures::master(12.0)).unwrap();
        let map = TimeMapping::new(vec![r(7, 10), r(0, 3), r(7, 10)], parent.duration, false).unwrap();
        let child = tv2_domain::commands::tests_support::fake_video("child", 9);
        let docs = derive_package(&parent, &child, &map, &AudioDisposition::Mixed { output_audio_index: 0 }, &[]).unwrap();
        let master: Value = serde_json::from_str(&docs["project.editorial.master.json"]).unwrap();
        assert_eq!(master["tracks"].as_object().unwrap().len(), 1);
        assert!(master["tracks"]["mix"]["words"].as_array().unwrap().is_empty());
        let evidence: Value = serde_json::from_str(&docs["archive/mapped-parent-evidence.json"]).unwrap();
        let words = evidence["tracks"]["A"]["words"].as_array().unwrap();
        assert_eq!(words[0]["word_id"], "A-w-2~s1");
        assert_eq!(words[2]["word_id"], "A-w-2~s3");
        assert_eq!(words[2]["source_id"], "A-w-2");
        assert_eq!(serde_json::from_str::<Value>(&docs["archive/parent.editorial.master.json"]).unwrap(), parent.raw);
        assert!(
            derive_package(&parent, &child, &map, &AudioDisposition::Preserved { tracks: BTreeMap::from([("A".into(), 0), ("B".into(), 0)]) }, &[])
                .is_err()
        );
    }
    #[test]
    fn mapping_rejects_invalid_ranges_overflow_and_lossy_v1_times() {
        assert!(TimeMapping::new(vec![r(2, 4), r(1, 3)], Ticks::from_seconds(10), true).is_err());
        assert!(TimeMapping::new(vec![TimeRange::new(Ticks::ZERO, Ticks::MAX); 2], Ticks::MAX, false).is_err());
        let map = TimeMapping::new(vec![TimeRange::new(Ticks(1), Ticks(1000))], Ticks(1001), false).unwrap();
        assert!(map.to_v1().is_err());
    }
    #[test]
    fn child_keeps_layer_identity_tombstones_and_clipped_range_provenance() {
        let parent = V1Master::parse(crate::master::fixtures::master(12.0)).unwrap();
        let map = TimeMapping::new(vec![r(0, 3), r(7, 10)], parent.duration, false).unwrap();
        let child = tv2_domain::commands::tests_support::fake_video("child", 6);
        let layer = json!({"schema":"editorial-layer/1","layer_id":"layer-one","kind":"user","items":[{"item_id":"item-one","label":"evidence","state":"accepted","edited":true,"ranges":[{"t_ini":1,"t_fin":9,"unknown":"keep"}]}],"deleted_item_ids":["item-deleted"],"revision":7,"future":{"keep":true}});
        let result = derive_layers(&[layer], &map, &child, "new-digest", "parent-digest").unwrap();
        assert_eq!(result[0]["layer_id"], "layer-one");
        assert_eq!(result[0]["items"][0]["ranges"].as_array().unwrap().len(), 2);
        assert_eq!(result[0]["items"][0]["ranges"][1]["unknown"], "keep");
        assert_eq!(result[0]["items"][0]["state"], "accepted");
        assert_eq!(result[0]["deleted_item_ids"], json!(["item-deleted"]));
        assert_eq!(result[0]["future"], json!({"keep":true}));
    }
}
