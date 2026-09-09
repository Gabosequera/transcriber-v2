//! Sesión de proyecto: única puerta de mutación.
//!
//! - Cada comando persistente llega en un [`CommandEnvelope`]. Si trae
//!   `base_revision` y no coincide con la vigente → `E_STALE_REVISION`, sin aplicar.
//! - Si trae `idempotency_key` ya vista → se devuelve el resultado anterior sin
//!   volver a aplicar.
//! - Se aplica sobre una copia; si falla, el proyecto no cambia (batch todo-o-nada).
//! - Al confirmar, la revisión sube en 1 y el historial guarda snapshots
//!   `before`/`after` con la revisión esperada (como `editorial_history.HistoryStack`).
//! - Undo/redo restauran contenido como **nueva revisión**; si la revisión no es
//!   la esperada (edición externa reconciliada entre medio) la entrada se descarta
//!   con `E_STALE_REVISION`.
//! - `dry_run` aplica sobre una copia y devuelve el diff sin confirmar.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use tv2_domain::error::{DomainError, DomainResult, ErrorCode};
use tv2_domain::project::{Project, Revision, now_iso};
use tv2_domain::{Command, CommandEffect};

pub const PROTOCOL_VERSION: &str = "tv2-command/1";
pub const HISTORY_DEPTH: usize = 200;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum Actor {
    #[default]
    Human,
    Agent {
        name: String,
    },
    External {
        source: String,
    },
    System,
}

