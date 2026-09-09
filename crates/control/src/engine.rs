use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use tv2_application::{Actor, CommandEnvelope, ProjectSession, session::PreparedCommand};
use tv2_domain::{ClipId, ItemId, LayerId};

const MAX_PROPOSALS: usize = 32;
const MAX_EVENTS: usize = 512;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Permissions {
    pub read: bool,
    pub propose: bool,
    pub apply: bool,
    pub selection: bool,
    pub transport: bool,
    pub jobs: bool,
    /// Local opt-in only. Empty by default and never granted by remote tools.
    /// Batch requires both `batch` and every contained command type.
    #[serde(default)]
    pub automatic_commands: BTreeSet<String>,
}
impl Permissions {
    pub fn read_only() -> Self {
        Self { read: true, ..Self::default() }
    }
    pub fn automatic_command_types() -> Vec<String> {
        crate::schema::command_types()
    }
    fn authorizes_automatically(&self, command: &Value) -> bool {
        fn allowed(command: &Value, scope: &BTreeSet<String>) -> bool {
            command["type"].as_str().is_some_and(|kind| {
                scope.contains(kind)
                    && (kind != "batch"
                        || command["commands"].as_array().is_some_and(|children| !children.is_empty() && children.iter().all(|c| allowed(c, scope))))
            })
        }
        self.propose && self.apply && allowed(command, &self.automatic_commands)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(default)]
    pub clip_ids: Vec<ClipId>,
    #[serde(default)]
    pub item_ids: Vec<ItemId>,
    #[serde(default)]
    pub layer_id: Option<LayerId>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HostState {
    pub selection: Selection,
    pub position_ticks: i64,
    pub playing: bool,
    #[serde(default)]
    pub pending_composition: bool,
    /// Informational registry from the host; never an arbitrary dispatch tool.
    #[serde(default)]
    pub ui_actions: std::sync::Arc<Vec<Value>>,
    /// The GUI owns the job source. Entries must contain stable `id` and `state`.
    pub jobs: Vec<Value>,
    pub supports_selection: bool,
    pub supports_transport: bool,
    pub supports_job_cancel: bool,
    pub supports_export: bool,
    #[serde(default)]
    pub supports_import: bool,
    /// Trusted local store chosen by the GUI. This field cannot be supplied by
    /// JSON clients and is never exposed as a filesystem locator in responses.
    #[serde(skip)]
    pub audit_store: Option<tv2_application::ProjectStore>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportOperation {
    Play,
    Pause,
    Seek,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostAction {
    Selection {
        selection: Selection,
    },
    Transport {
        operation: TransportOperation,
        position_ticks: Option<i64>,
    },
    CancelJob {
        job_id: String,
    },
    /// Starts the GUI's configured export. A remote client cannot choose a path
    /// or overwrite policy; the host validates its configured export settings.
    Export,
    /// Ask the GUI to present its normal local media chooser. Acceptance creates
    /// a pending-local-selection ticket, not a claim that media was imported.
    Import,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControlEvent {
    pub session_id: String,
    pub sequence: u64,
    pub at: String,
    pub kind: String,
    pub detail: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalSummary {
    pub id: String,
    pub project_id: String,
    pub base_revision: u64,
    pub base_digest: String,
    pub preview_digest: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub command_key: String,
    pub command: Value,
    pub diff: Value,
    pub reviewed: bool,
    /// Informational; apply recomputes against the current local permissions.
    #[serde(default)]
    pub automatic_eligible: bool,
    pub rejected: bool,
    pub receipt: Option<Value>,
    #[serde(default)]
    pub needs_revalidation: bool,
}
struct Proposal {
    summary: ProposalSummary,
    prepared: Option<PreparedCommand>,
    request_digest: String,
}

/// One controller per explicitly enabled GUI project session. Changing project
/// identity revokes all access; callers must create a fresh controller/token.
pub struct ControlEngine {
    project_id: String,
    session_id: String,
    permissions: Permissions,
    proposals: BTreeMap<String, Proposal>,
    events: VecDeque<ControlEvent>,
    next_event: u64,
    host_receipts: BTreeMap<String, (String, Value)>,
    persistence: Option<crate::persistence::Persistence>,
    observed: Option<Observed>,
    verification: Option<Verification>,
    verification_pending: Option<crossbeam_channel::Receiver<Result<Verification, String>>>,
}
struct Verification {
    project: tv2_domain::Project,
    /// Can be reused after an exact comparison with the prepared preview.
    content_digest: Option<String>,
    full_digest: Option<String>,
}
struct Observed {
    revision: u64,
    selection: Selection,
    playing: bool,
    position: i64,
    jobs: Vec<(Value, Value)>,
    at: std::time::Instant,
}
impl ControlEngine {
    pub fn new(project_id: impl Into<String>, permissions: Permissions) -> Self {
        Self {
            project_id: project_id.into(),
            session_id: format!("control-{}", tv2_domain::ids::random_hex12()),
            permissions,
            proposals: BTreeMap::new(),
            events: VecDeque::new(),
            next_event: 1,
            host_receipts: BTreeMap::new(),
            persistence: None,
            observed: None,
            verification: None,
            verification_pending: None,
        }
    }
    /// Read/validate on a worker. Restart always revokes prior local reviews;
    /// tv2_reprepare produces a new exact preview before another local review.
    pub fn restore(project_id: impl Into<String>, permissions: Permissions, path: std::path::PathBuf) -> Result<Self, String> {
        let mut engine = Self::new(project_id, permissions);
        // Acquire ownership before reading; another GUI cannot race a snapshot
        // read with its final write and silently replace proposals.
        engine.persistence = Some(crate::persistence::Persistence::start(path.clone())?);
        if let Some(snapshot) = crate::persistence::read(&path)? {
            if snapshot.schema != "transcriptor-control/1" || snapshot.project_id != engine.project_id {
                return Err("Control state belongs to another project/schema".into());
            }
            if snapshot.proposals.len() > 128 || snapshot.events.len() > MAX_EVENTS {
                return Err("Control state exceeds retention limits".into());
            }
            for mut summary in snapshot.proposals {
                if summary.project_id != engine.project_id {
                    return Err("Proposal belongs to another project".into());
                }
                crate::schema::parse_command(summary.command.clone())?;
                summary.reviewed = false;
                summary.automatic_eligible = false;
                summary.needs_revalidation = summary.receipt.is_none();
                if engine.proposals.insert(summary.id.clone(), Proposal { summary, prepared: None, request_digest: String::new() }).is_some() {
                    return Err("Duplicate proposal ID".into());
                }
            }
            engine.events = snapshot.events.into();
            engine.next_event = engine.events.iter().map(|e| e.sequence).max().unwrap_or(0).saturating_add(1);
            if engine.events.iter().any(|e| !tv2_domain::ids::is_valid_v1_id(&e.session_id)) {
                return Err("Invalid archived control session ID".into());
            }
        }
        engine.event("control_session_started", json!({"restored":true,"reviews_revoked":true}));
        Ok(engine)
    }
    pub fn set_state_path(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.persistence = Some(crate::persistence::Persistence::start(path)?);
        self.persist();
        Ok(())
    }
    pub fn persistence_status(&self) -> Value {
        match &self.persistence {
            Some(store) => match store.status() {
                Ok(status) => json!({"state":status}),
                Err(error) => json!({"state":"error","error":error}),
            },
            None => json!({"state":"memory_only"}),
        }
    }
    /// Observe cheap revision/selection/job metadata; no project serialization or
    /// digest per frame. Playing positions are sampled at most once per second.
    pub fn observe_host(&mut self, session: &ProjectSession, host: &HostState) {
        if self.check_project(session).is_err() {
            return;
        }
        let jobs: Vec<_> = host.jobs.iter().map(|job| (job["id"].clone(), job["state"].clone())).collect();
        let now = std::time::Instant::now();
        let revision_changed = self.observed.as_ref().is_none_or(|old| old.revision != session.revision());
        let selection_changed = self.observed.as_ref().is_none_or(|old| old.selection != host.selection);
        let jobs_changed = self.observed.as_ref().is_none_or(|old| old.jobs != jobs);
        let transport_changed = self.observed.as_ref().is_none_or(|old| {
            old.playing != host.playing
                || old.position != host.position_ticks && (!host.playing || now.duration_since(old.at) >= std::time::Duration::from_secs(1))
        });
        if revision_changed {
            self.event("project_revision", json!({"project_id":self.project_id,"revision":session.revision(),"dirty":session.is_dirty()}));
        }
        if selection_changed {
            self.event("selection", json!(host.selection));
        }
        if jobs_changed {
            self.event("jobs", json!(jobs));
        }
        if transport_changed {
            self.event("transport", json!({"playing":host.playing,"position_ticks":host.position_ticks}));
        }
        let (position, at) = if transport_changed {
            (host.position_ticks, now)
        } else {
            self.observed.as_ref().map(|old| (old.position, old.at)).unwrap_or((host.position_ticks, now))
        };
        self.observed = Some(Observed { revision: session.revision(), selection: host.selection.clone(), playing: host.playing, position, jobs, at });
    }
    fn persist(&self) {
        if let Some(store) = &self.persistence {
            store.enqueue(crate::persistence::Snapshot {
                schema: "transcriptor-control/1".into(),
                project_id: self.project_id.clone(),
                proposals: self.proposals(),
                events: self.events.iter().cloned().collect(),
            });
        }
    }
    pub fn is_background_request(message: &Value) -> bool {
        message["method"] == "tools/call"
            && matches!(
                message["params"]["name"].as_str(),
                Some(
                    "tv2_context"
                        | "tv2_query"
                        | "tv2_evidence"
                        | "tv2_propose"
                        | "tv2_reprepare"
                        | "tv2_preview"
                        | "tv2_proposals"
                        | "tv2_events"
                        | "tv2_jobs"
                        | "tv2_audit"
                )
            )
    }
    /// Move this engine to a worker with an immutable project snapshot. The GUI
    /// can continue editing; proposal review/apply subsequently checks its base.
    pub fn handle_background(mut self, project: tv2_domain::Project, host: HostState, message: Value) -> (Self, Value) {
        let response = if Self::is_background_request(&message) {
            self.handle(&mut ProjectSession::new(project), &host, message, |_| Err("Host action cannot run in background query".into()))
        } else {
            json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32600,"message":"Request requires live host session"}})
        };
        (self, response)
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn permissions(&self) -> &Permissions {
        &self.permissions
    }
    /// Only the local UI has this API. No MCP tool can grant/review its own access.
    pub fn set_permissions(&mut self, permissions: Permissions) {
        self.permissions = permissions;
        for proposal in self.proposals.values_mut() {
            proposal.summary.automatic_eligible =
                proposal.prepared.is_some() && !proposal.summary.rejected && self.permissions.authorizes_automatically(&proposal.summary.command);
        }
        self.event("permissions_changed", json!(self.permissions));
    }
    pub fn revoke(&mut self) {
        self.set_permissions(Permissions::default());
        for proposal in self.proposals.values_mut() {
            proposal.summary.reviewed = false;
        }
        self.persist();
    }
    pub fn proposals(&self) -> Vec<ProposalSummary> {
        self.proposals.values().map(|p| p.summary.clone()).collect()
    }
    pub fn events(&self) -> &VecDeque<ControlEvent> {
        &self.events
    }
    /// Local review is tied to exact base revision and digest, plus exact preview.
    pub fn review(&mut self, id: &str, approve: bool, session: &ProjectSession) -> Result<(), String> {
        self.check_project(session)?;
        let p = self.proposals.get_mut(id).ok_or("Unknown proposal")?;
        if p.summary.receipt.is_some() {
            return Err("Proposal already applied".into());
        }
        let prepared = p.prepared.as_ref().ok_or("Proposal restored without executable state; use tv2_reprepare first")?;
        if p.summary.base_revision != session.revision() || !prepared.matches_base(session.project()) {
            return Err("E_STALE_REVISION: recreate the proposal against current content".into());
        }
        p.summary.reviewed = approve;
        p.summary.rejected = !approve;
        p.summary.automatic_eligible = approve && self.permissions.authorizes_automatically(&p.summary.command);
        if !approve {
            p.prepared = None;
            p.summary.needs_revalidation = true;
        }
        self.event(if approve { "proposal_reviewed" } else { "proposal_rejected" }, json!({"proposal_id":id}));
        Ok(())
    }
    fn event(&mut self, kind: &str, detail: Value) {
        self.events.push_back(ControlEvent {
            session_id: self.session_id.clone(),
            sequence: self.next_event,
            at: tv2_domain::project::now_iso(),
            kind: kind.into(),
            detail,
        });
        self.next_event = self.next_event.saturating_add(1);
        if self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
        self.persist();
    }
    fn check_project(&self, session: &ProjectSession) -> Result<(), String> {
        if session.project().project_id != self.project_id {
            return Err("E_PERMISSION: project changed; enable a new control session".into());
        }
        Ok(())
    }
    /// All mutation callbacks complete before a successful MCP result is returned.
    /// Persistent edits commit through ProjectSession; the GUI is responsible for
    /// its normal durable autosave, exactly as for local GUI commands.
    pub fn handle(
        &mut self,
        session: &mut ProjectSession,
        host: &HostState,
        message: Value,
        mut execute_host: impl FnMut(HostAction) -> Result<Value, String>,
    ) -> Value {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let rpc_error = |code, text: &str| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":text}});
        if message["jsonrpc"] != "2.0" || !(id.is_string() || id.is_i64() || id.is_u64()) {
            return rpc_error(-32600, "Invalid JSON-RPC request");
        }
        let Some(method) = message["method"].as_str() else {
            return rpc_error(-32600, "Missing method");
        };
        let result = match method {
            "initialize" => json!({"protocolVersion":crate::MCP_VERSION,"capabilities":{"tools":{"listChanged":false}},
                "serverInfo":{"name":"transcriptor-v2","version":env!("CARGO_PKG_VERSION")},
                "instructions":"Use tv2_context for session/project scope. Writes require proposal dry-run, local review, then apply and verification. Capabilities are permission-scoped; refresh tools/list after local permission changes."}),
            "ping" => json!({}),
            "tools/list" => json!({"tools":crate::schema::tools(&self.permissions, host)}),
            "tools/call" => {
                let Some(name) = message["params"]["name"].as_str() else {
                    return rpc_error(-32602, "Missing tool name");
                };
                let arguments = message["params"].get("arguments").cloned().unwrap_or_else(|| json!({}));
                let result = self.call(session, host, name, &arguments, &mut execute_host);
                let (value, failed) = match result {
                    Ok(value) => (value, false),
                    Err(error) => (json!({"error":error}), true),
                };
                // Audit metadata never includes bearer tokens or full evidence.
                let audited_arguments = if !failed && arguments.to_string().len() <= 64 * 1024 {
                    arguments.clone()
                } else {
                    json!({"digest":tv2_domain::digest::digest_json(&arguments)})
                };
                let audited_response = if matches!(
                    name,
                    "tv2_propose"
                        | "tv2_reprepare"
                        | "tv2_apply"
                        | "tv2_verify"
                        | "tv2_select"
                        | "tv2_transport"
                        | "tv2_cancel_job"
                        | "tv2_export"
                        | "tv2_import"
                ) && value.to_string().len() <= 64 * 1024
                {
                    value.clone()
                } else {
                    json!({"digest":tv2_domain::digest::digest_json(&value),"error":value.get("error"),"item_count":value["items"].as_array().map(Vec::len)})
                };
                self.event(
                    "request",
                    json!({"tool":name,"arguments":audited_arguments,"response":audited_response,"failed":failed,"revision":session.revision()}),
                );
                json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":failed})
            }
            _ => return rpc_error(-32601, "Method not found"),
        };
        json!({"jsonrpc":"2.0","id":id,"result":result})
    }
    fn scope(&self, a: &Value, session: &ProjectSession) -> Result<(), String> {
        self.check_project(session)?;
        if a["session_id"].as_str() != Some(&self.session_id) || a["project_id"].as_str() != Some(&self.project_id) {
            return Err("E_PERMISSION: wrong control session/project scope".into());
        }
        Ok(())
    }
    fn call(
        &mut self,
        session: &mut ProjectSession,
        host: &HostState,
        name: &str,
        a: &Value,
        execute_host: &mut impl FnMut(HostAction) -> Result<Value, String>,
    ) -> Result<Value, String> {
        self.check_project(session)?;
        crate::schema::validate_arguments(name, a, &self.permissions, host)?;
        if name == "tv2_context" {
            return Ok(
                json!({"session_id":self.session_id,"project_id":self.project_id,"revision":session.revision(),"digest":session.digest().map_err(err)?,
                "name":session.project().name,"clock":{"unit":"flick","per_second":tv2_domain::FLICKS_PER_SECOND},"permissions":self.permissions,
                "selection":host.selection,"transport":{"playing":host.playing,"position_ticks":host.position_ticks,"pending_composition":host.pending_composition},
                "active_sequence":session.project().active_sequence,"counts":{"assets":session.project().assets.len(),"layers":session.project().layers.len(),"sequences":session.project().sequences.len()},
                "ui_actions":host.ui_actions,
                "limits":{"page_size":200,"pending_proposals":MAX_PROPOSALS,"event_retention":MAX_EVENTS},
                "persistence":{"edits":"project session journal and normal GUI save","control_state":self.persistence_status(),"events":"bounded polling; immutable archive when a store is configured","reviews":"revoked on restart; reprepare then review"}}),
            );
        }
        self.scope(a, session)?;
        match name {
            "tv2_query" => query(session.project(), a),
            "tv2_evidence" => evidence(session.project(), a),
            "tv2_jobs" => page_values(host.jobs.iter().cloned(), a, session.revision()),
            "tv2_audit" => {
                if a["revision"].as_u64() != Some(session.revision()) {
                    return Err("E_STALE_REVISION: query current project context".into());
                }
                let store = host.audit_store.as_ref().ok_or("Project has no saved audit store")?;
                let page = store.journal_page(a.get("cursor").and_then(Value::as_str), limit(a)?).map_err(err)?;
                // A reconciliation command can contain a complete old project.
                // Audit metadata never serializes those snapshots into one page.
                let events: Vec<_> = page
                    .events
                    .iter()
                    .map(|event| {
                        json!({
                            "at":event.at,"command_id":event.command_id,"actor":event.actor,"kind":event.kind,"label":event.label,
                            "base_revision":event.base_revision,"new_revision":event.new_revision,"idempotency_key":event.idempotency_key,
                            "diff":event.diff,"has_command":event.command.is_some(),"has_receipt":event.receipt.is_some()
                        })
                    })
                    .collect();
                Ok(json!({"project_id":self.project_id,"revision":session.revision(),"events":events,"next_cursor":page.next_cursor,
                    "snapshot_digest":page.snapshot_digest,"scope":"saved project journal; command/snapshot bodies omitted"}))
            }
            "tv2_events" => {
                let after = a.get("after").and_then(Value::as_u64).unwrap_or(0);
                let limit = limit(a)?;
                let events: Vec<_> = self.events.iter().filter(|e| e.sequence > after).take(limit).collect();
                Ok(json!({"events":events,"next_after":events.last().map_or(after,|e|e.sequence),
                    "gap":self.events.front().is_some_and(|first|after.saturating_add(1)<first.sequence)}))
            }
            "tv2_propose" => self.propose(session, a),
            "tv2_reprepare" => {
                let source = self.proposals.get(string(a, "proposal_id")?).ok_or("Unknown proposal")?;
                let mut args = a.clone();
                args.as_object_mut().unwrap().remove("proposal_id");
                args["command"] = source.summary.command.clone();
                self.propose(session, &args)
            }
            "tv2_proposals" => page_values(self.proposals.values().map(|p| json!(p.summary)), a, session.revision()),
            "tv2_preview" => {
                let id = string(a, "proposal_id")?;
                let p = self.proposals.get(id).ok_or("Unknown proposal")?;
                let prepared = p.prepared.as_ref().ok_or("Proposal already consumed")?;
                if p.summary.base_revision != session.revision() || !prepared.matches_base(session.project()) {
                    return Err("E_STALE_REVISION: proposal preview is obsolete".into());
                }
                let mut args = a.clone();
                args["revision"] = json!(prepared.preview().revision);
                query(prepared.preview(), &args)
            }
            "tv2_apply" => self.apply(session, a),
            "tv2_verify" => self.verify(session, a),
            "tv2_select" | "tv2_transport" | "tv2_cancel_job" | "tv2_export" | "tv2_import" => {
                let key = string(a, "idempotency_key")?.to_owned();
                let digest = tv2_domain::digest::digest_json(&json!({"tool":name,"arguments":a}));
                if let Some((previous, value)) = self.host_receipts.get(&key) {
                    return if previous == &digest {
                        Ok(json!({"replayed":true,"result":value}))
                    } else {
                        Err("E_IDEMPOTENCY: key reused with another request".into())
                    };
                }
                if self.host_receipts.len() >= 2048 {
                    return Err("Host receipt capacity reached; start a new control session".into());
                }
                if a["revision"].as_u64() != Some(session.revision()) {
                    return Err("E_STALE_REVISION: query current context".into());
                }
                let action = match name {
                    "tv2_select" => {
                        let selection: Selection = serde_json::from_value(a["selection"].clone()).map_err(err)?;
                        if selection.clip_ids.len() + selection.item_ids.len() > 1000 {
                            return Err("Selection exceeds 1000 objects".into());
                        }
                        let project = session.project();
                        if selection.clip_ids.iter().any(|id| !project.sequences.iter().any(|s| s.clips.iter().any(|c| &c.id == id))) {
                            return Err("Selection references unknown clip".into());
                        }
                        let layer = selection.layer_id.as_ref().and_then(|id| project.layer(id));
                        if selection.layer_id.is_some() && layer.is_none()
                            || selection.item_ids.iter().any(|id| layer.is_none_or(|l| l.item(id).is_none()))
                        {
                            return Err("Selection references unknown layer/item".into());
                        }
                        HostAction::Selection { selection }
                    }
                    "tv2_transport" => {
                        let operation: TransportOperation = serde_json::from_value(a["operation"].clone()).map_err(err)?;
                        let position_ticks = a.get("position_ticks").and_then(Value::as_i64);
                        if matches!(operation, TransportOperation::Seek) && position_ticks.is_none() {
                            return Err("seek requires position_ticks".into());
                        }
                        if position_ticks.is_some_and(|ticks| ticks < 0) {
                            return Err("Negative transport position".into());
                        }
                        HostAction::Transport { operation, position_ticks }
                    }
                    "tv2_cancel_job" => {
                        let job_id = string(a, "job_id")?.to_owned();
                        if !host.jobs.iter().any(|job| job["id"] == job_id) {
                            return Err("Unknown job".into());
                        }
                        HostAction::CancelJob { job_id }
                    }
                    "tv2_import" => HostAction::Import,
                    _ => HostAction::Export,
                };
                let value = execute_host(action)?;
                self.host_receipts.insert(key, (digest, value.clone()));
                self.event("host_action_completed", json!({"tool":name,"result":value}));
                Ok(json!({"replayed":false,"result":value}))
            }
            _ => Err("Unknown tool".into()),
        }
    }
    fn propose(&mut self, session: &ProjectSession, a: &Value) -> Result<Value, String> {
        let key = string(a, "idempotency_key")?;
        let request_digest = tv2_domain::digest::digest_json(a);
        if let Some(previous) = self.proposals.values().find(|p| p.summary.idempotency_key == key) {
            return if previous.request_digest == request_digest {
                Ok(json!(previous.summary))
            } else {
                Err("E_IDEMPOTENCY: key reused with another proposal".into())
            };
        }
        if self.proposals.values().filter(|p| p.prepared.is_some() && p.summary.receipt.is_none() && !p.summary.rejected).count() >= MAX_PROPOSALS {
            return Err("Pending proposal capacity reached; review/reject existing proposals".into());
        }
        if self.proposals.len() >= 128 {
            let obsolete = self
                .proposals
                .iter()
                .find(|(_, p)| p.summary.receipt.is_some() || p.summary.rejected || p.summary.needs_revalidation)
                .map(|(id, _)| id.clone())
                .ok_or("Proposal retention exhausted")?;
            self.proposals.remove(&obsolete);
        }
        let command = crate::schema::parse_command(a["command"].clone())?;
        let mut envelope = CommandEnvelope::human(command)
            .with_actor(Actor::Agent { name: self.session_id.clone() })
            .with_base(a["revision"].as_u64().ok_or("Missing revision")?)
            .with_idempotency(format!("{}:{key}", self.session_id));
        envelope.project_id = Some(self.project_id.clone());
        envelope.precondition_digest = Some(string(a, "digest")?.to_owned());
        let prepared = session.prepare_command(envelope).map_err(err)?;
        let id = format!("proposal-{}", tv2_domain::ids::random_hex12());
        let summary = ProposalSummary {
            id: id.clone(),
            project_id: self.project_id.clone(),
            base_revision: session.revision(),
            base_digest: string(a, "digest")?.into(),
            preview_digest: content_digest(prepared.preview()),
            idempotency_key: key.into(),
            command_key: format!("{}:{key}", self.session_id),
            command: a["command"].clone(),
            diff: json!(prepared.diff()),
            reviewed: false,
            automatic_eligible: self.permissions.authorizes_automatically(&a["command"]),
            rejected: false,
            receipt: None,
            needs_revalidation: false,
        };
        let value = json!(summary);
        self.proposals.insert(id.clone(), Proposal { summary, prepared: Some(prepared), request_digest });
        self.event("proposal_prepared", json!({"proposal_id":id}));
        Ok(value)
    }
    fn apply(&mut self, session: &mut ProjectSession, a: &Value) -> Result<Value, String> {
        let id = string(a, "proposal_id")?;
        let p = self.proposals.get_mut(id).ok_or("Unknown proposal")?;
        if a["preview_digest"] != p.summary.preview_digest || a["idempotency_key"] != p.summary.idempotency_key {
            return Err("E_PRECONDITION: preview digest or idempotency key mismatch".into());
        }
        if let Some(receipt) = &p.summary.receipt {
            if session.command_receipt(&p.summary.command_key).map_err(err)?.as_ref().is_none_or(|r| json!(r) != *receipt) {
                return Err(
                    "E_DURABILITY: control receipt is absent from this project's journal; restore the project checkpoint or reprepare and review"
                        .into(),
                );
            }
            return Ok(json!({"receipt":receipt,"replayed":true}));
        }
        let automatic = self.permissions.authorizes_automatically(&p.summary.command);
        if (!p.summary.reviewed && !automatic) || p.summary.rejected {
            return Err("E_REVIEW_REQUIRED: local UI must review the exact preview or explicitly authorize all command types in this scope".into());
        }
        if p.summary.base_revision != session.revision() || p.prepared.as_ref().is_none_or(|prepared| !prepared.matches_base(session.project())) {
            return Err("E_STALE_REVISION: recreate and review proposal".into());
        }
        let prepared = p.prepared.take().ok_or("Proposal has no prepared command")?;
        let mut preview = prepared.preview().clone();
        let result = session.commit_prepared(prepared).map_err(err)?;
        let receipt = json!(result);
        // Commit changes only these envelope metadata fields; compare every
        // other field exactly before reusing the worker's preview digest.
        preview.revision = session.revision();
        preview.updated_at.clone_from(&session.project().updated_at);
        let matches = &preview == session.project();
        self.verification =
            matches.then(|| Verification { project: preview, content_digest: Some(p.summary.preview_digest.clone()), full_digest: None });
        p.summary.receipt = Some(receipt.clone());
        p.summary.automatic_eligible = false;
        let authorization = if p.summary.reviewed { "exact_local_review" } else { "local_automatic_scope" };
        self.event(
            "proposal_applied",
            json!({"proposal_id":id,"revision":session.revision(),"matches_preview":matches,"authorization":authorization}),
        );
        Ok(json!({"receipt":receipt,"replayed":false,"matches_preview":matches,"dirty":session.is_dirty(),
            "verification":"exact_prepared_state","full_digest_status":"requires_tv2_verify"}))
    }
    fn verify(&mut self, session: &ProjectSession, a: &Value) -> Result<Value, String> {
        let id = string(a, "proposal_id")?;
        let p = self.proposals.get(id).ok_or("Unknown proposal")?;
        let receipt = p.summary.receipt.clone().ok_or("Proposal has not been applied")?;
        let preview_digest = p.summary.preview_digest.clone();
        // Always read the actual live session's receipt and dirty state. A
        // ProjectSession constructed from a background snapshot has no journal.
        let journal_receipt = session.command_receipt(&p.summary.command_key).map_err(err)?;
        let digests = self.verification_digests(session.project())?;
        let (full, content, matches) = match digests {
            Some((full, content)) => (Some(full), Some(content.clone()), Some(content == preview_digest)),
            None => (None, None, None),
        };
        Ok(json!({"proposal_id":id,"receipt":receipt,"current_revision":session.revision(),"current_digest":full,
            "current_content_digest":content,"matches_preview":matches,"is_current_revision":receipt["new_revision"]==session.revision(),
            "verification":if matches.is_some(){"complete"}else{"pending"},"retry_after_ms":if matches.is_some(){None}else{Some(50)},
            "receipt_in_project_journal":journal_receipt.as_ref().is_some_and(|r|json!(r)==receipt),
            "dirty":session.is_dirty(),"durability":"normal GUI save; dirty=false indicates project save acknowledged"}))
    }
    fn verification_digests(&mut self, project: &tv2_domain::Project) -> Result<Option<(String, String)>, String> {
        if let Some(receiver) = &self.verification_pending {
            let completed = match receiver.try_recv() {
                Ok(value) => Some(value),
                Err(crossbeam_channel::TryRecvError::Empty) => None,
                Err(_) => Some(Err("Verification worker ended without a result".into())),
            };
            if let Some(result) = completed {
                self.verification_pending = None;
                let result = result?;
                // Equality includes same-revision relocations and all metadata.
                // An obsolete worker must never validate the new live project.
                if &result.project == project {
                    self.verification = Some(result);
                }
            }
        }
        if self.verification.as_ref().is_some_and(|cached| &cached.project != project) {
            self.verification = None;
        }
        if let Some(cached) = &self.verification
            && let (Some(full), Some(content)) = (&cached.full_digest, &cached.content_digest)
        {
            return Ok(Some((full.clone(), content.clone())));
        }
        if self.verification_pending.is_none() {
            let snapshot = project.clone();
            let content = self.verification.as_ref().and_then(|cached| cached.content_digest.clone());
            let (sender, receiver) = crossbeam_channel::bounded(1);
            std::thread::Builder::new()
                .name("control-verification".into())
                .spawn(move || {
                    let result = (|| -> Result<Verification, String> {
                        let full = ProjectSession::new(snapshot.clone()).digest().map_err(err)?;
                        let content = content.unwrap_or_else(|| content_digest(&snapshot));
                        Ok(Verification { project: snapshot, content_digest: Some(content), full_digest: Some(full) })
                    })();
                    let _ = sender.send(result);
                })
                .map_err(err)?;
            self.verification_pending = Some(receiver);
        }
        Ok(None)
    }
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn string<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    a[key].as_str().filter(|s| !s.is_empty() && s.len() <= 256).ok_or_else(|| format!("Missing or invalid {key}"))
}
fn content_digest(project: &tv2_domain::Project) -> String {
    let mut value = json!(project);
    let object = value.as_object_mut().unwrap();
    object.remove("revision");
    object.remove("updated_at");
    tv2_domain::digest::digest_json(&value)
}
fn limit(a: &Value) -> Result<usize, String> {
    let size = a.get("limit").map_or(Some(100), Value::as_u64).ok_or("Invalid limit")?;
    if !(1..=200).contains(&size) {
        return Err("limit must be 1..200".into());
    }
    Ok(size as usize)
}
fn page_values(values: impl IntoIterator<Item = Value>, a: &Value, revision: u64) -> Result<Value, String> {
    if a["revision"].as_u64() != Some(revision) {
        return Err("E_STALE_REVISION: restart pagination with current revision".into());
    }
    let offset = a.get("offset").map_or(Some(0), Value::as_u64).ok_or("Invalid offset")?;
    let offset = usize::try_from(offset).map_err(err)?;
    let size = limit(a)?;
    let mut values = values.into_iter().skip(offset);
    let mut page = Vec::new();
    let mut bytes = 0usize;
    let mut more = false;
    for value in values.by_ref().take(size + 1) {
        let row_bytes = serde_json::to_vec(&value).map_err(err)?.len();
        if page.len() == size || bytes.saturating_add(row_bytes) > 1024 * 1024 {
            if page.is_empty() {
                return Err("One result exceeds 1 MiB; use a narrower metadata/temporal query".into());
            }
            more = true;
            break;
        }
        bytes += row_bytes;
        page.push(value);
    }
    let next = more.then_some(offset + page.len());
    Ok(json!({"revision":revision,"offset":offset,"items":page,"next_offset":next}))
}
fn query(project: &tv2_domain::Project, a: &Value) -> Result<Value, String> {
    let text = a.get("text").and_then(Value::as_str).map(str::to_lowercase);
    let matches = |fields: &[&str]| text.as_ref().is_none_or(|needle| fields.iter().any(|field| field.to_lowercase().contains(needle)));
    let start = a.get("start_ticks").and_then(Value::as_i64).unwrap_or(0);
    let end = a.get("end_ticks").and_then(Value::as_i64).unwrap_or(i64::MAX);
    if start < 0 || end <= start {
        return Err("Expected nonnegative half-open temporal range".into());
    }
    let intersects = |s: i64, e: i64| s < end && (e > start || s == e && s >= start);
    let target = a.get("target_id").and_then(Value::as_str);
    match a["kind"].as_str().ok_or("Missing query kind")? {
        "clips"=>page_values(project.sequences.iter().filter(|s|target.is_none_or(|id|s.id.as_str()==id)).flat_map(|s| s.clips.iter()
            .filter(|c|intersects(c.position.0,c.end().0)&&matches(&[&c.name])).map(move |c|json!({"sequence_id":s.id,"clip":c}))),a,project.revision),
        "items"=>page_values(project.layers.iter().filter(|l|!l.deleted&&target.is_none_or(|id|l.layer_id.as_str()==id)).flat_map(|l|l.items.iter()
            .filter(|i|i.ranges.iter().any(|r|intersects(r.start.0,r.end.0))&&matches(&[&i.label,&i.comment])).map(move|i|json!({"layer_id":l.layer_id,"asset_id":l.asset_id,"clock":"source","item":i}))),a,project.revision),
        "markers"=>page_values(project.sequences.iter().filter(|s|target.is_none_or(|id|s.id.as_str()==id)).flat_map(|s|s.markers.iter()
            .filter(|m|intersects(m.range.start.0,m.range.end.0)&&matches(&[&m.label,&m.comment])).map(move|m|json!({"sequence_id":s.id,"marker":m}))),a,project.revision),
        "layers"=>page_values(project.layers.iter().filter(|l|!l.deleted&&target.is_none_or(|id|l.layer_id.as_str()==id)&&matches(&[&l.name])).map(|l|json!({"layer_id":l.layer_id,"asset_id":l.asset_id,"kind":l.kind,"name":l.name,"locked":l.locked,"visible":l.visible,"item_count":l.items.len(),"source_master_digest":l.source_master_digest})),a,project.revision),
        "tracks"=>page_values(project.sequences.iter().filter(|s|target.is_none_or(|id|s.id.as_str()==id)).flat_map(|s|s.tracks.iter().filter(|t|matches(&[&t.name])).map(move|t|json!({"sequence_id":s.id,"track":t}))),a,project.revision),
        "assets"=>page_values(project.assets.iter().filter(|asset|target.is_none_or(|id|asset.id.as_str()==id)&&matches(&[&asset.name])).map(|asset|json!({"id":asset.id,"name":asset.name,"kind":asset.kind,"duration":asset.duration(),"fingerprint":asset.fingerprint,"missing":asset.missing})),a,project.revision),
        "sequences"=>page_values(project.sequences.iter().filter(|s|target.is_none_or(|id|s.id.as_str()==id)&&matches(&[&s.name])).map(|s|json!({"id":s.id,"name":s.name,"duration":s.extent(),"tracks":s.tracks.len(),"clips":s.clips.len()})),a,project.revision),
        _=>Err("Unknown query kind".into()),
    }
}
fn evidence(project: &tv2_domain::Project, a: &Value) -> Result<Value, String> {
    let text = a.get("text").and_then(Value::as_str).map(str::to_lowercase);
    // Inspect scalar fields of one record, never serialize or recursively walk
    // the entire immutable master merely to match a search term.
    let matches = |value: &Value| text.as_ref().is_none_or(|needle| scalar_text_matches(value, needle));
    let asset = string(a, "asset_id")?;
    let master = project.masters.iter().find(|m| m.asset_id.as_str() == asset).ok_or("No immutable evidence for asset")?;
    let section = string(a, "section")?;
    let start = a.get("start_ticks").and_then(Value::as_i64).unwrap_or(0) as f64 / tv2_domain::FLICKS_PER_SECOND as f64;
    let end = a.get("end_ticks").and_then(Value::as_i64).unwrap_or(i64::MAX) as f64 / tv2_domain::FLICKS_PER_SECOND as f64;
    if end <= start {
        return Err("Expected half-open source temporal range".into());
    }
    if matches!(section, "words" | "utterances" | "laughter" | "arousal" | "emotions") {
        let tracks = master.document["tracks"].as_object().ok_or("Evidence has no source tracks")?;
        let track_id = a.get("track_id").and_then(Value::as_str);
        let rows = tracks.iter().filter(|(id, _)| track_id.is_none_or(|target| target == id.as_str())).flat_map(|(id, track)| {
            track[section]
                .as_array()
                .into_iter()
                .flatten()
                .filter(move |row| {
                    let from = row["t_ini"].as_f64();
                    let to = row["t_fin"].as_f64();
                    matches!((from,to),(Some(from),Some(to)) if from<end && (to>start || from==to&&from>=start)) && matches(row)
                })
                .map(move |row| json!({"track_id":id,"record":row}))
        });
        let mut result = page_values(rows, a, project.revision)?;
        result["source_digest"] = json!(master.source_digest);
        result["asset_id"] = json!(master.asset_id);
        result["record_clock"] = json!("source seconds (t_ini/t_fin); query bounds in flicks");
        return Ok(result);
    }
    // Named top-level evidence sections only; never arbitrary JSON pointers or filesystem reads.
    if section == "index" {
        let sections:Vec<_>=master.document.as_object().into_iter().flat_map(|o|o.iter()).filter(|(key,_)|text.as_ref().is_none_or(|needle|key.to_lowercase().contains(needle))).map(|(key,value)|json!({"section":key,"kind":if value.is_array(){"array"}else if value.is_object(){"object"}else{"scalar"},"count":value.as_array().map(Vec::len)})).collect();
        return page_values(sections, a, project.revision);
    }
    let value = master.document.get(section).ok_or("Unknown evidence section")?;
    let values: Box<dyn Iterator<Item = Value> + '_> = if let Some(array) = value.as_array() {
        Box::new(array.iter().filter(|row| matches(row)).map(evidence_summary))
    } else if let Some(object) = value.as_object() {
        Box::new(
            object
                .iter()
                .filter(|(key, value)| text.as_ref().is_none_or(|needle| key.to_lowercase().contains(needle) || scalar_text_matches(value, needle)))
                .map(|(key, value)| json!({"key":key,"value":evidence_summary(value)})),
        )
    } else {
        Box::new(std::iter::once(value).filter(|value| matches(value)).cloned())
    };
    let mut result = page_values(values, a, project.revision)?;
    result["source_digest"] = json!(master.source_digest);
    result["asset_id"] = json!(master.asset_id);
    Ok(result)
}
fn scalar_text_matches(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.to_lowercase().contains(needle),
        Value::Object(fields) => fields.values().filter_map(Value::as_str).any(|text| text.to_lowercase().contains(needle)),
        _ => false,
    }
}
// Never return an unbounded nested transcript as one "page" row. Temporal
// record tools above are the drill-down for original evidence arrays.
fn evidence_summary(value: &Value) -> Value {
    if let Some(array) = value.as_array() {
        json!({"type":"array","count":array.len(),"use":"words/utterances/laughter/arousal/emotions temporal sections"})
    } else if let Some(object) = value.as_object() {
        Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let summary = if let Some(array) = value.as_array() {
                        json!({"type":"array","count":array.len()})
                    } else if let Some(object) = value.as_object() {
                        json!({"type":"object","keys":object.keys().collect::<Vec<_>>()})
                    } else {
                        value.clone()
                    };
                    (key.clone(), summary)
                })
                .collect(),
        )
    } else {
        value.clone()
    }
}
