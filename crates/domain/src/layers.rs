//! Capas semánticas: transcripción, hablantes, bloques, temas, risas, recortes,
//! propuestas, pedidos del autor… Contienen items con uno o varios rangos en
//! tiempo **fuente** del asset al que pertenecen. No renderizan por defecto.
//!
//! Reglas conservadas de V1 (`editorial_layers.py`):
//! - estados `proposed` / `accepted` / `disabled`; no existe estado `deleted`
//!   por item: los borrados van a `deleted_item_ids` (tombstones);
//! - rangos ordenados y no solapados dentro de un item; puntos solo si la capa lo admite;
//! - jerarquía por `parent_id`, acíclica, máximo 32 niveles, hijo dentro de los rangos del padre;
//! - `edited` marca revisión humana; `accepted` es marca de revisión: un
//!   `proposed` habilitado también se aplica (recortes).

use crate::error::{DomainError, DomainResult};
use crate::ids::{AssetId, ItemId, LayerId, is_valid_v1_id};
use crate::time::{Ticks, TimeRange};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemState {
    #[default]
    Proposed,
    Accepted,
    Disabled,
}

impl ItemState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemState::Proposed => "proposed",
            ItemState::Accepted => "accepted",
            ItemState::Disabled => "disabled",
        }
    }
    /// Un item «habilitado» se aplica (p. ej. un recorte corta) aunque no esté aceptado.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, ItemState::Disabled)
    }
}

/// Tipo de capa. Determina qué acciones admite (los bloques no se aceptan) y
/// cómo se presenta.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    /// Capa manual del usuario (V1 `user`).
    User,
    /// Temas/subtemas jerárquicos (V1 `topics`).
    Topics,
    /// Propuesta de AI (V1 `ai`).
    Ai,
    /// Marcas del autor (V1 adaptador `autor`).
    Author,
    /// Bloques contiguos (V1 `bloques`): no admiten aceptación.
    Blocks,
    /// Recortes (V1 `recortes`, un carril por `lane`).
    Trims,
    /// Transcripción / palabras / hablantes / señales (solo lectura desde análisis).
    Transcript,
    Speakers,
    Laughter,
    Arousal,
    Silence,
    Signals,
    Other(String),
}

