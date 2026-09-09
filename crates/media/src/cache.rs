//! Cachés progresivas de forma de onda y miniaturas por asset y nivel de detalle.
//!
//! - Se calculan en hilos de fondo con un proceso FFmpeg cada una (como máximo
//!   `MAX_CONCURRENT` a la vez, para no competir con el reproductor).
//! - Publican resultados parciales: la UI dibuja lo que ya está listo y pide
//!   solo lo visible (índice temporal → bucket/miniatura).
//! - Persisten en `cache/` (del proyecto o del usuario) con clave por identidad
//!   del medio (`hash_muestreado` + tamaño): mover el archivo no invalida la caché.
//!   Son regenerables y tienen presupuesto (`prune_dir`).
//!
//! Forma de onda: picos absolutos (u8) a `WAVE_BPS` buckets/s (nivel 0) y una
//! pirámide de niveles con factor 4 (máximo por bloque). `peaks(range, columnas)`
//! elige el nivel que deja ≤ 4 buckets por columna, de modo que el coste por
//! fotograma es proporcional a los píxeles visibles, no a la duración del medio.
//!
//! Miniaturas: dos niveles (grueso y fino) de intervalo fijo, decodificadas a
//! tamaño pequeño en orden temporal; el nivel grueso llega primero.

use crate::ffmpeg::FfmpegTools;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tv2_domain::asset::{Asset, AssetKind};
use tv2_domain::ids::AssetId;
use tv2_domain::time::{FLICKS_PER_SECOND, Ticks, TimeRange};

/// Buckets por segundo del nivel 0 de la forma de onda (3,90625 ms).
pub const WAVE_BPS: u32 = 256;
/// Frecuencia de decodificación para la forma de onda (32 muestras por bucket).
const WAVE_RATE: u32 = WAVE_BPS * 32;
const WAVE_MAX_BUCKETS: usize = 8 * 1024 * 1024;

fn wave_bucket_samples(duration: Ticks) -> u64 {
    let mut samples = u64::from(WAVE_RATE / WAVE_BPS);
    while duration.as_seconds_f64().max(0.0) * f64::from(WAVE_RATE) / samples as f64 > (WAVE_MAX_BUCKETS - WAVE_BPS as usize) as f64 {
        samples *= 2;
    }
    samples
}
/// Factor entre niveles de la pirámide.
const WAVE_FACTOR: usize = 4;
const WAVE_LEVELS: usize = 16; // incluye medios largos con coste por píxel acotado
pub const THUMB_W: u32 = 96;
/// Máximo de miniaturas del nivel fino; el intervalo crece con la duración.
const THUMB_FINE_MAX: i64 = 720;
const THUMB_COARSE_RATIO: i64 = 8;
pub const MAX_CONCURRENT: usize = 2;
/// Presupuesto de la carpeta de caché (bytes); se borran los archivos más antiguos.
pub const CACHE_BUDGET: u64 = 1024 * 1024 * 1024;
/// Buffers CPU de cachés, incluyendo copias temporales/LOD. FFmpeg/GPU se miden aparte.
pub const MEMORY_BUDGET: usize = 256 * 1024 * 1024;
pub const MAX_PENDING: usize = 64;

#[derive(Debug)]
struct Reservation {
    used: Arc<AtomicUsize>,
    bytes: usize,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[derive(Debug)]
pub struct WaveformData {
    /// Density is reduced for long media; every bucket retains its maximum peak.
    pub levels: Vec<Vec<u8>>,
    /// Buckets del nivel 0 ya calculados.
    pub ready: usize,
    pub complete: bool,
    pub error: Option<String>,
    bucket_samples: u64,
    reservation: Option<Arc<Reservation>>,
    budget_blocked: bool,
}

impl Default for WaveformData {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveformData {
    fn new() -> Self {
        WaveformData {
            levels: vec![Vec::new(); WAVE_LEVELS],
            ready: 0,
            complete: false,
            error: None,
            bucket_samples: u64::from(WAVE_RATE / WAVE_BPS),
            reservation: None,
            budget_blocked: false,
        }
    }

    fn from_level0(level0: Vec<u8>, bucket_samples: u64) -> Self {
        let mut d = WaveformData::new();
        d.bucket_samples = bucket_samples;
        d.ready = level0.len();
        d.levels[0] = level0;
        d.rebuild_levels(0);
        d.complete = true;
        d
    }

    /// Recalcula los niveles superiores a partir del bucket `from` del nivel 0.
    fn rebuild_levels(&mut self, from: usize) {
        for i in 1..WAVE_LEVELS {
            let factor = WAVE_FACTOR.pow(i as u32);
            let start = from / factor;
            let n = self.levels[0].len() / factor; // solo bloques completos
            let (lower, upper) = self.levels.split_at_mut(i);
            let src = &lower[0];
            let dst = &mut upper[0];
            dst.truncate(start);
            for b in start..n {
                let block = &src[b * factor..(b + 1) * factor];
                dst.push(block.iter().copied().max().unwrap_or(0));
            }
        }
    }

