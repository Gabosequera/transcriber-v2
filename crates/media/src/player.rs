//! Reproductor del timeline: hilo propietario del renderizador, del mezclador y
//! de la salida de audio. La GUI envía intenciones (`PlayerCommand`) y lee el
//! último fotograma presentado (`PresentedFrame`) y un `PlayerSnapshot`.
//!
//! Reglas:
//! - Cada seek/scrub incrementa la generación; fotogramas y audio de una
//!   generación anterior se descartan (nunca aparece un frame de un seek viejo).
//! - Durante scrub continuo solo cuenta la última posición pedida (coalescencia
//!   de comandos pendientes).
//! - Reloj: audio cuando hay clips audibles y velocidad ≤ 4×; si no, reloj de pared.
//! - Velocidades V1: 1×, 2×, 3×, 4× con `atempo` (tono preservado) y 8× como
//!   skim solo de fotogramas clave sin audio.

use crate::audio::{AudioMixer, AudioOutput, CHANNELS};
use crate::compositor::Frame;
use crate::ffmpeg::FfmpegTools;
use crate::render::{AssetSource, TimelineRenderer};
use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tv2_domain::ids::AssetId;
use tv2_domain::resolve::ResolvedTimeline;
use tv2_domain::time::{Ticks, TimeRange};

pub const SPEEDS: [f64; 5] = [1.0, 2.0, 3.0, 4.0, 8.0];

/// Política de audio por velocidad, visible en la UI.
pub fn audio_policy(rate: f64) -> &'static str {
    if rate <= 1.0 {
        "audio normal"
    } else if rate <= 4.0 {
        "audio acelerado con tono preservado (atempo)"
    } else {
        "skim de fotogramas clave · sin audio"
    }
}

