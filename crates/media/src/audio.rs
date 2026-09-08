//! Audio: decodificación por proceso FFmpeg (f32 intercalado a la frecuencia de
//! la secuencia, con `atempo` para velocidades > 1 preservando el tono),
//! mezclador de la timeline resuelta y salida por `cpal`.

use crate::ffmpeg::FfmpegTools;
use crate::render::AssetSource;
use crossbeam_channel::{Receiver, Sender, bounded};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tv2_domain::error::{DomainError, DomainResult};
use tv2_domain::ids::ClipId;
use tv2_domain::resolve::ResolvedTimeline;
use tv2_domain::time::Ticks;

pub const CHANNELS: usize = 2;

/// Filtro de velocidad como `medios.filtro_velocidad` de V1: `atempo` en cadena
/// (cada etapa ≤ 2.0 conserva calidad; FFmpeg admite hasta 100 en una sola).
pub fn tempo_filter(rate: f64) -> Option<String> {
    if (rate - 1.0).abs() < 1e-6 {
        return None;
    }
    let mut stages = Vec::new();
    let mut r = rate;
    while r > 2.0 {
        stages.push("atempo=2.0".to_string());
        r /= 2.0;
    }
    stages.push(format!("atempo={r:.6}"));
    Some(stages.join(","))
}

pub struct AudioDecoder {
    child: crate::process::CancellableChild,
    rx: Receiver<Vec<f32>>,
    pub sample_rate: u32,
    pub start: Ticks,
    pub rate: f64,
    /// Muestras (frames) ya devueltas.
    consumed: u64,
    pending: Vec<f32>,
    pending_pos: usize,
    finished: bool,
    stop: Arc<AtomicBool>,
    error: Option<DomainError>,
}

impl AudioDecoder {
    pub fn open(tools: &FfmpegTools, path: &Path, stream_index: u32, start: Ticks, sample_rate: u32, rate: f64) -> DomainResult<AudioDecoder> {
        Self::open_cancellable(tools, path, stream_index, start, sample_rate, rate, Arc::new(AtomicBool::new(false)))
    }

    pub fn open_cancellable(
        tools: &FfmpegTools,
        path: &Path,
        stream_index: u32,
        start: Ticks,
        sample_rate: u32,
        rate: f64,
        cancel: Arc<AtomicBool>,
    ) -> DomainResult<AudioDecoder> {
        let mut cmd = tools.ffmpeg_cmd();
        cmd.args(["-ss", &format!("{:.6}", start.max(Ticks::ZERO).as_seconds_f64())]);
        cmd.arg("-i").arg(path);
        cmd.args(["-vn", "-sn", "-dn", "-map", &format!("0:a:{stream_index}")]);
        let mut af = Vec::new();
        if let Some(t) = tempo_filter(rate) {
            af.push(t);
        }
        // PCM por pipe no lleva PTS: materializar el silencio anterior al stream.
        // Se hace ANTES de atempo para que el offset también respete la velocidad.
        af.insert(0, format!("aresample={sample_rate}:async=1:first_pts=0"));
        cmd.args(["-af", &af.join(",")]);
        cmd.args(["-ac", &CHANNELS.to_string(), "-ar", &sample_rate.to_string(), "-f", "f32le", "-"]);
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = crate::process::CancellableChild::spawn(&mut cmd, cancel)
            .map_err(|e| DomainError::process(format!("no se pudo iniciar ffmpeg (audio): {e}")))?;
        let mut stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = bounded::<Vec<f32>>(16);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_r = stop.clone();
        std::thread::Builder::new()
            .name("audio-decoder".into())
            .spawn(move || {
                let chunk_frames = 2048usize;
                let mut buf = vec![0u8; chunk_frames * CHANNELS * 4];
                loop {
                    if stop_r.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut filled = 0;
                    let mut eof = false;
                    while filled < buf.len() {
                        match stdout.read(&mut buf[filled..]) {
                            Ok(0) => {
                                eof = true;
                                break;
                            }
                            Ok(n) => filled += n,
                            Err(_) => {
                                eof = true;
                                break;
                            }
                        }
                    }
                    let usable = filled - (filled % (CHANNELS * 4));
                    if usable > 0 {
                        let samples: Vec<f32> =
                            buf[..usable].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                        if tx.send(samples).is_err() {
                            break;
                        }
                    }
                    if eof {
                        break;
                    }
                }
            })
            .map_err(|e| DomainError::process(e.to_string()))?;
        Ok(AudioDecoder { child, rx, sample_rate, start, rate, consumed: 0, pending: Vec::new(), pending_pos: 0, finished: false, stop, error: None })
    }