    pub fn level_bps(&self, level: usize) -> f64 {
        WAVE_RATE as f64 / self.bucket_samples as f64 / (WAVE_FACTOR.pow(level as u32) as f64)
    }

    /// Pico máximo (0..255) por columna para `range` repartido en `columns`
    /// columnas; `None` en columnas aún no calculadas. Coste O(columns).
    pub fn peaks(&self, range: TimeRange, columns: usize) -> Vec<Option<u8>> {
        let mut out = vec![None; columns];
        if columns == 0 || range.duration().0 <= 0 || self.levels.is_empty() {
            return out;
        }
        let secs_per_col = range.duration().as_seconds_f64() / columns as f64;
        // nivel más fino con ≤ 4 buckets por columna
        let mut level = 0;
        while level + 1 < self.levels.len() && !self.levels[level + 1].is_empty() && secs_per_col * self.level_bps(level) > 4.0 {
            level += 1;
        }
        let bps = self.level_bps(level);
        let data = &self.levels[level];
        let ready_here = if self.complete { data.len() } else { (self.ready / WAVE_FACTOR.pow(level as u32)).min(data.len()) };
        let start_s = range.start.as_seconds_f64();
        for (i, slot) in out.iter_mut().enumerate() {
            let t0 = start_s + i as f64 * secs_per_col;
            let t1 = t0 + secs_per_col;
            let b0 = (t0 * bps).floor().max(0.0) as usize;
            let b1 = ((t1 * bps).ceil() as usize).max(b0 + 1);
            if b0 >= ready_here {
                continue;
            }
            let b1 = b1.min(ready_here);
            *slot = data[b0..b1].iter().copied().max();
        }
        out
    }
}

pub type WaveformHandle = Arc<RwLock<WaveformData>>;

#[derive(Debug)]
pub struct ThumbLevel {
    pub interval: Ticks,
    pub width: u32,
    pub height: u32,
    /// Miniaturas previstas.
    pub count: usize,
    /// RGBA contiguo, `ready * width * height * 4` bytes válidos.
    pub rgba: Vec<u8>,
    pub ready: usize,
}

impl ThumbLevel {
    /// Índice de la miniatura para un tiempo fuente (`None` si aún no está).
    pub fn index_at(&self, t: Ticks) -> Option<usize> {
        if self.interval.0 <= 0 || t.is_negative() {
            return None;
        }
        let i = ((t.0 / self.interval.0) as usize).min(self.count.saturating_sub(1));
        (i < self.ready).then_some(i)
    }
}

#[derive(Debug, Default)]
pub struct ThumbData {
    /// `[grueso, fino]`.
    pub levels: Vec<ThumbLevel>,
    pub complete: bool,
    pub error: Option<String>,
    reservation: Option<Arc<Reservation>>,
    budget_blocked: bool,
}

impl ThumbData {
    /// Nivel a dibujar: el más fino que ya tenga datos (los niveles van de grueso
    /// a fino; el grueso solo se usa mientras el fino no exista). `min_interval`
    /// (separación visible entre miniaturas) permite quedarse en el grueso si ya
    /// basta, evitando muestrear el fino sin necesidad.
    pub fn best_level(&self, min_interval: Ticks) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, l) in self.levels.iter().enumerate() {
            if l.ready == 0 {
                continue;
            }
            if let Some(b) = best
                && l.interval < min_interval
                && self.levels[b].interval <= min_interval
            {
                break;
            }
            best = Some(i);
        }
        best
    }
}

pub type ThumbHandle = Arc<RwLock<ThumbData>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum JobKey {
    Wave(AssetId, u32),
    Thumb(AssetId),
}

struct Job {
    key: JobKey,
    run: Box<dyn FnOnce(Arc<AtomicBool>) + Send>,
}

struct Running {
    key: JobKey,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

/// Coordinador de cachés: cola acotada de trabajos de fondo y handles compartidos.
pub struct MediaCaches {
    tools: FfmpegTools,
    dir: Option<PathBuf>,
    wake: Arc<dyn Fn() + Send + Sync>,
    waveforms: HashMap<(AssetId, u32), WaveformHandle>,
    thumbs: HashMap<AssetId, ThumbHandle>,
    queue: VecDeque<Job>,
    running: Vec<Running>,
    used_memory: Arc<AtomicUsize>,
    memory_budget: usize,
}

impl MediaCaches {
    pub fn new(tools: FfmpegTools, dir: Option<PathBuf>, wake: Arc<dyn Fn() + Send + Sync>) -> MediaCaches {
        MediaCaches {
            tools,
            dir,
            wake,
            waveforms: HashMap::new(),
            thumbs: HashMap::new(),
            queue: VecDeque::new(),
            running: Vec::new(),
            used_memory: Arc::new(AtomicUsize::new(0)),
            memory_budget: MEMORY_BUDGET,
        }
    }

    pub fn reserved_memory(&self) -> usize {
        self.used_memory.load(Ordering::Acquire)
    }

