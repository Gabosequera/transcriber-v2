//! Índice de intervalos de clips, reconstruido únicamente al cambiar la revisión.
use std::collections::HashMap;
use tv2_domain::{
    ids::{SequenceId, TrackId},
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
}

pub struct PaintSequence {
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
}
impl PaintSequence {
    pub fn track(&self, id: &TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| &t.id == id)
    }
}

impl ClipIndex {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::{ids::AssetId, timeline::TrackKind};
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