impl std::fmt::Display for Actor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Actor::Human => write!(f, "persona"),
            Actor::Agent { name } => write!(f, "agente:{name}"),
            Actor::External { source } => write!(f, "externo:{source}"),
            Actor::System => write!(f, "sistema"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CommandEnvelope {
    #[serde(default = "new_command_id")]
    pub command_id: String,
    #[serde(default = "protocol")]
    pub protocol: String,
    #[serde(default)]
    pub actor: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Revisión sobre la que el emisor construyó el comando. `None` = sin precondición
    /// (solo aceptable para la GUI local, que siempre ve la revisión vigente).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<Revision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precondition_digest: Option<String>,
    pub command: Command,
}

fn new_command_id() -> String {
    format!("cmd-{}", tv2_domain::ids::random_hex12())
}

fn protocol() -> String {
    PROTOCOL_VERSION.to_string()
}

impl CommandEnvelope {
    pub fn human(command: Command) -> Self {
        CommandEnvelope {
            command_id: new_command_id(),
            protocol: protocol(),
            actor: Actor::Human,
            project_id: None,
            base_revision: None,
            idempotency_key: None,
            precondition_digest: None,
            command,
        }
    }

    pub fn with_base(mut self, revision: Revision) -> Self {
        self.base_revision = Some(revision);
        self
    }

    pub fn with_actor(mut self, actor: Actor) -> Self {
        self.actor = actor;
        self
    }

    pub fn with_idempotency(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub command_id: String,
    pub base_revision: Revision,
    pub new_revision: Revision,
    pub effect: CommandEffect,
    /// Resumen del diff (conteos por tipo de objeto) para auditoría y UI.
    pub diff: DiffSummary,
    /// `true` si se devolvió el resultado anterior de la misma `idempotency_key`.
    #[serde(default)]
    pub replayed: bool,
    pub applied_at: String,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct DryRunResult {
    pub base_revision: Revision,
    pub effect: CommandEffect,
    pub diff: DiffSummary,
    /// Proyecto resultante (no confirmado).
    pub preview: Project,
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct DiffSummary {
    #[serde(default)]
    pub project_changed: bool,
    #[serde(default)]
    pub sequences_changed: usize,
    pub clips_added: usize,
    pub clips_removed: usize,
    pub clips_changed: usize,
    pub tracks_changed: usize,
    pub assets_changed: usize,
    pub layers_changed: usize,
    pub items_added: usize,
    pub items_removed: usize,
    pub items_changed: usize,
    pub markers_changed: usize,
}

impl DiffSummary {
    /// Editorial differences. Commit-managed revision/updated_at are reported
    /// separately in CommandResult and do not alter a prepared editorial diff.
    pub fn compute(before: &Project, after: &Project) -> DiffSummary {
        let mut d = DiffSummary {
            project_changed: before.schema != after.schema
                || before.project_id != after.project_id
                || before.created_at != after.created_at
                || before.masters != after.masters
                || before.name != after.name
                || before.settings != after.settings
                || before.extra != after.extra
                || before.active_sequence != after.active_sequence
                || before.layer_order != after.layer_order,
            sequences_changed: after.sequences.iter().filter(|s| before.sequence(&s.id).is_none_or(|old| old != *s)).count()
                + before.sequences.iter().filter(|s| after.sequence(&s.id).is_none()).count(),
            layers_changed: before.layers.iter().filter(|l| after.layer(&l.layer_id).is_none()).count(),
            items_removed: before.layers.iter().filter(|l| after.layer(&l.layer_id).is_none()).map(|l| l.items.len()).sum(),
            ..Default::default()
        };
        let b_clips: HashMap<_, _> = before.sequences.iter().flat_map(|s| s.clips.iter()).map(|c| (&c.id, c)).collect();
        let a_clips: HashMap<_, _> = after.sequences.iter().flat_map(|s| s.clips.iter()).map(|c| (&c.id, c)).collect();
        for (id, c) in &a_clips {
            match b_clips.get(id) {
                None => d.clips_added += 1,
                Some(o) if o != c => d.clips_changed += 1,
                _ => {}
            }
        }
        d.clips_removed = b_clips.keys().filter(|k| !a_clips.contains_key(*k)).count();
        let b_tracks: Vec<_> = before.sequences.iter().flat_map(|s| s.tracks.iter()).collect();
        let a_tracks: Vec<_> = after.sequences.iter().flat_map(|s| s.tracks.iter()).collect();
        if b_tracks != a_tracks {
            d.tracks_changed = a_tracks.iter().filter(|t| !b_tracks.contains(t)).count().max(1);
        }
        if before.assets != after.assets {
            d.assets_changed = after.assets.iter().filter(|a| !before.assets.contains(a)).count().max(1);
        }
        for l in &after.layers {
            match before.layer(&l.layer_id) {
                None => {
                    d.layers_changed += 1;
                    d.items_added += l.items.len();
                }
                Some(o) if o != l => {
                    d.layers_changed += 1;
                    if o.items == l.items {
                        continue;
                    }
                    let original: HashMap<_, _> = o.items.iter().map(|item| (&item.item_id, item)).collect();
                    let current: std::collections::HashSet<_> = l.items.iter().map(|item| &item.item_id).collect();
                    for it in &l.items {
                        match original.get(&it.item_id).copied() {
                            None => d.items_added += 1,
                            Some(oi) if oi != it => d.items_changed += 1,
                            _ => {}
                        }
                    }
                    d.items_removed += o.items.iter().filter(|i| !current.contains(&i.item_id)).count();
                }
                _ => {}
            }
        }
        let bm: Vec<_> = before.sequences.iter().flat_map(|s| s.markers.iter()).collect();
        let am: Vec<_> = after.sequences.iter().flat_map(|s| s.markers.iter()).collect();
        if bm != am {
            d.markers_changed = 1;
        }
        d
    }

    pub fn is_empty(&self) -> bool {
        *self == DiffSummary::default()
    }

    pub fn human(&self) -> String {
        let mut parts = Vec::new();
        if self.project_changed {
            parts.push("propiedades del proyecto".into());
        }
        let push = |parts: &mut Vec<String>, n: usize, what: &str| {
            if n > 0 {
                parts.push(format!("{n} {what}"));
            }
        };
        push(&mut parts, self.clips_added, "clips añadidos");
        push(&mut parts, self.clips_removed, "clips eliminados");
        push(&mut parts, self.clips_changed, "clips modificados");
        push(&mut parts, self.tracks_changed, "pistas");
        push(&mut parts, self.assets_changed, "medios");
        push(&mut parts, self.layers_changed, "capas");
        push(&mut parts, self.items_added, "tramos añadidos");
        push(&mut parts, self.items_removed, "tramos borrados");
        push(&mut parts, self.items_changed, "tramos modificados");
        push(&mut parts, self.markers_changed, "marcadores");
        if parts.is_empty() {
            push(&mut parts, self.sequences_changed, "secuencias");
        }
        if parts.is_empty() { "sin cambios".into() } else { parts.join(", ") }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub label: String,
    pub actor: Actor,
    pub command_id: String,
    pub before: Project,
    pub after: Project,
    /// Revisión que debe tener el proyecto para que la entrada siga siendo válida.
    pub expect: Revision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DurableHistory {
    pub schema: String,
    pub project_id: String,
    pub revision: Revision,
    pub undo: VecDeque<HistoryEntry>,
    pub redo: VecDeque<HistoryEntry>,
}

impl DurableHistory {
    pub fn validate(&self, project: &Project) -> DomainResult<()> {
        if self.schema != "transcriptor-history/1" {
            return Err(DomainError::unsupported("schema de historial desconocido"));
        }
        if self.project_id != project.project_id || self.revision != project.revision {
            return Err(DomainError::precondition("historial de otro proyecto o revisión"));
        }
        if self.undo.len() > HISTORY_DEPTH || self.redo.len() > HISTORY_DEPTH {
            return Err(DomainError::invalid("historial excede profundidad máxima"));
        }
        for entry in self.undo.iter().chain(&self.redo) {
            for p in [&entry.before, &entry.after] {
                p.validate()?;
                if p.project_id != self.project_id || p.revision > self.revision {
                    return Err(DomainError::invalid("snapshot de historial incoherente"));
                }
            }
        }
        let equivalent = |a: &Project, b: &Project| {
            let mut a = a.clone();
            a.revision = b.revision;
            a.updated_at.clone_from(&b.updated_at);
            a == *b
        };
        if self.undo.back().is_some_and(|e| e.expect != self.revision || !equivalent(&e.after, project))
            || self.redo.back().is_some_and(|e| e.expect != self.revision || !equivalent(&e.before, project))
        {
            return Err(DomainError::invalid("la frontera de historial no coincide con el proyecto"));
        }
        for (i, entry) in self.undo.iter().enumerate().skip(1) {
            if !equivalent(&self.undo[i - 1].after, &entry.before) {
                return Err(DomainError::invalid("historial undo discontinuo"));
            }
        }
        for (i, entry) in self.redo.iter().enumerate().skip(1) {
            if !equivalent(&self.redo[i - 1].before, &entry.after) {
                return Err(DomainError::invalid("historial redo discontinuo"));
            }
        }
        Ok(())
    }

    pub fn map_paths(&mut self, f: impl Fn(&str) -> String) {
        let mut cache = HashMap::new();
        for entry in self.undo.iter_mut().chain(&mut self.redo) {
            for p in [&mut entry.before, &mut entry.after] {
                for asset in &mut p.assets {
                    asset.path = f(&asset.path);
                }
                crate::source_bundles::map_project(p, &f, &mut cache);
            }
        }
    }
}

/// Evento de journal (auditoría): una línea JSON por comando confirmado.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct JournalEvent {
    pub at: String,
    pub command_id: String,
    pub actor: Actor,
    pub kind: String,
    pub label: String,
    pub base_revision: Revision,
    pub new_revision: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub diff: DiffSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Command>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Box<CommandReceipt>>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub project_id: String,
    pub request: CommandEnvelope,
    pub result: CommandResult,
}

/// Validated command result bound to an immutable base; fields cannot be forged by clients.
pub struct PreparedCommand {
    envelope: CommandEnvelope,
    before: Project,
    after: Project,
    effect: tv2_domain::CommandEffect,
    diff: DiffSummary,
}
impl PreparedCommand {
    /// Exact immutable base equality, including content changed without a new
    /// revision (for example Save As relocating source bundles). Shared storage
    /// makes unchanged evidence/layer comparisons cheap without JSON hashing.
    pub fn matches_base(&self, project: &Project) -> bool {
        &self.before == project
    }
    pub fn preview(&self) -> &Project {
        &self.after
    }
    pub fn diff(&self) -> DiffSummary {
        self.diff.clone()
    }
}

pub struct ProjectSession {
    project: Project,
    validation: std::sync::Mutex<tv2_domain::validation::LayerValidationCache>,
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    idempotency: HashMap<String, crate::receipt_cache::StoredReceipt>,
    receipt_spill: Option<std::sync::Arc<crate::receipt_cache::ReceiptSpill>>,
    journal: Vec<JournalEvent>,
    legacy_keys: std::collections::HashSet<String>,
    dirty: bool,
}

impl ProjectSession {
    pub fn history_snapshot(&self) -> DurableHistory {
        DurableHistory {
            schema: "transcriptor-history/1".into(),
            project_id: self.project.project_id.clone(),
            revision: self.revision(),
            undo: self.undo.clone(),
            redo: self.redo.clone(),
        }
    }

    pub fn restore_history(&mut self, history: DurableHistory) -> DomainResult<()> {
        history.validate(&self.project)?;
        self.undo = history.undo;
        self.redo = history.redo;
        Ok(())
    }

    pub fn resolve_paths(&mut self, f: impl Fn(&str) -> String) {
        let mut cache = HashMap::new();
        crate::source_bundles::map_project(&mut self.project, &f, &mut cache);
        for asset in &mut self.project.assets {
            asset.path = f(&asset.path);
        }
        for entry in self.undo.iter_mut().chain(&mut self.redo) {
            for p in [&mut entry.before, &mut entry.after] {
                for asset in &mut p.assets {
                    asset.path = f(&asset.path);
                }
                crate::source_bundles::map_project(p, &f, &mut cache);
            }
        }
    }

    /// After a successful save, adopt verified project-owned evidence paths
    /// without changing revision, editorial content or concurrent edits.
    pub fn adopt_source_bundle_locations(&mut self, store: &crate::ProjectStore, saved: &Project, history: &DurableHistory) {
        crate::source_bundles::adopt_projects(
            std::iter::once(&mut self.project).chain(self.undo.iter_mut().chain(&mut self.redo).flat_map(|e| [&mut e.before, &mut e.after])),
            store,
            saved,
            history,
        );
    }

    pub fn refresh_asset_availability(&mut self, mut exists: impl FnMut(&str) -> bool) {
        for asset in &mut self.project.assets {
            asset.missing = !exists(&asset.path);
        }
        for entry in self.undo.iter_mut().chain(&mut self.redo) {
            for p in [&mut entry.before, &mut entry.after] {
                for asset in &mut p.assets {
                    asset.missing = !exists(&asset.path);
                }
            }
        }
    }
    pub fn new(project: Project) -> Self {
        ProjectSession {
            project,
            validation: Default::default(),
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            idempotency: HashMap::new(),
            receipt_spill: None,
            journal: Vec::new(),
            legacy_keys: Default::default(),
            dirty: false,
        }
    }

    /// Rehydrate receipts without replaying commands or marking the file dirty.
    /// Legacy keys remain reserved: old audit cannot reconstruct generated IDs.
    pub fn with_audit(project: Project, events: &[JournalEvent]) -> DomainResult<Self> {
        project.validate()?;
        let mut session = Self::new(project);
        session.restore_receipts(events)?;
        Ok(session)
    }

    pub(crate) fn restore_receipts(&mut self, events: &[JournalEvent]) -> DomainResult<()> {
        let mut ids = std::collections::HashSet::new();
        for event in events {
            if event.new_revision > self.revision() || event.new_revision <= event.base_revision || !ids.insert(&event.command_id) {
                return Err(DomainError::invalid("auditoría incoherente con el proyecto"));
            }
            if let Some(receipt) = &event.receipt {
                let request = &receipt.request;
                let result = &receipt.result;
                self.check_identity(request)?;
                if receipt.project_id != self.project.project_id
                    || request.idempotency_key != event.idempotency_key
                    || request.idempotency_key.is_none()
                    || request.command_id != event.command_id
                    || result.command_id != event.command_id
                    || request.actor != event.actor
                    || event.command.as_ref() != Some(&request.command)
                    || result.base_revision != event.base_revision
                    || result.new_revision != event.new_revision
                    || result.diff != event.diff
                    || result.applied_at != event.at
                    || result.replayed
                    || event.kind != "command"
                {
                    return Err(DomainError::invalid("recibo idempotente incoherente con auditoría"));
                }
                let key = request.idempotency_key.as_ref().unwrap();
                if self.legacy_keys.contains(key) || self.idempotency.contains_key(key) {
                    return Err(DomainError::invalid("clave idempotente duplicada en auditoría"));
                }
                let stored =
                    crate::receipt_cache::StoredReceipt::new(request.clone(), result.clone(), self.idempotency.len(), &mut self.receipt_spill)?;
                self.idempotency.insert(key.clone(), stored);
            } else if let Some(key) = &event.idempotency_key
                && (self.idempotency.contains_key(key) || !self.legacy_keys.insert(key.clone()))
            {
                return Err(DomainError::invalid("clave idempotente duplicada en auditoría"));
            }
        }
        Ok(())
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn revision(&self) -> Revision {
        self.project.revision
    }

    /// Read a recorded result without executing or acknowledging any command.
    /// Cold receipts are verified before returning; missing legacy bodies yield None.
    pub fn command_receipt(&self, idempotency_key: &str) -> DomainResult<Option<CommandResult>> {
        self.idempotency.get(idempotency_key).map(crate::receipt_cache::StoredReceipt::result).transpose()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn acknowledge_save(&mut self, revision: Revision, journal_count: usize) {
        self.journal.drain(..journal_count.min(self.journal.len()));
        if self.revision() == revision {
            self.mark_clean();
        }
    }

    pub fn pending_journal(&self) -> &[JournalEvent] {
        &self.journal
    }

    /// Preserve original audit when recovering a separate unsaved-project store.
    pub fn recover_with_audit(&mut self, candidate: Project, events: Vec<JournalEvent>) -> DomainResult<()> {
        self.recover_audit_checked(candidate, events, None)
    }

    fn recover_audit_checked(
        &mut self,
        candidate: Project,
        events: Vec<JournalEvent>,
        validated_history: Option<&DurableHistory>,
    ) -> DomainResult<()> {
        let mut ids = std::collections::HashSet::new();
        for event in &events {
            if event.new_revision > candidate.revision || event.new_revision <= event.base_revision || !ids.insert(&event.command_id) {
                return Err(DomainError::invalid("auditoría de recuperación incoherente"));
            }
        }
        // Validate before any mutation, including recovery of receipt identity.
        let restored = Self::with_audit(candidate.clone(), &events)?;
        let mut additions = Vec::new();
        for (key, stored) in restored.idempotency {
            if self.legacy_keys.contains(&key) {
                return Err(DomainError::precondition("recuperación reutiliza una clave de auditoría antigua"));
            }
            if let Some(previous) = self.idempotency.get(&key) {
                if previous.load()?.as_ref() != stored.load()?.as_ref() {
                    return Err(DomainError::precondition("recuperación contiene otro recibo para una clave existente"));
                }
                continue;
            }
            let count = self.idempotency.len() + additions.len();
            let stored = if count >= crate::receipt_cache::HOT_RECEIPTS && matches!(&stored, crate::receipt_cache::StoredReceipt::Hot(_)) {
                let receipt = stored.load()?;
                crate::receipt_cache::StoredReceipt::new(receipt.0.clone(), receipt.1.clone(), count, &mut self.receipt_spill)?
            } else {
                stored
            };
            additions.push((key, stored));
        }
        if restored.legacy_keys.iter().any(|key| self.idempotency.contains_key(key)) {
            return Err(DomainError::precondition("recuperación sustituye un recibo por una clave antigua sin contenido"));
        }
        self.recover_checked(candidate, validated_history)?;
        self.idempotency.extend(additions);
        self.legacy_keys.extend(restored.legacy_keys);
        let mut journal = events;
        journal.append(&mut self.journal);
        self.journal = journal;
        Ok(())
    }

    /// Explicit human recovery; preserve the saved snapshot as one undo step.
    pub fn recover_checkpoint(&mut self, mut candidate: Project, events: Vec<JournalEvent>, mut history: Option<DurableHistory>) -> DomainResult<()> {
        if let Some(history) = &mut history {
            history.validate(&candidate)?;
            self.recovery_bundle_locations(&mut candidate, history)?;
            history.validate(&candidate)?;
        }
        self.recover_audit_checked(candidate, events, history.as_ref())?;
        if let Some(mut history) = history {
            history.revision = self.revision();
            if let Some(entry) = history.undo.back_mut() {
                entry.expect = history.revision;
            }
            if let Some(entry) = history.redo.back_mut() {
                entry.expect = history.revision;
            }
            self.undo = history.undo;
            self.redo = history.redo;
        }
        Ok(())
    }

    /// Autosave may contain resolved paths while the saved checkpoint uses
    /// @project locators. Reuse current locators only for exactly the same
    /// captured content, in candidate and every history boundary together.
    fn recovery_bundle_locations(&self, candidate: &mut Project, history: &mut DurableHistory) -> DomainResult<()> {
        for master in &self.project.masters {
            master.validate(&self.project)?;
        }
        for project in
            std::iter::once(candidate).chain(history.undo.iter_mut().chain(&mut history.redo).flat_map(|entry| [&mut entry.before, &mut entry.after]))
        {
            for master in &mut project.masters {
                let Some(bundle) = &master.source_bundle else {
                    continue;
                };
                if let Some(existing) = self
                    .project
                    .masters
                    .iter()
                    .find(|old| old.asset_id == master.asset_id && old.source_digest == master.source_digest && old.document == master.document)
                    && let Some(located) = &existing.source_bundle
                    && bundle.same_content(located)
                {
                    master.source_bundle = Some(located.clone());
                }
            }
        }
        Ok(())
    }

    /// Legacy recovery without a persisted history gets one undo to saved content.
    pub fn recover(&mut self, candidate: Project) -> DomainResult<()> {
        self.recover_checked(candidate, None)
    }

    fn recover_checked(&mut self, mut candidate: Project, validated_history: Option<&DurableHistory>) -> DomainResult<()> {
        candidate.validate()?;
        let proof_base = validated_history.and_then(|history| self.undo_source_enrichment_base(&candidate, history));
        crate::protection::check_transition(proof_base.as_ref().unwrap_or(&self.project), &candidate, &Actor::Human)?;
        if candidate.project_id != self.project.project_id {
            return Err(DomainError::precondition("autosave pertenece a otro proyecto"));
        }
        let base = self.revision();
        candidate.revision = base.max(candidate.revision).checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
        candidate.updated_at = now_iso();
        let command_id = new_command_id();
        self.journal.push(JournalEvent {
            at: candidate.updated_at.clone(),
            command_id: command_id.clone(),
            actor: Actor::Human,
            kind: "recovery".into(),
            label: "Recuperar autosave".into(),
            base_revision: base,
            new_revision: candidate.revision,
            idempotency_key: None,
            diff: DiffSummary::compute(&self.project, &candidate),
            command: None,
            receipt: None,
        });
        self.undo.clear();
        self.redo.clear();
        self.undo.push_back(HistoryEntry {
            label: "Recuperar autosave".into(),
            actor: Actor::Human,
            command_id,
            before: self.project.clone(),
            after: candidate.clone(),
            expect: candidate.revision,
        });
        self.project = candidate;
        self.dirty = true;
        Ok(())
    }

    /// Called only by explicit recovery after DurableHistory::validate succeeds.
    /// The immediate redo entry must prove a pure source-folder enrichment and
    /// the candidate must be its exact preimage except generated revision/time.
    /// Only that original bundle may be absent; all other protection stays on.
    fn undo_source_enrichment_base(&self, candidate: &Project, history: &DurableHistory) -> Option<Project> {
        let entry = history.redo.back()?;
        let equivalent = |left: &Project, right: &Project| {
            let mut left = left.clone();
            left.revision = right.revision;
            left.updated_at.clone_from(&right.updated_at);
            left == *right
        };
        if !equivalent(&entry.before, candidate) {
            return None;
        }
        let mut before_enrichment = entry.after.clone();
        let mut added = HashMap::new();
        for master in &mut before_enrichment.masters {
            let previous = entry.before.masters.iter().find(|old| old.asset_id == master.asset_id)?;
            if previous.source_bundle.is_none() && master.source_bundle.is_some() && previous.allows_source_enrichment(master) {
                added.insert(master.asset_id.clone(), master.clone());
                master.source_bundle = None;
            }
        }
        if added.is_empty() || !equivalent(&before_enrichment, &entry.before) {
            return None;
        }
        let mut base = self.project.clone();
        for master in &mut base.masters {
            if added.get(&master.asset_id).is_some_and(|proof| proof == master) {
                master.source_bundle = None;
            }
        }
        Some(base)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.back().map(|e| e.label.as_str())
    }

    /// Journal acumulado desde la última llamada a [`Self::drain_journal`].
    pub fn drain_journal(&mut self) -> Vec<JournalEvent> {
        std::mem::take(&mut self.journal)
    }

    /// Sustituye un snapshot sin generar revisión ni auditoría. El llamador
    /// administra la reconciliación; un cambio de identidad invalida el historial.
    pub fn replace_project(&mut self, project: Project, keep_history: bool) {
        let keep_history = keep_history && self.project.project_id == project.project_id;
        if !keep_history {
            self.idempotency.clear();
            self.receipt_spill = None;
            self.legacy_keys.clear();
            self.journal.clear();
        }
        self.project = project;
        if !keep_history {
            self.undo.clear();
            self.redo.clear();
        }
        self.dirty = false;
    }

    fn check_base(&self, envelope: &CommandEnvelope) -> DomainResult<()> {
        self.check_identity(envelope)?;
        if let Some(base) = envelope.base_revision
            && base != self.project.revision
        {
            return Err(DomainError::stale(base, self.project.revision));
        }
        if let Some(expected) = &envelope.precondition_digest
            && expected != &self.digest()?
        {
            return Err(DomainError::new(ErrorCode::ExternalConflict, "el contenido del proyecto cambió desde la consulta"));
        }
        Ok(())
    }

    pub fn digest(&self) -> DomainResult<String> {
        Ok(tv2_domain::digest::digest_json(&serde_json::to_value(&self.project)?))
    }

    fn check_identity(&self, envelope: &CommandEnvelope) -> DomainResult<()> {
        if envelope.protocol != PROTOCOL_VERSION {
            return Err(DomainError::unsupported(format!("protocolo desconocido: {}", envelope.protocol)));
        }
        if let Some(key) = &envelope.idempotency_key
            && (key.is_empty() || key.len() > 256)
        {
            return Err(DomainError::invalid("idempotency_key debe tener entre 1 y 256 bytes"));
        }
        if let Some(pid) = &envelope.project_id
            && pid != &self.project.project_id
        {
            return Err(DomainError::precondition(format!("el comando es para el proyecto {pid}, no para {}", self.project.project_id)));
        }
        Ok(())
    }

    pub fn dry_run(&self, envelope: &CommandEnvelope) -> DomainResult<DryRunResult> {
        let PreparedCommand { before, after, effect, diff, .. } = self.prepare_command(envelope.clone())?;
        Ok(DryRunResult { base_revision: before.revision, effect, diff, preview: after })
    }

    pub fn execute(&mut self, envelope: CommandEnvelope) -> DomainResult<CommandResult> {
        self.check_identity(&envelope)?;
        if envelope.idempotency_key.as_ref().is_some_and(|key| self.legacy_keys.contains(key)) {
            return Err(DomainError::precondition("clave registrada en auditoría antigua sin recibo; no se repetirá la operación"));
        }
        if let Some(key) = &envelope.idempotency_key
            && let Some(stored) = self.idempotency.get(key)
        {
            let stored = stored.load()?;
            let (request, prev) = stored.as_ref();
            // command_id identifies an attempt; every other field identifies its intent.
            let mut retry = envelope.clone();
            retry.command_id.clone_from(&request.command_id);
            if &retry != request {
                return Err(DomainError::precondition("idempotency_key ya usada para otra solicitud"));
            }
            let mut r = prev.clone();
            r.replayed = true;
            return Ok(r);
        }
        let prepared = self.prepare_command(envelope)?;
        self.commit_prepared(prepared)
    }

    /// Run on a worker for expensive commands, retaining generated IDs for preview/apply.
    pub fn prepare_command(&self, envelope: CommandEnvelope) -> DomainResult<PreparedCommand> {
        self.check_base(&envelope)?;
        let before = self.project.clone();
        let mut after = before.clone();
        let effect = envelope.command.apply(&mut after)?;
        crate::protection::attribute(&before, &mut after, &envelope.actor)?;
        self.validation.lock().map_err(|_| DomainError::io("caché de validación no disponible"))?.validate(&after)?;
        crate::protection::check_transition(&before, &after, &envelope.actor)?;
        let diff = DiffSummary::compute(&before, &after);
        Ok(PreparedCommand { envelope, before, after, effect, diff })
    }

    pub fn commit_prepared(&mut self, prepared: PreparedCommand) -> DomainResult<CommandResult> {
        let PreparedCommand { envelope, before, mut after, effect, diff } = prepared;
        self.check_identity(&envelope)?;
        if let Some(key) = &envelope.idempotency_key {
            if let Some(stored) = self.idempotency.get(key) {
                let stored = stored.load()?;
                let (request, previous) = stored.as_ref();
                let mut retry = envelope.clone();
                retry.command_id.clone_from(&request.command_id);
                if &retry != request {
                    return Err(DomainError::precondition("idempotency_key ya usada para otra solicitud"));
                }
                let mut result = previous.clone();
                result.replayed = true;
                return Ok(result);
            }
            if self.legacy_keys.contains(key) {
                return Err(DomainError::precondition("recibo antiguo sin contenido; no se repetirá la operación"));
            }
        }
        // PreparedCommand is opaque and its digest was checked before preparing.
        // Exact before-state equality below revalidates that same content without
        // serializing a large master again on the commit/UI thread.
        if let Some(base) = envelope.base_revision
            && base != self.project.revision
        {
            return Err(DomainError::stale(base, self.project.revision));
        }
        if before != self.project {
            return Err(DomainError::precondition("la base del comando preparado cambió"));
        }
        let base = before.revision;
        after.revision = base.max(after.revision).checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
        after.updated_at = now_iso();
        let result = CommandResult {
            command_id: envelope.command_id.clone(),
            base_revision: base,
            new_revision: after.revision,
            effect: effect.clone(),
            diff: diff.clone(),
            replayed: false,
            applied_at: after.updated_at.clone(),
        };
        // Spill may fail (for example a full disk); do it before mutating any
        // editorial state or history so the command remains atomic on failure.
        let stored_receipt = envelope
            .idempotency_key
            .as_ref()
            .map(|_| crate::receipt_cache::StoredReceipt::new(envelope.clone(), result.clone(), self.idempotency.len(), &mut self.receipt_spill))
            .transpose()?;
        self.undo.push_back(HistoryEntry {
            label: effect.label.clone(),
            actor: envelope.actor.clone(),
            command_id: envelope.command_id.clone(),
            before,
            after: after.clone(),
            expect: after.revision,
        });
        while self.undo.len() > HISTORY_DEPTH {
            self.undo.pop_front();
        }
        self.redo.clear();
        self.journal.push(JournalEvent {
            at: after.updated_at.clone(),
            command_id: envelope.command_id.clone(),
            actor: envelope.actor.clone(),
            kind: "command".into(),
            label: effect.label.clone(),
            base_revision: base,
            new_revision: after.revision,
            idempotency_key: envelope.idempotency_key.clone(),
            diff,
            command: Some(envelope.command.clone()),
            receipt: envelope
                .idempotency_key
                .as_ref()
                .map(|_| Box::new(CommandReceipt { project_id: after.project_id.clone(), request: envelope.clone(), result: result.clone() })),
        });
        self.project = after;
        self.dirty = true;
        if let Some(key) = &envelope.idempotency_key
            && let Some(stored) = stored_receipt
        {
            self.idempotency.insert(key.clone(), stored);
        }
        Ok(result)
    }

    pub fn undo(&mut self, actor: Actor) -> DomainResult<CommandResult> {
        self.revision().checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
        if !matches!(actor, Actor::Human)
            && let Some(entry) = self.undo.back()
        {
            crate::protection::check_transition(&self.project, &entry.before, &actor)?;
        }
        let entry = self.undo.pop_back().ok_or_else(|| DomainError::new(ErrorCode::Empty, "nada que deshacer"))?;
        if entry.expect != self.project.revision {
            let err = DomainError::stale(entry.expect, self.project.revision)
                .with_action("la entrada de historial se descartó porque el proyecto cambió por fuera");
            return Err(err);
        }
        let result = self.restore(&entry.before, format!("Deshacer {}", entry.label), &actor, &entry.command_id);
        if let Some(previous) = self.undo.back_mut() {
            previous.expect = self.project.revision;
        }
        let mut entry = entry;
        entry.expect = self.project.revision;
        self.redo.push_back(entry);
        Ok(result)
    }

    pub fn redo(&mut self, actor: Actor) -> DomainResult<CommandResult> {
        self.revision().checked_add(1).ok_or_else(|| DomainError::out_of_range("revisión agotada"))?;
        if !matches!(actor, Actor::Human)
            && let Some(entry) = self.redo.back()
        {
            crate::protection::check_transition(&self.project, &entry.after, &actor)?;
        }
        let entry = self.redo.pop_back().ok_or_else(|| DomainError::new(ErrorCode::Empty, "nada que rehacer"))?;
        if entry.expect != self.project.revision {
            return Err(DomainError::stale(entry.expect, self.project.revision));
        }
        let result = self.restore(&entry.after, format!("Rehacer {}", entry.label), &actor, &entry.command_id);
        if let Some(next) = self.redo.back_mut() {
            next.expect = self.project.revision;
        }
        let mut entry = entry;
        entry.expect = self.project.revision;
        self.undo.push_back(entry);
        Ok(result)
    }

    fn restore(&mut self, target: &Project, label: String, actor: &Actor, command_id: &str) -> CommandResult {
        let base = self.project.revision;
        let mut next = target.clone();
        next.revision = base + 1;
        next.updated_at = now_iso();
        let diff = DiffSummary::compute(&self.project, &next);
        let cid = format!("{}-restore-{}", command_id, next.revision);
        self.journal.push(JournalEvent {
            at: next.updated_at.clone(),
            command_id: cid.clone(),
            actor: actor.clone(),
            kind: "restore".into(),
            label: label.clone(),
            base_revision: base,
            new_revision: next.revision,
            idempotency_key: None,
            diff: diff.clone(),
            command: None,
            receipt: None,
        });
        self.project = next;
        self.dirty = true;
        CommandResult {
            command_id: cid,
            base_revision: base,
            new_revision: self.project.revision,
            effect: CommandEffect { label, ..Default::default() },
            diff,
            replayed: false,
            applied_at: self.project.updated_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::commands::tests_support::fake_video;
    use tv2_domain::ids::{AssetId, TrackId};
    use tv2_domain::{ClipId, MovePolicy, Ticks, TimeRange, TrackKind};

    fn s(n: i64) -> Ticks {
        Ticks::from_seconds(n)
    }

    #[test]
    fn prepared_diff_survives_commit_revision_metadata_and_exact_base_rechecks() {
        let mut project = Project::new("base");
        project.revision = 7;
        project.schema = tv2_domain::project::LEGACY_PROJECT_SCHEMA.into();
        let mut session = ProjectSession::new(project);
        let mut next = session.project().clone();
        next.schema = tv2_domain::PROJECT_SCHEMA.into();
        next.created_at = "preserved-source-date".into();
        next.revision = 20;
        next.extra.insert("external_metadata".into(), serde_json::json!({"retained":true}));
        let request = CommandEnvelope::human(Command::ReconcileProject { project: Box::new(next) }).with_base(7).with_idempotency("prepared-diff");
        let dry = session.dry_run(&request).unwrap();
        let prepared = session.prepare_command(request.clone()).unwrap();
        let expected = prepared.diff();
        assert!(expected.project_changed);
        assert_eq!(expected, dry.diff);
        let result = session.commit_prepared(prepared).unwrap();
        assert_eq!(result.diff, expected);
        assert_eq!(result.base_revision, 7);
        assert_eq!(result.new_revision, 21);
        assert_eq!(session.pending_journal().last().unwrap().diff, expected);
        assert_eq!(result.applied_at, session.project().updated_at);
        assert_eq!(session.execute(request).unwrap().diff, expected);
        for mutate in
            [(|project: &mut Project| project.updated_at = "other-writer-same-revision".into()) as fn(&mut Project), |project: &mut Project| {
                project.extra.insert("other".into(), true.into());
            }]
        {
            let prepared = session.prepare_command(CommandEnvelope::human(Command::RenameProject { name: "stale".into() })).unwrap();
            let mut changed = session.project().clone();
            mutate(&mut changed);
            assert!(!prepared.matches_base(&changed));
            session.replace_project(changed.clone(), true);
            assert!(session.commit_prepared(prepared).is_err());
            assert_eq!(session.project(), &changed);
        }
    }

    #[test]
    fn validation_cache_never_trusts_unvalidated_bases_or_changed_layer_context() {
        use tv2_domain::{LayerKind, SemanticItem, SemanticLayer};
        let mut project = Project::new("cache");
        project.assets.push(fake_video("a", 20));
        let mut layer = SemanticLayer::new("a".into(), LayerKind::User, "items");
        layer.items.push(SemanticItem::new(TimeRange::new(s(0), s(10)), "item"));
        project.layers.push(layer);
        let mut cache = tv2_domain::validation::LayerValidationCache::default();
        cache.validate(&project).unwrap();
        let mutations: Vec<fn(&mut Project)> = vec![
            |p| p.assets[0].probe.duration = s(1),
            |p| p.layers[0].color = "invalid".into(),
            |p| {
                let id = p.layers[0].items[0].item_id.clone();
                p.layers[0].deleted_item_ids.push(id);
            },
            |p| {
                p.layers[0].kind = LayerKind::Blocks;
                p.layers[0].extra.insert("v1_chunks_header".into(), serde_json::json!({}));
            },
            |p| p.layers[0].items[0].ranges[0].end = s(30),
            |p| p.active_sequence = Some("missing".into()),
            |p| p.assets.clear(),
            |p| p.schema = "transcriptor-project/unknown".into(),
        ];
        for mutate in mutations {
            let mut invalid = project.clone();
            mutate(&mut invalid);
            assert!(cache.validate(&invalid).is_err());
            assert!(invalid.validate().is_err());
            let session = ProjectSession::new(invalid);
            assert!(session.dry_run(&CommandEnvelope::human(Command::RenameProject { name: "still invalid".into() })).is_err());
            cache.validate(&project).unwrap();
        }
        let mut session = ProjectSession::new(project.clone());
        session.dry_run(&CommandEnvelope::human(Command::RenameProject { name: "warm".into() })).unwrap();
        let mut invalid = project;
        invalid.assets[0].probe.duration = s(1);
        session.replace_project(invalid, true);
        assert!(session.execute(CommandEnvelope::human(Command::RenameProject { name: "reject cached".into() })).is_err());
    }

    #[test]
    fn recovery_cannot_replace_an_existing_idempotency_receipt() {
        let project = Project::new("base");
        let mut live = ProjectSession::new(project.clone());
        let original = CommandEnvelope::human(tv2_domain::Command::RenameProject { name: "local".into() }).with_idempotency("shared-key");
        let receipt = live.execute(original.clone()).unwrap();
        let mut candidate = ProjectSession::new(project);
        candidate
            .execute(CommandEnvelope::human(tv2_domain::Command::RenameProject { name: "different".into() }).with_idempotency("shared-key"))
            .unwrap();
        let before = live.project().clone();
        assert!(live.recover_with_audit(candidate.project().clone(), candidate.pending_journal().to_vec()).is_err());
        assert_eq!(live.project(), &before);
        assert_eq!(live.command_receipt("shared-key").unwrap(), Some(receipt));
        assert!(live.execute(original).unwrap().replayed);
    }

    #[test]
    fn paste_items_remaps_hierarchy_preserves_multirange_and_is_atomic() {
        use tv2_domain::{LayerKind, SemanticItem, SemanticLayer};
        let mut p = Project::new("copy");
        p.assets.push(fake_video("a", 20));
        let mut layer = SemanticLayer::new("a".into(), LayerKind::Topics, "topics");
        let mut parent = SemanticItem::new(TimeRange::new(s(0), s(3)), "parent");
        parent.ranges.push(TimeRange::new(s(5), s(7)));
        let mut child = SemanticItem::new(TimeRange::new(s(1), s(2)), "child");
        child.parent_id = Some(parent.item_id.clone());
        child.comment = "preserved".into();
        layer.items = vec![child.clone(), parent.clone()].into(); // order must not affect validation
        p.layers.push(layer.clone());
        let mut session = ProjectSession::new(p.clone());
        let result = session
            .execute(CommandEnvelope::human(Command::PasteItems {
                layer_id: layer.layer_id.clone(),
                items: layer.items.clone().into(),
                position: s(10),
            }))
            .unwrap();
        let after = session.project().layers[0].clone();
        let copied_child = after.item(&tv2_domain::ItemId::new(&result.effect.created[0])).unwrap();
        let copied_parent = after.item(&tv2_domain::ItemId::new(&result.effect.created[1])).unwrap();
        assert_eq!(copied_child.parent_id, Some(copied_parent.item_id.clone()));
        assert_eq!(copied_child.comment, child.comment);
        assert_eq!(copied_child.start(), s(11));
        assert_eq!(copied_parent.ranges, vec![TimeRange::new(s(10), s(13)), TimeRange::new(s(15), s(17))]);
        assert!(
            session
                .execute(CommandEnvelope::human(Command::PasteItems { layer_id: layer.layer_id, items: layer.items.into(), position: s(19) }))
                .is_err()
        );
        assert_eq!(session.project().layers[0], after);
        session.undo(Actor::Human).unwrap();
        assert_eq!(session.project().layers, p.layers);
    }

    #[test]
    fn paste_preserves_clip_properties_and_links_after_cut_and_rolls_back_collisions() {
        let (mut session, track) = session();
        let result = session.execute(CommandEnvelope::human(add(&track, 0, 5, 0))).unwrap();
        let id = ClipId::new(result.effect.created[0].clone());
        let mut source = session.project().active().unwrap().clip(&id).unwrap().clone();
        source.name = "Edited ñ".into();
        source.enabled = false;
        source.gain_db = -12.0;
        source.transform.opacity = 0.4;
        source.link_group = Some("original-group".into());
        source.extra.insert("custom".into(), serde_json::json!({"keep":true}));
        let mut second = source.clone();
        second.id = ClipId::new("clipboard-second");
        second.position = s(5);
        session.execute(CommandEnvelope::human(Command::RemoveClips { clip_ids: vec![id], ripple: false })).unwrap();
        let pasted = session.execute(CommandEnvelope::human(Command::PasteClips { clips: vec![source.clone(), second], position: s(10) })).unwrap();
        let seq = session.project().active().unwrap();
        let copies: Vec<_> = pasted.effect.created.iter().map(|id| seq.clip(&ClipId::new(id)).unwrap()).collect();
        assert_eq!(copies[0].position, s(10));
        assert_eq!(copies[1].position, s(15));
        for c in &copies {
            assert_eq!(c.name, source.name);
            assert_eq!(c.gain_db, source.gain_db);
            assert_eq!(c.transform, source.transform);
            assert_eq!(c.extra, source.extra);
            assert!(!c.enabled);
            assert_ne!(c.link_group, source.link_group);
        }
        assert_eq!(copies[0].link_group, copies[1].link_group);
        let before = session.project().clone();
        // Second pasted clip collides; the first must roll back as well.
        let mut collision = source.clone();
        collision.id = ClipId::new("collision");
        collision.position = s(10);
        assert!(session.execute(CommandEnvelope::human(Command::PasteClips { clips: vec![source, collision], position: s(0) })).is_err());
        assert_eq!(session.project(), &before);
        session.undo(Actor::Human).unwrap();
        assert!(session.project().active().unwrap().clips.is_empty());
    }

    #[test]
    fn editorial_structure_validates_children_and_undoes_as_one_change() {
        use tv2_domain::{LayerKind, SemanticItem, SemanticLayer};
        let mut p = Project::new("editorial");
        p.assets.push(tv2_domain::commands::tests_support::fake_video("a", 20));
        let mut layer = SemanticLayer::new("a".into(), LayerKind::Topics, "topics");
        let mut parent = SemanticItem::new(TimeRange::new(s(0), s(5)), "parent");
        parent.ranges.push(TimeRange::new(s(10), s(15)));
        let child = SemanticItem::new(TimeRange::new(s(1), s(2)), "child");
        layer.items = vec![parent.clone(), child.clone()].into();
        p.layers.push(layer.clone());
        let mut session = ProjectSession::new(p.clone());
        let structure = Command::SetItemStructure {
            layer_id: layer.layer_id.clone(),
            item_id: child.item_id.clone(),
            parent_id: Some(parent.item_id.clone()),
            ranges: vec![TimeRange::new(s(1), s(2)), TimeRange::new(s(11), s(12))],
        };
        session.execute(CommandEnvelope::human(structure)).unwrap();
        let before = session.project().clone();
        let cycle = Command::SetItemStructure {
            layer_id: layer.layer_id.clone(),
            item_id: parent.item_id.clone(),
            parent_id: Some(child.item_id.clone()),
            ranges: parent.ranges.clone(),
        };
        assert!(session.execute(CommandEnvelope::human(cycle)).is_err());
        let orphan = Command::SetItemProps {
            layer_id: layer.layer_id.clone(),
            item_id: parent.item_id.clone(),
            label: None,
            comment: None,
            ranges: Some(vec![TimeRange::new(s(0), s(3))]),
        };
        assert!(session.execute(CommandEnvelope::human(orphan)).is_err());
        assert_eq!(session.project(), &before);
        session.undo(Actor::Human).unwrap();
        assert_eq!(session.project().layers, p.layers);
        session.redo(Actor::Human).unwrap();
        assert_eq!(session.project().layers, before.layers);
    }

    #[test]
    fn receipts_survive_disk_reopen_and_do_not_repeat_generated_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::ProjectStore::at(dir.path());
        let mut session = ProjectSession::new(Project::new("original"));
        let request = CommandEnvelope::human(Command::AddTrack {
            sequence_id: None,
            kind: tv2_domain::TrackKind::Video,
            name: "generated".into(),
            index: None,
        })
        .with_base(0)
        .with_idempotency("durable");
        let first = session.execute(request.clone()).unwrap();
        store.save_with_journal(session.project(), session.pending_journal()).unwrap();
        let reopened = crate::ProjectStore::at(dir.path());
        let mut loaded = ProjectSession::with_audit(reopened.load().unwrap(), &reopened.read_journal().unwrap()).unwrap();
        let before = loaded.project().clone();
        let replay = loaded.execute(request.clone()).unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.effect.created, first.effect.created);
        assert_eq!(loaded.project(), &before);
        assert!(loaded.pending_journal().is_empty());
        let mut changed = request.clone();
        changed.command = Command::RenameProject { name: "bad retry".into() };
        assert!(loaded.execute(changed).is_err());
        let mut corrupt = reopened.read_journal().unwrap();
        corrupt[0].receipt.as_mut().unwrap().result.new_revision += 1;
        assert!(ProjectSession::with_audit(before.clone(), &corrupt).is_err());
        corrupt[0].receipt = None;
        let mut legacy = ProjectSession::with_audit(before, &corrupt).unwrap();
        assert!(legacy.execute(request).is_err());
    }

    #[test]
    fn multiple_undo_redo_preserves_content_and_monotonic_revision() {
        let mut session = ProjectSession::new(Project::new("start"));
        for name in ["one", "two", "three"] {
            session.execute(CommandEnvelope::human(Command::RenameProject { name: name.into() })).unwrap();
        }
        for name in ["two", "one", "start"] {
            session.undo(Actor::Human).unwrap();
            assert_eq!(session.project().name, name);
        }
        for name in ["one", "two", "three"] {
            session.redo(Actor::Human).unwrap();
            assert_eq!(session.project().name, name);
        }
        assert_eq!(session.revision(), 9);
    }

    #[test]
    fn idempotency_rejects_changed_request_and_wrong_project_before_replay() {
        let mut session = ProjectSession::new(Project::new("start"));
        let request = CommandEnvelope::human(Command::RenameProject { name: "one".into() }).with_idempotency("key");
        session.execute(request.clone()).unwrap();
        let mut changed = request.clone();
        changed.command = Command::RenameProject { name: "two".into() };
        assert_eq!(session.execute(changed).unwrap_err().code, ErrorCode::Precondition);
        let mut wrong_project = request.clone();
        wrong_project.project_id = Some("different".into());
        assert!(session.execute(wrong_project).is_err());
        let mut bad_protocol = request.clone();
        bad_protocol.protocol = "unknown".into();
        assert_eq!(session.execute(bad_protocol).unwrap_err().code, ErrorCode::Unsupported);
        assert!(session.execute(request).unwrap().replayed);
        assert_eq!(session.revision(), 1);
    }

    #[test]
    fn digest_checks_dry_run_and_apply_and_revision_cannot_overflow() {
        let mut session = ProjectSession::new(Project::new("start"));
        let mut request = CommandEnvelope::human(Command::RenameProject { name: "one".into() });
        request.precondition_digest = Some("stale".into());
        assert!(session.dry_run(&request).is_err());
        assert!(session.execute(request.clone()).is_err());
        request.precondition_digest = Some(session.digest().unwrap());
        session.execute(request).unwrap();
        let mut project = session.project().clone();
        project.revision = u64::MAX;
        session.replace_project(project.clone(), false);
        assert!(session.execute(CommandEnvelope::human(Command::RenameProject { name: "overflow".into() })).is_err());
        assert_eq!(session.project(), &project);
    }

    #[test]
    fn explicit_recovery_can_be_undone_without_rewinding_revision() {
        let saved = Project::new("saved");
        let mut session = ProjectSession::new(saved.clone());
        let mut recovery = saved;
        recovery.name = "recovered".into();
        recovery.revision = 7;
        session.recover(recovery).unwrap();
        assert_eq!(session.project().name, "recovered");
        assert_eq!(session.revision(), 8);
        assert!(session.is_dirty());
        session.undo(Actor::Human).unwrap();
        assert_eq!(session.project().name, "saved");
        assert_eq!(session.revision(), 9);
    }

    fn session() -> (ProjectSession, TrackId) {
        let mut ses = ProjectSession::new(Project::new("t"));
        ses.execute(CommandEnvelope::human(Command::ImportAsset { asset: fake_video("a", 100) })).unwrap();
        let r = ses
            .execute(CommandEnvelope::human(Command::AddTrack { sequence_id: None, kind: TrackKind::Video, name: "V1".into(), index: None }))
            .unwrap();
        (ses, TrackId::new(r.effect.created[0].clone()))
    }

    fn add(track: &TrackId, a: i64, b: i64, pos: i64) -> Command {
        Command::AddClip {
            track_id: track.clone(),
            asset_id: AssetId::new("a"),
            source: TimeRange::new(s(a), s(b)),
            position: s(pos),
            policy: MovePolicy::Reject,
            clip_id: None,
            link_group: None,
            audio_stream: None,
            provenance: None,
        }
    }

    #[test]
    fn revisions_grow_and_undo_creates_new_revision() {
        let (mut ses, track) = session();
        assert_eq!(ses.revision(), 2);
        let r = ses.execute(CommandEnvelope::human(add(&track, 0, 10, 0))).unwrap();
        assert_eq!((r.base_revision, r.new_revision), (2, 3));
        assert_eq!(r.diff.clips_added, 1);
        let u = ses.undo(Actor::Human).unwrap();
        assert_eq!(u.new_revision, 4);
        assert!(ses.project().active().unwrap().clips.is_empty());
        let rd = ses.redo(Actor::Human).unwrap();
        assert_eq!(rd.new_revision, 5);
        assert_eq!(ses.project().active().unwrap().clips.len(), 1);
        assert_eq!(ses.drain_journal().len(), 5);
    }

    #[test]
    fn stale_base_is_rejected_and_idempotent_retry_replays() {
        let (mut ses, track) = session();
        let base = ses.revision();
        let env = CommandEnvelope::human(add(&track, 0, 10, 0)).with_base(base).with_idempotency("k1");
        let r1 = ses.execute(env.clone()).unwrap();
        let r2 = ses.execute(env).unwrap();
        assert!(r2.replayed);
        assert_eq!(r1.new_revision, r2.new_revision);
        assert_eq!(ses.project().active().unwrap().clips.len(), 1);
        let stale = CommandEnvelope::human(add(&track, 20, 30, 50)).with_base(base);
        let err = ses.execute(stale).unwrap_err();
        assert_eq!(err.code, ErrorCode::StaleRevision);
        assert_eq!(ses.revision(), r1.new_revision);
    }

    #[test]
    fn failed_batch_leaves_project_untouched_and_dry_run_does_not_commit() {
        let (mut ses, track) = session();
        ses.execute(CommandEnvelope::human(add(&track, 0, 10, 0))).unwrap();
        let rev = ses.revision();
        let batch = Command::Batch { label: "b".into(), commands: vec![add(&track, 20, 25, 30), add(&track, 20, 25, 32)] };
        assert!(ses.execute(CommandEnvelope::human(batch)).is_err());
        assert_eq!(ses.revision(), rev);
        assert_eq!(ses.project().active().unwrap().clips.len(), 1);
        let dry = ses.dry_run(&CommandEnvelope::human(add(&track, 20, 25, 30))).unwrap();
        assert_eq!(dry.diff.clips_added, 1);
        assert_eq!(ses.revision(), rev);
        assert_eq!(ses.project().active().unwrap().clips.len(), 1);
        assert_eq!(dry.preview.active().unwrap().clips.len(), 2);
    }

    #[test]
    fn undo_after_external_change_is_rejected_not_applied() {
        let (mut ses, track) = session();
        let r = ses.execute(CommandEnvelope::human(add(&track, 0, 10, 0))).unwrap();
        let clip = ClipId::new(r.effect.created[0].clone());
        // simulación de reconciliación externa: el proyecto salta de revisión
        let mut p = ses.project().clone();
        p.revision += 5;
        ses.replace_project(p, true);
        let err = ses.undo(Actor::Human).unwrap_err();
        assert_eq!(err.code, ErrorCode::StaleRevision);
        assert!(ses.project().active().unwrap().clip(&clip).is_some());
    }
}
