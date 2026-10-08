//! Índice de intervalos de clips, reconstruido únicamente al cambiar la revisión.
use std::collections::HashMap;
use tv2_domain::{
    ids::{AssetId, LayerId, SequenceId, TrackId},
    time::{Ticks, TimeRange},
    timeline::{Clip, Sequence, Track},
};

struct Node {
    clip: usize,
    start: Ticks,
    end: Ticks,
    max_end: Ticks,
    left: Option<usize>,
    right: Option<usize>,
}

#[derive(Default)]
struct TrackIndex {
    nodes: Vec<Node>,
    root: Option<usize>,
}

#[derive(Clone, Copy)]
pub struct ItemOccurrence {
    pub item: usize,
    pub range_index: usize,
    pub range: TimeRange,
}

pub enum SemanticPaint {
    Exact(Vec<ItemOccurrence>),
    /// Dense geometry is represented by covered pixel intervals, separately
    /// for three states and selected/unselected. The model and hit tests stay exact.
    Dense(Vec<PaintCoverage>),
}
pub struct PaintCoverage {
    pub range: TimeRange,
    pub kind: usize,
}

struct PaintAccumulator {
    range: TimeRange,
    columns: usize,
    exact: Vec<ItemOccurrence>,
    count: usize,
    coverage: [Vec<i64>; 6],
}
impl PaintAccumulator {
    fn new(range: TimeRange, columns: usize) -> Self {
        let columns = columns.clamp(1, 16384);
        Self { range, columns, exact: Vec::new(), count: 0, coverage: std::array::from_fn(|_| vec![0; columns + 1]) }
    }
    fn add(&mut self, occurrence: ItemOccurrence, kind: usize) {
        let Some(coverage) = self.coverage.get_mut(kind) else { return };
        let duration = self.range.duration().0.max(1) as i128;
        let column = |tick: Ticks, round_up: bool| {
            let numerator = (tick.0 as i128 - self.range.start.0 as i128).clamp(0, duration) * self.columns as i128;
            ((numerator + if round_up { duration - 1 } else { 0 }) / duration) as usize
        };
        let start = column(occurrence.range.start, false).min(self.columns - 1);
        let end = column(occurrence.range.end, true).max(start + 1).min(self.columns);
        coverage[start] += 1;
        coverage[end] -= 1;
        self.count += 1;
        if self.count <= self.columns * 4 {
            self.exact.push(occurrence);
        } else {
            self.exact.clear();
        }
    }
    fn finish(mut self) -> SemanticPaint {
        if self.count <= self.columns * 4 {
            self.exact.sort_by_key(|o| (o.range.start, o.item, o.range_index));
            self.exact.dedup_by_key(|o| (o.item, o.range_index, o.range));
            return SemanticPaint::Exact(self.exact);
        }
        let tick =
            |column: usize| Ticks((self.range.start.0 as i128 + self.range.duration().0 as i128 * column as i128 / self.columns as i128) as i64);
        let mut out = Vec::new();
        for (kind, coverage) in self.coverage.iter().enumerate() {
            let (mut active, mut start) = (0, None);
            for (column, difference) in coverage.iter().enumerate() {
                active += difference;
                match (start, active > 0) {
                    (None, true) => start = Some(column),
                    (Some(first), false) => {
                        out.push(PaintCoverage { range: TimeRange::new(tick(first), tick(column)), kind });
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        SemanticPaint::Dense(out)
    }
}

#[derive(Default)]
pub struct SemanticIndex {
    stamp: Option<(String, u64, crate::app::ViewMode, Option<tv2_domain::ids::AssetId>)>,
    layers: HashMap<LayerId, AssetId>,
    source_layers: HashMap<LayerId, (Vec<ItemOccurrence>, TrackIndex)>,
    visible_layers: HashMap<AssetId, Vec<(LayerId, String)>>,
    projections: HashMap<AssetId, (Vec<(TimeRange, Ticks)>, TrackIndex)>,
    source_view: bool,
    pending: Option<crossbeam_channel::Receiver<SemanticIndex>>,
    failed: Option<(SemanticStamp, String)>,
}
type SemanticStamp = (String, u64, crate::app::ViewMode, Option<AssetId>);
struct LayerInput {
    id: LayerId,
    asset: AssetId,
    ranges: Vec<ItemOccurrence>,
}
struct SemanticInput {
    stamp: SemanticStamp,
    layers: Vec<LayerInput>,
    visible: HashMap<AssetId, Vec<(LayerId, String)>>,
    clips: HashMap<AssetId, Vec<(TimeRange, Ticks)>>,
}

impl SemanticIndex {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.failed.as_ref().map(|(_, error)| error.as_str())
    }
    pub fn retry(&mut self) {
        self.failed = None;
    }
    pub fn ensure(&mut self, app: &crate::app::TranscriptorApp, ctx: &egui::Context) {
        let stamp = (app.project().project_id.clone(), app.session.revision(), app.view, app.source_asset.clone());
        if self.stamp.as_ref() == Some(&stamp) {
            return;
        }
        // Occurrences contain Vec indexes, so an older revision must never be
        // used against the current layers, including while the worker runs.
        if self.stamp.take().is_some() {
            self.layers.clear();
            self.source_layers.clear();
            self.visible_layers.clear();
            self.projections.clear();
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    if result.stamp.as_ref() == Some(&stamp) {
                        *self = result;
                        return;
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return,
                Err(_) => {
                    self.pending = None;
                    self.failed = Some((stamp.clone(), "El worker de índices terminó sin resultado".into()));
                }
            }
        }
        if self.failed.as_ref().is_some_and(|(base, _)| base == &stamp) {
            return;
        }
        self.failed = None;
        // Only geometry is copied once; no master, evidence, labels or project
        // history is cloned. New revisions coalesce while one worker is active.
        let mut input = SemanticInput { stamp: stamp.clone(), layers: vec![], visible: HashMap::new(), clips: HashMap::new() };
        for layer in app.project().ordered_layers().into_iter().filter(|layer| layer.visible) {
            input.visible.entry(layer.asset_id.clone()).or_default().push((layer.layer_id.clone(), layer.color.clone()));
        }
        for layer in app.project().layers.iter().filter(|layer| !layer.deleted && layer.visible) {
            let ranges = layer
                .items
                .iter()
                .enumerate()
                .flat_map(|(item, value)| {
                    value.ranges.iter().enumerate().map(move |(range_index, range)| ItemOccurrence { item, range_index, range: *range })
                })
                .collect();
            input.layers.push(LayerInput { id: layer.layer_id.clone(), asset: layer.asset_id.clone(), ranges });
        }
        if app.view == crate::app::ViewMode::Sequence
            && let Some(sequence) = app.sequence()
        {
            for clip in sequence.clips.iter().filter(|clip| clip.enabled) {
                input.clips.entry(clip.asset_id.clone()).or_default().push((clip.source, clip.position));
            }
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        match std::thread::Builder::new().name("semantic-index".into()).spawn(move || {
            let _ = tx.send(Self::build(input));
            wake.request_repaint();
        }) {
            Ok(_) => self.pending = Some(rx),
            Err(error) => self.failed = Some((stamp, error.to_string())),
        }
    }
    fn build(input: SemanticInput) -> Self {
        let mut result = Self { visible_layers: input.visible, source_view: input.stamp.2 == crate::app::ViewMode::Source, ..Default::default() };
        for (asset, spans) in input.clips {
            result.projections.insert(asset, (spans, TrackIndex::default()));
        }
        for (spans, index) in result.projections.values_mut() {
            spans.sort_by_key(|(source, position)| (source.start, source.end, *position));
            spans.dedup(); // Linked A/V occurrences have the same mapping.
            let entries: Vec<_> = spans.iter().enumerate().map(|(i, (source, _))| (i, source.start, source.end)).collect();
            index.root = index.build(&entries);
        }
        for layer in input.layers {
            let mut sources = layer.ranges;
            if !result.source_view || input.stamp.3.as_ref() == Some(&layer.asset) {
                result.layers.insert(layer.id.clone(), layer.asset);
            }
            sources.sort_by_key(|o| (o.range.start, o.item, o.range_index));
            let index = occurrence_index(&sources);
            result.source_layers.insert(layer.id, (sources, index));
        }
        result.stamp = Some(input.stamp);
        result
    }

    pub fn query(&self, layer: &tv2_domain::ids::LayerId, range: TimeRange) -> Vec<ItemOccurrence> {
        let mut out = Vec::new();
        self.visit(layer, range, |occurrence| out.push(occurrence));
        out.sort_by_key(|o| (o.range.start, o.item, o.range_index));
        out.dedup_by_key(|o| (o.item, o.range_index, o.range));
        out
    }

    // Project only clips intersecting the query. Repeated media must never
    // materialize the item × clip Cartesian product in the resident index.
    fn visit(&self, layer: &LayerId, range: TimeRange, mut visitor: impl FnMut(ItemOccurrence)) {
        let Some(asset) = self.layers.get(layer) else { return };
        if self.source_view {
            for occurrence in self.query_source(layer, range) {
                visitor(occurrence);
            }
            return;
        }
        let Some((clips, _)) = self.projections.get(asset) else { return };
        for (source, position) in clips {
            let timeline = TimeRange::new(*position, *position + source.duration());
            let Some(visible) = timeline.intersection(&range) else { continue };
            let query = TimeRange::new(source.start + (visible.start - *position), source.start + (visible.end - *position));
            for occurrence in self.query_source(layer, query) {
                let overlap = if occurrence.range.is_point() {
                    source.contains(occurrence.range.start).then_some(occurrence.range)
                } else {
                    source.intersection(&occurrence.range)
                };
                if let Some(overlap) = overlap {
                    visitor(ItemOccurrence {
                        range: TimeRange::new(*position + (overlap.start - source.start), *position + (overlap.end - source.start)),
                        ..occurrence
                    });
                }
            }
        }
    }

    pub fn paint_query(
        &self,
        layer: &LayerId,
        range: TimeRange,
        columns: usize,
        classify: impl Fn(ItemOccurrence) -> Option<usize>,
    ) -> SemanticPaint {
        let mut paint = PaintAccumulator::new(range, columns);
        self.visit(layer, range, |occurrence| {
            if let Some(kind) = classify(occurrence) {
                paint.add(occurrence, kind);
            }
        });
        paint.finish()
    }

    pub fn paint_source(
        &self,
        layer: &LayerId,
        range: TimeRange,
        columns: usize,
        classify: impl Fn(ItemOccurrence) -> Option<usize>,
    ) -> SemanticPaint {
        let mut paint = PaintAccumulator::new(range, columns);
        for occurrence in self.query_source(layer, range) {
            if let Some(kind) = classify(occurrence) {
                paint.add(occurrence, kind);
            }
        }
        paint.finish()
    }

    /// Presentation-only preview. Iterate visible clip mappings before source
    /// ranges, never allocate a range × repetition product or change a command.
    pub fn paint_preview(
        &self,
        asset: &AssetId,
        ranges: impl IntoIterator<Item = (TimeRange, bool)>,
        visible: TimeRange,
        columns: usize,
    ) -> SemanticPaint {
        let ranges: Vec<_> = ranges.into_iter().collect();
        let mut paint = PaintAccumulator::new(visible, columns);
        let mut add = |item, range: TimeRange, removed| {
            if (range.is_point() && visible.contains(range.start)) || range.overlaps(&visible) {
                paint.add(ItemOccurrence { item, range_index: usize::from(removed), range }, usize::from(removed));
            }
        };
        if self.source_view {
            if self.stamp.as_ref().and_then(|stamp| stamp.3.as_ref()) == Some(asset) {
                for (item, (range, removed)) in ranges.iter().enumerate() {
                    add(item, *range, *removed);
                }
            }
        } else if let Some((clips, _)) = self.projections.get(asset) {
            for (source, position) in clips {
                if !TimeRange::new(*position, *position + source.duration()).overlaps(&visible) {
                    continue;
                }
                for (item, (range, removed)) in ranges.iter().enumerate() {
                    let overlap = if range.is_point() { source.contains(range.start).then_some(*range) } else { source.intersection(range) };
                    if let Some(overlap) = overlap {
                        add(item, TimeRange::new(*position + (overlap.start - source.start), *position + (overlap.end - source.start)), *removed);
                    }
                }
            }
        }
        paint.finish()
    }

    pub fn visible_layers(&self, asset: &AssetId) -> &[(LayerId, String)] {
        self.visible_layers.get(asset).map(Vec::as_slice).unwrap_or_default()
    }
    pub fn edges(&self, asset: &AssetId, range: TimeRange) -> Vec<Ticks> {
        let mut edges = Vec::new();
        for (layer, layer_asset) in &self.layers {
            if layer_asset == asset {
                self.visit(layer, TimeRange::new(Ticks(range.start.0.saturating_sub(1)), Ticks(range.end.0.saturating_add(1))), |occurrence| {
                    edges
                        .extend([occurrence.range.start, occurrence.range.end].into_iter().filter(|edge| *edge >= range.start && *edge <= range.end));
                });
            }
        }
        edges.sort_unstable();
        edges.dedup();
        edges
    }
    pub fn query_source(&self, layer: &LayerId, range: TimeRange) -> Vec<ItemOccurrence> {
        let Some((occurrences, index)) = self.source_layers.get(layer) else { return vec![] };
        let mut found = Vec::new();
        index.query(index.root, range, &mut found, &mut 0);
        found.into_iter().map(|i| occurrences[i]).collect()
    }
    pub fn gesture_source_pair(&self, asset: &AssetId, range: TimeRange, origin: Ticks, target: Ticks, tolerance: Ticks) -> Option<(Ticks, Ticks)> {
        let (clips, index) = self.projections.get(asset)?;
        let query = TimeRange::new(range.start, Ticks(range.end.0.max(range.start.0.saturating_add(1))));
        let mut found = Vec::new();
        index.query(index.root, query, &mut found, &mut 0);
        let mut offset = None;
        for i in found {
            let (source, position) = clips[i];
            let Some(overlap) = (if range.is_point() { source.contains(range.start).then_some(range) } else { source.intersection(&range) }) else {
                continue;
            };
            let start = position + (overlap.start - source.start);
            let end = position + (overlap.end - source.start);
            if origin >= start - tolerance && origin <= end + tolerance && target >= position && target <= position + source.duration() {
                let candidate = source.start - position;
                if offset.is_some_and(|old| old != candidate) {
                    return None;
                }
                offset = Some(candidate);
            }
        }
        offset.map(|offset| (origin + offset, target + offset))
    }
}
fn occurrence_index(occurrences: &[ItemOccurrence]) -> TrackIndex {
    let entries: Vec<_> =
        occurrences.iter().enumerate().map(|(i, o)| (i, o.range.start, Ticks(o.range.end.0.max(o.range.start.0.saturating_add(1))))).collect();
    let mut index = TrackIndex::default();
    index.root = index.build(&entries);
    index
}

impl TrackIndex {
    fn build(&mut self, entries: &[(usize, Ticks, Ticks)]) -> Option<usize> {
        if entries.is_empty() {
            return None;
        }
        let mid = entries.len() / 2;
        let (clip, start, end) = entries[mid];
        let left = self.build(&entries[..mid]);
        let right = self.build(&entries[mid + 1..]);
        let max_end = [left, right].into_iter().flatten().map(|i| self.nodes[i].max_end).fold(end, Ticks::max);
        let index = self.nodes.len();
        self.nodes.push(Node { clip, start, end, max_end, left, right });
        Some(index)
    }

    fn query(&self, node: Option<usize>, range: TimeRange, out: &mut Vec<usize>, visited: &mut usize) {
        let Some(i) = node else { return };
        *visited += 1;
        let node = &self.nodes[i];
        if node.max_end <= range.start {
            return;
        }
        self.query(node.left, range, out, visited);
        if node.start >= range.end {
            return;
        }
        if node.end > range.start {
            out.push(node.clip);
        }
        self.query(node.right, range, out, visited);
    }
}

#[derive(Default)]
pub struct ClipIndex {
    stamp: Option<(String, SequenceId, u64)>,
    tracks: HashMap<TrackId, TrackIndex>,
    pub edges: Vec<Ticks>,
    pub last_visited: usize,
    markers: TrackIndex,
    marker_edges: Vec<Ticks>,
    pending: Option<crossbeam_channel::Receiver<ClipIndex>>,
    failed: Option<((String, SequenceId, u64), String)>,
}

pub struct PaintSequence {
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
}
/// Presentation-only copy: evidence/provenance extras never participate in paint
/// or hit tests and may contain large imported documents.
pub fn paint_clip(clip: &Clip) -> Clip {
    Clip {
        id: clip.id.clone(),
        track_id: clip.track_id.clone(),
        asset_id: clip.asset_id.clone(),
        source: clip.source,
        position: clip.position,
        enabled: clip.enabled,
        name: clip.name.clone(),
        link_group: None,
        transform: clip.transform,
        gain_db: clip.gain_db,
        audio_stream: clip.audio_stream,
        provenance: Default::default(),
        extra: Default::default(),
    }
}
impl PaintSequence {
    pub fn track(&self, id: &TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| &t.id == id)
    }
}

impl ClipIndex {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.failed.as_ref().map(|(_, error)| error.as_str())
    }
    pub fn retry(&mut self) {
        self.failed = None;
    }
    pub fn ensure_async(&mut self, project: &str, revision: u64, seq: &Sequence, ctx: &egui::Context) {
        let stamp = (project.to_owned(), seq.id.clone(), revision);
        if self.stamp.as_ref() == Some(&stamp) {
            return;
        }
        if self.stamp.take().is_some() {
            self.tracks.clear();
            self.edges.clear();
            self.markers = TrackIndex::default();
            self.marker_edges.clear();
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    if result.stamp.as_ref() == Some(&stamp) {
                        *self = result;
                        return;
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return,
                Err(_) => {
                    self.pending = None;
                    self.failed = Some((stamp.clone(), "El worker de clips terminó sin resultado".into()));
                }
            }
        }
        if self.failed.as_ref().is_some_and(|(base, _)| base == &stamp) {
            return;
        }
        self.failed = None;
        let mut geometry = Sequence::new("index", seq.frame_rate, seq.width, seq.height, seq.sample_rate);
        geometry.id = seq.id.clone();
        geometry.clips = seq.clips.iter().map(paint_clip).collect();
        geometry.markers = seq
            .markers
            .iter()
            .map(|marker| tv2_domain::timeline::Marker {
                id: marker.id.clone(),
                range: marker.range,
                label: String::new(),
                color: String::new(),
                comment: String::new(),
            })
            .collect();
        let project = project.to_owned();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        match std::thread::Builder::new().name("clip-index".into()).spawn(move || {
            let mut index = Self::default();
            index.ensure(&project, revision, &geometry);
            let _ = tx.send(index);
            wake.request_repaint();
        }) {
            Ok(_) => self.pending = Some(rx),
            Err(error) => self.failed = Some((stamp, error.to_string())),
        }
    }
    pub fn query_markers(&self, range: TimeRange) -> Vec<usize> {
        let mut out = Vec::new();
        self.markers.query(self.markers.root, range, &mut out, &mut 0);
        out
    }
    pub fn marker_edges(&self, range: TimeRange) -> &[Ticks] {
        edge_window(&self.marker_edges, range)
    }
    pub fn query_track(&self, track: &TrackId, range: TimeRange) -> Vec<usize> {
        let mut out = Vec::new();
        if let Some(index) = self.tracks.get(track) {
            index.query(index.root, range, &mut out, &mut 0);
        }
        out
    }
    pub fn ensure(&mut self, project: &str, revision: u64, seq: &Sequence) {
        if self.stamp.as_ref().is_some_and(|(p, s, r)| p == project && s == &seq.id && *r == revision) {
            return;
        }
        let mut groups: HashMap<TrackId, Vec<(usize, Ticks, Ticks)>> = HashMap::new();
        for (i, clip) in seq.clips.iter().enumerate() {
            groups.entry(clip.track_id.clone()).or_default().push((i, clip.position, clip.end()));
        }
        self.tracks.clear();
        for (track, mut entries) in groups {
            entries.sort_by_key(|e| e.1);
            let mut index = TrackIndex::default();
            index.root = index.build(&entries);
            self.tracks.insert(track, index);
        }
        self.edges = seq.clip_edges();
        let mut markers: Vec<_> = seq
            .markers
            .iter()
            .enumerate()
            .map(|(i, marker)| (i, marker.range.start, Ticks(marker.range.end.0.max(marker.range.start.0.saturating_add(1)))))
            .collect();
        markers.sort_by_key(|(_, start, _)| *start);
        self.markers = TrackIndex::default();
        self.markers.root = self.markers.build(&markers);
        self.marker_edges = seq.markers.iter().flat_map(|marker| [marker.range.start, marker.range.end]).collect();
        self.marker_edges.sort_unstable();
        self.marker_edges.dedup();
        self.stamp = Some((project.to_owned(), seq.id.clone(), revision));
    }

    pub fn query(&mut self, tracks: &[TrackId], range: TimeRange) -> Vec<usize> {
        let mut out = Vec::new();
        self.last_visited = 0;
        for track in tracks {
            if let Some(index) = self.tracks.get(track) {
                index.query(index.root, range, &mut out, &mut self.last_visited);
            }
        }
        out
    }
}
fn edge_window(edges: &[Ticks], range: TimeRange) -> &[Ticks] {
    let first = edges.partition_point(|edge| *edge < range.start);
    let last = edges.partition_point(|edge| *edge <= range.end);
    &edges[first..last]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::{ids::AssetId, timeline::TrackKind};
    #[test]
    fn repeated_semantics_remain_lazy_and_exact_at_visible_clip_boundaries() {
        let range = |a, b| TimeRange::new(Ticks::from_seconds(a), Ticks::from_seconds(b));
        let asset = AssetId::new("synthetic");
        let layer = LayerId::new("dense");
        let sources: Vec<_> = (0..20_000).map(|item| ItemOccurrence { item, range_index: 0, range: range(0, 1) }).collect();
        let input = SemanticInput {
            stamp: ("p".into(), 1, crate::app::ViewMode::Sequence, None),
            layers: vec![LayerInput { id: layer.clone(), asset: asset.clone(), ranges: sources }],
            visible: HashMap::new(),
            // Linked A/V duplicates are deduplicated; distant repetitions stay distinct.
            clips: HashMap::from([(asset.clone(), (0..500).flat_map(|i| [(range(0, 1), Ticks::from_seconds(i)); 2]).collect())]),
        };
        let index = SemanticIndex::build(input);
        assert_eq!(index.source_layers[&layer].0.len(), 20_000);
        assert_eq!(index.projections[&asset].0.len(), 500);
        // No resident ten-million-occurrence product. Exact queries and snap
        // boundaries still expose every original item in only the visible repeat.
        let got = index.query(&layer, range(250, 251));
        assert_eq!(got.len(), 20_000);
        assert!(got.iter().all(|o| o.range == range(250, 251)));
        assert_eq!(index.edges(&asset, range(250, 251)), vec![Ticks::from_seconds(250), Ticks::from_seconds(251)]);
        let paint = index.paint_query(&layer, range(250, 251), 100, |o| Some(if o.item == 19_999 { 4 } else { 1 }));
        let SemanticPaint::Dense(coverage) = paint else { panic!("dense query must have bounded geometry") };
        assert_eq!(coverage.len(), 2);
        assert!(coverage.iter().all(|span| span.range == range(250, 251)));
        assert!(coverage.iter().any(|span| span.kind == 4));
        assert_eq!(index.query(&layer, range(250, 251)).len(), 20_000, "LOD never changes editing geometry");
    }
    #[test]
    fn pixel_coverage_keeps_gaps_points_states_and_selected_items() {
        let range = |a, b| TimeRange::new(Ticks::from_seconds(a), Ticks::from_seconds(b));
        let mut paint = PaintAccumulator::new(range(0, 10), 10);
        for item in 0..100_000 {
            paint.add(ItemOccurrence { item, range_index: 0, range: range(0, 2) }, 1);
        }
        paint.add(ItemOccurrence { item: 100_001, range_index: 0, range: range(5, 5) }, 4);
        paint.add(ItemOccurrence { item: 100_002, range_index: 0, range: range(8, 10) }, 2);
        let SemanticPaint::Dense(coverage) = paint.finish() else { panic!("density must aggregate") };
        assert_eq!(coverage.len(), 3);
        assert!(coverage.iter().any(|span| span.kind == 1 && span.range == range(0, 2)));
        assert!(coverage.iter().any(|span| span.kind == 4 && span.range == range(5, 6)));
        assert!(coverage.iter().any(|span| span.kind == 2 && span.range == range(8, 10)));
        // Sparse/zoomed-in presentation retains exact labels and ranges.
        let mut sparse = PaintAccumulator::new(range(0, 10), 10);
        sparse.add(ItemOccurrence { item: 3, range_index: 1, range: range(5, 5) }, 4);
        let SemanticPaint::Exact(items) = sparse.finish() else { panic!("sparse geometry must stay exact") };
        assert_eq!(items[0].item, 3);
        assert_eq!(items[0].range, range(5, 5));
    }
    #[test]
    fn massive_preview_culls_repetitions_and_bounds_geometry_without_mutating_ranges() {
        let range = |a, b| TimeRange::new(Ticks::from_seconds(a), Ticks::from_seconds(b));
        let asset = AssetId::new("preview");
        let input = SemanticInput {
            stamp: ("p".into(), 1, crate::app::ViewMode::Sequence, None),
            layers: vec![],
            visible: HashMap::new(),
            clips: HashMap::from([(asset.clone(), (0..500).map(|i| (range(0, 1), Ticks::from_seconds(i))).collect())]),
        };
        let index = SemanticIndex::build(input);
        let original: Vec<_> = (0..20_000).map(|i| (range(0, 1), i % 2 == 0)).collect();
        let SemanticPaint::Dense(coverage) = index.paint_preview(&asset, original.iter().copied(), range(250, 251), 100) else {
            panic!("preview must aggregate")
        };
        assert_eq!(coverage.len(), 2);
        assert!(coverage.iter().all(|span| span.range == range(250, 251)));
        assert!(coverage.iter().any(|span| span.kind == 0));
        assert!(coverage.iter().any(|span| span.kind == 1));
        let SemanticPaint::Dense(all) = index.paint_preview(&asset, original.iter().copied(), range(0, 500), 1000) else {
            panic!("ten million ghosts must aggregate")
        };
        assert_eq!(all.len(), 2, "mesh must depend on pixels, not repetitions × items");
        assert!(all.iter().all(|span| span.range == range(0, 500)));
        let SemanticPaint::Exact(empty) = index.paint_preview(&asset, original.iter().copied(), range(501, 502), 100) else {
            panic!("offscreen ghosts must be absent")
        };
        assert!(empty.is_empty());
        assert!(
            original.iter().enumerate().all(|(i, (r, removed))| *r == range(0, 1) && *removed == (i % 2 == 0)),
            "preview must not mutate command ranges"
        );
        assert_eq!(index.projections[&asset].0.len(), 500);
    }
    #[test]
    fn semantic_gesture_stays_on_one_source_mapping_across_repetitions() {
        let range = |a, b| TimeRange::new(Ticks::from_seconds(a), Ticks::from_seconds(b));
        let asset = AssetId::new("a");
        let input = SemanticInput {
            stamp: ("p".into(), 1, crate::app::ViewMode::Sequence, None),
            layers: vec![],
            visible: HashMap::new(),
            clips: HashMap::from([(
                asset.clone(),
                vec![(range(10, 20), Ticks::ZERO), (range(30, 40), Ticks::from_seconds(10)), (range(10, 20), Ticks::from_seconds(20))],
            )]),
        };
        let index = SemanticIndex::build(input);
        assert_eq!(
            index.gesture_source_pair(&asset, range(12, 14), Ticks::from_seconds(3), Ticks::from_seconds(5), Ticks::ZERO),
            Some((Ticks::from_seconds(13), Ticks::from_seconds(15)))
        );
        assert!(index.gesture_source_pair(&asset, range(12, 14), Ticks::from_seconds(3), Ticks::from_seconds(12), Ticks::ZERO).is_none());
        assert_eq!(
            index.gesture_source_pair(&asset, range(12, 14), Ticks::from_seconds(23), Ticks::from_seconds(25), Ticks::ZERO),
            Some((Ticks::from_seconds(13), Ticks::from_seconds(15)))
        );
    }
    #[test]
    fn dense_timeline_query_skips_offscreen_items_and_preserves_overlaps() {
        let mut seq = Sequence::new("Índice", tv2_domain::time::Rational::new(30, 1), 1920, 1080, 48000);
        let track = Track::new(TrackKind::Video, "V1");
        seq.tracks.push(track.clone());
        for i in 0..100_000 {
            seq.clips.push(Clip::new(
                track.id.clone(),
                AssetId::new("asset"),
                TimeRange::new(Ticks::ZERO, Ticks::from_seconds(1)),
                Ticks::from_seconds(i),
            ));
        }
        // Intervalo largo: obliga a consultar un árbol, no una tabla de end ordenados.
        seq.clips.push(Clip::new(track.id.clone(), AssetId::new("asset"), TimeRange::new(Ticks::ZERO, Ticks::from_seconds(100_000)), Ticks::ZERO));
        let mut index = ClipIndex::default();
        index.ensure("p", 1, &seq);
        let range = TimeRange::new(Ticks::from_seconds(50_000), Ticks::from_seconds(50_010));
        let got = index.query(&[track.id], range);
        assert_eq!(got.len(), 11);
        assert!(index.last_visited < 100, "{} nodos visitados entre 100001 clips", index.last_visited);
        assert!(got.iter().all(|i| seq.clips[*i].range().overlaps(&range)));
    }
}
