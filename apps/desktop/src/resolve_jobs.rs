//! One bounded composition worker. A result belongs to an exact project/view
//! snapshot; late results never replace the current graph or consume queued seeks.
use crate::app::{TranscriptorApp, ViewMode};
use std::{cell::Cell, collections::HashMap, sync::Arc};
use tv2_domain::{AssetId, Clip, ClipId, Project, Rational, ResolvedTimeline, Sequence, Ticks, TimeRange, Track, TrackId, TrackKind};
use tv2_media::{player::PlayerCommand, render::AssetSource};

type Key = (u64, ViewMode, Option<AssetId>);
struct Output {
    key: Key,
    project: Project,
    timeline: Arc<ResolvedTimeline>,
    assets: Arc<HashMap<AssetId, AssetSource>>,
    skips: Vec<TimeRange>,
}
pub struct Resolver {
    pending: Option<crossbeam_channel::Receiver<Output>>,
    wake: egui::Context,
    pub error: Option<String>,
    failed: Option<Key>,
    pub seek: Cell<Option<Ticks>>,
    pub playing: Cell<Option<bool>>,
}
impl Resolver {
    pub fn new(wake: egui::Context) -> Self {
        Self { pending: None, wake, error: None, failed: None, seek: Cell::new(None), playing: Cell::new(None) }
    }
    pub fn retry(&mut self) {
        self.failed = None;
        self.error = None;
    }
}
impl TranscriptorApp {
    pub fn composition_pending(&self) -> bool {
        self.resolved_revision.as_ref() != Some(&(self.session.revision(), self.view, self.source_asset.clone()))
    }
    pub fn refresh_resolved(&mut self) {
        let key = (self.session.revision(), self.view, self.source_asset.clone());
        if let Some(rx) = &self.resolver.pending {
            match rx.try_recv() {
                Ok(output) => {
                    self.resolver.pending = None;
                    // Equality also catches same-revision relinks, Save As and
                    // project replacement. Asset locators are checked separately.
                    if output.key == key && output.project == *self.project() && *output.assets == self.asset_sources() {
                        self.resolved = output.timeline.clone();
                        self.resolved_revision = Some(key.clone());
                        if let Some(player) = &self.player {
                            player.send(PlayerCommand::SetTimeline { timeline: output.timeline, assets: output.assets });
                            player.send(PlayerCommand::SetSkipRanges(output.skips));
                            if let Some(at) = self.resolver.seek.take() {
                                player.send(PlayerCommand::Seek(at));
                            }
                            if let Some(playing) = self.resolver.playing.take() {
                                player.send(if playing { PlayerCommand::Play } else { PlayerCommand::Pause });
                            }
                        }
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(_) => {
                    self.resolver.pending = None;
                    self.resolver.failed = Some(key.clone());
                    self.resolver.error = Some("El worker de composición terminó sin resultado".into());
                }
            }
        }
        if self.resolved_revision.as_ref() == Some(&key) || self.resolver.pending.is_some() || self.resolver.failed.as_ref() == Some(&key) {
            return;
        }
        let project = self.project().clone();
        let assets = Arc::new(self.asset_sources());
        let wake = self.resolver.wake.clone();
        let worker_key = key.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new().name("composition-resolve".into()).spawn(move || {
            let timeline = Arc::new(resolve(&project, worker_key.1, worker_key.2.as_ref()));
            let skips = if project.settings.skip_trims_on_play { tv2_domain::review::skip_ranges(&project, &timeline) } else { vec![] };
            let _ = tx.send(Output { key: worker_key, project, timeline, assets, skips });
            wake.request_repaint();
        }) {
            Ok(_) => {
                self.resolver.pending = Some(rx);
                self.resolver.error = None;
            }
            Err(error) => {
                self.resolver.failed = Some(key);
                self.resolver.error = Some(format!("No se pudo preparar la composición: {error}"));
            }
        }
    }
}

fn resolve(project: &Project, view: ViewMode, source: Option<&AssetId>) -> ResolvedTimeline {
    let result = match view {
        ViewMode::Sequence => project.active().map(|sequence| ResolvedTimeline::resolve(project, sequence)),
        ViewMode::Source => source.and_then(|id| project.asset(id)).map(|asset| {
            let (width, height) = asset.probe.video.as_ref().map(|video| video.display_size()).unwrap_or((1920, 1080));
            let mut sequence = Sequence::new("fuente", asset.frame_rate().unwrap_or(Rational::new(30, 1)), width, height, 48000);
            if asset.has_video() {
                let mut track = Track::new(TrackKind::Video, "V");
                track.id = TrackId::new("source-video");
                let mut clip = Clip::new(track.id.clone(), asset.id.clone(), TimeRange::new(Ticks::ZERO, asset.duration()), Ticks::ZERO);
                clip.name = asset.name.clone();
                clip.id = ClipId::new(format!("source-video-{}", asset.id));
                sequence.tracks.push(track);
                sequence.clips.push(clip);
            }
            for (index, _) in asset.probe.audio.iter().enumerate() {
                let mut track = Track::new(TrackKind::Audio, format!("A{}", index + 1));
                track.id = TrackId::new(format!("source-audio-{index}"));
                let mut clip = Clip::new(track.id.clone(), asset.id.clone(), TimeRange::new(Ticks::ZERO, asset.duration()), Ticks::ZERO);
                clip.id = ClipId::new(format!("source-audio-{index}-{}", asset.id));
                clip.audio_stream = Some(index as u32);
                sequence.tracks.push(track);
                sequence.clips.push(clip);
            }
            let mut source_project = Project::new("fuente");
            source_project.assets = vec![asset.clone()];
            ResolvedTimeline::resolve(&source_project, &sequence)
        }),
    };
    result.unwrap_or(ResolvedTimeline {
        frame_rate: project.active().map(|sequence| sequence.frame_rate).unwrap_or_default(),
        width: 1920,
        height: 1080,
        sample_rate: 48000,
        duration: Ticks::ZERO,
        pieces: vec![],
    })
}