    /// Tiempo fuente de la siguiente muestra.
    pub fn next_time(&self) -> Ticks {
        self.start + Ticks((self.consumed as f64 * self.rate / self.sample_rate as f64 * tv2_domain::time::FLICKS_PER_SECOND as f64).round() as i64)
    }

    /// Rellena `out` (intercalado estéreo) con hasta `out.len()/2` frames; devuelve frames escritos.
    /// Bloquea hasta tener datos o fin de stream.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let mut written = 0;
        while written < out.len() {
            if self.pending_pos >= self.pending.len() {
                if self.finished {
                    break;
                }
                match self.rx.recv() {
                    Ok(chunk) => {
                        self.pending = chunk;
                        self.pending_pos = 0;
                    }
                    Err(_) => {
                        self.finished = true;
                        match self.child.wait() {
                            Ok(status) if status.success() => {}
                            Ok(status) => self.error = Some(DomainError::process(format!("FFmpeg audio terminó con {status}"))),
                            Err(e) => self.error = Some(DomainError::process(format!("no se pudo comprobar FFmpeg audio: {e}"))),
                        }
                        break;
                    }
                }
                continue;
            }
            let n = (out.len() - written).min(self.pending.len() - self.pending_pos);
            out[written..written + n].copy_from_slice(&self.pending[self.pending_pos..self.pending_pos + n]);
            self.pending_pos += n;
            written += n;
        }
        self.consumed += (written / CHANNELS) as u64;
        written / CHANNELS
    }

    /// Salta `frames` muestras (para alinear con un tiempo objetivo).
    pub fn skip(&mut self, frames: usize) {
        let mut tmp = vec![0f32; 1024 * CHANNELS];
        let mut left = frames;
        while left > 0 {
            let n = left.min(1024);
            let got = self.read(&mut tmp[..n * CHANNELS]);
            if got == 0 {
                break;
            }
            left -= got;
        }
    }
}

impl Drop for AudioDecoder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Mezcla la timeline resuelta a estéreo f32 a `sample_rate`, en orden de
/// secuencia, empezando en `position` y a velocidad `rate` (con tono
/// preservado). Idéntico para visor y export (export siempre rate = 1).
pub struct AudioMixer {
    tools: FfmpegTools,
    timeline: Arc<ResolvedTimeline>,
    assets: Arc<HashMap<tv2_domain::ids::AssetId, AssetSource>>,
    pub sample_rate: u32,
    pub rate: f64,
    /// Próximo tiempo de secuencia a producir.
    cursor: Ticks,
    decoders: HashMap<ClipId, AudioDecoder>,
    scratch: Vec<f32>,
    error: Option<DomainError>,
    pub cancel: Arc<AtomicBool>,
}

