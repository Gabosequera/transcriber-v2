//! External editorial review contracts. These are pure adapters: their commands
//! must go through Session::prepare/execute_prepared with the external actor.
//! Request/pass documents belong to the same recoverable publication transaction.
use crate::{V1Result, invalid, master::V1Master};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use tv2_domain::{
    AssetId, Command, ItemState, LayerKind, Project, SemanticLayer, TimeRange,
    digest::{digest_json, digest_object_without},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    Layers,
    Topics,
    Trims,
    Montage,
}

impl ReviewKind {
    pub fn stem(self) -> &'static str {
        match self {
            Self::Layers => "layers",
            Self::Topics => "topics",
            Self::Trims => "trims",
            Self::Montage => "montage",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewRequest {
    pub request: Value,
    pub snapshot: Value,
    pub documents: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct PreparedReview {
    /// None means topics pass 1, which publishes evidence only.
    pub command: Option<Command>,
    pub documents: BTreeMap<String, String>,
    pub proposal_digest: String,
    pub request_id: String,
    pub pass: u64,
    pub warnings: Vec<String>,
}

/// Legacy generic V1 responses had no request ID. This is a separate, explicit
/// binding operation: callers must present its source and target before opting
/// in, preserve the original proposal, then use the normal prepared-command gate.
/// Neither source digest is rewritten; stale or mismatched evidence still fails.
pub fn adopt_legacy_layers_proposal(project: &Project, asset: &AssetId, request: &Value, proposal: &Value) -> V1Result<Value> {
    if proposal["schema"] != "editorial-layers-proposal/1" || request["schema"] != "editorial-layers-request/1" {
        return Err(invalid("La adopción explícita solo admite respuestas V1 genéricas de capas"));
    }
    if proposal.get("request_id").is_some_and(|id| !id.is_null()) {
        return Err(invalid("La respuesta ya identifica un pedido; no se reasocia a otro ciclo"));
    }
    if proposal.get("tv2_adoption").is_some() {
        return Err(invalid("La respuesta ya contiene una declaración de adopción"));
    }
    let request_id = required(request, "request_id")?;
    if !tv2_domain::ids::is_valid_v1_id(request_id) {
        return Err(invalid("El pedido de adopción requiere un ID portable"));
    }
    let mut bound = proposal.clone();
    bound["request_id"] = json!(request_id);
    validate_base(project, asset, request, &bound)?;
    bound["tv2_adoption"] = json!({"schema":"transcriptor-v1-proposal-adoption/1","original_proposal_digest":digest_json(proposal),"request_id":request_id,"project_id":project.project_id,"project_revision":project.revision});
    Ok(bound)
}

pub(crate) fn master(project: &Project, asset: &AssetId) -> V1Result<V1Master> {
    let evidence = project.masters.iter().find(|m| &m.asset_id == asset).ok_or_else(|| invalid("revisión requiere master de evidencia"))?;
    V1Master::parse((*evidence.document).clone())
}

/// Same digest contract as editorial_layers.snapshot_value, including deleted
/// layers/tombstones. Evidence projections are not editable layers.
pub fn layers_snapshot(project: &Project, asset: &AssetId) -> V1Result<Value> {
    let source = project.asset(asset).ok_or_else(|| invalid("medio inexistente"))?;
    let evidence = project.masters.iter().find(|m| &m.asset_id == asset).ok_or_else(|| invalid("snapshot de capas requiere master"))?;
    let layers: Vec<_> = project
        .layers
        .iter()
        .filter(|l| {
            &l.asset_id == asset
                && !l.extra.contains_key("master_projection")
                && matches!(l.kind, LayerKind::User | LayerKind::Topics | LayerKind::Ai | LayerKind::Other(_))
        })
        .map(|l| crate::layers::layer_to_v1(l, &source.fingerprint))
        .collect();
    let mut snapshot = json!({"schema":"editorial-layers-view/1","source_master_digest":evidence.document.source_digest(),"layers":layers});
    snapshot["source_layers_digest"] = json!(digest_json(&snapshot));
    Ok(snapshot)
}

pub fn prepare_request(project: &Project, asset: &AssetId, request_id: &str, kind: ReviewKind, scope: Option<TimeRange>) -> V1Result<ReviewRequest> {
    if !tv2_domain::ids::is_valid_v1_id(request_id) {
        return Err(invalid("request_id debe ser un ID portable único del ciclo"));
    }
    let source = project.asset(asset).ok_or_else(|| invalid("medio inexistente"))?;
    let snapshot = layers_snapshot(project, asset)?;
    let scope = scope.unwrap_or(TimeRange::new(tv2_domain::Ticks::ZERO, source.duration()));
    if scope.start < tv2_domain::Ticks::ZERO || scope.end <= scope.start || scope.end > source.duration() {
        return Err(invalid("ámbito de pedido fuera de medio"));
    }
    let mut request = json!({"schema":format!("editorial-{}-request/1",kind.stem()),"request_id":request_id,
        "source_master_digest":snapshot["source_master_digest"],"source_layers_digest":snapshot["source_layers_digest"],
        "tv2_project_id":project.project_id,"tv2_project_revision":project.revision,"pass_required":1,
        "scope":{"t_ini":crate::secs_json(scope.start),"t_fin":crate::secs_json(scope.end)}});
    if kind == ReviewKind::Topics {
        request["layer_id"] = json!(format!("topics-{request_id}"));
    }
    crate::review_media::extend_request(project, asset, kind, &mut request)?;
    let mut documents = BTreeMap::new();
    documents.insert("project.editorial.master.json".into(), serde_json::to_string_pretty(&master(project, asset)?.raw)?);
    documents.insert("views/layers.json".into(), serde_json::to_string_pretty(&snapshot)?);
    documents.insert(format!("views/{}-request.json", kind.stem()), serde_json::to_string_pretty(&request)?);
    documents.insert(format!("views/{}-agent-request.md", kind.stem()), request_markdown(&request, kind));
    Ok(ReviewRequest { request, snapshot, documents })
}

fn request_markdown(request: &Value, kind: ReviewKind) -> String {
    format!(
        "# Revisión editorial: {}\n\nLee layers.json y el master de evidencia completo para el ámbito solicitado.\nDevuelve editorial-{}-proposal/1 con los digests y request_id de {}-request.json.\nPasada requerida: {}. No inventes aceptación humana ni borres decisiones editadas.\n{}\n",
        kind.stem(),
        kind.stem(),
        kind.stem(),
        request["pass_required"],
        match kind {
            ReviewKind::Topics =>
                "Pasada 1: mapa cronológico completo (items, complete:true). Pasada 2: unifica recurrencias citando previous_pass_digest y source_item_ids; conserva cada rango y cada item de la primera pasada exactamente una vez.",
            ReviewKind::Layers =>
                "Incluye layer o layers con kind user, topics o ai, items proposed y edited:false. No propongas capas de evidencia, bloques, recortes ni marcas por este contrato genérico.",
            ReviewKind::Trims =>
                "Incluye mode (content/deep), lane ai*, cuts con t_ini, t_fin, reason y evidencia. Lee trims.json antes de proponer; el pedido fija trims_digest.",
            ReviewKind::Montage =>
                "Incluye pass, montage_digest y clips ordenados con source_ini/source_fin o keep (ID existente). Explica cada clip; repeticiones requieren repeat:true y reason. Conserva referencias a intervenciones/temas. La aplicación crea una secuencia nueva y conserva la antecesora.",
        }
    )
}

pub(crate) fn required<'a>(v: &'a Value, key: &str) -> V1Result<&'a str> {
    v[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| invalid(format!("falta {key}")))
}

fn validate_base(project: &Project, asset: &AssetId, request: &Value, proposal: &Value) -> V1Result<ReviewKind> {
    let kind = match request["schema"].as_str() {
        Some("editorial-layers-request/1") => ReviewKind::Layers,
        Some("editorial-topics-request/1") => ReviewKind::Topics,
        Some("editorial-trims-request/1") => ReviewKind::Trims,
        Some("editorial-montage-request/1") => ReviewKind::Montage,
        _ => return Err(invalid("schema de pedido no soportado; conservar documento sin aplicarlo")),
    };
    if proposal["schema"] != format!("editorial-{}-proposal/1", kind.stem()) {
        return Err(invalid("schema de propuesta no coincide con pedido"));
    }
    let snapshot = layers_snapshot(project, asset)?;
    for key in ["source_master_digest", "source_layers_digest"] {
        if required(request, key)? != required(&snapshot, key)? {
            return Err(invalid(format!("pedido obsoleto: {key}; prepara un ciclo nuevo")));
        }
    }
    for key in ["request_id", "source_master_digest", "source_layers_digest"] {
        if required(request, key)? != required(proposal, key)? {
            return Err(invalid(format!("propuesta ajena al ciclo: {key}")));
        }
    }
    if let Some(id) = request.get("tv2_project_id")
        && *id != json!(project.project_id)
    {
        return Err(invalid("pedido pertenece a otro proyecto"));
    }
    if let Some(revision) = request.get("tv2_project_revision")
        && revision.as_u64() != Some(project.revision)
    {
        return Err(invalid("revisión del pedido obsoleta"));
    }
    Ok(kind)
}

fn proposal_layers(proposal: &Value) -> V1Result<Vec<Value>> {
    let mut result = Vec::new();
    if let Some(layer) = proposal.get("layer") {
        if !layer.is_object() {
            return Err(invalid("layer debe ser objeto"));
        }
        result.push(layer.clone());
    }
    if let Some(layers) = proposal.get("layers") {
        result.extend(layers.as_array().ok_or_else(|| invalid("layers debe ser lista"))?.iter().cloned());
    }
    if result.is_empty() {
        return Err(invalid("respuesta sin capas"));
    }
    Ok(result)
}

/// Protect human edits, acceptance, disabled decisions and tombstones. Agent
/// metadata cannot fabricate human attribution. Domain validates the merged tree.
pub(crate) fn merge_layer(project: &Project, mut proposed: SemanticLayer) -> V1Result<SemanticLayer> {
    for item in &mut proposed.items {
        item.edited = false;
        item.state = ItemState::Proposed;
        item.origin = Some("agent-editorial".into());
    }
    proposed.deleted = false;
    proposed.deleted_item_ids.clear();
    proposed.locked = false;
    if let Some(old) = project.layer(&proposed.layer_id) {
        if old.asset_id != proposed.asset_id || old.kind != proposed.kind || old.deleted || old.locked {
            return Err(invalid("capa destino protegida, borrada o de otro tipo/medio"));
        }
        let protected: Vec<_> = old
            .items
            .iter()
            .filter(|i| i.edited || i.state != ItemState::Proposed || !matches!(i.origin.as_deref(), Some("ai" | "agent" | "agent-editorial")))
            .cloned()
            .collect();
        let protected_ids: HashSet<_> = protected.iter().map(|i| i.item_id.clone()).collect();
        let mut deleted: HashSet<_> = old.deleted_item_ids.iter().cloned().collect();
        loop {
            let before = deleted.len();
            for item in &proposed.items {
                if item.parent_id.as_ref().is_some_and(|p| deleted.contains(p)) {
                    deleted.insert(item.item_id.clone());
                }
            }
            if deleted.len() == before {
                break;
            }
        }
        proposed.items.retain(|i| !protected_ids.contains(&i.item_id) && !deleted.contains(&i.item_id));
        proposed.items.extend(protected);
        proposed.deleted_item_ids = old.deleted_item_ids.clone();
        proposed.revision = old.revision;
        proposed.visible = old.visible;
        proposed.lane = old.lane.clone();
        // Existing metadata remains authoritative unless the proposal adds new keys.
        for (key, value) in &old.extra {
            proposed.extra.insert(key.clone(), value.clone());
        }
    }
    let duration = project.asset(&proposed.asset_id).ok_or_else(|| invalid("medio ausente"))?.duration();
    tv2_domain::layers::validate_layer(&proposed, duration).map_err(|e| invalid(e.to_string()))?;
    Ok(proposed)
}

fn union(items: &[Value]) -> V1Result<Vec<(tv2_domain::Ticks, tv2_domain::Ticks)>> {
    let mut ranges = Vec::new();
    for item in items {
        for r in item["ranges"].as_array().ok_or_else(|| invalid("item sin ranges"))? {
            let a = r["t_ini"].as_f64().ok_or_else(|| invalid("t_ini inválido"))?;
            let b = r["t_fin"].as_f64().ok_or_else(|| invalid("t_fin inválido"))?;
            ranges.push((crate::secs_to_ticks(a), crate::secs_to_ticks(b)));
        }
    }
    ranges.sort();
    let mut result: Vec<(tv2_domain::Ticks, tv2_domain::Ticks)> = Vec::new();
    for (a, b) in ranges {
        if let Some(last) = result.last_mut()
            && a <= last.1
        {
            last.1 = last.1.max(b);
        } else {
            result.push((a, b));
        }
    }
    Ok(result)
}

pub fn prepare_proposal(project: &Project, asset: &AssetId, request: &Value, proposal: &Value, previous: Option<&Value>) -> V1Result<PreparedReview> {
    let kind = validate_base(project, asset, request, proposal)?;
    if matches!(kind, ReviewKind::Trims | ReviewKind::Montage) {
        return crate::review_media::prepare(project, asset, kind, request, proposal);
    }
    let source = project.asset(asset).ok_or_else(|| invalid("medio inexistente"))?;
    let pass = request["pass_required"].as_u64().filter(|p| *p > 0).ok_or_else(|| invalid("pass_required inválida"))?;
    let digest = digest_json(proposal);
    let request_id = required(request, "request_id")?.to_string();
    let mut documents = BTreeMap::new();
    let mut layers = if kind == ReviewKind::Layers {
        proposal_layers(proposal)?
    } else {
        if !matches!(pass, 1 | 2) || proposal["pass"].as_u64() != Some(pass) || proposal["complete"] != true {
            return Err(invalid("pasada de temas incompleta o fuera de orden"));
        }
        let mut layer = json!({"schema":"editorial-layer/1","layer_id":required(request,"layer_id")?,"kind":"topics","name":"Temas y subtemas","color":"#9a70bc","revision":0,"media_fingerprint":source.fingerprint,"source_master_digest":request["source_master_digest"],"items":proposal["items"]});
        let validated = crate::layers::layer_from_v1(&layer, asset, Some(&source.fingerprint), source.duration())?;
        let a = request["scope"]["t_ini"].as_f64().ok_or_else(|| invalid("pedido sin scope"))?;
        let b = request["scope"]["t_fin"].as_f64().ok_or_else(|| invalid("pedido sin scope"))?;
        if !a.is_finite() || !b.is_finite() || a < 0.0 || b <= a || b > source.duration().as_seconds_f64() {
            return Err(invalid("scope inválido"));
        }
        if validated.items.iter().flat_map(|i| &i.ranges).any(|r| r.start < crate::secs_to_ticks(a) || r.end > crate::secs_to_ticks(b)) {
            return Err(invalid("tema fuera del ámbito solicitado"));
        }
        if pass == 1 {
            let evidence = master(project, asset)?;
            for item in layer["items"].as_array_mut().unwrap() {
                for part in item["ranges"].as_array_mut().unwrap() {
                    let (range, diagnostics) =
                        crate::review_media::snap(&evidence, part["t_ini"].as_f64().unwrap(), part["t_fin"].as_f64().unwrap())?;
                    if range.start < crate::secs_to_ticks(a) || range.end > crate::secs_to_ticks(b) {
                        return Err(invalid("ajuste seguro del tema excede ámbito; revisa propuesta"));
                    }
                    part["t_ini"] = crate::secs_json(range.start);
                    part["t_fin"] = crate::secs_json(range.end);
                    part["boundary_diagnostics"] = diagnostics;
                }
            }
            crate::layers::layer_from_v1(&layer, asset, Some(&source.fingerprint), source.duration())?;
        }
        if pass == 2 {
            let previous = previous.ok_or_else(|| invalid("segunda pasada requiere mapa completo anterior"))?;
            if previous["pass"] != 1 || previous["request_id"] != request["request_id"] || previous["complete"] != true {
                return Err(invalid("primera pasada inválida"));
            }
            let previous_digest = digest_json(previous);
            if proposal["previous_pass_digest"] != previous_digest || request["previous_pass_digest"] != previous_digest {
                return Err(invalid("digest de primera pasada obsoleto"));
            }
            let known: BTreeMap<_, _> = previous["items"]
                .as_array()
                .ok_or_else(|| invalid("primera pasada sin items"))?
                .iter()
                .map(|i| Ok((required(i, "item_id")?.to_owned(), i.clone())))
                .collect::<V1Result<_>>()?;
            let mut used = HashSet::new();
            for item in proposal["items"].as_array().unwrap() {
                let ids = item["source_item_ids"].as_array().filter(|a| !a.is_empty()).ok_or_else(|| invalid("source_item_ids vacíos"))?;
                let mut sources = Vec::new();
                for id in ids {
                    let id = id.as_str().ok_or_else(|| invalid("source_item_id inválido"))?;
                    if !used.insert(id.to_owned()) {
                        return Err(invalid("primera pasada duplicada en recurrencias"));
                    }
                    sources.push(known.get(id).ok_or_else(|| invalid("source_item_id desconocido"))?.clone());
                }
                if union(std::slice::from_ref(item))? != union(&sources)? {
                    return Err(invalid("segunda pasada altera rangos del mapa validado"));
                }
            }
            if used.len() != known.len() {
                return Err(invalid("segunda pasada omite elementos del mapa"));
            }
            layer["analysis"] = json!({"request_id":request_id,"pass1_digest":previous_digest,"pass2_digest":digest,"scope":request["scope"]});
        }
        let mut validated_proposal = proposal.clone();
        validated_proposal["items"] = layer["items"].clone();
        validated_proposal["source_proposal_digest"] = json!(digest);
        documents.insert(format!("views/topics-pass{pass}.json"), serde_json::to_string_pretty(&validated_proposal)?);
        if pass == 1 {
            let mut next = request.clone();
            next["pass_required"] = json!(2);
            next["previous_pass_digest"] = json!(digest_json(&validated_proposal));
            documents.insert("views/topics-request.json".into(), serde_json::to_string_pretty(&next)?);
            documents.insert("views/topics-agent-request.md".into(), request_markdown(&next, kind));
            return Ok(PreparedReview { command: None, documents, proposal_digest: digest, request_id, pass, warnings: vec![] });
        }
        vec![layer]
    };
    let mut commands = Vec::new();
    let mut ids = HashSet::new();
    for layer in &mut layers {
        let mut normalized = crate::layers::layer_from_v1(layer, asset, Some(&source.fingerprint), source.duration())?;
        if !ids.insert(normalized.layer_id.clone()) {
            return Err(invalid("layer_id repetido en propuesta"));
        }
        if kind == ReviewKind::Layers && !matches!(normalized.kind, LayerKind::User | LayerKind::Topics | LayerKind::Ai) {
            return Err(invalid("propuesta genérica solo puede escribir capas user, topics o ai"));
        }
        if normalized.extra.contains_key("master_projection") || normalized.extra.contains_key("tv2_v1_source_path") {
            return Err(invalid("La propuesta no puede atribuir evidencia del master ni localizadores internos a una capa"));
        }
        normalized.source_master_digest = Some(required(request, "source_master_digest")?.into());
        normalized = merge_layer(project, normalized)?;
        commands.push(Command::ReplaceLayer { layer: normalized });
    }
    documents.insert(format!("views/{}.proposed.json", kind.stem()), serde_json::to_string_pretty(proposal)?);
    documents.insert(format!(".work/reviews/{request_id}/{digest}.json"),serde_json::to_string_pretty(&json!({"schema":"transcriptor-editorial-review/1","request":request,"proposal":proposal,"proposal_digest":digest,"base_revision":project.revision,"state":"prepared"}))?);
    for path in documents.keys() {
        crate::folder::validate_path(path)?;
    }
    Ok(PreparedReview {
        command: Some(Command::Batch { label: format!("Propuesta editorial {request_id}"), commands }),
        documents,
        proposal_digest: digest,
        request_id,
        pass,
        warnings: vec![],
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputDigest {
    pub size: u64,
    pub sha256: String,
}

/// Manifest verification uses content bytes; paths, schema and result digest
/// are checked before any caller may advertise reuse. No pipeline is executed.
pub fn validate_manifest(manifest: &Value, outputs: &BTreeMap<String, OutputDigest>, expected_key: Option<&str>) -> V1Result<()> {
    if manifest["schema"] != "editorial-step/1" {
        return Err(invalid("schema de manifest desconocido"));
    }
    required(manifest, "step")?;
    let key = required(manifest, "key")?;
    if expected_key.is_some_and(|expected| expected != key) {
        return Err(invalid("clave de manifest obsoleta"));
    }
    if required(manifest, "result_digest")? != digest_object_without(manifest, &["result_digest"]) {
        return Err(invalid("digest de manifest no coincide"));
    }
    for (path, value) in manifest["outputs"].as_object().filter(|m| !m.is_empty()).ok_or_else(|| invalid("manifest sin outputs"))? {
        crate::folder::validate_path(path)?;
        let expected: OutputDigest = serde_json::from_value(value.clone())?;
        if outputs.get(path) != Some(&expected) {
            return Err(invalid(format!("salida de manifest modificada o ausente: {path}")));
        }
    }
    Ok(())
}

pub fn document_digests(documents: &BTreeMap<String, String>) -> BTreeMap<String, OutputDigest> {
    documents
        .iter()
        .map(|(path, text)| (path.clone(), OutputDigest { size: text.len() as u64, sha256: hex::encode(Sha256::digest(text.as_bytes())) }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::Ticks;
    fn project() -> (Project, AssetId) {
        let mut p = Project::new("review");
        let asset = tv2_domain::commands::tests_support::fake_video("source", 12);
        let mut raw = crate::master::fixtures::master(12.0);
        raw["media"]["fingerprint"] = json!(asset.fingerprint);
        p.masters.push(V1Master::parse(raw).unwrap().evidence(&asset.id));
        let id = asset.id.clone();
        p.assets.push(asset);
        (p, id)
    }
    fn item(id: &str, a: f64, b: f64) -> Value {
        json!({"item_id":id,"state":"proposed","ranges":[{"t_ini":a,"t_fin":b}],"label":"tema"})
    }
    fn proposal(request: &Value, kind: ReviewKind) -> Value {
        json!({"schema":format!("editorial-{}-proposal/1",kind.stem()),"request_id":request["request_id"],"source_master_digest":request["source_master_digest"],"source_layers_digest":request["source_layers_digest"]})
    }
    #[test]
    fn two_pass_topics_persist_validated_map_and_reject_omission_and_stale_base() {
        let (p, id) = project();
        let request = prepare_request(&p, &id, "cycle-one", ReviewKind::Topics, None).unwrap();
        let mut first = proposal(&request.request, ReviewKind::Topics);
        first["pass"] = json!(1);
        first["complete"] = json!(true);
        first["items"] = json!([item("topic-one", 0.0, 3.0), item("topic-two", 7.0, 10.0)]);
        let prepared = prepare_proposal(&p, &id, &request.request, &first, None).unwrap();
        assert!(prepared.command.is_none());
        let next: Value = serde_json::from_str(&prepared.documents["views/topics-request.json"]).unwrap();
        let previous: Value = serde_json::from_str(&prepared.documents["views/topics-pass1.json"]).unwrap();
        assert_eq!(next["previous_pass_digest"], digest_json(&previous));
        let mut second = proposal(&next, ReviewKind::Topics);
        second["pass"] = json!(2);
        second["complete"] = json!(true);
        second["previous_pass_digest"] = next["previous_pass_digest"].clone();
        second["items"] = json!([{"item_id":"topic-unified","label":"recurrente","state":"proposed","source_item_ids":["topic-one","topic-two"],"ranges":[previous["items"][0]["ranges"][0],previous["items"][1]["ranges"][0]]}]);
        let result = prepare_proposal(&p, &id, &next, &second, Some(&previous)).unwrap();
        let mut applied = p.clone();
        result.command.unwrap().apply(&mut applied).unwrap();
        assert_eq!(applied.layers.last().unwrap().items[0].ranges.len(), 2);
        second["items"][0]["source_item_ids"] = json!(["topic-one"]);
        assert!(prepare_proposal(&p, &id, &next, &second, Some(&previous)).is_err());
        let mut stale = p;
        stale.revision += 1;
        assert!(prepare_proposal(&stale, &id, &request.request, &first, None).is_err());
    }
    #[test]
    fn ai_layers_preserve_human_items_and_reject_cross_cycle() {
        let (mut p, id) = project();
        let fp = &p.assets[0].fingerprint;
        let mut old = json!({"schema":"editorial-layer/1","layer_id":"ai-layer","kind":"ai","items":[item("protected-item",0.0,3.0)],"deleted_item_ids":["deleted-item"]});
        old["items"][0]["edited"] = json!(true);
        old["items"][0]["state"] = json!("accepted");
        p.layers.push(crate::layers::layer_from_v1(&old, &id, Some(fp), Ticks::from_seconds(12)).unwrap());
        let request = prepare_request(&p, &id, "cycle-ai", ReviewKind::Layers, None).unwrap();
        let mut reply = proposal(&request.request, ReviewKind::Layers);
        let mut changed = old.clone();
        changed["items"] = json!([item("protected-item", 4.0, 5.0), item("deleted-item", 7.0, 10.0)]);
        reply["layer"] = changed;
        let prepared = prepare_proposal(&p, &id, &request.request, &reply, None).unwrap();
        prepared.command.unwrap().apply(&mut p).unwrap();
        let retained = &p.layers[0];
        assert_eq!(retained.items.len(), 1);
        assert_eq!(retained.items[0].state, ItemState::Accepted);
        assert_eq!(retained.items[0].start(), Ticks::ZERO);
        reply["request_id"] = json!("old-cycle");
        assert!(prepare_proposal(&p, &id, &request.request, &reply, None).is_err());
    }
    #[test]
    fn legacy_layers_require_explicit_binding_and_preserve_original_digests() {
        let (p, id) = project();
        let request = prepare_request(&p, &id, "legacy-binding", ReviewKind::Layers, None).unwrap().request;
        let legacy = json!({"schema":"editorial-layers-proposal/1","source_master_digest":request["source_master_digest"],"source_layers_digest":request["source_layers_digest"],"layer":{"schema":"editorial-layer/1","layer_id":"legacy-ai","kind":"ai","items":[item("legacy-item",0.0,3.0)]}});
        assert!(prepare_proposal(&p, &id, &request, &legacy, None).is_err());
        let bound = adopt_legacy_layers_proposal(&p, &id, &request, &legacy).unwrap();
        assert!(legacy.get("request_id").is_none());
        assert_eq!(bound["request_id"], request["request_id"]);
        for key in ["source_master_digest", "source_layers_digest", "layer"] {
            assert_eq!(bound[key], legacy[key]);
        }
        assert_eq!(bound["tv2_adoption"]["original_proposal_digest"], digest_json(&legacy));
        assert!(prepare_proposal(&p, &id, &request, &bound, None).unwrap().command.is_some());
        assert!(adopt_legacy_layers_proposal(&p, &id, &request, &bound).is_err());
        let mut stale = p.clone();
        stale.revision += 1;
        assert!(adopt_legacy_layers_proposal(&stale, &id, &request, &legacy).is_err());
        let mut mismatched = legacy.clone();
        mismatched["source_layers_digest"] = json!("different");
        assert!(adopt_legacy_layers_proposal(&p, &id, &request, &mismatched).is_err());
        mismatched = legacy;
        mismatched["request_id"] = json!("other-request");
        assert!(adopt_legacy_layers_proposal(&p, &id, &request, &mismatched).is_err());
    }
    #[test]
    fn generic_v1_layers_accept_user_topics_ai_without_granting_human_or_evidence_authorship() {
        let (p, id) = project();
        let request = prepare_request(&p, &id, "generic-kinds", ReviewKind::Layers, None).unwrap().request;
        let mut reply = proposal(&request, ReviewKind::Layers);
        let layers=["user","topics","ai"].iter().map(|kind|json!({"schema":"editorial-layer/1","layer_id":format!("generic-{kind}"),"kind":kind,"items":[{"item_id":format!("item-{kind}"),"label":"proposal","state":"accepted","edited":true,"ranges":[{"t_ini":0.0,"t_fin":3.0}]}]})).collect::<Vec<_>>();
        reply["layers"] = json!(layers);
        let result = prepare_proposal(&p, &id, &request, &reply, None).unwrap();
        let mut candidate = p.clone();
        result.command.unwrap().apply(&mut candidate).unwrap();
        assert!(
            candidate.layers.iter().filter(|layer| layer.layer_id.as_str().starts_with("generic-")).flat_map(|layer| &layer.items).all(|item| item
                .state
                == ItemState::Proposed
                && !item.edited
                && item.origin.as_deref() == Some("agent-editorial"))
        );
        for forbidden in ["bloques", "recortes", "autor", "transcript", "signals"] {
            reply["layers"][0]["kind"] = json!(forbidden);
            assert!(prepare_proposal(&p, &id, &request, &reply, None).is_err());
        }
        reply["layers"][0]["kind"] = json!("user");
        reply["layers"][0]["master_projection"] = json!({"collection":"words"});
        assert!(prepare_proposal(&p, &id, &request, &reply, None).is_err());
    }
    #[test]
    fn manifest_requires_unchanged_bytes_and_key() {
        let documents = BTreeMap::from([("tracks/evidence.json".into(), "{\"original\":true}\r\n".into())]);
        let outputs = document_digests(&documents);
        let mut manifest = json!({"schema":"editorial-step/1","step":"fixture","key":"content-key","outputs":outputs});
        manifest["result_digest"] = json!(digest_json(&manifest));
        assert!(validate_manifest(&manifest, &outputs, Some("content-key")).is_ok());
        assert!(validate_manifest(&manifest, &outputs, Some("older-key")).is_err());
        let changed = document_digests(&BTreeMap::from([("tracks/evidence.json".into(), "{\"original\":true}\n".into())]));
        assert!(validate_manifest(&manifest, &changed, None).is_err());
    }
    #[test]
    fn trims_and_montage_use_commands_and_reject_malformed_or_repeated_sources() {
        let (p, id) = project();
        let request = prepare_request(&p, &id, "cycle-trims", ReviewKind::Trims, None).unwrap();
        let mut response = proposal(&request.request, ReviewKind::Trims);
        response["cuts"] = json!([{"t_ini":10.0,"t_fin":11.0,"reason":"synthetic"}]);
        let prepared = prepare_proposal(&p, &id, &request.request, &response, None).unwrap();
        let mut applied = p.clone();
        prepared.command.unwrap().apply(&mut applied).unwrap();
        assert!(applied.layers.iter().any(|l| l.lane.as_deref() == Some("ai") && !l.items.is_empty()));
        response["cuts"] = json!([true]);
        assert!(prepare_proposal(&p, &id, &request.request, &response, None).is_err());
        let request = prepare_request(&p, &id, "cycle-montage", ReviewKind::Montage, None).unwrap();
        let mut response = proposal(&request.request, ReviewKind::Montage);
        response["pass"] = json!(1);
        response["montage_digest"] = Value::Null;
        response["clips"] = json!([{"source_ini":10.0,"source_fin":11.0,"reason":"synthetic"}]);
        let prepared = prepare_proposal(&p, &id, &request.request, &response, None).unwrap();
        let mut applied = p.clone();
        prepared.command.unwrap().apply(&mut applied).unwrap();
        assert_eq!(applied.sequences.len(), p.sequences.len() + 1);
        response["clips"].as_array_mut().unwrap().push(json!({"source_ini":10.0,"source_fin":11.0}));
        assert!(prepare_proposal(&p, &id, &request.request, &response, None).is_err());
    }
}