    fn reserve(&self, bytes: usize) -> Option<Arc<Reservation>> {
        if self.queue.len() >= MAX_PENDING {
            return None;
        }
        self.used_memory
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| used.checked_add(bytes).filter(|next| *next <= self.memory_budget))
            .ok()?;
        Some(Arc::new(Reservation { used: self.used_memory.clone(), bytes }))
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Cambia la carpeta de caché (al guardar/abrir un proyecto). Los datos en
    /// memoria se conservan; los trabajos futuros persisten en la nueva carpeta.
    pub fn set_dir(&mut self, dir: Option<PathBuf>) {
        if self.dir != dir {
            self.dir = dir;
        }
    }

    /// Cancela todo y olvida los datos (proyecto nuevo).
    pub fn clear(&mut self) {
        self.queue.clear();
        for r in &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        // Los cancelados siguen ocupando su plaza hasta que terminan.
        self.waveforms.clear();
        self.thumbs.clear();
    }

    pub fn forget_asset(&mut self, asset: &AssetId) {
        self.queue.retain(|j| !matches!(&j.key, JobKey::Wave(a, _) | JobKey::Thumb(a) if a == asset));
        for r in self.running.iter().filter(|r| matches!(&r.key, JobKey::Wave(a, _) | JobKey::Thumb(a) if a == asset)) {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.waveforms.retain(|(a, _), _| a != asset);
        self.thumbs.remove(asset);
    }

    pub fn pending_jobs(&self) -> usize {
        self.queue.len() + self.running.len()
    }

    /// Olvida las cachés de los assets que ya no están en el proyecto.
    pub fn retain_assets(&mut self, keep: &std::collections::HashSet<AssetId>) {
        let gone: Vec<AssetId> = self
            .thumbs
            .keys()
            .chain(self.waveforms.keys().map(|(a, _)| a))
            .filter(|a| !keep.contains(*a))
            .cloned()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        for a in gone {
            self.forget_asset(&a);
        }
    }

    /// Handle existente (sin encolar nada).
    pub fn waveform_handle(&self, asset: &AssetId, stream: u32) -> Option<WaveformHandle> {
        self.waveforms.get(&(asset.clone(), stream)).cloned()
    }

    pub fn thumb_handle(&self, asset: &AssetId) -> Option<ThumbHandle> {
        self.thumbs.get(asset).cloned()
    }

    /// Arranca trabajos en cola si hay hueco. Llamar una vez por fotograma de UI.
    pub fn tick(&mut self) {
        self.running.retain(|r| !r.done.load(Ordering::Relaxed));
        while self.running.len() < MAX_CONCURRENT {
            let Some(job) = self.queue.pop_front() else { break };
            let cancel = Arc::new(AtomicBool::new(false));
            let done = Arc::new(AtomicBool::new(false));
            let (c2, d2) = (cancel.clone(), done.clone());
            let wake = self.wake.clone();
            let run = job.run;
            let spawned = std::thread::Builder::new().name("media-cache".into()).spawn(move || {
                run(c2);
                d2.store(true, Ordering::Relaxed);
                wake();
            });
            if let Ok(thread) = spawned {
                self.running.push(Running { key: job.key, cancel, done, thread });
            }
        }
    }

    fn cache_key(asset: &Asset) -> String {
        let h = &asset.fingerprint.hash_muestreado;
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(format!("media-cache-v2:{h}:{}:{}", asset.fingerprint.size, asset.fingerprint.inventario_sha256)))
    }

    /// Forma de onda de la pista de audio `stream` del asset; encola el cálculo si falta.
    pub fn waveform(&mut self, asset: &Asset, stream: u32, path: &Path) -> WaveformHandle {
        let key = (asset.id.clone(), stream);
        if let Some(h) = self.waveforms.get(&key).cloned() {
            if !h.read().budget_blocked {
                return h;
            }
            self.waveforms.remove(&key);
        }
        let handle: WaveformHandle = Arc::new(RwLock::new(WaveformData::new()));
        self.waveforms.insert(key.clone(), handle.clone());
        let file = self.dir.as_ref().map(|d| d.join(format!("wave-{}-a{}.bin", Self::cache_key(asset), stream)));
        if asset.missing || asset.probe.audio.is_empty() {
            {
                let mut d = handle.write();
                d.complete = true;
                d.error = Some("sin audio".into());
            }
            return handle;
        }
        let bucket_samples = wave_bucket_samples(asset.duration());
        handle.write().bucket_samples = bucket_samples;
        let bytes = ((asset.duration().as_seconds_f64().max(0.0) * WAVE_RATE as f64 / bucket_samples as f64).ceil() as usize)
            .saturating_add(WAVE_BPS as usize)
            .saturating_mul(6)
            .saturating_add(1024 * 1024);
        let Some(reservation) = self.reserve(bytes) else {
            let mut data = handle.write();
            data.complete = true;
            data.error = Some("Límite de cachés: reduce los medios visibles para liberar memoria".into());
            data.budget_blocked = true;
            drop(data);
            return handle;
        };
        handle.write().reservation = Some(reservation);
        let tools = self.tools.clone();
        let path = path.to_path_buf();
        let duration = asset.duration();
        let h2 = handle.clone();
        let wake = self.wake.clone();
        self.queue.push_back(Job {
            key: JobKey::Wave(asset.id.clone(), stream),
            run: Box::new(move |cancel| {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if let Some((level0, bucket_samples)) = file.as_ref().and_then(|f| read_wave_file(f, duration)) {
                    let mut data = h2.write();
                    let reservation = data.reservation.take();
                    *data = WaveformData::from_level0(level0, bucket_samples);
                    data.reservation = reservation;
                    return;
                }
                compute_waveform(&tools, &path, stream, duration, &h2, &cancel, file, &*wake);
            }),
        });
        handle
    }

