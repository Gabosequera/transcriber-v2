use crate::app::{Severity, TranscriptorApp};
use tv2_domain::{Asset, Command, DomainError, LayerKind, Project, error::DomainResult};
use tv2_v1compat::author_candidates::{self, Candidate};

pub struct AuthorReview {
    project: String,
    revision: u64,
    asset: Asset,
    candidates: Vec<Candidate>,
    selected: Option<usize>,
    content: String,
    current: String,
}
pub struct AuthorScan {
    result: crossbeam_channel::Receiver<DomainResult<AuthorReview>>,
}
impl AuthorReview {
    pub fn new(project: &Project, asset: Asset, candidates: Vec<Candidate>) -> Self {
        let selected = author_candidates::preferred(&candidates).ok().flatten();
        let current = project
            .layers
            .iter()
            .find(|l| l.asset_id == asset.id && l.kind == LayerKind::Author && !l.deleted)
            .and_then(|l| tv2_v1compat::author::to_v1(l, &asset).ok())
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| "Sin colección actual".into());
        Self { project: project.project_id.clone(), revision: project.revision, asset, candidates, selected, content: String::new(), current }
    }
}
impl TranscriptorApp {
    pub fn scan_author(&mut self, explicit_file: bool) {
        if self.author_scan.is_some() || self.semantic_job.is_some() {
            self.toast(Severity::Warn, "Hay una lectura editorial en curso");
            return;
        }
        let Some(asset) = self.current_layer_asset().and_then(|id| self.project().asset(&id)).cloned() else {
            self.toast(Severity::Warn, "Selecciona un medio");
            return;
        };
        let file = if explicit_file {
            let Some(p) = rfd::FileDialog::new().set_title("Candidato de marcas V1").add_filter("JSON", &["json"]).pick_file() else {
                return;
            };
            Some(p)
        } else {
            None
        };
        let project = self.project().clone();
        let stores = crate::paths::v1_marks_stores();
        let (tx, result) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("author-candidates".into()).spawn(move || {
            let work = || -> DomainResult<AuthorReview> {
                let mut paths = author_candidates::paths(&asset, None, &stores)?;
                if let Some(p) = file {
                    paths.push(p);
                }
                paths.sort();
                paths.dedup();
                let candidates = author_candidates::discover(&paths, &asset);
                Ok(AuthorReview::new(&project, asset, candidates))
            };
            let _ = tx.send(work());
        }) {
            Ok(_) => self.author_scan = Some(AuthorScan { result }),
            Err(e) => self.report(e.into()),
        }
    }
    pub fn poll_author(&mut self) {
        let Some(scan) = &self.author_scan else {
            return;
        };
        let result = match scan.result.try_recv() {
            Ok(r) => r,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(_) => Err(DomainError::process("Lectura de marcas terminó sin resultado")),
        };
        self.author_scan = None;
        match result {
            Ok(review) => self.author_review = Some(review),
            Err(e) => self.report(e),
        }
    }
}

pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let Some(mut review) = app.author_review.take() else {
        return;
    };
    let stale = review.project != app.project().project_id || review.revision != app.session.revision();
    let mut open = true;
    let mut apply = false;
    egui::Window::new("Resolver marcas del autor").open(&mut open).default_width(780.0).show(ctx, |ui| {
        ui.label("Revisa las fuentes y el contenido antes de adoptar una colección. Deshacer conserva la colección anterior.");
        if stale {
            ui.colored_label(egui::Color32::YELLOW, "El proyecto cambió. Cierra y vuelve a buscar candidatos.");
        }
        if review.candidates.is_empty() {
            ui.label("No se encontraron marcas junto al medio ni en el almacén V1 configurado.");
        }
        for (index, candidate) in review.candidates.iter().enumerate() {
            match &candidate.layer {
                Ok(layer) => {
                    ui.radio_value(
                        &mut review.selected,
                        Some(index),
                        format!("{} · rev {} · {} marcas", candidate.path.display(), layer.revision, layer.items.len()),
                    );
                }
                Err(error) => {
                    ui.colored_label(egui::Color32::YELLOW, format!("{}: {error}", candidate.path.display()));
                }
            }
        }
        if let Some(index) = review.selected
            && let Ok(layer) = &review.candidates[index].layer
        {
            if review.content.is_empty() || ui.memory(|m| m.data.get_temp::<usize>(egui::Id::new("author-choice"))) != Some(index) {
                review.content =
                    tv2_v1compat::author::to_v1(layer, &review.asset).ok().and_then(|v| serde_json::to_string_pretty(&v).ok()).unwrap_or_default();
                ui.memory_mut(|m| m.data.insert_temp(egui::Id::new("author-choice"), index));
            }
            ui.columns(2, |cols| {
                cols[0].label("Colección actual");
                cols[1].label("Candidato (incluye cuarentena y metadatos)");
                egui::ScrollArea::both().id_salt("author-before").max_height(360.0).show(&mut cols[0], |ui| {
                    ui.monospace(&review.current);
                });
                egui::ScrollArea::both().id_salt("author-after").max_height(360.0).show(&mut cols[1], |ui| {
                    ui.monospace(&review.content);
                });
            });
            apply = ui.add_enabled(!stale && app.semantic_job.is_none(), egui::Button::new("Adoptar este contenido explícitamente")).clicked();
        }
    });
    if apply {
        let candidate = review.candidates[review.selected.unwrap()].clone();
        let project = app.project().clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        let launch = std::thread::Builder::new().name("author-adopt".into()).spawn(move || {
            let work = || -> DomainResult<tv2_application::session::PreparedCommand> {
                let (digest, layer) = author_candidates::read(&candidate.path, &review.asset)?;
                if Some(digest) != candidate.digest {
                    return Err(DomainError::precondition("El candidato cambió en disco; vuelve a revisarlo"));
                }
                let existing = project.layers.iter().find(|l| l.asset_id == review.asset.id && l.kind == LayerKind::Author && !l.deleted);
                let layer = author_candidates::adopt(&layer, existing)?;
                // The person explicitly selected and reviewed this content. Automatic imports remain External.
                let request = tv2_application::CommandEnvelope::human(Command::ReplaceLayer { layer }).with_base(project.revision);
                tv2_application::ProjectSession::new(project).prepare_command(request)
            };
            let _ = tx.send(work());
        });
        match launch {
            Ok(_) => app.semantic_job = Some(crate::semantic_jobs::SemanticJob { result }),
            Err(e) => app.report(e.into()),
        }
    } else if open {
        app.author_review = Some(review);
    }
}
