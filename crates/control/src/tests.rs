use crate::*;
use serde_json::{Value, json};
use tv2_application::{CommandEnvelope, ProjectSession};
use tv2_domain::{Command, Project, Ticks, TimeRange};

fn fixture() -> (ControlEngine, ProjectSession, HostState) {
    let session = ProjectSession::new(Project::new("Synthetic MCP fixture"));
    (
        ControlEngine::new(
            session.project().project_id.clone(),
            Permissions { read: true, propose: true, apply: true, selection: true, transport: true, jobs: true, ..Permissions::default() },
        ),
        session,
        HostState::default(),
    )
}
fn call(engine: &mut ControlEngine, session: &mut ProjectSession, host: &HostState, name: &str, mut args: Value) -> Value {
    if name != "tv2_context" {
        args["session_id"] = json!(engine.session_id());
        args["project_id"] = json!(session.project().project_id);
    }
    engine.handle(session, host, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}), |_| {
        Err("No host handler".into())
    })["result"]
        .clone()
}
fn proposal(engine: &mut ControlEngine, session: &mut ProjectSession, host: &HostState, key: &str) -> Value {
    let args = json!({"revision":session.revision(),"digest":session.digest().unwrap(),"idempotency_key":key,"command":{"type":"rename_project","name":"Reviewed name"}});
    let result = call(engine, session, host, "tv2_propose", args);
    assert_eq!(result["isError"], false, "{result}");
    result["structuredContent"].clone()
}
fn verify_ready(engine: &mut ControlEngine, session: &mut ProjectSession, host: &HostState, id: &Value) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let value = call(engine, session, host, "tv2_verify", json!({"proposal_id":id}));
        assert_eq!(value["isError"], false, "{value}");
        if value["structuredContent"]["verification"] == "complete" {
            return value;
        }
        assert!(value["structuredContent"]["matches_preview"].is_null(), "pending must never claim verification");
        assert!(std::time::Instant::now() < deadline, "verification worker timed out");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn proposal_review_apply_verify_and_receipt_replay() {
    let (mut engine, mut session, host) = fixture();
    let p = proposal(&mut engine, &mut session, &host, "first");
    assert_eq!(session.revision(), 0);
    let args = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":"first"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", args.clone())["isError"], true);
    engine.review(p["id"].as_str().unwrap(), true, &session).unwrap();
    let result = call(&mut engine, &mut session, &host, "tv2_apply", args.clone());
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["matches_preview"], true);
    assert_eq!(session.project().name, "Reviewed name");
    assert_eq!(session.pending_journal().len(), 1);
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", args)["structuredContent"]["replayed"], true);
    assert_eq!(session.revision(), 1);
    let verify = verify_ready(&mut engine, &mut session, &host, &p["id"]);
    assert_eq!(verify["structuredContent"]["matches_preview"], true);
    assert_eq!(verify["structuredContent"]["dirty"], true);
}
#[test]
fn verification_pending_and_cache_reject_same_revision_content_replacement() {
    let (mut engine, mut session, host) = fixture();
    let p = proposal(&mut engine, &mut session, &host, "verify-cache");
    engine.review(p["id"].as_str().unwrap(), true, &session).unwrap();
    let args = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":"verify-cache"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", args)["isError"], false);
    let pending = call(&mut engine, &mut session, &host, "tv2_verify", json!({"proposal_id":p["id"]}));
    assert_eq!(pending["structuredContent"]["verification"], "pending");
    assert!(pending["structuredContent"]["matches_preview"].is_null());
    let verified = verify_ready(&mut engine, &mut session, &host, &p["id"]);
    assert_eq!(verified["structuredContent"]["matches_preview"], true);
    assert_eq!(verified["structuredContent"]["receipt_in_project_journal"], true);
    let mut replacement = session.project().clone();
    replacement.name = "Same revision, different content".into();
    session = ProjectSession::new(replacement);
    let pending = call(&mut engine, &mut session, &host, "tv2_verify", json!({"proposal_id":p["id"]}));
    assert_eq!(pending["structuredContent"]["verification"], "pending");
    assert!(pending["structuredContent"]["matches_preview"].is_null());
    assert_eq!(pending["structuredContent"]["receipt_in_project_journal"], false);
    assert_eq!(verify_ready(&mut engine, &mut session, &host, &p["id"])["structuredContent"]["matches_preview"], false);
}
#[test]
fn automatic_scope_requires_every_batch_child_and_revokes_immediately() {
    let (mut engine, mut session, host) = fixture();
    let mut permissions = engine.permissions().clone();
    permissions.automatic_commands.extend(["batch".into(), "rename_project".into()]);
    engine.set_permissions(permissions.clone());
    let args = json!({"revision":0,"digest":session.digest().unwrap(),"idempotency_key":"auto-batch","command":{"type":"batch","label":"Scoped batch","commands":[{"type":"rename_project","name":"Automatic"},{"type":"set_skip_trims","enabled":true}]}});
    let prepared = call(&mut engine, &mut session, &host, "tv2_propose", args);
    assert_eq!(prepared["isError"], false, "{prepared}");
    let proposal = &prepared["structuredContent"];
    assert_eq!(proposal["automatic_eligible"], false);
    let apply = json!({"proposal_id":proposal["id"],"preview_digest":proposal["preview_digest"],"idempotency_key":"auto-batch"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", apply.clone())["isError"], true);
    permissions.automatic_commands.insert("set_skip_trims".into());
    engine.set_permissions(permissions.clone());
    assert!(engine.proposals()[0].automatic_eligible);
    permissions.automatic_commands.remove("rename_project");
    engine.set_permissions(permissions.clone());
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", apply.clone())["isError"], true);
    permissions.automatic_commands.insert("rename_project".into());
    engine.set_permissions(permissions);
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", apply)["isError"], false);
    assert_eq!(session.project().name, "Automatic");
    assert!(engine.events().iter().any(|event| event.detail["authorization"] == "local_automatic_scope"));
}
#[test]
fn text_search_filters_before_pagination_and_preserves_original_evidence() {
    let mut project = Project::new("Search");
    project.masters.push(tv2_domain::evidence::MasterEvidence {
        asset_id: "source".into(), source_digest: "original".into(), source_bundle: None,
        document: json!({"tracks":{"speaker-1":{"words":[{"t_ini":0,"t_fin":1,"text":"Nada"},{"t_ini":1,"t_fin":2,"text":"Árbol"},{"t_ini":2,"t_fin":3,"text":"ÁRBOL grande"}]}}}).into(),
    });
    let mut session = ProjectSession::new(project);
    let mut engine = ControlEngine::new(session.project().project_id.clone(), Permissions::read_only());
    let host = HostState::default();
    let args = json!({"revision":0,"asset_id":"source","section":"words","text":"árbol","start_ticks":0,"end_ticks":3*tv2_domain::FLICKS_PER_SECOND,"limit":1});
    let first = call(&mut engine, &mut session, &host, "tv2_evidence", args.clone());
    assert_eq!(first["structuredContent"]["items"][0]["record"]["text"], "Árbol");
    assert_eq!(first["structuredContent"]["next_offset"], 1);
    let mut next = args;
    next["offset"] = json!(1);
    let second = call(&mut engine, &mut session, &host, "tv2_evidence", next);
    assert_eq!(second["structuredContent"]["items"][0]["record"]["text"], "ÁRBOL grande");
    assert!(second["structuredContent"]["next_offset"].is_null());
    let too_long = json!({"revision":0,"kind":"assets","text":"x".repeat(257)});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_query", too_long)["isError"], true);
}
#[test]
fn stale_content_and_permission_revocation_prevent_apply() {
    let (mut engine, mut session, host) = fixture();
    let p = proposal(&mut engine, &mut session, &host, "stale");
    engine.review(p["id"].as_str().unwrap(), true, &session).unwrap();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "Human update".into() })).unwrap();
    let args = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":"stale"});
    let result = call(&mut engine, &mut session, &host, "tv2_apply", args);
    assert!(result["structuredContent"]["error"].as_str().unwrap().contains("E_STALE_REVISION"));
    assert_eq!(session.project().name, "Human update");
    engine.revoke();
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_context", json!({}))["isError"], true);
}
#[test]
fn closed_schema_rejects_shell_replace_human_actor_and_extra_fields() {
    let (mut engine, mut session, host) = fixture();
    for command in [
        json!({"type":"reconcile_project","project":session.project()}),
        json!({"type":"batch","commands":[]}),
        json!({"type":"rename_project","name":"ok","shell":"bad"}),
    ] {
        let args = json!({"revision":0,"digest":session.digest().unwrap(),"idempotency_key":"bad","command":command});
        assert_eq!(call(&mut engine, &mut session, &host, "tv2_propose", args)["isError"], true);
    }
    let mut args = json!({"revision":0,"digest":session.digest().unwrap(),"idempotency_key":"bad","command":{"type":"rename_project","name":"ok"}});
    args["actor"] = json!({"kind":"human"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_propose", args)["isError"], true);
    assert_eq!(session.revision(), 0);
}
#[test]
fn temporal_pagination_is_bounded_and_revision_checked() {
    let (mut engine, mut session, host) = fixture();
    for index in 0..5 {
        session
            .execute(CommandEnvelope::human(Command::AddMarker {
                range: TimeRange::new(Ticks(index * 10), Ticks(index * 10 + 5)),
                label: format!("marker {index}"),
                color: "#ffffff".into(),
                marker_id: None,
            }))
            .unwrap();
    }
    let args = json!({"revision":5,"kind":"markers","start_ticks":10,"end_ticks":45,"limit":2});
    let first = call(&mut engine, &mut session, &host, "tv2_query", args.clone());
    assert_eq!(first["structuredContent"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(first["structuredContent"]["next_offset"], 2);
    let mut second_args = args;
    second_args["offset"] = json!(2);
    let second = call(&mut engine, &mut session, &host, "tv2_query", second_args);
    assert_eq!(second["structuredContent"]["items"].as_array().unwrap().len(), 2);
    assert!(second["structuredContent"]["next_offset"].is_null());
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_query", json!({"revision":0,"kind":"markers"}))["isError"], true);
}
#[test]
fn host_receipts_require_success_and_replay_without_duplicate_action() {
    let (mut engine, mut session, mut host) = fixture();
    host.supports_transport = true;
    let message = json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"tv2_transport","arguments":{"session_id":engine.session_id(),"project_id":session.project().project_id,"revision":0,"operation":"seek","position_ticks":100,"idempotency_key":"seek1"}}});
    let failed = engine.handle(&mut session, &host, message.clone(), |_| Err("Player unavailable".into()));
    assert_eq!(failed["result"]["isError"], true);
    let mut calls = 0;
    for _ in 0..2 {
        let result = engine.handle(&mut session, &host, message.clone(), |action| {
            assert!(matches!(action, HostAction::Transport { .. }));
            calls += 1;
            Ok(json!({"position_ticks":100}))
        });
        assert_eq!(result["result"]["isError"], false);
    }
    assert_eq!(calls, 1);
}
#[test]
fn project_switch_and_session_scope_cannot_reuse_permissions() {
    let (mut engine, mut session, host) = fixture();
    let response=engine.handle(&mut session,&host,json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tv2_query","arguments":{"session_id":"wrong","project_id":"wrong","revision":0,"kind":"tracks"}}}),|_|unreachable!());
    assert_eq!(response["result"]["isError"], true);
    session.replace_project(Project::new("Other project"), false);
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_context", json!({}))["isError"], true);
}
#[test]
fn proposal_idempotency_detects_conflicting_payload() {
    let (mut engine, mut session, host) = fixture();
    let first = proposal(&mut engine, &mut session, &host, "repeat");
    assert_eq!(first["id"], proposal(&mut engine, &mut session, &host, "repeat")["id"]);
    let args =
        json!({"revision":0,"digest":session.digest().unwrap(),"idempotency_key":"repeat","command":{"type":"rename_project","name":"Different"}});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_propose", args)["isError"], true);
}
#[test]
fn failed_history_commit_revokes_review_and_exposes_reprepare() {
    let (mut engine, mut session, host) = fixture();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "Changed".into() })).unwrap();
    let args = json!({"revision":session.revision(),"digest":session.digest().unwrap(),"idempotency_key":"history-fails","command":{"type":"undo"}});
    let result = call(&mut engine, &mut session, &host, "tv2_propose", args);
    assert_eq!(result["isError"], false);
    let p = &result["structuredContent"];
    engine.review(p["id"].as_str().unwrap(), true, &session).unwrap();
    let mut history = session.history_snapshot();
    history.undo.back_mut().unwrap().command_id = "replacement-entry".into();
    session.restore_history(history).unwrap();
    let before = session.project().clone();
    let apply = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":"history-fails"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", apply)["isError"], true);
    assert_eq!(session.project(), &before);
    let summary = engine.proposals().pop().unwrap();
    assert!(summary.needs_revalidation && !summary.reviewed && !summary.automatic_eligible);
    assert!(summary.receipt.is_none());
    let reprepare = json!({"proposal_id":p["id"],"revision":session.revision(),"digest":session.digest().unwrap(),"idempotency_key":"history-retry"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_reprepare", reprepare)["isError"], false);
}
#[test]
fn history_proposals_use_worker_snapshot_review_receipts_and_exact_preview() {
    let (engine, mut session, host) = fixture();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "Human rename".into() })).unwrap();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tv2_propose","arguments":{
        "session_id":engine.session_id(),"project_id":session.project().project_id,"revision":session.revision(),
        "digest":session.digest().unwrap(),"idempotency_key":"undo-worker","command":{"type":"undo"}}}});
    assert!(engine.preparation_needs_history(&request));
    let (mut engine, response) = engine.handle_background(session.preparation_snapshot(), host.clone(), request);
    assert_eq!(response["result"]["isError"], false, "{response}");
    let p = &response["result"]["structuredContent"];
    let apply = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":"undo-worker"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", apply.clone())["isError"], true);
    engine.review(p["id"].as_str().unwrap(), true, &session).unwrap();
    let result = call(&mut engine, &mut session, &host, "tv2_apply", apply.clone());
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["matches_preview"], true);
    assert_eq!(session.project().name, "Synthetic MCP fixture");
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", apply)["structuredContent"]["replayed"], true);
    let verified = verify_ready(&mut engine, &mut session, &host, &p["id"]);
    assert_eq!(verified["structuredContent"]["receipt_in_project_journal"], true);
    assert_eq!(verified["structuredContent"]["matches_preview"], true);
    assert!(Permissions::automatic_command_types().iter().any(|name| name == "redo"));
    assert!(crate::schema::parse_command(json!({"type":"batch","label":"nested history","commands":[{"type":"undo"}]})).is_err());
}
#[test]
fn stale_proposals_can_be_rejected_to_release_preparation_capacity() {
    let (mut engine, mut session, host) = fixture();
    let proposals: Vec<_> = (0..32).map(|i| proposal(&mut engine, &mut session, &host, &format!("queued-{i}"))).collect();
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "Local edit".into() })).unwrap();
    let args = json!({"revision":session.revision(),"digest":session.digest().unwrap(),"idempotency_key":"new-base","command":{"type":"rename_project","name":"Next"}});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_propose", args.clone())["isError"], true);
    let id = proposals[0]["id"].as_str().unwrap();
    assert!(engine.review(id, true, &session).is_err());
    engine.review(id, false, &session).unwrap();
    let rejected = engine.proposals().into_iter().find(|p| p.id == id).unwrap();
    assert!(rejected.rejected && !rejected.reviewed && !rejected.automatic_eligible);
    assert!(engine.review(id, true, &session).is_err());
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_propose", args)["isError"], false);
    assert_eq!(session.project().name, "Local edit");
    assert_eq!(session.revision(), 1);
}
#[test]
fn restored_requests_keep_diff_but_require_new_preview_and_review() {
    let (mut engine, mut session, host) = fixture();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("control.json");
    engine.set_state_path(path.clone()).unwrap();
    let old = proposal(&mut engine, &mut session, &host, "before_restart");
    engine.review(old["id"].as_str().unwrap(), true, &session).unwrap();
    wait_saved(&engine);
    drop(engine);
    let mut restored = ControlEngine::restore(
        session.project().project_id.clone(),
        Permissions { read: true, propose: true, apply: true, ..Permissions::default() },
        path.clone(),
    )
    .unwrap();
    let restored_summary = restored.proposals().pop().unwrap();
    assert!(!restored_summary.reviewed);
    assert!(restored_summary.needs_revalidation);
    assert_eq!(restored_summary.diff, old["diff"]);
    assert!(restored.review(&restored_summary.id, true, &session).is_err());
    restored.review(&restored_summary.id, false, &session).unwrap();
    assert!(restored.proposals()[0].rejected);
    let args = json!({"proposal_id":old["id"],"revision":session.revision(),"digest":session.digest().unwrap(),"idempotency_key":"after_restart"});
    let result = call(&mut restored, &mut session, &host, "tv2_reprepare", args);
    assert_eq!(result["isError"], false, "{result}");
    assert_ne!(result["structuredContent"]["id"], old["id"]);
    assert_eq!(result["structuredContent"]["reviewed"], false);
    wait_saved(&restored);
    assert!(std::fs::read_dir(path.with_extension("audit")).unwrap().count() >= 3);
}
fn wait_saved(engine: &ControlEngine) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status = engine.persistence_status();
        if status["state"] == "saved" {
            break;
        }
        assert_ne!(status["state"], "error", "{status}");
        assert!(std::time::Instant::now() < deadline, "Persistence timeout: {status}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn background_proposal_cannot_overwrite_concurrent_gui_edit() {
    let (engine, mut session, host) = fixture();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tv2_propose","arguments":{"session_id":engine.session_id(),"project_id":session.project().project_id,"revision":session.revision(),"digest":session.digest().unwrap(),"idempotency_key":"background","command":{"type":"rename_project","name":"Background"}}}});
    assert!(ControlEngine::is_background_request(&request));
    let snapshot = session.preparation_snapshot();
    let worker = std::thread::spawn(move || engine.handle_background(snapshot, host, request));
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "Concurrent human edit".into() })).unwrap();
    let (mut engine, result) = worker.join().unwrap();
    assert_eq!(result["result"]["isError"], false);
    let id = result["result"]["structuredContent"]["id"].as_str().unwrap();
    assert!(engine.review(id, true, &session).is_err());
    assert_eq!(session.project().name, "Concurrent human edit");
}
#[test]
fn real_loopback_http_auth_origin_and_json_rpc_dispatch() {
    use std::io::{Read, Write};
    let service = ControlService::start().unwrap();
    let address = service.endpoint().trim_start_matches("http://").trim_end_matches("/mcp").to_owned();
    let request = |token: String, origin: Option<&str>, body: String| {
        let mut stream = std::net::TcpStream::connect(&address).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        write!(stream,"POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",origin.map_or(String::new(),|o|format!("Origin: {o}\r\n")),body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    };
    let body=json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":MCP_VERSION,"capabilities":{},"clientInfo":{"name":"synthetic-test","version":"1"}}}).to_string();
    assert!(request("wrong-token".into(), None, body.clone()).starts_with("HTTP/1.1 401"));
    assert!(request(service.token().into(), Some("https://hostile.invalid"), body.clone()).starts_with("HTTP/1.1 403"));
    let token = service.token().to_owned();
    std::thread::scope(|scope| {
        let client = scope.spawn(|| request(token, None, body));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pending = loop {
            if let Some(pending) = service.try_recv() {
                break pending;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let (mut engine, mut session, host) = fixture();
        let response = engine.handle(&mut session, &host, pending.message.clone(), |_| unreachable!());
        pending.respond(response);
        let response = client.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        let body: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["result"]["protocolVersion"], MCP_VERSION);
        assert_eq!(body["result"]["capabilities"]["tools"]["listChanged"], false);
    });
}
#[test]
fn discovered_command_schemas_match_actual_accepted_commands() {
    let (mut engine, mut session, host) = fixture();
    for command in [
        json!({"type":"set_skip_trims","enabled":true}),
        json!({"type":"add_marker","range":{"start":0,"end":1},"label":"Synthetic","color":"#abcdef"}),
    ] {
        let args = json!({"revision":0,"digest":session.digest().unwrap(),"idempotency_key":command["type"],"command":command});
        let result = call(&mut engine, &mut session, &host, "tv2_propose", args);
        assert_eq!(result["isError"], false, "{result}");
    }
}

#[test]
fn restored_control_receipt_does_not_claim_an_unsaved_project_commit() {
    let (mut engine, mut session, host) = fixture();
    let old_project = session.project().clone();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("control.json");
    engine.set_state_path(path.clone()).unwrap();
    let p = proposal(&mut engine, &mut session, &host, "unsaved");
    engine.review(p["id"].as_str().unwrap(), true, &session).unwrap();
    let args = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":"unsaved"});
    assert_eq!(call(&mut engine, &mut session, &host, "tv2_apply", args.clone())["isError"], false);
    wait_saved(&engine);
    drop(engine);
    let mut reopened = ProjectSession::new(old_project);
    let mut engine =
        ControlEngine::restore(reopened.project().project_id.clone(), Permissions { read: true, apply: true, ..Permissions::default() }, path)
            .unwrap();
    let replay = call(&mut engine, &mut reopened, &host, "tv2_apply", args);
    assert_eq!(replay["isError"], true);
    assert!(replay["structuredContent"]["error"].as_str().unwrap().contains("E_DURABILITY"));
    assert_eq!(reopened.revision(), 0);
    wait_saved(&engine);
}

#[test]
fn observing_gui_changes_emits_events_without_repeated_unchanged_entries() {
    let (mut engine, mut session, mut host) = fixture();
    engine.observe_host(&session, &host);
    let baseline = engine.events().len();
    engine.observe_host(&session, &host);
    assert_eq!(engine.events().len(), baseline);
    session.execute(CommandEnvelope::human(Command::RenameProject { name: "Local UI edit".into() })).unwrap();
    host.position_ticks = 500;
    engine.observe_host(&session, &host);
    assert_eq!(engine.events().len(), baseline + 2);
    assert!(engine.events().iter().any(|e| e.kind == "project_revision" && e.detail["revision"] == 1));
    for _ in 0..5 {
        let _ = call(&mut engine, &mut session, &host, "tv2_events", json!({"after":0,"limit":200}));
    }
    let bytes = serde_json::to_vec(engine.events()).unwrap();
    assert!(bytes.len() < 16000, "event polling must not recursively archive earlier event responses");
}

#[test]
fn same_revision_content_replacement_invalidates_prepared_preview() {
    let (mut engine, mut session, host) = fixture();
    let p = proposal(&mut engine, &mut session, &host, "digest");
    engine.review(p["id"].as_str().unwrap(), true, &session).unwrap();
    let mut changed = session.project().clone();
    changed.name = "Changed without revision".into();
    session.replace_project(changed, true);
    let args = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":"digest"});
    let result = call(&mut engine, &mut session, &host, "tv2_apply", args);
    assert_eq!(result["isError"], true);
    assert!(result["structuredContent"]["error"].as_str().unwrap().contains("E_STALE_REVISION"));
}

fn apply_command(engine: &mut ControlEngine, session: &mut ProjectSession, host: &HostState, key: &str, command: Value) -> Value {
    let args = json!({"revision":session.revision(),"digest":session.digest().unwrap(),"idempotency_key":key,"command":command});
    let result = call(engine, session, host, "tv2_propose", args);
    assert_eq!(result["isError"], false, "{result}");
    let p = result["structuredContent"].clone();
    engine.review(p["id"].as_str().unwrap(), true, session).unwrap();
    let args = json!({"proposal_id":p["id"],"preview_digest":p["preview_digest"],"idempotency_key":key});
    let result = call(engine, session, host, "tv2_apply", args);
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["matches_preview"], true);
    p
}
#[test]
fn semantic_batch_create_move_split_and_structure_uses_core_commands() {
    let (mut engine, mut session, host) = fixture();
    session
        .execute(CommandEnvelope::human(Command::ImportAsset { asset: tv2_domain::commands::tests_support::fake_video("asset-synthetic", 10) }))
        .unwrap();
    apply_command(
        &mut engine,
        &mut session,
        &host,
        "create",
        json!({"type":"batch","label":"Create proposed layer and item","commands":[
            {"type":"create_layer","asset_id":"asset-synthetic","kind":"ai","name":"Synthetic proposal","layer_id":"layer-test"},
            {"type":"add_item","layer_id":"layer-test","item":{"item_id":"item-test","label":"Synthetic","ranges":[{"start":0,"end":Ticks::from_seconds(2)}]}}
        ]}),
    );
    let item = session.project().layer(&"layer-test".into()).unwrap().item(&"item-test".into()).unwrap();
    assert_eq!(item.origin.as_deref(), Some("ai"));
    assert!(!item.edited);
    apply_command(
        &mut engine,
        &mut session,
        &host,
        "move_split",
        json!({"type":"batch","label":"Move and split","commands":[
            {"type":"shift_items","layer_id":"layer-test","item_ids":["item-test"],"delta":Ticks::from_seconds(1)},
            {"type":"split_item","layer_id":"layer-test","item_id":"item-test","at":Ticks::from_seconds(2)}
        ]}),
    );
    let layer = session.project().layer(&"layer-test".into()).unwrap();
    assert_eq!(layer.items.len(), 2);
    assert!(layer.items.iter().all(|item| !item.edited));
}
#[test]
fn linked_asset_insertion_and_track_properties_have_exact_previews() {
    let (mut engine, mut session, host) = fixture();
    session
        .execute(CommandEnvelope::human(Command::ImportAsset { asset: tv2_domain::commands::tests_support::fake_video("asset-synthetic", 10) }))
        .unwrap();
    apply_command(&mut engine, &mut session, &host, "linked", json!({"type":"insert_asset_linked","asset_id":"asset-synthetic","position":0}));
    let sequence = session.project().active().unwrap();
    assert!(sequence.clips.len() >= 2);
    let track_id = sequence.tracks[0].id.clone();
    let clip_id = sequence.clips[0].id.clone();
    apply_command(
        &mut engine,
        &mut session,
        &host,
        "props",
        json!({"type":"batch","label":"Properties","commands":[
            {"type":"set_track_props","track_id":track_id,"name":"Renamed track","visible":true},
            {"type":"set_clip_props","clip_id":clip_id,"transform":{"x":0.1,"y":0.2,"scale":0.8,"opacity":0.5,"fit":"fill"}}
        ]}),
    );
    assert_eq!(session.project().active().unwrap().tracks[0].name, "Renamed track");
}
#[test]
fn batch_depth_count_and_nested_state_injection_are_rejected() {
    let mut nested = json!({"type":"rename_project","name":"leaf"});
    for _ in 0..5 {
        nested = json!({"type":"batch","label":"nested","commands":[nested]});
    }
    assert!(crate::schema::parse_command(nested).is_err());
    let oversized = json!({"type":"batch","label":"oversized","commands":vec![json!({"type":"rename_project","name":"leaf"});129]});
    assert!(crate::schema::parse_command(oversized).is_err());
    let malicious = json!({"type":"batch","label":"no raw state","commands":[{"type":"rename_project","name":"safe"},{"type":"reconcile_project","project":Project::new("forbidden")} ]});
    assert!(crate::schema::parse_command(malicious).is_err());
    let forged =
        json!({"type":"add_item","layer_id":"layer-test","item":{"item_id":"item-test","ranges":[{"start":0,"end":1}],"extra":{"accepted":true}}});
    assert!(crate::schema::parse_command(forged).is_err());
}

#[test]
fn control_store_excludes_a_second_project_writer() {
    let (mut engine, session, _) = fixture();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("control.json");
    engine.set_state_path(path.clone()).unwrap();
    let error =
        ControlEngine::restore(session.project().project_id.clone(), Permissions::read_only(), path).err().expect("second writer must be rejected");
    assert!(error.contains("already in use"), "{error}");
    wait_saved(&engine);
}

#[test]
fn archived_audit_survives_burst_larger_than_memory_ring() {
    let (mut engine, _, _) = fixture();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("control.json");
    engine.set_state_path(path.clone()).unwrap();
    for index in 0..530 {
        engine.set_permissions(Permissions { read: index % 2 == 0, ..Permissions::default() });
    }
    assert_eq!(engine.events().len(), 512);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while engine.persistence_status()["state"] != "saved" {
        assert_ne!(engine.persistence_status()["state"], "error");
        assert!(std::time::Instant::now() < deadline, "audit writer timeout");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(std::fs::read_dir(path.with_extension("audit")).unwrap().count(), 530);
}