    /// Miniaturas del asset (video o imagen); encola el cálculo si falta.
    pub fn thumbnails(&mut self, asset: &Asset, path: &Path) -> ThumbHandle {
        if let Some(h) = self.thumbs.get(&asset.id).cloned() {
            if !h.read().budget_blocked {
                return h;
            }
            self.thumbs.remove(&asset.id);
        }
        let handle: ThumbHandle = Arc::new(RwLock::new(ThumbData::default()));
        self.thumbs.insert(asset.id.clone(), handle.clone());
        let Some(v) = asset.probe.video.as_ref() else {
            {
                let mut d = handle.write();
                d.complete = true;
                d.error = Some("sin video".into());
            }
            return handle;
        };
        let (dw, dh) = v.display_size();
        let w = THUMB_W;
        let h = ((THUMB_W as f64 * dh.max(1) as f64 / dw.max(1) as f64).round() as u32).clamp(24, 128) & !1;
        let duration = asset.duration();
        let (coarse, fine) = thumb_intervals(duration, asset.kind == AssetKind::Image);
        let plan: Vec<(Ticks, usize)> = {
            let n_fine = ((duration.0 + fine.0 - 1) / fine.0).max(1) as usize;
            let n_coarse = ((duration.0 + coarse.0 - 1) / coarse.0).max(1) as usize;
            if coarse == fine { vec![(fine, n_fine)] } else { vec![(coarse, n_coarse), (fine, n_fine)] }
        };
        {
            let mut d = handle.write();
            for (interval, count) in &plan {
                d.levels.push(ThumbLevel { interval: *interval, width: w, height: h, count: *count, rgba: Vec::new(), ready: 0 });
            }
        }
        let files: Vec<Option<PathBuf>> =
            (0..plan.len()).map(|i| self.dir.as_ref().map(|d| d.join(format!("thumbs-{}-L{i}.bin", Self::cache_key(asset))))).collect();
        if asset.missing {
            {
                let mut d = handle.write();
                d.complete = true;
                d.error = Some("medio ausente".into());
            }
            return handle;
        }
        let bytes = plan
            .iter()
            .map(|(_, count)| count.saturating_mul(w as usize * h as usize * 4))
            .sum::<usize>()
            .saturating_mul(4)
            .saturating_add(1024 * 1024);
        let Some(reservation) = self.reserve(bytes) else {
            let mut data = handle.write();
            data.complete = true;
            data.error = Some("Límite de cachés: reduce los medios visibles para liberar memoria".into());
            data.budget_blocked = true;
            drop(data);
            return handle;
        };
        handle.write().reservation = Some(reservation);
        let tools = self.tools.clone();
        let path = path.to_path_buf();
        let h2 = handle.clone();
        let wake = self.wake.clone();
        let is_image = asset.kind == AssetKind::Image;
        self.queue.push_back(Job {
            key: JobKey::Thumb(asset.id.clone()),
            run: Box::new(move |cancel| {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                for (i, f) in files.iter().enumerate() {
                    if let Some(rgba) = f.as_ref().and_then(|f| read_thumb_file(f, plan[i].0, w, h, plan[i].1)) {
                        let mut d = h2.write();
                        d.levels[i].ready = plan[i].1;
                        d.levels[i].rgba = rgba;
                    }
                }
                compute_thumbs(&tools, &path, is_image, &h2, &cancel, files, &*wake);
            }),
        });
        handle
    }
}

impl Drop for MediaCaches {
    fn drop(&mut self) {
        self.clear();
        for running in self.running.drain(..) {
            let _ = running.thread.join();
        }
    }
}

/// Intervalos (grueso, fino) de miniaturas según la duración.
pub fn thumb_intervals(duration: Ticks, is_image: bool) -> (Ticks, Ticks) {
    if is_image {
        let d = duration.max(Ticks::from_seconds(1));
        return (d, d);
    }
    let secs = duration.as_seconds_f64().max(1.0);
    let fine_s = (secs / THUMB_FINE_MAX as f64).ceil().max(1.0) as i64;
    let fine = Ticks::from_seconds(fine_s);
    let coarse = if secs / fine_s as f64 > 64.0 { Ticks::from_seconds(fine_s * THUMB_COARSE_RATIO) } else { fine };
    (coarse, fine)
}

#[allow(clippy::too_many_arguments)]
fn compute_waveform(
    tools: &FfmpegTools,
    path: &Path,
    stream: u32,
    duration: Ticks,
    handle: &WaveformHandle,
    cancel: &Arc<AtomicBool>,
    file: Option<PathBuf>,
    wake: &(dyn Fn() + Send + Sync),
) {
    let fail = |msg: String| {
        let mut d = handle.write();
        d.error = Some(msg);
        d.complete = true;
    };
    let samples_per_bucket = wave_bucket_samples(duration);
    handle.write().bucket_samples = samples_per_bucket;
    let mut cmd = tools.ffmpeg_cmd();
    cmd.arg("-i").arg(path);
    cmd.args(["-af", &format!("aresample={WAVE_RATE}:async=1:first_pts=0")]);
    cmd.args(["-vn", "-sn", "-dn", "-map", &format!("0:a:{stream}"), "-ac", "1", "-ar", &WAVE_RATE.to_string(), "-f", "f32le", "-"]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = match crate::process::CancellableChild::spawn(&mut cmd, cancel.clone()) {
        Ok(c) => c,
        Err(e) => return fail(format!("ffmpeg (forma de onda): {e}")),
    };
    let mut stdout = child.stdout.take().expect("stdout");
    let bps = WAVE_RATE as f64 / samples_per_bucket as f64;
    let expected_buckets = ((duration.as_seconds_f64() * bps).ceil() as usize).max(1);
    let mut buf = vec![0u8; WAVE_RATE as usize * 4]; // one second, independent of bucket duration
    let mut level0: Vec<u8> = Vec::with_capacity(expected_buckets);
    let mut bucket_count = 0u64;
    let mut bucket_peak = 0f32;
    let mut last_publish = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return;
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
        let usable = filled - (filled % 4);
        for bytes in buf[..usable].as_chunks::<4>().0 {
            bucket_peak = bucket_peak.max(f32::from_le_bytes(*bytes).abs()).min(1.0);
            bucket_count += 1;
            if bucket_count == samples_per_bucket {
                level0.push((bucket_peak * 255.0).round() as u8);
                bucket_count = 0;
                bucket_peak = 0.0;
            }
        }
        if level0.len() > WAVE_MAX_BUCKETS.min(expected_buckets.saturating_add(bps.ceil() as usize)) {
            let _ = child.kill();
            let _ = child.wait();
            return fail("La forma de onda supera la duración declarada o el presupuesto por stream".into());
        }
        if eof && bucket_count > 0 {
            level0.push((bucket_peak * 255.0).round() as u8);
        }
        if level0.len() - last_publish >= (bps.ceil() as usize).max(1) || eof {
            let mut d = handle.write();
            let from = d.levels[0].len();
            d.levels[0].extend_from_slice(&level0[from..]);
            d.ready = d.levels[0].len();
            d.rebuild_levels(from);
            last_publish = level0.len();
            drop(d);
            wake();
        }
        if eof {
            break;
        }
    }
    let status = child.wait();
    if level0.is_empty() {
        return fail("ffmpeg no produjo audio para la forma de onda".into());
    }
    match status {
        Ok(st) if st.success() => {}
        other => return fail(format!("forma de onda: FFmpeg no terminó correctamente: {other:?}")),
    }
    if let Some(f) = file
        && let Err(e) = write_wave_file(&f, &level0, samples_per_bucket)
    {
        tracing::warn!("caché de forma de onda: {e}");
    }
    handle.write().complete = true;
    wake();
}

#[allow(clippy::too_many_arguments)]
fn compute_thumbs(
    tools: &FfmpegTools,
    path: &Path,
    is_image: bool,
    handle: &ThumbHandle,
    cancel: &Arc<AtomicBool>,
    files: Vec<Option<PathBuf>>,
    wake: &(dyn Fn() + Send + Sync),
) {
    let plan: Vec<(Ticks, u32, u32, usize, usize)> = handle.read().levels.iter().map(|l| (l.interval, l.width, l.height, l.count, l.ready)).collect();
    for (li, (interval, w, h, count, ready)) in plan.into_iter().enumerate() {
        if ready >= count {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut cmd = tools.ffmpeg_cmd();
        cmd.arg("-i").arg(path);
        cmd.args(["-an", "-sn", "-dn"]);
        let vf = if is_image {
            format!("scale={w}:{h}:flags=area,format=rgba")
        } else {
            // fps=1/intervalo, con la primera miniatura en el primer fotograma presentable
            format!("fps=fps={}/{}:start_time=0:round=up,scale={w}:{h}:flags=area,format=rgba", FLICKS_PER_SECOND, interval.0)
        };
        cmd.args(["-vf", &vf, "-frames:v", &count.to_string(), "-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = match crate::process::CancellableChild::spawn(&mut cmd, cancel.clone()) {
            Ok(c) => c,
            Err(e) => {
                let mut d = handle.write();
                d.error = Some(format!("ffmpeg (miniaturas): {e}"));
                d.complete = true;
                return;
            }
        };
        let mut stdout = child.stdout.take().expect("stdout");
        let frame_len = (w * h * 4) as usize;
        let mut frame = vec![0u8; frame_len];
        let mut got = 0usize;
        let mut rgba: Vec<u8> = Vec::with_capacity(frame_len * count);
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            let mut filled = 0;
            let mut eof = false;
            while filled < frame_len {
                match stdout.read(&mut frame[filled..]) {
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
            if filled == frame_len {
                rgba.extend_from_slice(&frame);
                got += 1;
                if got.is_multiple_of(4) || got == count {
                    let mut d = handle.write();
                    let l = &mut d.levels[li];
                    l.rgba = rgba.clone();
                    l.ready = got;
                    drop(d);
                    wake();
                }
            }
            if eof || got >= count {
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        // si el medio es más corto de lo previsto, repetir la última miniatura hasta completar
        while got < count && got > 0 {
            let last = rgba[(got - 1) * frame_len..got * frame_len].to_vec();
            rgba.extend_from_slice(&last);
            got += 1;
        }
        {
            let mut d = handle.write();
            let l = &mut d.levels[li];
            l.rgba = rgba.clone();
            l.ready = got;
        }
        if got == 0 {
            let mut d = handle.write();
            d.error = Some("ffmpeg no produjo miniaturas".into());
            d.complete = true;
            wake();
            return;
        }
        if let Some(Some(f)) = files.get(li)
            && let Err(e) = write_thumb_file(f, interval, w, h, count, &rgba)
        {
            tracing::warn!("caché de miniaturas: {e}");
        }
        wake();
    }
    handle.write().complete = true;
    wake();
}

const WAVE_MAGIC: &[u8; 4] = b"TV2W";
const THUMB_MAGIC: &[u8; 4] = b"TV2T";

fn write_wave_file(path: &Path, level0: &[u8], bucket_samples: u64) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    {
        let f = tmp.as_file_mut();
        f.write_all(WAVE_MAGIC)?;
        f.write_all(&2u32.to_le_bytes())?;
        f.write_all(&bucket_samples.to_le_bytes())?;
        f.write_all(&(level0.len() as u64).to_le_bytes())?;
        f.write_all(level0)?;
        f.flush()?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    if let Some(p) = path.parent() {
        prune_dir(p, CACHE_BUDGET);
    }
    Ok(())
}

fn read_wave_file(path: &Path, duration: Ticks) -> Option<(Vec<u8>, u64)> {
    if std::fs::metadata(path).ok()?.len() > WAVE_MAX_BUCKETS as u64 + 24 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 24 || &bytes[..4] != WAVE_MAGIC || bytes[4..8] != 2u32.to_le_bytes() {
        return None;
    }
    let bucket_samples = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let n = usize::try_from(u64::from_le_bytes(bytes[16..24].try_into().ok()?)).ok()?;
    if bucket_samples != wave_bucket_samples(duration) || n != bytes.len() - 24 {
        return None;
    }
    // coherencia con la duración (±1 s)
    let bps = WAVE_RATE as f64 / bucket_samples as f64;
    let expected = duration.as_seconds_f64() * bps;
    if (n as f64 - expected).abs() > bps.max(1.0) {
        return None;
    }
    Some((bytes[24..24 + n].to_vec(), bucket_samples))
}

fn write_thumb_file(path: &Path, interval: Ticks, w: u32, h: u32, count: usize, rgba: &[u8]) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    {
        let f = tmp.as_file_mut();
        f.write_all(THUMB_MAGIC)?;
        f.write_all(&1u32.to_le_bytes())?;
        f.write_all(&interval.0.to_le_bytes())?;
        f.write_all(&w.to_le_bytes())?;
        f.write_all(&h.to_le_bytes())?;
        f.write_all(&(count as u32).to_le_bytes())?;
        f.write_all(rgba)?;
        f.flush()?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    if let Some(p) = path.parent() {
        prune_dir(p, CACHE_BUDGET);
    }
    Ok(())
}

fn read_thumb_file(path: &Path, interval: Ticks, w: u32, h: u32, count: usize) -> Option<Vec<u8>> {
    let expected = 28u64 + w as u64 * h as u64 * 4 * count as u64;
    if expected > 64 * 1024 * 1024 || std::fs::metadata(path).ok()?.len() != expected {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 28 || &bytes[..4] != THUMB_MAGIC || bytes[4..8] != 1u32.to_le_bytes() {
        return None;
    }
    let iv = i64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let fw = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    let fh = u32::from_le_bytes(bytes[20..24].try_into().ok()?);
    let n = u32::from_le_bytes(bytes[24..28].try_into().ok()?) as usize;
    if iv != interval.0 || fw != w || fh != h || n != count {
        return None;
    }
    let len = (w * h * 4) as usize * count;
    (bytes.len() == 28 + len).then(|| bytes[28..].to_vec())
}

/// Mantiene la carpeta bajo `budget` bytes borrando los archivos menos recientes.
pub fn prune_dir(dir: &Path, budget: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (e.path(), m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= budget {
        return;
    }
    files.sort_by_key(|f| f.2);
    for (p, len, _) in files {
        if total <= budget {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media").join(name)
    }

    fn wait_until(deadline: Duration, mut f: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        f()
    }

    #[test]
    fn memory_admission_holds_reservation_until_last_handle_and_retries_after_release() {
        let tools = FfmpegTools::locate().unwrap();
        let path = fixture("fixture-sync.mp4");
        let asset = tools.import(&path, path.display().to_string()).unwrap();
        let mut caches = MediaCaches::new(tools, None, Arc::new(|| {}));
        caches.memory_budget = 2 * 1024 * 1024;
        let wave = caches.waveform(&asset, 0, &path);
        let mut other = asset.clone();
        other.id = AssetId::new("asset-other");
        assert!(!wave.read().budget_blocked);
        let reserved = caches.reserved_memory();
        assert!(reserved > 1024 * 1024 && reserved < caches.memory_budget);
        let denied = caches.thumbnails(&other, &path);
        assert!(denied.read().budget_blocked);
        assert_eq!(caches.reserved_memory(), reserved);
        caches.forget_asset(&asset.id);
        assert_eq!(caches.reserved_memory(), reserved, "el consumidor aún retiene datos");
        drop(wave);
        assert_eq!(caches.reserved_memory(), 0);
        let thumbs = caches.thumbnails(&other, &path);
        assert!(!thumbs.read().budget_blocked);
        caches.tick();
        assert!(wait_until(Duration::from_secs(10), || thumbs.read().complete));
        assert!(thumbs.read().error.is_none());
        assert!(caches.reserved_memory() <= caches.memory_budget);
        drop(thumbs);
        caches.clear();
        assert!(wait_until(Duration::from_secs(2), || {
            caches.tick();
            caches.reserved_memory() == 0
        }));
    }

    #[test]
    fn waveform_is_progressive_lod_and_persisted() {
        let tools = FfmpegTools::locate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut caches = MediaCaches::new(tools.clone(), Some(dir.path().to_path_buf()), Arc::new(|| {}));
        let path = fixture("fixture-sync.mp4");
        let asset = tools.import(&path, path.to_string_lossy().to_string()).unwrap();
        let h = caches.waveform(&asset, 0, &path);
        caches.tick();
        assert!(wait_until(Duration::from_secs(20), || h.read().complete), "forma de onda completa");
        let d = h.read();
        assert!(d.error.is_none(), "{:?}", d.error);
        // 12 s × 256 = 3072 buckets (±1)
        assert!((3071..=3073).contains(&d.levels[0].len()), "{}", d.levels[0].len());
        assert_eq!(d.levels[1].len(), d.levels[0].len() / 4);
        // ráfaga de 60 ms en cada segundo entero: bucket 0 fuerte, bucket 0,5 s en silencio
        assert!(d.levels[0][0] > 150, "{}", d.levels[0][0]);
        assert!(d.levels[0][128] < 5, "{}", d.levels[0][128]);
        assert!(d.levels[0][256] > 150);
        // consulta por columnas de un tramo visible (2–4 s en 100 columnas): las columnas con ráfaga destacan
        let peaks = d.peaks(TimeRange::new(Ticks::from_seconds(2), Ticks::from_seconds(4)), 100);
        assert_eq!(peaks.len(), 100);
        assert!(peaks[0].unwrap() > 150 && peaks[50].unwrap() > 150 && peaks[25].unwrap() < 5, "{:?}", &peaks[..60]);
        // nivel elegido: 2 s / 100 col = 20 ms/col → nivel 0 (5,12 buckets/col > 4 → nivel 1: 1,28/col)
        drop(d);
        // persistido y recargado en otro coordinador sin recomputar
        let file = dir.path().join(format!("wave-{}-a0.bin", MediaCaches::cache_key(&asset)));
        assert!(file.is_file());
        let mut caches2 = MediaCaches::new(tools.clone(), Some(dir.path().to_path_buf()), Arc::new(|| {}));
        caches2.tools.ffmpeg = dir.path().join("encoder-inexistente.exe"); // prueba que la recarga no decodifica
        let h2 = caches2.waveform(&asset, 0, &path);
        caches2.tick();
        assert!(wait_until(Duration::from_secs(20), || h2.read().complete));
        caches2.tick();
        assert_eq!(caches2.pending_jobs(), 0);
        assert_eq!(h2.read().levels[0], h.read().levels[0]);
    }

    #[test]
    fn thumbnails_coarse_then_fine_indexed_by_time() {
        let tools = FfmpegTools::locate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut caches = MediaCaches::new(tools.clone(), Some(dir.path().to_path_buf()), Arc::new(|| {}));
        let path = fixture("fixture-a.mp4");
        let asset = tools.import(&path, path.to_string_lossy().to_string()).unwrap();
        let h = caches.thumbnails(&asset, &path);
        caches.tick();
        assert!(wait_until(Duration::from_secs(20), || h.read().complete));
        let d = h.read();
        assert!(d.error.is_none(), "{:?}", d.error);
        // 12 s → intervalo fino 1 s (12 miniaturas), sin nivel grueso
        assert_eq!(d.levels.len(), 1);
        let l = &d.levels[0];
        assert_eq!((l.width, l.height, l.count, l.ready), (96, 54, 12, 12));
        assert_eq!(l.index_at(Ticks::from_millis(3500)), Some(3));
        assert_eq!(l.index_at(Ticks::from_seconds(20)), Some(11));
        // miniaturas distintas entre sí (el contador cambia) y no negras
        let fl = (l.width * l.height * 4) as usize;
        assert_ne!(&l.rgba[..fl], &l.rgba[5 * fl..6 * fl]);
        assert!(l.rgba[..fl].iter().any(|b| *b > 40));
        drop(d);
        // intervalos por duración
        assert_eq!(thumb_intervals(Ticks::from_seconds(2 * 3600), false), (Ticks::from_seconds(80), Ticks::from_seconds(10)));
        assert_eq!(thumb_intervals(Ticks::from_seconds(60), false), (Ticks::from_seconds(1), Ticks::from_seconds(1)));
        // persistencia
        let mut caches2 = MediaCaches::new(tools, Some(dir.path().to_path_buf()), Arc::new(|| {}));
        caches2.tools.ffmpeg = dir.path().join("encoder-inexistente.exe");
        let h2 = caches2.thumbnails(&asset, &path);
        caches2.tick();
        assert!(wait_until(Duration::from_secs(20), || h2.read().complete));
        caches2.tick();
        assert!(h2.read().complete && caches2.pending_jobs() == 0);
    }

    #[test]
    fn prune_respects_budget() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("f{i}.bin")), vec![0u8; 1000]).unwrap();
            std::thread::sleep(Duration::from_millis(15));
        }
        prune_dir(dir.path(), 2500);
        let left: Vec<String> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(left.len(), 2, "{left:?}");
        assert!(left.contains(&"f4.bin".to_string()) && left.contains(&"f3.bin".to_string()), "{left:?}");
    }

    #[test]
    fn corrupt_cache_version_and_lengths_are_rejected_and_regenerated() {
        let tools = FfmpegTools::locate().unwrap();
        let path = fixture("fixture-sync.mp4");
        let asset = tools.import(&path, path.to_string_lossy().into_owned()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("wave-{}-a0.bin", MediaCaches::cache_key(&asset)));
        write_wave_file(&file, &vec![42; 12 * WAVE_BPS as usize], u64::from(WAVE_RATE / WAVE_BPS)).unwrap();
        let mut bytes = std::fs::read(&file).unwrap();
        bytes[4] = 9;
        std::fs::write(&file, &bytes).unwrap();
        assert!(read_wave_file(&file, asset.duration()).is_none());
        bytes[4] = 2;
        bytes[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        std::fs::write(&file, bytes).unwrap();
        assert!(read_wave_file(&file, asset.duration()).is_none());
        let mut caches = MediaCaches::new(tools, Some(dir.path().into()), Arc::new(|| {}));
        let wave = caches.waveform(&asset, 0, &path);
        assert!(!wave.read().complete, "ninguna lectura síncrona en el caller");
        caches.tick();
        assert!(wait_until(Duration::from_secs(20), || wave.read().complete));
        assert!(wave.read().error.is_none());
        assert!(wave.read().levels[0][0] > 150);
        assert!(read_wave_file(&file, asset.duration()).is_some());
    }

    #[test]
    fn long_waveform_density_preserves_time_mapping_and_cache_metadata() {
        let duration = Ticks::from_seconds(7 * 24 * 3600);
        let samples = wave_bucket_samples(duration);
        assert!(samples > u64::from(WAVE_RATE / WAVE_BPS));
        let bps = WAVE_RATE as f64 / samples as f64;
        let count = (duration.as_seconds_f64() * bps).ceil() as usize;
        assert!(count < WAVE_MAX_BUCKETS);
        let mut values = vec![0; count];
        let halfway = count / 2;
        values[halfway] = 211;
        let wave = WaveformData::from_level0(values.clone(), samples);
        let at = Ticks::from_seconds_f64(halfway as f64 / bps);
        assert_eq!(wave.peaks(TimeRange::new(at, at + Ticks::from_seconds_f64(1.0 / bps)), 1), vec![Some(211)]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long-wave.bin");
        write_wave_file(&path, &values, samples).unwrap();
        assert_eq!(read_wave_file(&path, duration), Some((values, samples)));
        assert!(read_wave_file(&path, Ticks::from_seconds(1)).is_none());
    }

    #[test]
    fn clear_cancels_workers_without_freeing_their_slots_early() {
        let tools = FfmpegTools::locate().unwrap();
        let mut caches = MediaCaches::new(tools, None, Arc::new(|| {}));
        let released = Arc::new(AtomicBool::new(false));
        for n in 0..MAX_CONCURRENT {
            let released = released.clone();
            caches.queue.push_back(Job {
                key: JobKey::Thumb(AssetId::new(format!("asset-{n}"))),
                run: Box::new(move |_| {
                    while !released.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }),
            });
        }
        caches.tick();
        caches.clear();
        assert_eq!(caches.running.len(), MAX_CONCURRENT);
        assert!(caches.running.iter().all(|r| r.cancel.load(Ordering::Relaxed)));
        released.store(true, Ordering::Relaxed);
        assert!(wait_until(Duration::from_secs(2), || {
            caches.tick();
            caches.pending_jobs() == 0
        }));
    }
}