#[derive(Clone, Debug)]
pub enum PlayerCommand {
    SetTimeline {
        timeline: Arc<ResolvedTimeline>,
        assets: Arc<HashMap<AssetId, AssetSource>>,
    },
    SetViewportSize {
        width: u32,
        height: u32,
    },
    Seek(Ticks),
    /// Scrub: como seek pero se coalesce agresivamente y no reanuda reproducción.
    Scrub(Ticks),
    Play,
    Pause,
    TogglePlay,
    SetRate(f64),
    StepFrames(i64),
    SetLoop(Option<TimeRange>),
    SetSkipRanges(Vec<TimeRange>),
    SetMuted(bool),
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct PresentedFrame {
    pub position: Ticks,
    pub generation: u64,
    pub frame: Frame,
    pub complete: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerSnapshot {
    pub position: Ticks,
    pub playing: bool,
    pub rate: f64,
    pub generation: u64,
    pub buffering: bool,
    pub audio_available: bool,
    pub muted: bool,
    pub loop_range: Option<TimeRange>,
    pub last_error: Option<String>,
    pub decode_ms: f32,
}

struct Shared {
    frame: Mutex<Option<PresentedFrame>>,
    frame_seq: AtomicU64,
    snapshot: Mutex<PlayerSnapshot>,
}

pub struct PlayerHandle {
    tx: Sender<PlayerCommand>,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

impl PlayerHandle {
    pub fn send(&self, cmd: PlayerCommand) {
        let _ = self.tx.send(cmd);
    }

    /// Último fotograma presentado si es más nuevo que `seen` (número de secuencia).
    pub fn take_frame(&self, seen: &mut u64) -> Option<PresentedFrame> {
        let seq = self.shared.frame_seq.load(Ordering::Acquire);
        if seq == *seen {
            return None;
        }
        *seen = seq;
        self.shared.frame.lock().clone()
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.shared.snapshot.lock().clone()
    }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.tx.send(PlayerCommand::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Nueva generación: la salida de audio es la única fuente cuando existe (los bloques
/// se etiquetan con ella y el callback descarta los de generaciones anteriores).
fn next_generation(audio_out: &Option<AudioOutput>, current: u64) -> u64 {
    match audio_out {
        Some(a) => a.flush(),
        None => current + 1,
    }
}

/// Reloj de reproducción.
struct Clock {
    anchor_pos: Ticks,
    anchor_instant: Instant,
    anchor_frames: u64,
    rate: f64,
    use_audio: bool,
}

pub fn spawn(tools: FfmpegTools, wake: Arc<dyn Fn() + Send + Sync>) -> PlayerHandle {
    let (tx, rx) = unbounded::<PlayerCommand>();
    let shared = Arc::new(Shared {
        frame: Mutex::new(None),
        frame_seq: AtomicU64::new(0),
        snapshot: Mutex::new(PlayerSnapshot { rate: 1.0, ..Default::default() }),
    });
    let shared_t = shared.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_thread = cancel.clone();
    let thread =
        std::thread::Builder::new().name("player".into()).spawn(move || run(tools, rx, shared_t, wake, cancel_thread)).expect("hilo del reproductor");
    PlayerHandle { tx, shared, thread: Some(thread), cancel }
}

fn run(tools: FfmpegTools, rx: Receiver<PlayerCommand>, shared: Arc<Shared>, wake: Arc<dyn Fn() + Send + Sync>, cancel: Arc<AtomicBool>) {
    let audio_out = match AudioOutput::open() {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!("sin salida de audio: {e}");
            None
        }
    };
    let out_rate = audio_out.as_ref().map(|a| a.sample_rate).unwrap_or(48_000);
    let mut timeline: Arc<ResolvedTimeline> = Arc::new(ResolvedTimeline {
        frame_rate: tv2_domain::time::Rational::new(30, 1),
        width: 1920,
        height: 1080,
        sample_rate: 48_000,
        duration: Ticks::ZERO,
        pieces: Vec::new(),
    });
    let mut assets: Arc<HashMap<AssetId, AssetSource>> = Arc::new(HashMap::new());
    let mut renderer: Option<TimelineRenderer> = None;
    let mut viewport = (640u32, 360u32);
    let mut position = Ticks::ZERO;
    let mut playing = false;
    let mut rate = 1.0f64;
    let mut generation: u64 = 0;
    let mut loop_range: Option<TimeRange> = None;
    let mut skip_ranges: Vec<TimeRange> = Vec::new();
    let mut muted = false;
    let mut mixer: Option<AudioMixer> = None;
    let mut clock: Option<Clock> = None;
    let mut last_presented: Option<Ticks> = None;
    let mut last_error: Option<String> = None;
    let mut dirty = true; // hay que presentar un fotograma para `position`
    let mut buffering = false;
    let mut carry: Option<PlayerCommand> = None; // comando recibido durante la espera entre fotogramas

    let publish = |shared: &Shared, pf: PresentedFrame| {
        *shared.frame.lock() = Some(pf);
        shared.frame_seq.fetch_add(1, Ordering::Release);
    };

    loop {
        // 1. comandos (coalescer scrubs/seeks: solo el último cuenta)
        let mut pending: Vec<PlayerCommand> = Vec::new();
        let first = match carry.take() {
            Some(c) => Some(c),
            None if playing || dirty => rx.try_recv().ok(),
            None => rx.recv_timeout(Duration::from_millis(250)).ok(),
        };
        if let Some(c) = first {
            pending.push(c);
            while let Ok(c) = rx.try_recv() {
                pending.push(c);
            }
        }
        // quedarse solo con el último Seek/Scrub
        let last_seek_idx = pending.iter().rposition(|c| matches!(c, PlayerCommand::Seek(_) | PlayerCommand::Scrub(_)));
        let mut shutdown = false;
        for (i, cmd) in pending.into_iter().enumerate() {
            match cmd {
                PlayerCommand::Shutdown => shutdown = true,
                PlayerCommand::SetTimeline { timeline: t, assets: a } => {
                    timeline = t;
                    assets = a;
                    match renderer.as_mut() {
                        Some(r) => r.set_timeline(timeline.clone(), assets.clone()),
                        None => renderer = Some(TimelineRenderer::new(tools.clone(), timeline.clone(), assets.clone(), viewport.0, viewport.1)),
                    }
                    if let Some(r) = &mut renderer {
                        r.cancel = cancel.clone();
                    }
                    // la mezcla en curso sigue con la timeline nueva desde la posición actual
                    if playing {
                        generation = next_generation(&audio_out, generation);
                        mixer = None;
                        clock = None;
                    }
                    position = position.clamp(Ticks::ZERO, timeline.duration.max(Ticks::ZERO));
                    dirty = true;
                }
                PlayerCommand::SetViewportSize { width, height } => {
                    let w = width.clamp(64, 3840);
                    let h = height.clamp(36, 2160);
                    if (w, h) != viewport {
                        viewport = (w, h);
                        if let Some(r) = renderer.as_mut() {
                            r.set_size(w, h);
                        }
                        dirty = true;
                    }
                }
                PlayerCommand::Seek(t) | PlayerCommand::Scrub(t) => {
                    if Some(i) != last_seek_idx {
                        continue;
                    }
                    position = t.clamp(Ticks::ZERO, timeline.duration.max(Ticks::ZERO));
                    generation = next_generation(&audio_out, generation);
                    mixer = None;
                    clock = None;
                    dirty = true;
                }
                PlayerCommand::Play => {
                    if !playing {
                        if position >= timeline.duration {
                            position = loop_range.map(|l| l.start).unwrap_or(Ticks::ZERO);
                        }
                        playing = true;
                        clock = None;
                        mixer = None;
                        dirty = true;
                    }
                }
                PlayerCommand::Pause => {
                    if playing {
                        playing = false;
                        generation = next_generation(&audio_out, generation);
                        mixer = None;
                        clock = None;
                        // presentar exactamente la posición de pausa alineada a fotograma
                        position = position.floor_to_frame(timeline.frame_rate);
                        dirty = true;
                    }
                }
                PlayerCommand::TogglePlay => {
                    if playing {
                        playing = false;
                        generation = next_generation(&audio_out, generation);
                        mixer = None;
                        clock = None;
                        position = position.floor_to_frame(timeline.frame_rate);
                    } else {
                        if position >= timeline.duration {
                            position = loop_range.map(|l| l.start).unwrap_or(Ticks::ZERO);
                        }
                        playing = true;
                        clock = None;
                        mixer = None;
                    }
                    dirty = true;
                }
                PlayerCommand::SetRate(r) => {
                    let r = r.clamp(0.25, 16.0);
                    if (r - rate).abs() > 1e-9 {
                        rate = r;
                        generation = next_generation(&audio_out, generation);
                        mixer = None;
                        clock = None;
                        if let Some(rd) = renderer.as_mut() {
                            rd.keyframes_only = rate >= 6.0;
                            rd.drop_decoders();
                        }
                        dirty = true;
                    }
                }
                PlayerCommand::StepFrames(n) => {
                    playing = false;
                    generation = next_generation(&audio_out, generation);
                    mixer = None;
                    clock = None;
                    let fd = timeline.frame_rate.frame_duration();
                    let base = position.floor_to_frame(timeline.frame_rate);
                    position = (base + Ticks(fd.0 * n)).clamp(Ticks::ZERO, timeline.duration.max(Ticks::ZERO));
                    generation = next_generation(&audio_out, generation);
                    dirty = true;
                }
                PlayerCommand::SetLoop(l) => loop_range = l,
                PlayerCommand::SetSkipRanges(mut ranges) => {
                    ranges.retain(|r| r.start >= Ticks::ZERO && r.end > r.start);
                    skip_ranges = tv2_domain::layers::merge_intervals(&mut ranges);
                    generation = next_generation(&audio_out, generation);
                    mixer = None;
                    clock = None;
                }
                PlayerCommand::SetMuted(m) => {
                    muted = m;
                    generation = next_generation(&audio_out, generation);
                    mixer = None;
                    clock = None;
                }
            }
        }
        if shutdown {
            break;
        }
        let Some(r) = renderer.as_mut() else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };

        // 2. reproducción: avanzar el reloj
        if playing && let Some(target) = tv2_domain::review::skip_target(&skip_ranges, position) {
            position = target.min(timeline.duration);
            if position >= timeline.duration || loop_range.is_some_and(|l| position >= l.end) {
                playing = false;
            }
            generation = next_generation(&audio_out, generation);
            mixer = None;
            clock = None;
            dirty = true;
        }
        if playing {
            let audio_possible = audio_out.is_some() && !muted && rate <= 4.0 && timeline.pieces.iter().any(|p| !p.audio.is_empty());
            if clock.is_none() {
                let frames = audio_out.as_ref().map(|a| a.frames_played.load(Ordering::Relaxed)).unwrap_or(0);
                clock = Some(Clock { anchor_pos: position, anchor_instant: Instant::now(), anchor_frames: frames, rate, use_audio: audio_possible });
                if audio_possible {
                    generation = audio_out.as_ref().unwrap().current_generation();
                    mixer = Some(AudioMixer::new(tools.clone(), timeline.clone(), assets.clone(), out_rate, position, rate));
                    if let Some(m) = &mut mixer {
                        m.cancel = cancel.clone();
                    }
                }
            }
            // alimentar audio (hasta ~400 ms por delante)
            if let (Some(a), Some(m)) = (&audio_out, mixer.as_mut()) {
                let target_queue = (out_rate as f64 * 0.4) as u64;
                let mut guard = 0;
                while a.queued() < target_queue && guard < 8 {
                    if tv2_domain::review::skip_target(&skip_ranges, m.cursor()).is_some() {
                        break;
                    }
                    let boundary = skip_ranges.iter().find(|r| r.start > m.cursor()).map(|r| r.start);
                    let frames =
                        boundary.map(|b| (((b - m.cursor()).as_seconds_f64() / rate * out_rate as f64).ceil() as usize).min(1024)).unwrap_or(1024);
                    if frames == 0 {
                        break;
                    }
                    let mut buf = vec![0f32; frames * CHANNELS];
                    let n = m.mix(&mut buf);
                    if n == 0 {
                        break;
                    }
                    buf.truncate(n * CHANNELS);
                    if !a.push(generation, buf) {
                        break;
                    }
                    guard += 1;
                }
            }
            let c = clock.as_ref().unwrap();
            let new_pos = if c.use_audio {
                let frames = audio_out.as_ref().unwrap().frames_played.load(Ordering::Relaxed);
                let played = frames.saturating_sub(c.anchor_frames);
                c.anchor_pos + Ticks((played as f64 / out_rate as f64 * c.rate * tv2_domain::time::FLICKS_PER_SECOND as f64).round() as i64)
            } else {
                c.anchor_pos + Ticks((c.anchor_instant.elapsed().as_secs_f64() * c.rate * tv2_domain::time::FLICKS_PER_SECOND as f64).round() as i64)
            };
            position = new_pos;
            if let Some(target) = tv2_domain::review::skip_target(&skip_ranges, position) {
                position = target.min(timeline.duration);
                generation = next_generation(&audio_out, generation);
                mixer = None;
                clock = None;
                // A fully trimmed loop cannot spin forever.
                if loop_range
                    .is_some_and(|l| position >= l.end && tv2_domain::review::skip_target(&skip_ranges, l.start).is_some_and(|end| end >= l.end))
                {
                    playing = false;
                }
            }
            // loop / fin
            if let Some(l) = loop_range
                && position >= l.end
            {
                position = l.start;
                generation = next_generation(&audio_out, generation);
                mixer = None;
                clock = None;
            } else if position >= timeline.duration {
                position = timeline.duration;
                playing = false;
                generation = next_generation(&audio_out, generation);
                mixer = None;
                clock = None;
            }
            dirty = true;
        }

        // 3. presentar si el fotograma cambió
        let frame_pos = position.floor_to_frame(timeline.frame_rate);
        if dirty && (last_presented != Some(frame_pos) || !playing) {
            let started = Instant::now();
            let deadline = if playing { Duration::from_millis(60) } else { Duration::from_millis(1500) };
            let gen_before = generation;
            let (frame, complete) = r.render(frame_pos, deadline);
            buffering = !complete;
            last_presented = Some(frame_pos);
            // si llegó un seek nuevo mientras decodificábamos, este fotograma es viejo
            let stale = rx.try_recv_peek();
            if !stale {
                publish(&shared, PresentedFrame { position: frame_pos, generation: gen_before, frame, complete });
                wake();
            }
            {
                let mut s = shared.snapshot.lock();
                s.decode_ms = started.elapsed().as_secs_f32() * 1000.0;
            }
            dirty = playing;
        }
        {
            let mut s = shared.snapshot.lock();
            s.position = position;
            s.playing = playing;
            s.rate = rate;
            s.generation = generation;
            s.buffering = buffering;
            s.audio_available = audio_out.is_some();
            s.muted = muted;
            s.loop_range = loop_range;
            if let Some(e) = last_error.take() {
                s.last_error = Some(e);
            }
        }
        if playing {
            // dormir hasta el siguiente fotograma (o antes si hay comandos)
            let fd = timeline.frame_rate.frame_duration().as_seconds_f64() / rate;
            let sleep = Duration::from_secs_f64((fd / 2.0).clamp(0.002, 0.02));
            if let Ok(c) = rx.recv_timeout(sleep) {
                carry = Some(c); // se procesa al inicio del próximo ciclo
            }
        }
    }
}

trait PeekExt {
    fn try_recv_peek(&self) -> bool;
}

impl PeekExt for Receiver<PlayerCommand> {
    /// `true` si hay un Seek/Scrub pendiente (sin consumirlo). Como crossbeam no
    /// permite peek, usamos `len()`: cualquier comando pendiente fuerza un nuevo ciclo.
    fn try_recv_peek(&self) -> bool {
        !self.is_empty()
    }
}
