//! One lazy, bounded inspector query. Cached rows are shared between frames;
//! changing the selected item/revision never displays the previous result.
use crate::app::TranscriptorApp;
use std::sync::Arc;
use tv2_domain::{ItemId, LayerId, Sequence, SequenceId, timeline::SourceOccurrence};

type Key = (String, u64, SequenceId, LayerId, ItemId, u64);
type Rows = Arc<Vec<SourceOccurrence>>;
#[derive(Clone, Default)]
struct Query {
    pending: Option<(Key, crossbeam_channel::Receiver<Rows>)>,
    ready: Option<(Key, Rows)>,
    error: Option<(Key, String)>,
}
impl Query {
    fn poll(&mut self, wanted: &Key) {
        let Some((key, receiver)) = &self.pending else { return };
        let result = match receiver.try_recv() {
            Ok(rows) => Ok(rows),
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(_) => Err("El worker de ocurrencias terminó sin resultado".into()),
        };
        if key == wanted {
            match result {
                Ok(rows) => self.ready = Some((key.clone(), rows)),
                Err(error) => self.error = Some((key.clone(), error)),
            }
        }
        self.pending = None;
    }
}

pub fn draw(app: &mut TranscriptorApp, ui: &mut egui::Ui, layer: &LayerId, item: &ItemId) {
    let Some(sequence) = app.sequence() else {
        ui.label("Sin secuencia activa");
        return;
    };
    let wanted = (app.project().project_id.clone(), app.session.revision(), sequence.id.clone(), layer.clone(), item.clone(), app.inspector_epoch);
    // A single slot bounds concurrency even while selection changes rapidly.
    let slot = egui::Id::new("inspector-occurrences-worker");
    let mut query = ui.ctx().data_mut(|data| data.get_temp::<Query>(slot)).unwrap_or_default();
    query.poll(&wanted);
    if query.ready.as_ref().is_some_and(|(key, _)| key != &wanted) {
        query.ready = None;
    }
    if query.error.as_ref().is_some_and(|(key, _)| key != &wanted) {
        query.error = None;
    }
    if query.pending.is_none() && query.ready.is_none() && query.error.is_none() {
        // Earlier inspector controls can commit in this same frame. Read the
        // ranges from the revision used by the query, not its prior UI snapshot.
        let Some(layer) = app.project().layer(layer) else { return };
        let Some(item) = layer.item(item) else { return };
        let asset = &layer.asset_id;
        // Copy only mapping fields once per query, never imported clip evidence.
        let mut snapshot = Sequence::new("consulta", sequence.frame_rate, sequence.width, sequence.height, sequence.sample_rate);
        snapshot.clips =
            sequence.clips.iter().filter(|clip| clip.enabled && &clip.asset_id == asset).map(crate::timeline_index::paint_clip).collect();
        let asset = asset.clone();
        let ranges = item.ranges.clone();
        let wake = ui.ctx().clone();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("inspector-occurrences".into()).spawn(move || {
            let mut rows: Vec<_> = ranges.into_iter().flat_map(|range| snapshot.range_occurrences(&asset, range)).collect();
            rows.sort_by_key(|row| (row.sequence.start, row.clip_id.clone()));
            let _ = sender.send(Arc::new(rows));
            wake.request_repaint();
        }) {
            Ok(_) => query.pending = Some((wanted.clone(), receiver)),
            Err(error) => query.error = Some((wanted.clone(), error.to_string())),
        }
    }
    if let Some((_, rows)) = &query.ready {
        ui.label(format!("{} ocurrencias", rows.len()));
        egui::ScrollArea::vertical().max_height(200.0).show_rows(ui, 22.0, rows.len(), |ui, visible| {
            for row in visible {
                let occurrence = &rows[row];
                if ui.button(format!("{} · {}", occurrence.sequence.start.timecode_ms(), occurrence.clip_id)).clicked() {
                    app.reveal_occurrence(occurrence.clip_id.clone(), occurrence.sequence.start);
                }
            }
        });
    } else if let Some((_, error)) = &query.error {
        ui.label(error);
        if ui.button("Reintentar ocurrencias").clicked() {
            query.error = None;
            ui.ctx().request_repaint();
        }
    } else {
        ui.spinner();
        ui.label("Calculando ocurrencias…");
    }
    ui.ctx().data_mut(|data| data.insert_temp(slot, query));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_results_never_replace_the_current_selection() {
        let old = ("project".into(), 1, SequenceId::new("sequence"), LayerId::new("layer"), ItemId::new("old"), 0);
        let mut current = old.clone();
        current.4 = ItemId::new("new");
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let mut query = Query { pending: Some((old, receiver)), ..Default::default() };
        sender.send(Arc::new(vec![])).unwrap();
        query.poll(&current);
        assert!(query.pending.is_none());
        assert!(query.ready.is_none());
        assert!(query.error.is_none());
    }

    #[test]
    fn reopened_copy_with_identical_ids_and_revision_rejects_old_results() {
        let old = ("project".into(), 1, SequenceId::new("sequence"), LayerId::new("layer"), ItemId::new("item"), 0);
        let mut current = old.clone();
        current.5 = 1;
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let mut query = Query { pending: Some((old, receiver)), ..Default::default() };
        sender.send(Arc::new(vec![])).unwrap();
        query.poll(&current);
        assert!(query.pending.is_none());
        assert!(query.ready.is_none());
    }
}