impl LayerKind {
    pub fn fresh_item_id(&self) -> ItemId {
        let hex = crate::ids::random_hex12();
        match self {
            LayerKind::Trims => ItemId::new(format!("cut-{:06}", u64::from_str_radix(&hex, 16).unwrap())),
            LayerKind::Author => ItemId::new(format!("m{:04}", u64::from_str_radix(&hex, 16).unwrap())),
            _ => ItemId::new(format!("item-{hex}")),
        }
    }
    pub fn accepts_acceptance(&self) -> bool {
        self.is_editable() && !matches!(self, LayerKind::Blocks)
    }
    pub fn allows_points(&self) -> bool {
        // V1 persiste capas con `allow_points=False`; solo las marcas del autor (adaptador) llevan puntos.
        matches!(self, LayerKind::Author)
    }
    pub fn is_editable(&self) -> bool {
        !matches!(self, LayerKind::Transcript | LayerKind::Speakers | LayerKind::Laughter | LayerKind::Arousal | LayerKind::Signals)
    }
    pub fn v1_name(&self) -> String {
        match self {
            LayerKind::User => "user".into(),
            LayerKind::Topics => "topics".into(),
            LayerKind::Ai => "ai".into(),
            LayerKind::Author => "autor".into(),
            LayerKind::Blocks => "bloques".into(),
            LayerKind::Trims => "recortes".into(),
            LayerKind::Transcript => "transcript".into(),
            LayerKind::Speakers => "speakers".into(),
            LayerKind::Laughter => "laughter".into(),
            LayerKind::Arousal => "arousal".into(),
            LayerKind::Silence => "silence".into(),
            LayerKind::Signals => "signals".into(),
            LayerKind::Other(s) => s.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SemanticItem {
    pub item_id: ItemId,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub state: ItemState,
    #[serde(default)]
    pub edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<ItemId>,
    /// Rangos en tiempo fuente del asset de la capa, ordenados.
    pub ranges: Vec<TimeRange>,
    /// `user`, `ai`, `silence`, `import-v1`…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Evidencia (IDs de palabras/utterances, señales…) y campos V1 desconocidos.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SemanticItem {
    pub fn new(range: TimeRange, label: impl Into<String>) -> Self {
        SemanticItem {
            item_id: ItemId::random(),
            label: label.into(),
            comment: String::new(),
            state: ItemState::Proposed,
            edited: true,
            parent_id: None,
            ranges: vec![range],
            origin: Some("user".into()),
            extra: Default::default(),
        }
    }

    pub fn start(&self) -> Ticks {
        self.ranges.iter().map(|r| r.start).min().unwrap_or(Ticks::ZERO)
    }

    /// Trims keep human acceptance independently of their enabled state.
    pub fn is_human_accepted(&self) -> bool {
        self.state == ItemState::Accepted || self.extra.get("accepted").and_then(|v| v.as_bool()) == Some(true)
    }

    pub fn end(&self) -> Ticks {
        self.ranges.iter().map(|r| r.end).max().unwrap_or(Ticks::ZERO)
    }

    pub fn total_duration(&self) -> Ticks {
        self.ranges.iter().fold(Ticks::ZERO, |acc, r| acc + r.duration())
    }

    pub fn is_point(&self) -> bool {
        self.ranges.len() == 1 && self.ranges[0].is_point()
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SemanticLayer {
    pub layer_id: LayerId,
    pub kind: LayerKind,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    /// Asset cuya línea de tiempo fuente usan los rangos.
    pub asset_id: AssetId,
    pub items: Vec<SemanticItem>,
    /// Tombstones: items borrados que un merge no debe resucitar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_item_ids: Vec<ItemId>,
    /// Revisión propia de la capa (V1 `revision`).
    #[serde(default)]
    pub revision: u64,
    /// Capa borrada (tombstone de capa, V1 `deleted: true`).
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    /// Carril V1 para recortes (`main`, `ai-deep`…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_master_digest: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_color() -> String {
    "#d09947".to_string()
}

impl SemanticLayer {
    pub fn new(asset_id: AssetId, kind: LayerKind, name: impl Into<String>) -> Self {
        SemanticLayer {
            layer_id: LayerId::random(),
            kind,
            name: name.into(),
            color: default_color(),
            asset_id,
            items: Vec::new(),
            deleted_item_ids: Vec::new(),
            revision: 0,
            deleted: false,
            visible: true,
            locked: false,
            lane: None,
            source_master_digest: None,
            extra: Default::default(),
        }
    }

    pub fn item(&self, id: &ItemId) -> Option<&SemanticItem> {
        self.items.iter().find(|i| &i.item_id == id)
    }

    pub fn item_mut(&mut self, id: &ItemId) -> Option<&mut SemanticItem> {
        self.items.iter_mut().find(|i| &i.item_id == id)
    }

    pub fn depth_of(&self, id: &ItemId) -> usize {
        let by_id: HashMap<&ItemId, &SemanticItem> = self.items.iter().map(|i| (&i.item_id, i)).collect();
        let mut depth = 0;
        let mut seen = HashSet::new();
        let mut cur = by_id.get(id).and_then(|i| i.parent_id.as_ref());
        while let Some(p) = cur {
            if !seen.insert(p) || !by_id.contains_key(p) {
                break;
            }
            depth += 1;
            cur = by_id.get(p).and_then(|i| i.parent_id.as_ref());
        }
        depth
    }

    /// Items habilitados (no `disabled`), ordenados por inicio.
    pub fn enabled_items(&self) -> Vec<&SemanticItem> {
        let mut v: Vec<&SemanticItem> = self.items.iter().filter(|i| i.state.is_enabled()).collect();
        v.sort_by_key(|i| i.start());
        v
    }

    /// Intervalos fuente habilitados, unidos (como `editorial_trims.enabled_intervals`).
    pub fn enabled_intervals(&self) -> Vec<TimeRange> {
        let mut ranges: Vec<TimeRange> = self.items.iter().filter(|i| i.state.is_enabled()).flat_map(|i| i.ranges.iter().copied()).collect();
        merge_intervals(&mut ranges)
    }
}

/// Une intervalos que se tocan o solapan.
pub fn merge_intervals(ranges: &mut Vec<TimeRange>) -> Vec<TimeRange> {
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<TimeRange> = Vec::new();
    for r in ranges.drain(..) {
        if let Some(last) = out.last_mut()
            && r.start <= last.end
        {
            last.end = last.end.max(r.end);
            continue;
        }
        out.push(r);
    }
    out
}

pub const MAX_HIERARCHY_DEPTH: usize = 32;

/// Validación de items equivalente a `editorial_layers.validate_items`.
pub fn validate_items(items: &[SemanticItem], duration: Ticks, allow_points: bool) -> DomainResult<()> {
    let mut seen: HashSet<&ItemId> = HashSet::new();
    for item in items {
        if !is_valid_v1_id(item.item_id.as_str()) || !seen.insert(&item.item_id) {
            return Err(DomainError::invalid(format!("item_id inválido o duplicado: {}", item.item_id)));
        }
        if item.ranges.is_empty() {
            return Err(DomainError::invalid(format!("el item {} necesita rangos", item.item_id)));
        }
        let mut previous = Ticks(-1);
        for r in &item.ranges {
            let bad = r.start < Ticks::ZERO || r.end > duration || r.end < r.start || (r.end == r.start && !allow_points) || r.start < previous;
            if bad {
                return Err(DomainError::out_of_range(format!(
                    "item {}: rango fuera del medio, vacío o solapado ({} – {})",
                    item.item_id, r.start, r.end
                )));
            }
            previous = r.end;
        }
    }
    let by_id: HashMap<&ItemId, &SemanticItem> = items.iter().map(|i| (&i.item_id, i)).collect();
    for item in items {
        let mut visited: HashSet<&ItemId> = HashSet::from([&item.item_id]);
        let mut parent = item.parent_id.as_ref();
        while let Some(p) = parent {
            if !by_id.contains_key(p) || visited.contains(p) {
                return Err(DomainError::invalid(format!("jerarquía desconocida o cíclica en {}", item.item_id)));
            }
            visited.insert(p);
            if visited.len() > MAX_HIERARCHY_DEPTH {
                return Err(DomainError::invalid("jerarquía demasiado profunda (máximo 32 niveles)"));
            }
            parent = by_id[p].parent_id.as_ref();
        }
        if let Some(p) = &item.parent_id {
            let parent = by_id[p];
            for r in &item.ranges {
                if !parent.ranges.iter().any(|pr| pr.start <= r.start && r.end <= pr.end) {
                    return Err(DomainError::out_of_range(format!("subtema {} fuera de los rangos de su tema {}", item.item_id, p)));
                }
            }
        }
    }
    Ok(())
}

pub fn validate_layer(layer: &SemanticLayer, duration: Ticks) -> DomainResult<()> {
    if !is_valid_v1_id(layer.layer_id.as_str()) {
        return Err(DomainError::invalid(format!("layer_id inválido: {}", layer.layer_id)));
    }
    if layer.name.trim().is_empty() {
        return Err(DomainError::invalid("falta el nombre de la capa"));
    }
    if !is_hex_color(&layer.color) {
        return Err(DomainError::invalid(format!("color inválido (#RRGGBB): {}", layer.color)));
    }
    validate_items(&layer.items, duration, layer.kind.allows_points())?;
    if layer.kind == LayerKind::Author
        && layer
            .items
            .iter()
            .any(|i| i.ranges.len() != 1 || i.parent_id.is_some() || (i.state != ItemState::Proposed && i.total_duration() < Ticks::from_millis(200)))
    {
        return Err(DomainError::invalid("una marca exige un único punto o región; una decisión necesita al menos 200 ms"));
    }
    if layer.kind == LayerKind::Trims
        && layer.items.iter().any(|i| i.ranges.len() != 1 || i.parent_id.is_some() || i.total_duration() < Ticks::from_millis(50))
    {
        return Err(DomainError::invalid("un recorte exige un rango de al menos 50 ms y no admite jerarquía"));
    }
    if layer.kind == LayerKind::Blocks && layer.extra.contains_key("v1_chunks_header") && !layer.deleted {
        let mut items: Vec<_> = layer.items.iter().collect();
        items.sort_by_key(|i| i.start());
        let mut previous = Ticks::ZERO;
        for item in items {
            if item.ranges.len() != 1
                || item.parent_id.is_some()
                || item.start() != previous
                || item.total_duration() > Ticks::from_seconds(3000)
                || item.state != ItemState::Proposed
            {
                return Err(DomainError::invalid("bloques: partición contigua desde cero, sin jerarquía ni aceptación, máximo 50 minutos"));
            }
            previous = item.end();
        }
        if previous != duration {
            return Err(DomainError::invalid("los bloques deben cubrir el medio completo"));
        }
    }
    let ids: HashSet<&ItemId> = layer.items.iter().map(|i| &i.item_id).collect();
    if let Some(dead) = layer.deleted_item_ids.iter().find(|d| ids.contains(d)) {
        return Err(DomainError::invalid(format!("el item {dead} está a la vez vivo y en deleted_item_ids")));
    }
    Ok(())
}

pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, a: i64, b: i64) -> SemanticItem {
        let mut i = SemanticItem::new(TimeRange::new(Ticks::from_seconds(a), Ticks::from_seconds(b)), id);
        i.item_id = ItemId::new(id);
        i
    }

    #[test]
    fn hierarchy_and_multirange_rules_match_v1() {
        let mut topic = item("topic", 0, 3);
        topic.ranges.push(TimeRange::new(Ticks::from_seconds(7), Ticks::from_seconds(12)));
        let mut sub = item("sub", 8, 9);
        sub.parent_id = Some(topic.item_id.clone());
        let dur = Ticks::from_seconds(12);
        validate_items(&[topic.clone(), sub.clone()], dur, false).unwrap();
        sub.ranges[0] = TimeRange::new(Ticks::from_seconds(4), Ticks::from_seconds(5));
        let err = validate_items(&[topic.clone(), sub.clone()], dur, false).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::OutOfRange);
        sub.ranges[0] = TimeRange::new(Ticks::from_seconds(8), Ticks::from_seconds(9));
        topic.parent_id = Some(sub.item_id.clone());
        let err = validate_items(&[topic, sub], dur, false).unwrap_err();
        assert!(err.message.contains("cíclica"));
    }

    #[test]
    fn deep_hierarchy_limit_is_32() {
        let mut items = Vec::new();
        for n in 0..34 {
            let mut i = item(&format!("i{n}"), 0, 10);
            if n > 0 {
                i.parent_id = Some(ItemId::new(format!("i{}", n - 1)));
            }
            items.push(i);
        }
        assert!(validate_items(&items, Ticks::from_seconds(10), false).is_err());
        items.truncate(32);
        assert!(validate_items(&items, Ticks::from_seconds(10), false).is_ok());
    }

    #[test]
    fn points_only_when_allowed_and_ranges_ordered() {
        let p = item("p", 3, 3);
        assert!(validate_items(std::slice::from_ref(&p), Ticks::from_seconds(10), false).is_err());
        assert!(validate_items(&[p], Ticks::from_seconds(10), true).is_ok());
        let mut m = item("m", 5, 6);
        m.ranges.push(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(2)));
        assert!(validate_items(&[m], Ticks::from_seconds(10), false).is_err());
        assert!(validate_items(&[item("x", 0, 11)], Ticks::from_seconds(10), false).is_err());
        assert!(validate_items(&[item("a", 0, 1), item("a", 2, 3)], Ticks::from_seconds(10), false).is_err());
    }

    #[test]
    fn enabled_intervals_merge_regardless_of_acceptance() {
        let mut layer = SemanticLayer::new(AssetId::new("a"), LayerKind::Trims, "Recortes");
        let mut a = item("a", 1, 3);
        a.state = ItemState::Accepted;
        let b = item("b", 2, 4); // proposed → también corta
        let mut c = item("c", 6, 7);
        c.state = ItemState::Disabled;
        layer.items = vec![a, b, c];
        assert_eq!(layer.enabled_intervals(), vec![TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(4))]);
        assert!(!LayerKind::Blocks.accepts_acceptance());
        assert!(LayerKind::Trims.accepts_acceptance());
    }
}