impl AudioMixer {
    pub fn new(
        tools: FfmpegTools,
        timeline: Arc<ResolvedTimeline>,
        assets: Arc<HashMap<tv2_domain::ids::AssetId, AssetSource>>,
        sample_rate: u32,
        position: Ticks,
        rate: f64,
    ) -> AudioMixer {
        AudioMixer {
            tools,
            timeline,
            assets,
            sample_rate,
            rate,
            cursor: position,
            decoders: HashMap::new(),
            scratch: Vec::new(),
            error: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn take_error(&mut self) -> Option<DomainError> {
        self.error.take()
    }

    pub fn cursor(&self) -> Ticks {
        self.cursor
    }

    pub fn is_finished(&self) -> bool {
        self.cursor >= self.timeline.duration
    }

    /// Produce `frames` frames estéreo en `out` (debe medir `frames*2`). Devuelve
    /// los frames producidos (menos que `frames` solo al terminar la secuencia).
    pub fn mix(&mut self, out: &mut [f32]) -> usize {
        let frames = out.len() / CHANNELS;
        out.iter_mut().for_each(|s| *s = 0.0);
        let mut produced = 0usize;
        while produced < frames {
            if self.cursor >= self.timeline.duration {
                break;
            }
            let piece = match self.timeline.piece_at(self.cursor) {
                Some(p) => p.clone(),
                None => break,
            };
            // frames hasta el fin del tramo a esta velocidad
            let remaining_seq = piece.range.end - self.cursor;
            let remaining_frames = ((remaining_seq.as_seconds_f64() / self.rate) * self.sample_rate as f64).ceil().max(1.0) as usize;
            let n = remaining_frames.min(frames - produced);
            let active: Vec<ClipId> = piece.audio.iter().map(|c| c.clip_id.clone()).collect();
            self.decoders.retain(|id, _| active.contains(id));
            for rc in &piece.audio {
                let Some(src) = self.assets.get(&rc.asset_id) else {
                    self.error = Some(DomainError::not_found("medio de audio", &rc.asset_id));
                    continue;
                };
                let source_t = rc.source_at(self.cursor);
                let need_open = match self.decoders.get(&rc.clip_id) {
                    None => true,
                    Some(d) => {
                        let drift = (d.next_time() - source_t).abs();
                        drift > Ticks::from_millis(40)
                    }
                };
                if need_open {
                    match AudioDecoder::open_cancellable(
                        &self.tools,
                        &src.path,
                        rc.audio_stream,
                        source_t,
                        self.sample_rate,
                        self.rate,
                        self.cancel.clone(),
                    ) {
                        Ok(d) => {
                            self.decoders.insert(rc.clip_id.clone(), d);
                        }
                        Err(e) => {
                            tracing::warn!(clip = %rc.clip_id, "audio: {e}");
                            self.error = Some(e.with("asset_id", rc.asset_id.to_string()));
                            continue;
                        }
                    }
                }
                let d = self.decoders.get_mut(&rc.clip_id).unwrap();
                self.scratch.clear();
                self.scratch.resize(n * CHANNELS, 0.0);
                let got = d.read(&mut self.scratch);
                if let Some(error) = d.error.take() {
                    self.error = Some(error.with("asset_id", rc.asset_id.to_string()).with("path", src.path.display().to_string()));
                }
                let gain = rc.gain;
                for i in 0..got * CHANNELS {
                    out[(produced * CHANNELS) + i] += self.scratch[i] * gain;
                }
            }
            produced += n;
            self.cursor += Ticks((n as f64 * self.rate / self.sample_rate as f64 * tv2_domain::time::FLICKS_PER_SECOND as f64).round() as i64);
            if self.cursor > piece.range.end || (piece.range.end - self.cursor) < Ticks::from_samples(1, self.sample_rate) {
                self.cursor = piece.range.end;
            }
        }
        // limitador suave
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
        produced
    }
}

/// Bloque de audio listo para el dispositivo, etiquetado con generación.
pub struct AudioChunk {
    pub generation: u64,
    pub samples: Vec<f32>,
}

/// Salida de audio con reloj: cuenta los frames realmente reproducidos de la
/// generación vigente; en underrun emite silencio y **no** avanza el reloj.
pub struct AudioOutput {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    tx: Sender<AudioChunk>,
    pub generation: Arc<AtomicU64>,
    pub frames_played: Arc<AtomicU64>,
    pub queued_frames: Arc<AtomicU64>,
}

impl AudioOutput {
    pub fn open() -> DomainResult<AudioOutput> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| DomainError::unsupported("no hay dispositivo de salida de audio"))?;
        let default = device.default_output_config().map_err(|e| DomainError::unsupported(format!("configuración de audio: {e}")))?;
        let sample_rate = default.sample_rate();
        let sample_rate_hz: u32 = sample_rate;
        let config = cpal::StreamConfig { channels: CHANNELS as u16, sample_rate, buffer_size: cpal::BufferSize::Default };
        let (tx, rx) = bounded::<AudioChunk>(64);
        let generation = Arc::new(AtomicU64::new(0));
        let frames_played = Arc::new(AtomicU64::new(0));
        let queued = Arc::new(AtomicU64::new(0));
        let (g, fp, q) = (generation.clone(), frames_played.clone(), queued.clone());
        let mut current: Option<AudioChunk> = None;
        let mut pos = 0usize;
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let gen_now = g.load(Ordering::Relaxed);
                    let mut written = 0usize;
                    while written < data.len() {
                        if current.as_ref().is_none_or(|c| pos >= c.samples.len()) {
                            match rx.try_recv() {
                                Ok(chunk) => {
                                    q.fetch_sub((chunk.samples.len() / CHANNELS) as u64, Ordering::Relaxed);
                                    if chunk.generation != gen_now {
                                        current = None;
                                        continue;
                                    }
                                    current = Some(chunk);
                                    pos = 0;
                                }
                                Err(_) => break,
                            }
                        }
                        let c = current.as_ref().unwrap();
                        if c.generation != gen_now {
                            current = None;
                            continue;
                        }
                        let n = (data.len() - written).min(c.samples.len() - pos);
                        data[written..written + n].copy_from_slice(&c.samples[pos..pos + n]);
                        pos += n;
                        written += n;
                        fp.fetch_add((n / CHANNELS) as u64, Ordering::Relaxed);
                    }
                    for s in &mut data[written..] {
                        *s = 0.0;
                    }
                },
                |e| tracing::warn!("audio: {e}"),
                None,
            )
            .map_err(|e| DomainError::unsupported(format!("no se pudo abrir la salida de audio: {e}")))?;
        stream.play().map_err(|e| DomainError::unsupported(format!("audio: {e}")))?;
        Ok(AudioOutput { _stream: stream, sample_rate: sample_rate_hz, tx, generation, frames_played, queued_frames: queued })
    }

    /// Cambia de generación: los bloques anteriores se descartan al llegar al callback.
    pub fn flush(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    pub fn push(&self, generation: u64, samples: Vec<f32>) -> bool {
        let frames = (samples.len() / CHANNELS) as u64;
        match self.tx.try_send(AudioChunk { generation, samples }) {
            Ok(()) => {
                self.queued_frames.fetch_add(frames, Ordering::Relaxed);
                true
            }
            Err(_) => false,
        }
    }

    pub fn queued(&self) -> u64 {
        self.queued_frames.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media").join(name)
    }

    #[test]
    fn tempo_chain_matches_v1_policy() {
        assert_eq!(tempo_filter(1.0), None);
        assert_eq!(tempo_filter(2.0).unwrap(), "atempo=2.000000");
        assert_eq!(tempo_filter(3.0).unwrap(), "atempo=2.0,atempo=1.500000");
        assert_eq!(tempo_filter(4.0).unwrap(), "atempo=2.0,atempo=2.000000");
    }

    /// Instantes (s) en que empieza cada ráfaga de la fixture de sincronía (1 kHz, 60 ms cada segundo).
    fn burst_onsets(d: &mut AudioDecoder, sample_rate: u32, max_secs: f64) -> Vec<f64> {
        let mut onsets = Vec::new();
        let mut buf = vec![0f32; 4096 * CHANNELS];
        let mut idx: u64 = 0;
        let mut quiet_run: u64 = sample_rate as u64; // empieza «en silencio»
        loop {
            let n = d.read(&mut buf);
            if n == 0 {
                break;
            }
            for f in 0..n {
                let s = buf[f * CHANNELS].abs();
                if s > 0.3 {
                    if quiet_run > (sample_rate / 50) as u64 {
                        onsets.push(idx as f64 / sample_rate as f64);
                    }
                    quiet_run = 0;
                } else {
                    quiet_run += 1;
                }
                idx += 1;
            }
            if idx as f64 / sample_rate as f64 > max_secs {
                break;
            }
        }
        onsets
    }

    /// F-001: la ruta de audio acelerado (`atempo`) conserva la posición temporal:
    /// una ráfaga en el segundo `k` de la fuente suena en `k / rate` con desvío acotado.
    #[test]
    fn atempo_path_keeps_time_alignment_within_tolerance() {
        let tools = FfmpegTools::locate().unwrap();
        let mut lines = Vec::new();
        for rate in [1.0f64, 2.0, 3.0, 4.0] {
            let mut d = AudioDecoder::open(&tools, &fixture("fixture-sync.mp4"), 0, Ticks::ZERO, 48000, rate).unwrap();
            let onsets = burst_onsets(&mut d, 48000, 12.0 / rate + 0.5);
            assert!(onsets.len() >= (10.0 / rate) as usize, "rate {rate}: {onsets:?}");
            let mut worst = 0.0f64;
            for (k, t) in onsets.iter().enumerate() {
                let expected = k as f64 / rate;
                let err = (t - expected).abs() * 1000.0;
                worst = worst.max(err);
            }
            lines.push(format!("rate {rate}: {} ráfagas, desvío máximo {worst:.1} ms", onsets.len()));
            assert!(worst <= 40.0, "rate {rate}: desvío {worst:.1} ms ({onsets:?})");
        }
        // también desde un `-ss` intermedio (2,5 s): la primera ráfaga (3,0 s) cae a 0,5 s de salida
        let mut d = AudioDecoder::open(&tools, &fixture("fixture-sync.mp4"), 0, Ticks::from_millis(2500), 48000, 2.0).unwrap();
        let onsets = burst_onsets(&mut d, 48000, 2.0);
        assert!(!onsets.is_empty());
        let err = (onsets[0] - 0.25).abs() * 1000.0;
        lines.push(format!("rate 2 desde 2,5 s: primera ráfaga a {:.3} s (esperado 0,250), desvío {err:.1} ms", onsets[0]));
        assert!(err <= 40.0, "{err}");
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../implementation/evidence/e2/av-sync-atempo.txt");
        let _ = std::fs::write(&out, lines.join("\n") + "\n");
        println!("{}", lines.join("\n"));
    }

    #[test]
    fn delayed_audio_stream_preserves_leading_silence_after_seek_and_rate_change() {
        let tools = FfmpegTools::locate().unwrap();
        for (start_ms, rate, expected) in [(0, 1.0, 0.25), (100, 1.0, 0.15), (0, 2.0, 0.125)] {
            let mut decoder = AudioDecoder::open(&tools, &fixture("fixture-audio-delay.mp4"), 0, Ticks::from_millis(start_ms), 48000, rate).unwrap();
            let onsets = burst_onsets(&mut decoder, 48000, 2.0);
            assert!(!onsets.is_empty());
            assert!((onsets[0] - expected).abs() < 0.025, "start={start_ms}, rate={rate}, onsets={onsets:?}");
        }
    }

    #[test]
    fn decodes_tone_and_rate_2x_halves_length() {
        let tools = FfmpegTools::locate().unwrap();
        let mut d = AudioDecoder::open(&tools, &fixture("tone-220.wav"), 0, Ticks::from_seconds(9), 48000, 1.0).unwrap();
        let mut buf = vec![0f32; 48000 * 2 * 2];
        let mut total = 0;
        loop {
            let n = d.read(&mut buf);
            if n == 0 {
                break;
            }
            total += n;
        }
        assert!((47_000..=49_000).contains(&total), "total={total}");
        let mut d2 = AudioDecoder::open(&tools, &fixture("tone-220.wav"), 0, Ticks::ZERO, 48000, 2.0).unwrap();
        let mut total2 = 0;
        loop {
            let n = d2.read(&mut buf);
            if n == 0 {
                break;
            }
            total2 += n;
        }
        // 10 s a 2× ≈ 5 s de audio
        assert!((235_000..=245_000).contains(&total2), "total2={total2}");
        // el tono no es silencio
        let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "peak={peak}"); // sine de FFmpeg: ≈ -21 dBFS
    }
}
