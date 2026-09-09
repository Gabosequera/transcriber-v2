//! Ephemeral review navigation. Skipping never changes composition or export.
use crate::{LayerKind, Project, ResolvedTimeline, Ticks, TimeRange};

/// A mixed piece is skipped only where every contributing clip is trimmed.
/// This prevents a trim on one asset from hiding an unrelated overlay/audio.
pub fn skip_ranges(project: &Project, timeline: &ResolvedTimeline) -> Vec<TimeRange> {
    let mut result = Vec::new();
    for piece in &timeline.pieces {
        if piece.is_gap() {
            continue;
        }
        let mut intersection = vec![piece.range];
        for clip in piece.video.iter().chain(&piece.audio) {
            let source = TimeRange::from_start_duration(clip.source_start, piece.range.duration());
            let mapped: Vec<_> = project
                .layers
                .iter()
                .filter(|l| l.asset_id == clip.asset_id && l.kind == LayerKind::Trims && !l.deleted)
                .flat_map(|l| l.enabled_intervals())
                .filter_map(|r| {
                    let start = r.start.max(source.start);
                    let end = r.end.min(source.end);
                    (start < end).then(|| TimeRange::new(piece.range.start + (start - source.start), piece.range.start + (end - source.start)))
                })
                .collect();
            intersection = intersection
                .iter()
                .flat_map(|a| {
                    mapped.iter().filter_map(move |b| {
                        let start = a.start.max(b.start);
                        let end = a.end.min(b.end);
                        (start < end).then_some(TimeRange::new(start, end))
                    })
                })
                .collect();
            if intersection.is_empty() {
                break;
            }
        }
        result.extend(intersection);
    }
    crate::layers::merge_intervals(&mut result)
}

pub fn skip_target(ranges: &[TimeRange], position: Ticks) -> Option<Ticks> {
    let idx = ranges.partition_point(|r| r.end <= position);
    ranges.get(idx).filter(|r| r.start <= position).map(|r| r.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests_support::fake_video;
    use crate::{Command, ItemState, SemanticItem, SemanticLayer};
    #[test]
    fn repeated_trims_skip_proposals_but_not_disabled_cuts_or_unrelated_audio() {
        let mut p = Project::new("review");
        Command::ImportAsset { asset: fake_video("a", 10) }.apply(&mut p).unwrap();
        for position in [0, 10] {
            Command::InsertAssetLinked { asset_id: "a".into(), position: Ticks::from_seconds(position), video_track: None, source: None }
                .apply(&mut p)
                .unwrap();
        }
        let mut l = SemanticLayer::new("a".into(), LayerKind::Trims, "Trims");
        l.items.push(SemanticItem::new(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(3)), "proposed"));
        let mut disabled = SemanticItem::new(TimeRange::new(Ticks::from_seconds(5), Ticks::from_seconds(6)), "disabled");
        disabled.state = ItemState::Disabled;
        l.items.push(disabled);
        p.layers.push(l);
        let timeline = ResolvedTimeline::resolve(&p, p.active().unwrap());
        let skips = skip_ranges(&p, &timeline);
        assert_eq!(
            skips,
            vec![TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(3)), TimeRange::new(Ticks::from_seconds(11), Ticks::from_seconds(13))]
        );
        assert_eq!(skip_target(&skips, Ticks::from_seconds(1)), Some(Ticks::from_seconds(3)));
        assert_eq!(skip_target(&skips, Ticks::from_seconds(3)), None);
        Command::ImportAsset { asset: fake_video("b", 10) }.apply(&mut p).unwrap();
        Command::InsertAssetLinked { asset_id: "b".into(), position: Ticks::ZERO, video_track: None, source: None }.apply(&mut p).unwrap();
        let skips = skip_ranges(&p, &ResolvedTimeline::resolve(&p, p.active().unwrap()));
        assert_eq!(skips, vec![TimeRange::new(Ticks::from_seconds(11), Ticks::from_seconds(13))]);
    }
}
