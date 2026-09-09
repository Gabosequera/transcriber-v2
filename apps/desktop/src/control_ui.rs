use crate::app::TranscriptorApp;
use serde_json::{Value, json};
use tv2_control::{ControlEngine, ControlService, HostAction, HostState, Permissions, TransportOperation};
use tv2_media::player::PlayerCommand;

#[derive(Default)]
pub struct ControlUi {
    pub open: bool,
    project: String,
    service: Option<ControlService>,
    engine: Option<ControlEngine>,
    request: String,
    response: String,
    pending_json: Option<Value>,
    error: Option<String>,
    background: Option<crossbeam_channel::Receiver<BackgroundResult>>,
    restore: Option<crossbeam_channel::Receiver<Result<ControlEngine, String>>>,
    import: Option<crossbeam_channel::Receiver<Result<String, String>>>,
    import_tickets: Vec<ImportTicket>,
}
struct ImportTicket {
    id: String,
    revision: u64,
    state: String,
    paths: Vec<std::path::PathBuf>,
}
struct BackgroundResult {
    engine: ControlEngine,
    message: Value,
    response: Value,
    request: Option<tv2_control::PendingRequest>,
}
impl TranscriptorApp {
    fn control_host_state(&self) -> HostState {
        let project = &self.project().project_id;
        let mut jobs: Vec<Value> = self
            .export_jobs_recovery
            .iter()
            .filter(|j| &j.project_id == project)
            .map(|j| json!({"id":j.id,"state":j.state,"revision":j.revision}))
            .collect();
        for queued in &self.export.pending {
            if let Some(j) = &queued.record
                && &j.project_id == project
            {
                jobs.retain(|v| v["id"] != j.id);
                jobs.push(json!({"id":j.id,"state":"queued","revision":j.revision}));
            }
        }
        if let Some(running) = self.export.running.as_ref().filter(|r| &r.project_id == project) {
            jobs.retain(|j| j["id"] != running.job_id);
            jobs.push(json!({"id":running.job_id,"state":"running","revision":running.revision,"progress":running.progress.lock().fraction}));
        }
        HostState {
            selection: tv2_control::Selection {
                clip_ids: self.selection.clips.clone(),
                item_ids: self.selection.items.iter().map(|(_, i)| i.clone()).collect(),
                layer_id: self.selection.layer.clone(),
            },
            position_ticks: self.playhead.0,
            playing: self.player_snapshot.playing,
            pending_composition: self.composition_pending(),
            ui_actions: self.keymap.manifest.clone(),
            jobs,
            supports_selection: true,
            supports_transport: self.player.is_some(),
            supports_job_cancel: true,
            supports_export: self.tools.is_ok(),
            supports_import: self.tools.is_ok(),
            audit_store: self.store.clone(),
        }
    }
    pub fn poll_control(&mut self, ctx: &egui::Context) {
        let mut control = std::mem::take(&mut self.control);
        if control.project != self.project().project_id {
            control.engine = None;
            control.service = None;
            control.project = self.project().project_id.clone();
            control.background = None;
            control.restore = None;
            control.pending_json = None;
            control.import = None;
            control.import_tickets.clear();
        }
        if let Some(rx) = &control.import {
            match rx.try_recv() {
                Ok(result) => {
                    control.import = None;
                    match result {
                        Ok(text) => control.request = text,
                        Err(e) => control.error = Some(e),
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(_) => {
                    control.import = None;
                    control.error = Some("Lector JSON terminó sin resultado".into());
                }
            }
        }
        if let Some(rx) = &control.restore {
            match rx.try_recv() {
                Ok(result) => {
                    control.restore = None;
                    match result {
                        Ok(engine) => control.engine = Some(engine),
                        Err(e) => {
                            control.error = Some(e);
                            control.service = None;
                        }
                    }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    control.restore = None;
                    control.error = Some("Falló la recuperación de propuestas".into());
                    control.service = None;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &control.background {
            match rx.try_recv() {
                Ok(result) => {
                    control.background = None;
                    control.engine = Some(result.engine);
                    control.request = serde_json::to_string_pretty(&result.message).unwrap_or_default();
                    control.response = serde_json::to_string_pretty(&result.response).unwrap_or_default();
                    if let Some(request) = result.request {
                        request.respond(result.response);
                    }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    control.background = None;
                    control.service = None;
                    control.error = Some("Worker MCP terminó sin resultado".into());
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
        }
        if control.engine.is_some() {
            for ticket in &mut control.import_tickets {
                if matches!(ticket.state.as_str(), "queued" | "cancelling") && !self.imports.busy() {
                    ticket.state = if ticket.state == "cancelling" {
                        "cancelled"
                    } else if ticket.paths.iter().all(|path| self.project().assets.iter().any(|asset| self.resolve_asset_path(asset) == *path)) {
                        "succeeded"
                    } else {
                        "failed_or_cancelled"
                    }
                    .into();
                }
            }
            let mut observed = self.control_host_state();
            observed.jobs.extend(control.import_tickets.iter().map(|t| json!({"id":t.id,"kind":"import","state":t.state,"revision":t.revision})));
            control.engine.as_mut().unwrap().observe_host(&self.session, &observed);
            // Bounded requests per frame keep playback/keyboard serviced.
            for _ in 0..4 {
                let request = control.service.as_ref().and_then(|s| s.try_recv());
                let message = match &request {
                    Some(r) => Some(r.message.clone()),
                    None => control.pending_json.take(),
                };
                let Some(message) = message else { break };
                let Some(mut engine) = control.engine.take() else { break };
                let mut host = self.control_host_state();
                host.jobs.extend(control.import_tickets.iter().map(|t| json!({"id":t.id,"kind":"import","state":t.state,"revision":t.revision})));
                if ControlEngine::is_background_request(&message) {
                    let project = self.project().clone();
                    let wake = ctx.clone();
                    let (tx, rx) = crossbeam_channel::bounded(1);
                    match std::thread::Builder::new().name("mcp-query-prepare".into()).spawn(move || {
                        let (engine, response) = engine.handle_background(project, host, message.clone());
                        let _ = tx.send(BackgroundResult { engine, message, response, request });
                        wake.request_repaint();
                    }) {
                        Ok(_) => control.background = Some(rx),
                        Err(e) => {
                            control.error = Some(e.to_string());
                            control.service = None;
                        }
                    }
                    break;
                }
                let revision = self.session.revision();
                let project_id = self.project().project_id.clone();
                let mut export_payload = if message["params"]["name"] == "tv2_export" {
                    let presets = tv2_media::export::presets();
                    Some(self.prepare_export_payload(presets[self.export.preset_idx.min(presets.len() - 1)].clone()).map_err(|e| e.to_string()))
                } else {
                    None
                };
                let duration = self.duration();
                let composition_pending = self.composition_pending();
                let mut transport_command = None;
                let requested_selection = message.pointer("/params/arguments/selection");
                let selection_asset = requested_selection
                    .and_then(|picked| picked["layer_id"].as_str())
                    .and_then(|id| self.project().layer(&tv2_domain::LayerId::new(id)))
                    .map(|layer| layer.asset_id.clone());
                let selection_in_active = requested_selection.and_then(|picked| picked["clip_ids"].as_array()).is_none_or(|ids| {
                    ids.iter()
                        .all(|id| id.as_str().is_some_and(|id| self.sequence().is_some_and(|seq| seq.clip(&tv2_domain::ClipId::new(id)).is_some())))
                });
                let selection = &mut self.selection;
                let playhead = &mut self.playhead;
                let player = &self.player;
                let export = &mut self.export;
                let imports = &mut self.imports;
                let source_asset = &mut self.source_asset;
                let view = &mut self.view;
                let resolved_revision = &mut self.resolved_revision;
                let response = engine.handle(&mut self.session,&host,message.clone(),|action| {
                    match action {
                        HostAction::Selection { selection: picked } => {
                            if !selection_in_active{return Err("Selecciona primero la secuencia de estos clips mediante una propuesta".into());}
                            if !picked.clip_ids.is_empty() && *view!=crate::app::ViewMode::Sequence{*view=crate::app::ViewMode::Sequence;*resolved_revision=None;}
                            if let Some(asset)=&selection_asset && *view==crate::app::ViewMode::Source && source_asset.as_ref()!=Some(asset){*source_asset=Some(asset.clone());*resolved_revision=None;}
                            selection.clips = picked.clip_ids;
                            selection.items = picked.item_ids.into_iter().filter_map(|i|picked.layer_id.clone().map(|l|(l,i))).collect();
                            selection.layer = picked.layer_id;
                            Ok(json!({"selected":true}))
                        }
                        HostAction::Transport { operation, position_ticks } => {
                            player.as_ref().ok_or("Transporte no disponible")?;
                            match operation {
                                TransportOperation::Play => transport_command = Some(PlayerCommand::Play),
                                TransportOperation::Pause => transport_command = Some(PlayerCommand::Pause),
                                TransportOperation::Seek => {
                                    let time = tv2_domain::Ticks(position_ticks.ok_or("Falta tiempo")?);
                                    if time < tv2_domain::Ticks::ZERO || time > duration { return Err("Tiempo fuera de la vista activa".into()); }
                                    *playhead = time; transport_command = Some(PlayerCommand::Seek(time));
                                }
                            }
                            Ok(json!({"accepted":true,"position_ticks":playhead.0,"pending_composition":composition_pending}))
                        }
                        HostAction::CancelJob { job_id } => {
                            if let Some(ticket)=control.import_tickets.iter_mut().find(|t|t.id==job_id && t.state=="pending_local_selection") {ticket.state="cancelled".into();return Ok(json!({"id":job_id,"state":"cancelled"}));}
                            if let Some(ticket)=control.import_tickets.iter_mut().find(|t|t.id==job_id && t.state=="queued") {
                                imports.pending.retain(|(project,path)|project!=&project_id || !ticket.paths.contains(path));
                                if let Some(running)=imports.running.as_ref().filter(|r|r.project==project_id && ticket.paths.contains(&r.path)){running.cancel.store(true,std::sync::atomic::Ordering::Release);}
                                ticket.state="cancelling".into();return Ok(json!({"id":job_id,"state":"cancelling","note":"already imported assets remain in the project"}));
                            }
                            if let Some(job) = export.running.as_ref().filter(|r|r.job_id == job_id && r.project_id==project_id) {
                                job.cancel.store(true,std::sync::atomic::Ordering::Release);
                                return Ok(json!({"id":job_id,"state":"cancelling"}));
                            }
                            if let Some(index) = export.pending.iter().position(|q|q.record.as_ref().is_some_and(|r|r.id==job_id && r.project_id==project_id)) {
                                export.pending.remove(index).unwrap().cancel().map_err(|e|e.to_string())?;
                                return Ok(json!({"id":job_id,"state":"cancelling"}));
                            }
                            Err("El trabajo no está activo en esta ventana".into())
                        }
                        HostAction::Export => {
                            let payload=export_payload.take().ok_or("Falta configuración export")??;
                            let destination=payload.request.destination.clone();
                            let job=crate::durable_exports::QueuedExport::new(crate::durable_exports::root(),project_id.clone(),payload).map_err(|e|e.to_string())?;
                            export.pending.push_back(job);
                            Ok(json!({"state":"awaiting_durable_enqueue","revision":revision,"destination":destination,"verification":"query jobs after persistence; media success requires a succeeded receipt"}))
                        }
                        HostAction::Import => {
                            if control.import_tickets.len()>=128 {return Err("Límite de tickets de importación de esta sesión".into());}
                            let id=format!("import-{}",tv2_domain::ids::random_hex12());
                            control.import_tickets.push(ImportTicket{id:id.clone(),revision,state:"pending_local_selection".into(),paths:Vec::new()});
                            control.open=true;
                            Ok(json!({"id":id,"state":"pending_local_selection","revision":revision}))
                        }
                    }
                });
                if let Some(command) = transport_command {
                    self.player_send(command);
                }
                if self.session.revision() != revision {
                    self.after_change();
                }
                control.request = serde_json::to_string_pretty(&message).unwrap_or_default();
                control.response = serde_json::to_string_pretty(&response).unwrap_or_default();
                if let Some(request) = request {
                    request.respond(response);
                }
                control.engine = Some(engine);
            }
        }
        self.control = control;
    }
}
pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.control);
    if state.open {
        egui::Window::new("Control externo y propuestas MCP").open(&mut state.open).default_width(800.0).show(ctx,|ui| {
            if state.service.is_none() {
                ui.label("Servicio local por sesión. Al cambiar de proyecto se cierran el puerto y los permisos.");
                if ui.button("Iniciar servicio local (lectura)").clicked() {
                    let wake = ctx.clone();
                    match ControlService::start_with_wake(move ||wake.request_repaint()) {
                        Ok(service) => {
                            state.project = app.project().project_id.clone();
                            let project=state.project.clone();let path=crate::paths::config_dir().join("control").join(format!("{}.json",tv2_domain::digest::digest_json(&json!(project))));
                            let wake=ctx.clone();let (tx,rx)=crossbeam_channel::bounded(1);
                            match std::thread::Builder::new().name("mcp-restore".into()).spawn(move||{
                                let _=tx.send(ControlEngine::restore(project,Permissions::read_only(),path));wake.request_repaint();
                            }) {Ok(_)=>state.restore=Some(rx),Err(e)=>state.error=Some(e.to_string())}
                            state.service = Some(service);
                        }
                        Err(e) => state.error = Some(e.to_string())
                    }
                }
            } else {
                let service = state.service.as_ref().unwrap();
                ui.monospace(service.endpoint());
                if ui.button("Copiar token de esta sesión").clicked() { ctx.copy_text(service.token().to_string()); }
                if ui.button("Detener servicio y revocar permisos").clicked() { state.service = None; state.engine = None; state.background=None;state.restore=None; }
            }
            if state.background.is_some() || state.restore.is_some() {ui.spinner();ui.label("Preparando consulta/propuesta en segundo plano…");}
            for ticket in &mut state.import_tickets {
                ui.horizontal(|ui| {
                    ui.label(format!("{} · {}",ticket.id,ticket.state));
                    if ticket.state=="pending_local_selection" {
                        if ticket.revision!=app.session.revision(){ticket.state="stale".into();}
                        else if ui.button("Seleccionar medios…").clicked(){
                            if let Some(paths)=rfd::FileDialog::new().pick_files(){app.import_paths(&paths);ticket.paths=paths;ticket.state="queued".into();}
                            else{ticket.state="cancelled".into();}
                        }
                    }
                });
            }
            if let Some(engine) = &mut state.engine {
                ui.label(format!("Persistencia: {}",engine.persistence_status()));
                let mut permissions = engine.permissions().clone();
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    changed |= ui.checkbox(&mut permissions.read,"Consultar").changed();
                    changed |= ui.checkbox(&mut permissions.propose,"Proponer").changed();
                    changed |= ui.checkbox(&mut permissions.apply,"Aplicar revisadas").changed();
                    changed |= ui.checkbox(&mut permissions.selection,"Selección").changed();
                    changed |= ui.checkbox(&mut permissions.transport,"Transporte").changed();
                    changed |= ui.checkbox(&mut permissions.jobs,"Trabajos").changed();
                });
                ui.collapsing("Aplicación automática: alcance autorizado en esta sesión",|ui|{
                    ui.label("Las operaciones marcadas pueden aplicarse después del preview sin aprobación individual. Siguen sujetas a revisión/digest y protección de decisiones humanas. Batch exige autorizar también cada operación que contiene.");
                    if ui.button("Revocar todo el alcance automático").clicked(){permissions.automatic_commands.clear();changed=true;}
                    static TYPES:std::sync::OnceLock<Vec<String>>=std::sync::OnceLock::new();
                    egui::ScrollArea::vertical().max_height(180.0).show(ui,|ui|{
                        for kind in TYPES.get_or_init(Permissions::automatic_command_types){
                            let mut selected=permissions.automatic_commands.contains(kind);
                            if ui.checkbox(&mut selected,kind).changed(){if selected{permissions.automatic_commands.insert(kind.clone());}else{permissions.automatic_commands.remove(kind);}changed=true;}
                        }
                    });
                });
                if changed { engine.set_permissions(permissions); }
                egui::ScrollArea::vertical().max_height(290.0).show(ui,|ui| {
                    for proposal in engine.proposals() {
                        let stale = (proposal.base_revision != app.session.revision() || proposal.needs_revalidation) && proposal.receipt.is_none();
                        ui.collapsing(format!("{} · revisión {} · {}",proposal.id,proposal.base_revision,if proposal.receipt.is_some(){"aplicada"}else if stale{"obsoleta"}else if proposal.rejected{"rechazada"}else if proposal.reviewed{"revisada"}else{"pendiente"}),|ui| {
                            ui.monospace(serde_json::to_string_pretty(&proposal.command).unwrap_or_default());
                            ui.monospace(serde_json::to_string_pretty(&proposal.diff).unwrap_or_default());
                            ui.label(format!("Digest de preview: {}",proposal.preview_digest));
                            if proposal.automatic_eligible{ui.label("Dentro del alcance automático autorizado localmente");}
                            if stale && ui.button("Repreparar sobre la revisión actual").clicked(){
                                match app.session.digest(){Ok(digest)=>state.pending_json=Some(json!({"jsonrpc":"2.0","id":"local-reprepare","method":"tools/call","params":{"name":"tv2_reprepare","arguments":{"session_id":engine.session_id(),"project_id":app.project().project_id,"proposal_id":proposal.id,"revision":app.session.revision(),"digest":digest,"idempotency_key":format!("reprepare-{}",tv2_domain::ids::random_hex12())}}})),Err(e)=>state.error=Some(e.to_string())}
                            }
                            for (label,approve) in [("Aprobar esta propuesta",true),("Rechazar",false)] {
                                if ui.add_enabled(!stale && proposal.receipt.is_none(),egui::Button::new(label)).clicked()
                                    && let Err(e) = engine.review(&proposal.id,approve,&app.session) { state.error=Some(e); }
                            }
                        });
                    }
                });
                ui.collapsing("Requests/respuestas JSON (mismo protocolo)",|ui| {
                    ui.add(egui::TextEdit::multiline(&mut state.request).desired_rows(8).code_editor());
                    if ui.button("Procesar request JSON").clicked() { match serde_json::from_str(&state.request) { Ok(value)=>state.pending_json=Some(value),Err(e)=>state.error=Some(e.to_string()) } }
                    if ui.button("Importar request JSON…").clicked() && let Some(path)=rfd::FileDialog::new().add_filter("JSON",&["json"]).pick_file() {
                        let(tx,rx)=crossbeam_channel::bounded(1);let wake=ctx.clone();
                        match std::thread::Builder::new().name("mcp-json-read".into()).spawn(move||{
                            let work=||->Result<String,String>{use std::io::Read;let mut text=String::new();std::fs::File::open(path).map_err(|e|e.to_string())?.take(1024*1024+1).read_to_string(&mut text).map_err(|e|e.to_string())?;if text.len()>1024*1024{return Err("Request supera 1 MiB".into());}Ok(text)};
                            let _=tx.send(work());wake.request_repaint();
                        }){Ok(_)=>state.import=Some(rx),Err(e)=>state.error=Some(e.to_string())}
                    }
                    ui.add(egui::TextEdit::multiline(&mut state.response).desired_rows(8).code_editor());
                    if ui.button("Copiar respuesta").clicked() { ctx.copy_text(state.response.clone()); }
                });
                ui.collapsing("Eventos de la sesión",|ui| { egui::ScrollArea::vertical().max_height(180.0).show(ui,|ui| {
                    for event in engine.events().iter().rev().take(100) { ui.label(format!("{} · {} · {}",event.sequence,event.kind,event.detail)); }
                }); });
            }
            if let Some(error)=&state.error { ui.colored_label(egui::Color32::YELLOW,error); }
        });
    }
    app.control = state;
    if app.control.pending_json.is_some() {
        ctx.request_repaint();
    }
}
