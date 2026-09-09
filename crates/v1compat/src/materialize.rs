//! V1 block review package generated from the selected plan and immutable master.
use crate::{V1Result, invalid};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn put(docs: &mut BTreeMap<String, String>, path: String, value: Value) -> V1Result<()> {
    docs.insert(path, serde_json::to_string_pretty(&value)?);
    Ok(())
}
pub fn blocks(project: &tv2_domain::Project, id: &tv2_domain::LayerId) -> V1Result<BTreeMap<String, String>> {
    let layer = project.layer(id).ok_or_else(|| invalid("bloques inexistentes"))?;
    let asset = project.asset(&layer.asset_id).ok_or_else(|| invalid("medio inexistente"))?;
    let evidence = project.masters.iter().find(|m| m.asset_id == asset.id).ok_or_else(|| invalid("materialización requiere master"))?;
    let mut plan = crate::chunks::to_v1(layer, asset.duration())?;
    plan["source_master_digest"] = json!(evidence.source_digest);
    let mut master = (*evidence.document).clone();
    master["chunks"] = plan["chunks"].clone();
    let mut docs = BTreeMap::new();
    put(&mut docs, "project.editorial.master.json".into(), master.clone())?;
    for path in [".work/chunks.selected.json", "views/chunks.json"] {
        put(&mut docs, path.into(), plan.clone())?;
    }
    let clean: std::collections::HashSet<_> =
        master.pointer("/conversation/clean_utterance_ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let mut overview = String::from("# Chunks editoriales\n\n");
    for chunk in plan["chunks"].as_array().ok_or_else(|| invalid("plan sin chunks"))? {
        let cid = chunk["chunk_id"].as_str().ok_or_else(|| invalid("chunk sin ID"))?;
        if !tv2_domain::ids::is_valid_v1_id(cid) {
            return Err(invalid("chunk ID no portable"));
        }
        let a = chunk["t_ini"].as_f64().unwrap();
        let b = chunk["t_fin"].as_f64().unwrap();
        let overlaps = |event: &&Value| event["t_ini"].as_f64().is_some_and(|t| t < b) && event["t_fin"].as_f64().is_some_and(|t| t > a);
        let mut transcript = String::new();
        for u in master.pointer("/conversation/utterances").and_then(Value::as_array).into_iter().flatten().filter(overlaps) {
            if !u["utterance_id"].as_str().is_some_and(|id| clean.contains(id)) {
                continue;
            }
            transcript.push_str(&format!(
                "{}–{} [{}] `{}`\n{}\n\n",
                u["t_ini"],
                u["t_fin"],
                u["track_id"].as_str().unwrap_or(""),
                u["utterance_id"].as_str().unwrap_or(""),
                u["text"].as_str().unwrap_or("")
            ));
        }
        docs.insert(format!("chunks/{cid}/transcript.md"), transcript);
        let mut summary = serde_json::Map::new();
        let mut laughter = serde_json::Map::new();
        let mut arousal = serde_json::Map::new();
        let mut intensity = serde_json::Map::new();
        for (tid, track) in master["tracks"].as_object().into_iter().flatten() {
            let words: Vec<_> = track["words"].as_array().into_iter().flatten().filter(overlaps).collect();
            let laughs: Vec<_> = track["laughter"].as_array().into_iter().flatten().filter(overlaps).collect();
            let arousals: Vec<_> = track["arousal"].as_array().into_iter().flatten().filter(overlaps).collect();
            let max = |events: &[&Value], key: &str| events.iter().filter_map(|e| e[key].as_f64()).reduce(f64::max);
            summary.insert(tid.clone(),json!({"label":track["label"],"word_count":words.len(),"laughter_count":laughs.len(),"laughter_max":laughs.iter().filter_map(|e|e.get("max_conf").or_else(||e.get("conf")).and_then(Value::as_f64)).reduce(f64::max),"arousal_z_max":max(&arousals,"arousal_z"),"intensity_z_max":max(&words,"intensity_z"),"emphasis_max":max(&words,"emphasis_score")}));
            laughter.insert(tid.clone(), json!(laughs));
            arousal.insert(tid.clone(), json!(arousals));
            intensity.insert(
                tid.clone(),
                json!(
                    words
                        .iter()
                        .map(|w| ["word_id", "t_ini", "t_fin", "text", "rms_dbfs", "peak_dbfs", "intensity_z", "emphasis_score"]
                            .into_iter()
                            .map(|key| (key.to_string(), w[key].clone()))
                            .collect::<serde_json::Map<_, _>>())
                        .collect::<Vec<_>>()
                ),
            );
        }
        for (name, value) in [
            ("signals-summary.json", json!({"t_ini":a,"t_fin":b,"tracks":summary})),
            ("laughter.json", json!(laughter)),
            ("arousal.json", json!(arousal)),
            ("intensity.json", json!(intensity)),
        ] {
            put(&mut docs, format!("chunks/{cid}/{name}"), value)?;
        }
        let title = chunk["title"].as_str().unwrap_or(cid);
        overview.push_str(&format!("- `{cid}` {a:.3}–{b:.3}: {title}\n"));
        docs.insert(
            format!("chunks/{cid}/analysis-request.md"),
            format!("# Análisis: {title}\n\nRevisa transcript.md y signals-summary.json. Cita IDs, tiempos y evidencia.\n"),
        );
        docs.insert(format!("chunks/{cid}/moments.md"), format!("# Momentos — {title}\n\nPendiente de revisión.\n"));
    }
    docs.insert("views/chunks.md".into(), overview);
    Ok(docs)
}
