//! Bounded, read-only browsing of immutable conversation evidence.
use crate::app::{TranscriptorApp, ViewMode};
use serde_json::Value;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tv2_domain::{AssetId, ClipId, Ticks, TimeRange, evidence::EvidenceDocument};

const PAGE_SIZE: usize = 64;
const TEXT_CHARS: usize = 4000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Conversation,
    Utterances,
    Words,
}
impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Conversation => "Conversación limpia",
            Self::Utterances => "Todas las intervenciones",
            Self::Words => "Palabras",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RowKey {
    start: Ticks,
    track: String,
    id: String,
    index: usize,
}
#[derive(Clone, Debug)]
struct Row {
    key: RowKey,
    range: TimeRange,
    text: String,
    truncated: bool,
    overlap: Option<String>,
    duplicate: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Query {
    project: String,
    revision: u64,
    asset: AssetId,
    search: String,
    track: String,
    mode: Mode,
    from: Option<Ticks>,
    to: Option<Ticks>,
    after: Option<RowKey>,
}
#[derive(Default)]
struct Page {
    rows: Vec<Row>,
    total: usize,
    remaining: usize,
    invalid: usize,
    tracks: Vec<(String, String)>,
}
struct Pending {
    rx: crossbeam_channel::Receiver<Option<Page>>,
    cancel: Arc<AtomicBool>,
}
#[derive(Clone)]
struct OccurrenceClip {
    id: ClipId,
    source: TimeRange,
    position: Ticks,
    group: Option<String>,
    audio: Option<u32>,
}
type OccurrenceKey = (Ticks, ClipId);
#[derive(Default)]
struct OccurrencePage {
    rows: Vec<(OccurrenceKey, Option<u32>)>,
    total: usize,
    remaining: usize,
}
struct OccurrenceView {
    open: bool,
    project: String,
    revision: u64,
    sequence: tv2_domain::SequenceId,
    source: Ticks,
    clips: Arc<Vec<OccurrenceClip>>,
    page: OccurrencePage,
    cursors: Vec<Option<OccurrenceKey>>,
    pending: Option<(crossbeam_channel::Receiver<Option<OccurrencePage>>, Arc<AtomicBool>)>,
    error: Option<String>,
}
impl Drop for OccurrenceView {
    fn drop(&mut self) {
        if let Some((_, cancel)) = &self.pending {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
fn occurrence_page(clips: &[OccurrenceClip], source: Ticks, after: Option<&OccurrenceKey>, cancel: &AtomicBool) -> Option<OccurrencePage> {
    let mut groups = std::collections::BTreeMap::new();
    for clip in clips {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        if !clip.source.contains(source) {
            continue;
        }
        // Only linked clips with identical complete mappings are one occurrence.
        let group = (clip.group.clone(), clip.group.is_none().then(|| clip.id.clone()), clip.source.start, clip.source.end, clip.position);
        let chosen = groups.entry(group).or_insert(clip);
        if (clip.audio.is_some(), &clip.id) < (chosen.audio.is_some(), &chosen.id) {
            *chosen = clip;
        }
    }
    let mut page = OccurrencePage { total: groups.len(), ..Default::default() };
    for clip in groups.values() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let key = (clip.position + (source - clip.source.start), clip.id.clone());
        if after.is_some_and(|after| &key <= after) {
            continue;
        }
        page.remaining += 1;
        let position = page.rows.binary_search_by(|(other, _)| other.cmp(&key)).unwrap_or_else(|index| index);
        if position < PAGE_SIZE {
            page.rows.insert(position, (key, clip.audio));
            page.rows.truncate(PAGE_SIZE);
        }
    }
    Some(page)
}
impl OccurrenceView {
    fn new(app: &TranscriptorApp, asset: &AssetId, source: Ticks) -> Option<Self> {
        let sequence = app.sequence()?;
        let clips = sequence
            .clips
            .iter()
            .filter(|clip| clip.enabled && &clip.asset_id == asset)
            .map(|clip| OccurrenceClip {
                id: clip.id.clone(),
                source: clip.source,
                position: clip.position,
                group: clip.link_group.clone(),
                audio: clip.audio_stream,
            })
            .collect();
        Some(Self {
            open: true,
            project: app.project().project_id.clone(),
            revision: app.project().revision,
            sequence: sequence.id.clone(),
            source,
            clips: Arc::new(clips),
            page: OccurrencePage::default(),
            cursors: vec![None],
            pending: None,
            error: None,
        })
    }
    fn request(&mut self, ctx: &egui::Context) {
        if let Some((_, cancel)) = self.pending.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.page = OccurrencePage::default();
        let clips = self.clips.clone();
        let source = self.source;
        let after = self.cursors.last().cloned().flatten();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let wake = ctx.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("conversation-occurrences".into()).spawn(move || {
            let page = occurrence_page(&clips, source, after.as_ref(), &worker_cancel);
            let _ = tx.send(page);
            wake.request_repaint();
        }) {
            Ok(_) => {
                self.pending = Some((rx, cancel));
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

pub struct ConversationUi {
    pub open: bool,
    asset: Option<AssetId>,
    mode: Mode,
    search: String,
    track: String,
    from: String,
    to: String,
    query: Option<Query>,
    document: Option<EvidenceDocument>,
    pending: Option<Pending>,
    page: Page,
    cursors: Vec<Option<RowKey>>,
    changed: Instant,
    error: Option<String>,
    occurrences: Option<OccurrenceView>,
}
impl Default for ConversationUi {
    fn default() -> Self {
        Self {
            open: false,
            asset: None,
            mode: Mode::default(),
            search: String::new(),
            track: String::new(),
            from: String::new(),
            to: String::new(),
            query: None,
            document: None,
            pending: None,
            page: Page::default(),
            cursors: vec![None],
            changed: Instant::now(),
            error: None,
            occurrences: None,
        }
    }
}
impl Drop for ConversationUi {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }
}
fn seconds(text: &str) -> Result<Option<Ticks>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = text.trim().replace(',', ".").parse::<f64>().map_err(|_| "Los límites deben ser segundos numéricos".to_string())?;
    if !value.is_finite() || value < 0.0 || value > Ticks::MAX.as_seconds_f64() {
        return Err("Límite temporal fuera de rango".into());
    }
    Ok(Some(Ticks::from_seconds_f64(value)))
}
fn record_range(record: &Value, duration: Ticks) -> Option<TimeRange> {
    let start = record.get("t_ini")?.as_f64()?;
    let end = record.get("t_fin")?.as_f64()?;
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || end > duration.as_seconds_f64() {
        return None;
    }
    Some(TimeRange::new(Ticks::from_seconds_f64(start), Ticks::from_seconds_f64(end)))
}
fn query_page(document: &Value, duration: Ticks, query: &Query, cancel: &AtomicBool) -> Option<Page> {
    let mut page = Page::default();
    let tracks = document.get("tracks").and_then(Value::as_object)?;
    page.tracks = tracks.iter().map(|(id, track)| (id.clone(), track["label"].as_str().unwrap_or(id).to_string())).collect();
    let search = query.search.trim().to_lowercase();
    let mut clean = None;
    if query.mode == Mode::Conversation
        && let Some(ids) = document.pointer("/conversation/clean_utterance_ids").and_then(Value::as_array)
    {
        let mut selected = HashSet::new();
        for id in ids {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(id) = id.as_str() {
                selected.insert(id);
            }
        }
        clean = Some(selected);
    }
    let mut visit = |record: &Value, track: &str, index: usize, id_field: &str| {
        if !query.track.is_empty() && query.track != track {
            return;
        }
        let Some(id) = record.get(id_field).and_then(Value::as_str) else {
            page.invalid += 1;
            return;
        };
        if query.mode == Mode::Conversation && clean.as_ref().is_some_and(|ids| !ids.contains(id)) {
            return;
        }
        let Some(range) = record_range(record, duration) else {
            page.invalid += 1;
            return;
        };
        if query.from.is_some_and(|from| range.end <= from) || query.to.is_some_and(|to| range.start >= to) {
            return;
        }
        let text = record.get("text").or_else(|| record.get("word")).and_then(Value::as_str).unwrap_or("");
        if !search.is_empty()
            && !text.to_lowercase().contains(&search)
            && !id.to_lowercase().contains(&search)
            && !track.to_lowercase().contains(&search)
        {
            return;
        }
        page.total += 1;
        let key = RowKey { start: range.start, track: track.into(), id: id.into(), index };
        if query.after.as_ref().is_some_and(|after| key <= *after) {
            return;
        }
        page.remaining += 1;
        let position = page.rows.binary_search_by(|row| row.key.cmp(&key)).unwrap_or_else(|index| index);
        if position >= PAGE_SIZE {
            return;
        }
        let mut chars = text.chars();
        let text = chars.by_ref().take(TEXT_CHARS).collect();
        let truncated = chars.next().is_some();
        page.rows.insert(
            position,
            Row {
                key,
                range,
                text,
                truncated,
                overlap: record.get("overlap_group").and_then(Value::as_str).map(str::to_string),
                duplicate: record.get("duplicate_of").and_then(Value::as_str).map(str::to_string),
            },
        );
        page.rows.truncate(PAGE_SIZE);
    };
    if query.mode == Mode::Conversation
        && let Some(records) = document.pointer("/conversation/utterances").and_then(Value::as_array)
    {
        for (index, record) in records.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Some(track) = record.get("track_id").and_then(Value::as_str) else {
                continue;
            };
            visit(record, track, index, "utterance_id");
        }
    } else {
        let (collection, id_field) = if query.mode == Mode::Words { ("words", "word_id") } else { ("utterances", "utterance_id") };
        for (track, source) in tracks {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(records) = source.get(collection).and_then(Value::as_array) {
                for (index, record) in records.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        return None;
                    }
                    visit(record, track, index, id_field);
                }
            }
        }
    }
    Some(page)
}
enum Navigate {
    Source(AssetId, TimeRange),
    Sequence(ClipId, Ticks),
}

fn draw_occurrences(app: &TranscriptorApp, ctx: &egui::Context, state: &mut Option<OccurrenceView>) -> Option<Navigate> {
    let view = state.as_mut()?;
    let current = view.project == app.project().project_id
        && view.revision == app.project().revision
        && app.sequence().is_some_and(|sequence| sequence.id == view.sequence);
    if !current {
        if let Some((_, cancel)) = view.pending.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        view.page = OccurrencePage::default();
        view.error = Some("La secuencia cambió; vuelve a abrir las apariciones desde la intervención o palabra.".into());
    }
    if let Some((rx, _)) = &view.pending {
        match rx.try_recv() {
            Ok(Some(page)) => {
                view.page = page;
                view.pending = None;
            }
            Ok(None) => {
                view.pending = None;
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                view.error = Some("La búsqueda de apariciones terminó sin resultado".into());
                view.pending = None;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
    }
    let mut navigate = None;
    let mut previous = false;
    let mut next = false;
    egui::Window::new("Apariciones en la secuencia").open(&mut view.open).default_width(480.0).show(ctx, |ui| {
        ui.label(format!("Instante de fuente: {}", view.source.timecode_ms()));
        ui.label("Video y audio enlazados con el mismo intervalo aparecen una vez. El audio independiente conserva su aparición.");
        if let Some(error) = &view.error {
            ui.colored_label(egui::Color32::YELLOW, error);
        }
        if view.pending.is_some() {
            ui.spinner();
        }
        ui.horizontal(|ui| {
            previous = ui.add_enabled(current && view.pending.is_none() && view.cursors.len() > 1, egui::Button::new("Anterior")).clicked();
            ui.label(format!("Página {} · {} apariciones", view.cursors.len(), view.page.total));
            next = ui.add_enabled(current && view.pending.is_none() && view.page.remaining > PAGE_SIZE, egui::Button::new("Siguiente")).clicked();
        });
        if current && view.pending.is_none() && view.page.total == 0 {
            ui.label("Este instante de fuente no aparece en la secuencia activa.");
        }
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            for ((time, id), audio) in &view.page.rows {
                let track = audio.map(|stream| format!("audio {}", stream + 1)).unwrap_or_else(|| "video".into());
                if ui.button(format!("{} · {track} · {id}", time.timecode_ms())).clicked() {
                    navigate = Some(Navigate::Sequence(id.clone(), *time));
                }
            }
        });
    });
    if previous {
        view.cursors.pop();
        view.request(ctx);
    }
    if next && let Some((last, _)) = view.page.rows.last() {
        view.cursors.push(Some(last.clone()));
        view.request(ctx);
    }
    if !view.open {
        *state = None;
    }
    navigate
}

/// Requires `TranscriptorApp::conversation: ConversationUi`, initialized with Default.
pub fn draw(app: &mut TranscriptorApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.conversation);
    if !state.open {
        state.occurrences = None;
        if let Some(pending) = state.pending.take() {
            pending.cancel.store(true, Ordering::Relaxed);
            state.query = None;
        }
        app.conversation = state;
        return;
    }
    if state.query.as_ref().is_some_and(|query| query.project != app.project().project_id) {
        if let Some(pending) = state.pending.take() {
            pending.cancel.store(true, Ordering::Relaxed);
        }
        state.query = None;
        state.document = None;
        state.asset = None;
        state.cursors = vec![None];
        state.page = Page::default();
    }
    if state.asset.as_ref().is_none_or(|asset| !app.project().masters.iter().any(|master| &master.asset_id == asset)) {
        state.asset = app
            .current_layer_asset()
            .filter(|asset| app.project().masters.iter().any(|master| &master.asset_id == asset))
            .or_else(|| app.project().masters.first().map(|master| master.asset_id.clone()));
        state.track.clear();
        state.cursors = vec![None];
        state.page = Page::default();
    }
    let result_current = state.query.as_ref().is_some_and(|query| {
        Some(&query.asset) == state.asset.as_ref() && query.project == app.project().project_id && query.revision == app.project().revision
    }) && state.document.as_ref().is_some_and(|document| {
        app.project().masters.iter().any(|master| Some(&master.asset_id) == state.asset.as_ref() && document.shares_storage(&master.document))
    });
    let mut filters_changed = false;
    let mut navigate = None;
    let mut open_occurrences = None;
    let mut previous = false;
    let mut next = false;
    egui::Window::new("Conversación y transcripción").open(&mut state.open).default_width(780.0).default_height(600.0).show(ctx, |ui| {
        ui.label("Evidencia de fuente de solo lectura. Conversación limpia muestra las intervenciones seleccionadas en el análisis original.");
        if app.project().masters.is_empty() {
            ui.label("Importa una carpeta editorial con master para consultar su conversación.");
            return;
        }
        ui.horizontal(|ui| {
            let label = state.asset.as_ref().and_then(|id| app.project().asset(id)).map(|asset| asset.name.as_str()).unwrap_or("Medio");
            egui::ComboBox::from_id_salt("conversation_asset").selected_text(label).show_ui(ui, |ui| {
                for master in &app.project().masters {
                    if let Some(asset) = app.project().asset(&master.asset_id)
                        && ui.selectable_value(&mut state.asset, Some(asset.id.clone()), &asset.name).changed()
                    {
                        state.track.clear();
                        state.page.tracks.clear();
                        filters_changed = true;
                    }
                }
            });
            egui::ComboBox::from_id_salt("conversation_mode").selected_text(state.mode.label()).show_ui(ui, |ui| {
                for mode in [Mode::Conversation, Mode::Utterances, Mode::Words] {
                    filters_changed |= ui.selectable_value(&mut state.mode, mode, mode.label()).changed();
                }
            });
            egui::ComboBox::from_id_salt("conversation_track")
                .selected_text(if state.track.is_empty() { "Todas las pistas" } else { &state.track })
                .show_ui(ui, |ui| {
                    filters_changed |= ui.selectable_value(&mut state.track, String::new(), "Todas las pistas").changed();
                    for (id, label) in &state.page.tracks {
                        filters_changed |= ui.selectable_value(&mut state.track, id.clone(), format!("{id} · {label}")).changed();
                    }
                });
        });
        ui.horizontal(|ui| {
            filters_changed |= ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Buscar texto o ID").desired_width(280.0)).changed();
            ui.label("Fuente desde/hasta (s):");
            filters_changed |= ui.add(egui::TextEdit::singleline(&mut state.from).desired_width(65.0)).changed();
            filters_changed |= ui.add(egui::TextEdit::singleline(&mut state.to).desired_width(65.0)).changed();
        });
        if let Some(error) = &state.error {
            ui.colored_label(egui::Color32::YELLOW, error);
            if state.pending.is_none() && ui.button("Reintentar consulta").clicked() {
                state.query = None;
            }
        }
        if state.pending.is_some() {
            ui.spinner();
        }
        ui.horizontal(|ui| {
            previous = ui
                .add_enabled(result_current && !filters_changed && state.pending.is_none() && state.cursors.len() > 1, egui::Button::new("Anterior"))
                .clicked();
            ui.label(format!("Página {} · {} resultados · hasta {PAGE_SIZE} por página", state.cursors.len(), state.page.total));
            next = ui
                .add_enabled(
                    result_current && !filters_changed && state.pending.is_none() && state.page.remaining > PAGE_SIZE,
                    egui::Button::new("Siguiente"),
                )
                .clicked();
        });
        if state.page.invalid > 0 {
            ui.label(format!("{} registros sin ID/tiempos válidos no son navegables; el master permanece intacto.", state.page.invalid));
        }
        egui::ScrollArea::vertical().id_salt("conversation_rows").show(ui, |ui| {
            if filters_changed || !result_current {
                return;
            }
            for row in &state.page.rows {
                ui.push_id((&row.key.track, &row.key.id, row.key.index), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(format!("{}–{}", row.range.start.timecode_ms(), row.range.end.timecode_ms()))
                            .on_hover_text("Abrir este intervalo en Fuente")
                            .clicked()
                            && let Some(asset) = &state.asset
                        {
                            navigate = Some(Navigate::Source(asset.clone(), row.range));
                        }
                        ui.label(&row.key.track);
                        ui.monospace(&row.key.id);
                        if ui.button("En secuencia...").clicked()
                            && let Some(asset) = &state.asset
                        {
                            open_occurrences = Some((asset.clone(), row.range.start));
                        }
                    });
                    ui.label(&row.text);
                    if row.truncated {
                        ui.small("Vista abreviada a 4000 caracteres; la búsqueda consulta el texto completo.");
                    }
                    if let Some(overlap) = &row.overlap {
                        ui.small(format!("Solape: {overlap}"));
                    }
                    if let Some(duplicate) = &row.duplicate {
                        ui.small(format!("Duplicado de: {duplicate}"));
                    }
                    ui.separator();
                });
            }
        });
    });
    if !state.open {
        state.occurrences = None;
        if let Some(pending) = state.pending.take() {
            pending.cancel.store(true, Ordering::Relaxed);
            state.query = None;
        }
        app.conversation = state;
        return;
    }
    if filters_changed {
        state.cursors = vec![None];
        state.changed = Instant::now();
    }
    if previous {
        state.cursors.pop();
    }
    if next && let Some(last) = state.page.rows.last() {
        state.cursors.push(Some(last.key.clone()));
    }
    let limits = seconds(&state.from).and_then(|from| {
        seconds(&state.to).and_then(|to| {
            if from.zip(to).is_some_and(|(a, b)| a >= b) { Err("El límite final debe ser mayor que el inicial".into()) } else { Ok((from, to)) }
        })
    });
    if let (Some(asset), Ok((from, to))) = (&state.asset, &limits)
        && let Some(master) = app.project().masters.iter().find(|master| &master.asset_id == asset)
    {
        let query = Query {
            project: app.project().project_id.clone(),
            revision: app.project().revision,
            asset: asset.clone(),
            search: state.search.clone(),
            track: state.track.clone(),
            mode: state.mode,
            from: *from,
            to: *to,
            after: state.cursors.last().cloned().flatten(),
        };
        let changed =
            state.query.as_ref() != Some(&query) || state.document.as_ref().is_none_or(|document| !document.shares_storage(&master.document));
        if changed {
            if let Some(pending) = state.pending.take() {
                pending.cancel.store(true, Ordering::Relaxed);
            }
            state.page.rows.clear();
            state.page.total = 0;
            state.page.remaining = 0;
            if state.changed.elapsed() >= Duration::from_millis(200) {
                let document = master.document.clone();
                state.document = Some(document.clone());
                state.query = Some(query.clone());
                let duration = app.project().asset(asset).map(|asset| asset.duration()).unwrap_or(Ticks::ZERO);
                let cancel = Arc::new(AtomicBool::new(false));
                let worker_cancel = cancel.clone();
                let wake = ctx.clone();
                let (tx, rx) = crossbeam_channel::bounded(1);
                match std::thread::Builder::new().name("conversation-query".into()).spawn(move || {
                    let result = query_page(&document, duration, &query, &worker_cancel);
                    let _ = tx.send(result);
                    wake.request_repaint();
                }) {
                    Ok(_) => {
                        state.pending = Some(Pending { rx, cancel });
                        state.error = None;
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        if let Some(pending) = &state.pending {
            match pending.rx.try_recv() {
                Ok(Some(page)) => {
                    state.page = page;
                    state.pending = None;
                }
                Ok(None) => {
                    state.error = Some("El master no contiene pistas consultables o se canceló la consulta".into());
                    state.pending = None;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    state.error = Some("La consulta terminó sin resultado".into());
                    state.pending = None;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
        }
    } else if let Err(error) = limits {
        state.error = Some(error);
        if let Some(pending) = state.pending.take() {
            pending.cancel.store(true, Ordering::Relaxed);
        }
        state.page.rows.clear();
    }
    if let Some((asset, source)) = open_occurrences {
        state.occurrences = OccurrenceView::new(app, &asset, source);
        if let Some(view) = &mut state.occurrences {
            view.request(ctx);
        }
    }
    if let Some(action) = draw_occurrences(app, ctx, &mut state.occurrences) {
        navigate = Some(action);
    }
    if let Some(action) = navigate {
        match action {
            Navigate::Source(asset, range) => {
                app.player_send(tv2_media::player::PlayerCommand::Pause);
                app.selection.clips.clear();
                app.view = ViewMode::Source;
                app.set_source_asset(asset);
                app.in_point = Some(range.start);
                app.out_point = Some(range.end);
                app.seek(range.start);
                app.timeline_view.center_on(range.start);
            }
            Navigate::Sequence(clip, time) => app.reveal_occurrence(clip, time),
        }
    }
    app.conversation = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occurrence_pages_keep_independent_audio_and_deduplicate_only_linked_twins() {
        let source = TimeRange::new(Ticks::ZERO, Ticks::from_seconds(3));
        let mut clips = vec![];
        for index in 0..70 {
            for audio in [None, Some(0)] {
                clips.push(OccurrenceClip {
                    id: ClipId::new(format!("clip-{index:03}-{}", audio.is_some())),
                    source,
                    position: Ticks::from_seconds(index * 3),
                    group: Some(format!("group-{index}")),
                    audio,
                });
            }
        }
        clips.push(OccurrenceClip { id: "independent-audio".into(), source, position: Ticks::from_seconds(1), group: None, audio: Some(1) });
        let cancel = AtomicBool::new(false);
        let page = occurrence_page(&clips, Ticks::from_seconds(1), None, &cancel).unwrap();
        assert_eq!(page.total, 71);
        assert_eq!(page.rows.len(), 64);
        assert!(page.rows.iter().any(|((_, id), audio)| id.as_str() == "independent-audio" && *audio == Some(1)));
        assert!(page.rows.iter().filter(|((_, id), _)| id.as_str() != "independent-audio").all(|(_, audio)| audio.is_none()));
        let next = occurrence_page(&clips, Ticks::from_seconds(1), Some(&page.rows.last().unwrap().0), &cancel).unwrap();
        assert_eq!(next.rows.len(), 7);
        assert_eq!(occurrence_page(&clips, Ticks::from_seconds(3), None, &cancel).unwrap().total, 0);
        cancel.store(true, Ordering::Relaxed);
        assert!(occurrence_page(&clips, Ticks::ZERO, None, &cancel).is_none());
    }
    fn query() -> Query {
        Query {
            project: "p".into(),
            revision: 1,
            asset: "a".into(),
            search: String::new(),
            track: String::new(),
            mode: Mode::Conversation,
            from: None,
            to: None,
            after: None,
        }
    }
    #[test]
    fn merged_pages_keep_source_ids_and_search_all_text() {
        let records:Vec<_>=(0..150).rev().map(|n|serde_json::json!({"utterance_id":format!("u-{n:04}"),"track_id":if n%2==0{"A"}else{"B"},"t_ini":n,"t_fin":n+1,"text":format!("text {n}")})).collect();
        let ids: Vec<_> = records.iter().map(|record| record["utterance_id"].clone()).collect();
        let document = serde_json::json!({"tracks":{"A":{"label":"first"},"B":{"label":"second"}},"conversation":{"utterances":records,"clean_utterance_ids":ids}});
        let cancel = AtomicBool::new(false);
        let mut query = query();
        let page = query_page(&document, Ticks::from_seconds(200), &query, &cancel).unwrap();
        assert_eq!(page.total, 150);
        assert_eq!(page.rows.len(), PAGE_SIZE);
        assert_eq!(page.rows[0].key.id, "u-0000");
        query.after = Some(page.rows.last().unwrap().key.clone());
        let next = query_page(&document, Ticks::from_seconds(200), &query, &cancel).unwrap();
        assert_eq!(next.rows[0].key.id, "u-0064");
        query.after = None;
        query.track = "B".into();
        query.search = "text 1".into();
        let filtered = query_page(&document, Ticks::from_seconds(200), &query, &cancel).unwrap();
        assert!(filtered.rows.iter().all(|row| row.key.track == "B" && row.text.contains("text 1")));
        cancel.store(true, Ordering::Relaxed);
        assert!(query_page(&document, Ticks::from_seconds(200), &query, &cancel).is_none());
    }
    #[test]
    fn clean_selection_and_word_times_remain_readonly() {
        let record = serde_json::json!({"utterance_id":"u-1","word_id":"w-1","track_id":"A","t_ini":2,"t_fin":3,"text":"original"});
        let document = serde_json::json!({"tracks":{"A":{"words":[record.clone()],"utterances":[record.clone()]}},"conversation":{"utterances":[record],"clean_utterance_ids":[]}});
        let mut query = query();
        let cancel = AtomicBool::new(false);
        assert_eq!(query_page(&document, Ticks::from_seconds(5), &query, &cancel).unwrap().total, 0);
        query.mode = Mode::Words;
        let page = query_page(&document, Ticks::from_seconds(5), &query, &cancel).unwrap();
        assert_eq!(page.rows[0].key.id, "w-1");
        assert_eq!(page.rows[0].range.start, Ticks::from_seconds(2));
        assert!(seconds("NaN").is_err());
        assert!(seconds("-1").is_err());
    }
}
