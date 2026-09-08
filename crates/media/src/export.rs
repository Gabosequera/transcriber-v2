//! Exportación: el mismo `TimelineRenderer` y `AudioMixer` del visor producen
//! fotogramas RGBA y audio f32 que se entregan a un proceso FFmpeg codificador.
//!
//! - Se congela la timeline resuelta y el preset al iniciar.
//! - Se escribe en un archivo de staging (`.partial`) y solo se publica tras
//!   verificar con ffprobe duración y streams. Nunca se sobrescribe un archivo existente.
//! - Cancelación cooperativa por `AtomicBool`; el proceso hijo se termina.

use crate::audio::{AudioMixer, CHANNELS};
use crate::ffmpeg::FfmpegTools;
use crate::render::{AssetSource, TimelineRenderer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tv2_domain::error::{DomainError, DomainResult, ErrorCode};
use tv2_domain::ids::AssetId;
use tv2_domain::resolve::ResolvedTimeline;
use tv2_domain::time::{Rational, Ticks, TimeRange};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ExportPreset {
    pub id: String,
    pub label: String,
    pub container: String,
    pub width: u32,
    pub height: u32,
    /// `None` = frecuencia de la secuencia.
    pub frame_rate: Option<Rational>,
    pub video_args: Vec<String>,
    pub audio_args: Vec<String>,
    pub has_video: bool,
    pub notes: String,
    /// Codificadores que exige el preset (se comprueban contra el build de FFmpeg).
    #[serde(default)]
    pub required_encoders: Vec<String>,
    /// Muxer que exige el preset.
    #[serde(default)]
    pub required_muxer: String,
    /// Codificador por hardware que debe probarse en caliente antes de ofrecerlo.
    #[serde(default)]
    pub hardware_encoder: Option<String>,
    /// `codec_name` que ffprobe debe informar en el video/audio de salida (verificación).
    #[serde(default)]
    pub expect_video_codec: Option<String>,
    #[serde(default)]
    pub expect_audio_codec: Option<String>,
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// Matriz de presets. «Todos los formatos» significa cobertura amplia con las
/// capacidades realmente incluidas: cada preset declara qué codificadores y muxer
/// necesita y `availability` explica la causa cuando falta alguno.
pub fn presets() -> Vec<ExportPreset> {
    let video = |id: &str,
                 label: &str,
                 container: &str,
                 w: u32,
                 h: u32,
                 vargs: &[&str],
                 aargs: &[&str],
                 enc: &[&str],
                 vcodec: &str,
                 acodec: &str,
                 notes: &str| {
        ExportPreset {
            id: id.into(),
            label: label.into(),
            container: container.into(),
            width: w,
            height: h,
            frame_rate: None,
            video_args: args(vargs),
            audio_args: args(aargs),
            has_video: true,
            notes: notes.into(),
            required_encoders: enc.iter().map(|s| s.to_string()).collect(),
            required_muxer: container.into(),
            hardware_encoder: None,
            expect_video_codec: Some(vcodec.into()),
            expect_audio_codec: Some(acodec.into()),
        }
    };
    let audio = |id: &str, label: &str, container: &str, aargs: &[&str], enc: &str, acodec: &str, notes: &str| ExportPreset {
        id: id.into(),
        label: label.into(),
        container: container.into(),
        width: 0,
        height: 0,
        frame_rate: None,
        video_args: vec![],
        audio_args: args(aargs),
        has_video: false,
        notes: notes.into(),
        required_encoders: vec![enc.into()],
        required_muxer: container.into(),
        hardware_encoder: None,
        expect_video_codec: None,
        expect_audio_codec: Some(acodec.into()),
    };
    const X264: &[&str] = &["-c:v", "libx264", "-preset", "fast", "-crf", "20", "-pix_fmt", "yuv420p"];
    const AAC: &[&str] = &["-c:a", "aac", "-b:a", "192k"];
    let h264 = |id: &str, label: &str, w: u32, h: u32| {
        video(id, label, "mp4", w, h, X264, AAC, &["libx264", "aac"], "h264", "aac", "H.264 CRF 20 + AAC 192k, corte exacto por fotograma")
    };
    let mut v = vec![
        h264("h264-720p", "MP4 H.264 · 1280×720", 1280, 720),
        h264("h264-1080p", "MP4 H.264 · 1920×1080", 1920, 1080),
        h264("h264-2160p", "MP4 H.264 · 3840×2160", 3840, 2160),
        h264("h264-vertical", "MP4 H.264 · vertical 1080×1920", 1080, 1920),
        video(
            "hevc-1080p",
            "MP4 HEVC (H.265) · 1920×1080",
            "mp4",
            1920,
            1080,
            &["-c:v", "libx265", "-preset", "fast", "-crf", "24", "-pix_fmt", "yuv420p", "-tag:v", "hvc1", "-x265-params", "log-level=error"],
            AAC,
            &["libx265", "aac"],
            "hevc",
            "aac",
            "HEVC CRF 24 (libx265, tag hvc1 para QuickTime) + AAC 192k",
        ),
        video(
            "av1-1080p",
            "MP4 AV1 · 1920×1080",
            "mp4",
            1920,
            1080,
            &["-c:v", "libaom-av1", "-cpu-used", "8", "-usage", "realtime", "-row-mt", "1", "-crf", "34", "-b:v", "0", "-pix_fmt", "yuv420p"],
            AAC,
            &["libaom-av1", "aac"],
            "av1",
            "aac",
            "AV1 (libaom, modo realtime cpu-used 8: rápido, calidad media) + AAC 192k",
        ),
        video(
            "prores-1080p",
            "MOV ProRes 422 HQ · 1920×1080",
            "mov",
            1920,
            1080,
            &["-c:v", "prores_ks", "-profile:v", "3", "-pix_fmt", "yuv422p10le", "-vendor", "apl0"],
            &["-c:a", "pcm_s16le"],
            &["prores_ks", "pcm_s16le"],
            "prores",
            "pcm_s16le",
            "ProRes 422 HQ (10 bit 4:2:2) + PCM 16 bit; archivos grandes, edición intermedia",
        ),
        video(
            "vp9-webm-1080p",
            "WebM VP9 + Opus · 1920×1080",
            "webm",
            1920,
            1080,
            &["-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-row-mt", "1", "-crf", "33", "-b:v", "0", "-pix_fmt", "yuv420p"],
            &["-c:a", "libopus", "-b:a", "128k"],
            &["libvpx-vp9", "libopus"],
            "vp9",
            "opus",
            "VP9 (realtime) + Opus 128k para web",
        ),
        video(
            "mkv-h264-1080p",
            "MKV H.264 + FLAC · 1920×1080",
            "matroska",
            1920,
            1080,
            X264,
            &["-c:a", "flac"],
            &["libx264", "flac"],
            "h264",
            "flac",
            "Matroska con H.264 CRF 20 y audio FLAC sin pérdida",
        ),
        ExportPreset {
            hardware_encoder: Some("h264_nvenc".into()),
            ..video(
                "h264-nvenc-1080p",
                "MP4 H.264 NVENC (GPU NVIDIA) · 1920×1080",
                "mp4",
                1920,
                1080,
                &["-c:v", "h264_nvenc", "-preset", "p4", "-rc", "vbr", "-cq", "23", "-b:v", "0", "-pix_fmt", "yuv420p"],
                AAC,
                &["h264_nvenc", "aac"],
                "h264",
                "aac",
                "H.264 por hardware (NVENC); solo si la GPU lo admite (se comprueba al abrir el diálogo)",
            )
        },
        audio("wav-pcm", "WAV PCM 16 bit (solo audio)", "wav", &["-c:a", "pcm_s16le"], "pcm_s16le", "pcm_s16le", "Mezcla de la secuencia a PCM"),
        audio("flac", "FLAC (solo audio, sin pérdida)", "flac", &["-c:a", "flac"], "flac", "flac", "Mezcla de la secuencia a FLAC"),
        audio(
            "mp3-192",
            "MP3 192k (solo audio)",
            "mp3",
            &["-c:a", "libmp3lame", "-b:a", "192k"],
            "libmp3lame",
            "mp3",
            "Mezcla de la secuencia a MP3",
        ),
    ];
    // el contenedor mkv se escribe con `-f matroska` pero la extensión es .mkv
    for p in v.iter_mut() {
        if p.container == "matroska" {
            p.container = "mkv".into();
            p.required_muxer = "matroska".into();
        }
    }
    v
}

impl ExportPreset {
    /// Nombre de formato para `-f`.
    pub fn muxer(&self) -> &str {
        if self.required_muxer.is_empty() { &self.container } else { &self.required_muxer }
    }
}

/// Disponibilidad real de un preset en este build/equipo (`Err(causa)` si falta algo).
pub fn availability(tools: &FfmpegTools, preset: &ExportPreset) -> Result<(), String> {
    let caps = tools.capabilities();
    for e in &preset.required_encoders {
        if !caps.has_encoder(e) {
            return Err(format!("el build de FFmpeg no incluye el codificador {e}"));
        }
    }
    let muxer = preset.muxer();
    if !caps.has_muxer(muxer) {
        return Err(format!("el build de FFmpeg no incluye el contenedor {muxer}"));
    }
    if let Some(hw) = &preset.hardware_encoder {
        tools.check_hardware_encoder(hw).map_err(|e| format!("{hw} no funciona en este equipo: {e}"))?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub timeline: Arc<ResolvedTimeline>,
    pub assets: Arc<HashMap<AssetId, AssetSource>>,
    pub preset: ExportPreset,
    /// Rango de secuencia a exportar (`None` = todo).
    pub range: Option<TimeRange>,
    pub destination: PathBuf,
    pub project_revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ExportProgress {
    pub fraction: f32,
    pub stage: String,
    pub frames_done: i64,
    pub frames_total: i64,
    pub fps: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportResult {
    pub path: PathBuf,
    pub duration: Ticks,
    pub expected_duration: Ticks,
    pub video_streams: usize,
    pub audio_streams: usize,
    pub frames_written: i64,
    pub preset_id: String,
    pub project_revision: u64,
    pub elapsed_s: f32,
    pub sha256: String,
}

pub struct ExportJob;

impl ExportJob {
    /// Se ejecuta en el worker, también para snapshots que esperaron en cola.
    fn validate_sources(req: &ExportRequest, range: TimeRange) -> DomainResult<()> {
        let mut checked = std::collections::HashSet::new();
        for piece in &req.timeline.pieces {
            if piece.range.end <= range.start || piece.range.start >= range.end {
                continue;
            }
            for clip in piece.audio.iter().chain(piece.video.iter().filter(|_| req.preset.has_video)) {
                if !checked.insert(&clip.asset_id) {
                    continue;
                }
                let source = req.assets.get(&clip.asset_id).ok_or_else(|| DomainError::not_found("medio de exportación", &clip.asset_id))?;
                let accessible = std::fs::File::open(&source.path).and_then(|f| f.metadata());
                if source.missing || accessible.as_ref().map_or(true, |m| !m.is_file()) {
                    return Err(DomainError::precondition(format!("medio ausente o inaccesible: {}", source.path.display()))
                        .with("asset_id", clip.asset_id.to_string())
                        .with_action("vuelve a enlazar el medio y reintenta la exportación"));
                }
            }
        }
        Ok(())
    }

    pub fn run(
        tools: &FfmpegTools,
        req: &ExportRequest,
        cancel: &Arc<AtomicBool>,
        mut progress: impl FnMut(ExportProgress),
    ) -> DomainResult<ExportResult> {
        let started = Instant::now();
        let range = req.range.unwrap_or(TimeRange::new(Ticks::ZERO, req.timeline.duration));
        if range.duration().0 <= 0 {
            return Err(DomainError::invalid("el rango a exportar está vacío"));
        }
        Self::validate_sources(req, range)?;
        if req.destination.exists() {
            return Err(DomainError::precondition(format!("ya existe {}", req.destination.display()))
                .with_action("elige otro nombre; nunca se sobrescribe una exportación"));
        }
        if let Err(cause) = availability(tools, &req.preset) {
            return Err(DomainError::unsupported(format!("el preset «{}» no está disponible: {cause}", req.preset.label))
                .with_action("elige otro preset; la matriz de formatos indica la causa de cada ausencia"));
        }
        let dir = req.destination.parent().ok_or_else(|| DomainError::invalid("destino sin carpeta"))?;
        std::fs::create_dir_all(dir).map_err(|e| {
            DomainError::io(format!("no se puede crear la carpeta de destino {}: {e}", dir.display())).with_action("elige una carpeta accesible")
        })?;
        // comprobar que el destino es escribible antes de renderizar (fallo temprano y explicable)
        {
            let probe = dir.join(format!(".{}.write-test", req.destination.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
            std::fs::write(&probe, b"ok")
                .map_err(|e| DomainError::io(format!("no se puede escribir en {}: {e}", dir.display())).with_action("elige otra carpeta"))?;
            let _ = std::fs::remove_file(&probe);
        }
        let staging =
            dir.join(format!(".{}.partial", req.destination.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "export".into())));
        let _ = std::fs::remove_file(&staging);
        let fps = req.preset.frame_rate.unwrap_or(req.timeline.frame_rate);
        let sample_rate = req.timeline.sample_rate.max(8000);
        let has_audio = req.timeline.pieces.iter().any(|p| !p.audio.is_empty());

        // 1) audio → WAV temporal (misma mezcla que el visor, rate 1)
        progress(ExportProgress { stage: "mezclando audio".into(), ..Default::default() });
        let audio_tmp = dir.join(format!(".{}.audio.wav", req.destination.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
        let _cleanup = CleanupPaths(vec![audio_tmp.clone(), staging.clone()]);
        let total_frames_audio = ((range.duration().as_seconds_f64()) * sample_rate as f64).round() as u64;
        {
            let mut mixer = AudioMixer::new(tools.clone(), req.timeline.clone(), req.assets.clone(), sample_rate, range.start, 1.0);
            mixer.cancel = cancel.clone();
            let mut wav = WavWriter::create(&audio_tmp, sample_rate)?;
            let mut written = 0u64;
            let mut buf = vec![0f32; 4096 * CHANNELS];
            while written < total_frames_audio {
                if cancel.load(Ordering::Relaxed) {
                    drop(wav);
                    let _ = std::fs::remove_file(&audio_tmp);
                    return Err(DomainError::new(ErrorCode::Cancelled, "exportación cancelada"));
                }
                let want = ((total_frames_audio - written) as usize).min(4096);
                let n = if has_audio { mixer.mix(&mut buf[..want * CHANNELS]) } else { 0 };
                if let Some(error) = mixer.take_error() {
                    drop(wav);
                    let _ = std::fs::remove_file(&audio_tmp);
                    return Err(if cancel.load(Ordering::Relaxed) {
                        DomainError::new(ErrorCode::Cancelled, "exportación cancelada")
                    } else {
                        error.with_action("comprueba o vuelve a enlazar el medio de audio; no se publicó la exportación")
                    });
                }
                let n = if n == 0 {
                    // silencio hasta completar (huecos o fin)
                    buf[..want * CHANNELS].iter_mut().for_each(|s| *s = 0.0);
                    want
                } else {
                    n
                };
                wav.write(&buf[..n * CHANNELS])?;
                written += n as u64;
                if written % (sample_rate as u64) < 4096 {
                    progress(ExportProgress {
                        stage: "mezclando audio".into(),
                        fraction: 0.1 * (written as f32 / total_frames_audio.max(1) as f32),
                        ..Default::default()
                    });
                }
            }
            wav.finish()?;
        }

        // 2) video → ffmpeg por pipe
        let result = if req.preset.has_video {
            Self::encode_video(tools, req, range, fps, &staging, &audio_tmp, cancel, &mut progress)
        } else {
            Self::encode_audio_only(tools, req, &staging, &audio_tmp, cancel)
        };
        let _ = std::fs::remove_file(&audio_tmp);
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&staging);
            return Err(DomainError::new(ErrorCode::Cancelled, "exportación cancelada"));
        }
        let frames_written = match result {
            Ok(n) => n,
            Err(e) => {
                let _ = std::fs::remove_file(&staging);
                return Err(e);
            }
        };

        // 3) verificación y publicación
        progress(ExportProgress { stage: "verificando".into(), fraction: 0.98, ..Default::default() });
        let probe = match tools.probe(&staging) {
            Ok(p) => p,
            Err(e) => {
                let _ = std::fs::remove_file(&staging);
                return Err(DomainError::process(format!("la salida no es legible: {e}")));
            }
        };
        let expected = range.duration();
        let tolerance = Ticks::from_millis(150).max(fps.frame_duration() * 2);
        let video_streams = probe.video.iter().count();
        let audio_streams = probe.audio.len();
        if (probe.duration - expected).abs() > tolerance || (req.preset.has_video && video_streams != 1) || audio_streams != 1 {
            let _ = std::fs::remove_file(&staging);
            return Err(DomainError::process(format!(
                "duración o streams incorrectos (duración {} esperada {}, video {}, audio {}); no se publicó",
                probe.duration, expected, video_streams, audio_streams
            )));
        }
        if let Some(want) = &req.preset.expect_video_codec
            && let Some(v) = &probe.video
            && &v.codec != want
        {
            let _ = std::fs::remove_file(&staging);
            return Err(DomainError::process(format!("la salida lleva video {} y el preset esperaba {want}; no se publicó", v.codec)));
        }
        if let Some(want) = &req.preset.expect_audio_codec
            && let Some(a) = probe.audio.first()
            && &a.codec != want
        {
            let _ = std::fs::remove_file(&staging);
            return Err(DomainError::process(format!("la salida lleva audio {} y el preset esperaba {want}; no se publicó", a.codec)));
        }
        std::fs::rename(&staging, &req.destination)
            .map_err(|e| DomainError::io(format!("no se pudo publicar {}: {e}", req.destination.display())))?;
        let sha256 = file_sha256(&req.destination)?;
        progress(ExportProgress {
            stage: "listo".into(),
            fraction: 1.0,
            frames_done: frames_written,
            frames_total: frames_written,
            ..Default::default()
        });
        Ok(ExportResult {
            path: req.destination.clone(),
            duration: probe.duration,
            expected_duration: expected,
            video_streams,
            audio_streams,
            frames_written,
            preset_id: req.preset.id.clone(),
            project_revision: req.project_revision,
            elapsed_s: started.elapsed().as_secs_f32(),
            sha256,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_video(
        tools: &FfmpegTools,
        req: &ExportRequest,
        range: TimeRange,
        fps: Rational,
        staging: &Path,
        audio: &Path,
        cancel: &Arc<AtomicBool>,
        progress: &mut impl FnMut(ExportProgress),
    ) -> DomainResult<i64> {
        let (w, h) = (req.preset.width & !1, req.preset.height & !1);
        let total = (range.duration().0 + fps.frame_duration().0 - 1) / fps.frame_duration().0;
        let mut cmd = tools.ffmpeg_cmd();
        cmd.args(["-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-r", &format!("{}/{}", fps.num, fps.den), "-i", "pipe:0"]);
        cmd.arg("-i").arg(audio);
        cmd.args(["-map", "0:v:0", "-map", "1:a:0"]);
        cmd.args(&req.preset.video_args);
        cmd.args(&req.preset.audio_args);
        cmd.arg("-shortest");
        if matches!(req.preset.muxer(), "mp4" | "mov") {
            cmd.args(["-movflags", "+faststart"]);
        }
        cmd.arg("-f").arg(req.preset.muxer());
        cmd.arg(staging);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = crate::process::CancellableChild::spawn(&mut cmd, cancel.clone())
            .map_err(|e| DomainError::process(format!("no se pudo iniciar el codificador: {e}")))?;
        let mut stdin = child.stdin.take().expect("stdin");
        let stderr = child.stderr.take().expect("stderr");
        let err_tail = std::thread::spawn(move || {
            use std::io::Read;
            let mut s = String::new();
            let mut r = stderr;
            let _ = r.read_to_string(&mut s);
            s.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect::<String>()
        });
        let mut renderer = TimelineRenderer::new(tools.clone(), req.timeline.clone(), req.assets.clone(), w, h);
        renderer.cancel = cancel.clone();
        let started = Instant::now();
        let mut written = 0i64;
        let mut failure: Option<DomainError> = None;
        for i in 0..total {
            if cancel.load(Ordering::Relaxed) {
                failure = Some(DomainError::new(ErrorCode::Cancelled, "exportación cancelada"));
                break;
            }
            let t = range.start + Ticks::from_frames(i, fps);
            let (frame, complete) = renderer.render(t, Duration::from_secs(30));
            if !complete {
                failure = Some(
                    DomainError::process(format!("fotograma {i} incompleto en {t}; no se publicó la exportación"))
                        .with_action("comprueba los medios y sus rangos fuente; vuelve a enlazar los archivos ausentes"),
                );
                break;
            }
            if let Err(e) = stdin.write_all(&frame.rgba) {
                failure = Some(DomainError::process(format!("el codificador cerró la entrada: {e}")));
                break;
            }
            written += 1;
            if i % 15 == 0 {
                let el = started.elapsed().as_secs_f32().max(0.001);
                progress(ExportProgress {
                    stage: "codificando video".into(),
                    fraction: 0.1 + 0.85 * (written as f32 / total.max(1) as f32),
                    frames_done: written,
                    frames_total: total,
                    fps: written as f32 / el,
                });
            }
        }
        drop(stdin);
        if let Some(f) = failure {
            let _ = child.kill();
            let _ = child.wait();
            return Err(f);
        }
        let status = child.wait().map_err(|e| DomainError::process(e.to_string()))?;
        let tail = err_tail.join().unwrap_or_default();
        if !status.success() {
            return Err(DomainError::process(format!("FFmpeg terminó con error: {}", tail.trim())));
        }
        Ok(written)
    }

    fn encode_audio_only(tools: &FfmpegTools, req: &ExportRequest, staging: &Path, audio: &Path, cancel: &Arc<AtomicBool>) -> DomainResult<i64> {
        let mut cmd = tools.ffmpeg_cmd();
        cmd.arg("-y").arg("-i").arg(audio);
        cmd.args(&req.preset.audio_args);
        cmd.arg("-f").arg(req.preset.muxer());
        cmd.arg(staging);
        cmd.stdout(Stdio::null()).stderr(Stdio::piped());
        let mut process = crate::process::CancellableChild::spawn(&mut cmd, cancel.clone()).map_err(|e| DomainError::process(e.to_string()))?;
        let mut stderr = process.stderr.take().expect("stderr");
        let mut tail = String::new();
        std::io::Read::read_to_string(&mut stderr, &mut tail)?;
        if !process.wait()?.success() {
            return Err(DomainError::process(format!("FFmpeg: {}", tail.trim())));
        }
        Ok(0)
    }
}

struct WavWriter {
    file: std::fs::File,
    frames: u64,
    sample_rate: u32,
}

struct CleanupPaths(Vec<PathBuf>);
impl Drop for CleanupPaths {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl WavWriter {
    fn create(path: &Path, sample_rate: u32) -> DomainResult<WavWriter> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(&[0u8; 44])?;
        Ok(WavWriter { file, frames: 0, sample_rate })
    }

    fn write(&mut self, samples: &[f32]) -> DomainResult<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all(&bytes)?;
        self.frames += (samples.len() / CHANNELS) as u64;
        Ok(())
    }

    fn finish(mut self) -> DomainResult<()> {
        use std::io::Seek;
        let data_len = self.frames * CHANNELS as u64 * 4;
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        h.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
        h.extend_from_slice(&self.sample_rate.to_le_bytes());
        h.extend_from_slice(&(self.sample_rate * CHANNELS as u32 * 4).to_le_bytes());
        h.extend_from_slice(&((CHANNELS * 4) as u16).to_le_bytes());
        h.extend_from_slice(&32u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&(data_len as u32).to_le_bytes());
        self.file.seek(std::io::SeekFrom::Start(0))?;
        self.file.write_all(&h)?;
        self.file.flush()?;
        Ok(())
    }
}

pub fn file_sha256(path: &Path) -> DomainResult<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tv2_domain::commands::Command;
    use tv2_domain::project::Project;
    use tv2_domain::time::TimeRange;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media").join(name)
    }

    /// Proyecto con la fixture insertada entera; devuelve la timeline resuelta y los assets.
    fn timeline(tools: &FfmpegTools, name: &str) -> (Arc<ResolvedTimeline>, Arc<HashMap<AssetId, AssetSource>>) {
        let mut p = Project::new("export-test");
        p.sequences[0].width = 640;
        p.sequences[0].height = 360;
        let path = fixture(name);
        let a = tools.import(&path, path.to_string_lossy().to_string()).unwrap();
        let aid = a.id.clone();
        Command::ImportAsset { asset: a }.apply(&mut p).unwrap();
        Command::InsertAssetLinked { asset_id: aid, position: Ticks::ZERO, video_track: None, source: None }.apply(&mut p).unwrap();
        let seq = p.active().unwrap();
        let tl = Arc::new(ResolvedTimeline::resolve(&p, seq));
        let assets: HashMap<AssetId, AssetSource> =
            p.assets.iter().map(|a| (a.id.clone(), AssetSource::from_asset(a, PathBuf::from(&a.path)))).collect();
        (tl, Arc::new(assets))
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with('.')).collect()
    }

    #[test]
    fn mute_solo_and_gain_match_preview_and_pcm_export() {
        use crate::audio::AudioDecoder;
        use tv2_domain::{MovePolicy, ids::TrackId, timeline::TrackKind};
        let tools = FfmpegTools::locate().unwrap();
        let path = fixture("fixture-a.mp4");
        let asset = tools.import(&path, path.display().to_string()).unwrap();
        let aid = asset.id.clone();
        let assets = Arc::new(HashMap::from([(aid.clone(), AssetSource::from_asset(&asset, path.clone()))]));
        let mut project = Project::new("Mezcla verificada");
        Command::ImportAsset { asset }.apply(&mut project).unwrap();
        let mut tracks = Vec::new();
        for name in ["Audio A", "Audio B"] {
            let effect = Command::AddTrack { sequence_id: None, kind: TrackKind::Audio, name: name.into(), index: None }.apply(&mut project).unwrap();
            let track_id = TrackId::new(effect.created[0].clone());
            Command::AddClip {
                track_id: track_id.clone(),
                asset_id: aid.clone(),
                source: TimeRange::new(Ticks::ZERO, Ticks::from_seconds(1)),
                position: Ticks::ZERO,
                policy: MovePolicy::Reject,
                clip_id: None,
                link_group: None,
                audio_stream: Some(0),
                provenance: None,
            }
            .apply(&mut project)
            .unwrap();
            tracks.push(track_id);
        }
        let rate = project.active().unwrap().sample_rate;
        let n = rate as usize / 4;
        let mut original = vec![0.0; n * CHANNELS];
        let mut direct = AudioDecoder::open(&tools, &path, 0, Ticks::ZERO, rate, 1.0).unwrap();
        assert_eq!(direct.read(&mut original), n);
        assert!(original.iter().any(|x| x.abs() > 0.01));
        let dir = tempfile::tempdir().unwrap();
        for (name, mute_a, mute_b, solo_b, expected_gain) in [
            ("mix", false, false, false, 0.75),
            ("mute", true, false, false, 0.25),
            ("solo", false, false, true, 0.25),
            ("silence", true, true, false, 0.0),
        ] {
            let mut p = project.clone();
            for (index, track) in tracks.iter().enumerate() {
                Command::SetTrackProps {
                    track_id: track.clone(),
                    name: None,
                    muted: Some(if index == 0 { mute_a } else { mute_b }),
                    solo: Some(index == 1 && solo_b),
                    locked: None,
                    visible: None,
                    gain_db: Some(if index == 0 { -6.0206 } else { -12.0412 }),
                    height: None,
                }
                .apply(&mut p)
                .unwrap();
            }
            let timeline = Arc::new(ResolvedTimeline::resolve(&p, p.active().unwrap()));
            let mut mixer = AudioMixer::new(tools.clone(), timeline.clone(), assets.clone(), rate, Ticks::ZERO, 1.0);
            let mut preview = vec![0.0; n * CHANNELS];
            assert_eq!(mixer.mix(&mut preview), n);
            assert!(mixer.take_error().is_none());
            let req = ExportRequest {
                timeline,
                assets: assets.clone(),
                preset: presets().into_iter().find(|p| p.id == "wav-pcm").unwrap(),
                range: Some(TimeRange::new(Ticks::ZERO, Ticks::from_millis(250))),
                destination: dir.path().join(format!("{name}.wav")),
                project_revision: 1,
            };
            ExportJob::run(&tools, &req, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
            let mut decoder = AudioDecoder::open(&tools, &req.destination, 0, Ticks::ZERO, rate, 1.0).unwrap();
            let mut exported = vec![0.0; n * CHANNELS];
            assert_eq!(decoder.read(&mut exported), n);
            let mut max_error = 0f32;
            for ((source, preview), exported) in original.iter().zip(&preview).zip(&exported) {
                assert!((preview - source * expected_gain).abs() < 0.00001, "{name}: mezcla incorrecta");
                max_error = max_error.max((preview - exported).abs());
            }
            assert!(max_error <= 1.0 / 32768.0, "{name}: diferencia PCM={max_error}");
            println!("{name}: {n} frames, gain={expected_gain}, max_error={max_error}");
        }
    }

    #[test]
    fn missing_sources_are_checked_at_execution_and_only_for_requested_content() {
        let tools = FfmpegTools::locate().unwrap();
        let (tl, assets) = timeline(&tools, "fixture-a.mp4");
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("medio temporal ñ.mp4");
        std::fs::copy(fixture("fixture-a.mp4"), &copy).unwrap();
        let mut sources = (*assets).clone();
        for source in sources.values_mut() {
            source.path = copy.clone();
        }
        let mut req = ExportRequest {
            timeline: tl,
            assets: Arc::new(sources),
            preset: presets().remove(0),
            range: Some(TimeRange::new(Ticks::ZERO, Ticks::from_millis(200))),
            destination: dir.path().join("resultado.mp4"),
            project_revision: 7,
        };
        // Snapshot congelado antes de que el archivo desaparezca (igual que la cola).
        std::fs::remove_file(copy).unwrap();
        let error = ExportJob::run(&tools, &req, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap_err();
        assert_eq!(error.code, ErrorCode::Precondition);
        assert!(error.action.is_some());
        assert!(!req.destination.exists());
        assert!(leftovers(dir.path()).is_empty());
        // Un clip solo visual ausente no impide exportar audio o un hueco posterior.
        Arc::make_mut(&mut req.timeline).pieces.iter_mut().for_each(|p| p.audio.clear());
        req.preset = presets().into_iter().find(|p| p.id == "wav-pcm").unwrap();
        req.destination = dir.path().join("silencio.wav");
        ExportJob::run(&tools, &req, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        req.preset = presets().remove(0);
        req.range = Some(TimeRange::new(Ticks::from_seconds(13), Ticks::from_millis(13200)));
        req.destination = dir.path().join("hueco.mp4");
        ExportJob::run(&tools, &req, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    }

    #[test]
    fn decoder_failures_do_not_publish_black_video_or_silent_audio() {
        let tools = FfmpegTools::locate().unwrap();
        let (tl, assets) = timeline(&tools, "fixture-a.mp4");
        let dir = tempfile::tempdir().unwrap();
        let corrupt = dir.path().join("corrupto.mp4");
        std::fs::write(&corrupt, b"not a media file").unwrap();
        let mut sources = (*assets).clone();
        for source in sources.values_mut() {
            source.path = corrupt.clone();
        }
        for video in [false, true] {
            let mut resolved = (*tl).clone();
            if video {
                resolved.pieces.iter_mut().for_each(|p| p.audio.clear());
            }
            let preset = if video { presets().remove(0) } else { presets().into_iter().find(|p| p.id == "wav-pcm").unwrap() };
            let req = ExportRequest {
                timeline: Arc::new(resolved),
                assets: Arc::new(sources.clone()),
                preset,
                range: Some(TimeRange::new(Ticks::ZERO, Ticks::from_millis(200))),
                destination: dir.path().join(if video { "bad.mp4" } else { "bad.wav" }),
                project_revision: 1,
            };
            let error = ExportJob::run(&tools, &req, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap_err();
            assert_eq!(error.code, ErrorCode::Process, "{error}");
            assert!(!req.destination.exists());
            assert!(leftovers(dir.path()).is_empty());
        }
    }

    #[test]
    fn cancel_leaves_no_output_nor_staging() {
        let tools = FfmpegTools::locate().unwrap();
        let (tl, assets) = timeline(&tools, "fixture-a.mp4");
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("cancelado.mp4");
        let preset = presets().into_iter().find(|p| p.id == "h264-720p").unwrap();
        let req = ExportRequest { timeline: tl, assets, preset, range: None, destination: dest.clone(), project_revision: 1 };
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        // cancelar en cuanto empiece a codificar video
        let err = ExportJob::run(&tools, &req, &cancel, move |p| {
            if p.stage == "codificando video" {
                c2.store(true, Ordering::Relaxed);
            }
        })
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled, "{err}");
        assert!(!dest.exists(), "no se publica nada al cancelar");
        assert!(leftovers(dir.path()).is_empty(), "sin staging residual: {:?}", leftovers(dir.path()));
    }

    #[test]
    fn encoder_failure_and_bad_destination_are_explained_without_leftovers() {
        let tools = FfmpegTools::locate().unwrap();
        let (tl, assets) = timeline(&tools, "fixture-a.mp4");
        let dir = tempfile::tempdir().unwrap();
        // codificador inexistente: FFmpeg falla y no se publica
        let mut preset = presets().into_iter().find(|p| p.id == "h264-720p").unwrap();
        preset.video_args = args(&["-c:v", "codec_inexistente"]);
        let dest = dir.path().join("fallo-encoder.mp4");
        let req = ExportRequest {
            timeline: tl.clone(),
            assets: assets.clone(),
            preset,
            range: Some(TimeRange::new(Ticks::ZERO, Ticks::from_seconds(1))),
            destination: dest.clone(),
            project_revision: 1,
        };
        let err = ExportJob::run(&tools, &req, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::Process, "{err}");
        assert!(err.message.contains("codificador") || err.message.contains("FFmpeg"), "{err}");
        assert!(!dest.exists() && leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
        // preset que exige un codificador ausente del build: causa explicada sin lanzar FFmpeg
        let mut preset = presets().into_iter().find(|p| p.id == "h264-720p").unwrap();
        preset.required_encoders = vec!["libfoo".into()];
        let req2 = ExportRequest { preset, destination: dir.path().join("no-disponible.mp4"), ..req.clone() };
        let err = ExportJob::run(&tools, &req2, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported);
        assert!(err.message.contains("libfoo"), "{err}");
        // destino en una unidad inexistente
        let req3 = ExportRequest {
            preset: presets().into_iter().find(|p| p.id == "wav-pcm").unwrap(),
            destination: PathBuf::from(r"Q:\no-existe\salida.wav"),
            ..req.clone()
        };
        let err = ExportJob::run(&tools, &req3, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::Io, "{err}");
        assert!(err.action.is_some());
        // destino existente: nunca se sobrescribe
        let existing = dir.path().join("existe.wav");
        std::fs::write(&existing, b"x").unwrap();
        let req4 = ExportRequest { destination: existing.clone(), ..req3 };
        let err = ExportJob::run(&tools, &req4, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::Precondition);
        assert_eq!(std::fs::read(&existing).unwrap(), b"x");
    }

    /// Matriz de formatos: cada preset disponible produce una salida inspeccionable
    /// (codec, streams, duración) sobre 1 s de la fixture; los no disponibles explican la causa.
    #[test]
    fn format_matrix_every_available_preset_produces_inspectable_output() {
        let tools = FfmpegTools::locate().unwrap();
        let (tl, assets) = timeline(&tools, "fixture-a.mp4");
        let dir = tempfile::tempdir().unwrap();
        let mut report: Vec<String> = Vec::new();
        for preset in presets() {
            if let Err(cause) = availability(&tools, &preset) {
                report.push(format!("{}: NO DISPONIBLE ({cause})", preset.id));
                continue;
            }
            let dest = dir.path().join(format!("{}.{}", preset.id, preset.container));
            let started = std::time::Instant::now();
            let req = ExportRequest {
                timeline: tl.clone(),
                assets: assets.clone(),
                preset: preset.clone(),
                range: Some(TimeRange::new(Ticks::from_millis(500), Ticks::from_millis(1500))),
                destination: dest.clone(),
                project_revision: 1,
            };
            let r = ExportJob::run(&tools, &req, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap_or_else(|e| panic!("{}: {e}", preset.id));
            let probe = tools.probe(&dest).unwrap();
            assert_eq!(probe.audio.len(), 1, "{}", preset.id);
            assert!((probe.duration - Ticks::from_seconds(1)).abs() <= Ticks::from_millis(150), "{}: {}", preset.id, probe.duration);
            if preset.has_video {
                let v = probe.video.as_ref().unwrap();
                assert_eq!((v.width, v.height), (preset.width, preset.height), "{}", preset.id);
                assert_eq!(Some(&v.codec), preset.expect_video_codec.as_ref(), "{}", preset.id);
                assert_eq!(r.frames_written, 30, "{}", preset.id);
            }
            assert_eq!(Some(&probe.audio[0].codec), preset.expect_audio_codec.as_ref(), "{}", preset.id);
            report.push(format!(
                "{}: OK {} {}x{} {} · {} · {:.1} s · {} bytes",
                preset.id,
                probe.container,
                probe.video.as_ref().map(|v| v.width).unwrap_or(0),
                probe.video.as_ref().map(|v| v.height).unwrap_or(0),
                probe.video.as_ref().map(|v| v.codec.clone()).unwrap_or_default(),
                probe.audio[0].codec,
                started.elapsed().as_secs_f32(),
                std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0)
            ));
        }
        let text = report.join("\n");
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../implementation/evidence/e2/format-matrix.txt");
        let _ = std::fs::write(&out, format!("{}\n{text}\n", tools.version));
        println!("{text}");
        assert!(report.iter().filter(|l| l.contains(": OK")).count() >= 10, "{text}");
    }

    #[test]
    fn exported_flash_and_audio_bursts_remain_synchronized_after_non_keyframe_cut() {
        let tools = FfmpegTools::locate().unwrap();
        let (timeline, assets) = timeline(&tools, "fixture-sync.mp4");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.mp4");
        let preset = presets().into_iter().find(|p| p.id == "h264-720p").unwrap();
        let request = ExportRequest {
            timeline,
            assets,
            preset,
            destination: path.clone(),
            project_revision: 42,
            range: Some(TimeRange::new(Ticks::from_millis(500), Ticks::from_millis(10500))),
        };
        let result = ExportJob::run(&tools, &request, &Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let mut decoder = crate::VideoDecoder::open(&tools, &path, Ticks::ZERO, 320, 180, Rational::new(30, 1), false).unwrap();
        let mut flashes = Vec::new();
        let mut white_before = false;
        while let Some((t, frame)) = decoder.next(Duration::from_secs(5)).unwrap() {
            // Recuadro blanco de la fixture propia, lejos de los contadores (sin OCR).
            let white = (124..132).all(|y| (246..254).all(|x| frame.pixel(x, y)[..3].iter().all(|c| *c > 235)));
            if white && !white_before {
                flashes.push(t.as_seconds_f64());
            }
            white_before = white;
        }
        let mut audio = crate::AudioDecoder::open(&tools, &path, 0, Ticks::ZERO, 48000, 1.0).unwrap();
        let mut samples = vec![0f32; 2048 * CHANNELS];
        let mut onsets = Vec::new();
        let mut quiet = 48000usize;
        let mut index = 0usize;
        loop {
            let n = audio.read(&mut samples);
            if n == 0 {
                break;
            }
            for frame in samples[..n * CHANNELS].as_chunks::<CHANNELS>().0 {
                if frame[0].abs() > 0.3 {
                    if quiet > 960 {
                        onsets.push(index as f64 / 48000.0);
                    }
                    quiet = 0;
                } else {
                    quiet += 1;
                }
                index += 1;
            }
        }
        assert_eq!(flashes.len(), 10, "{flashes:?}");
        assert_eq!(onsets.len(), 10, "{onsets:?}");
        let mut report = Vec::new();
        for (i, (video, audio)) in flashes.iter().zip(&onsets).enumerate() {
            let expected = i as f64 + 0.5;
            assert!((video - expected).abs() <= 1.0 / 30.0 + 0.001);
            assert!((audio - expected).abs() <= 0.020);
            let delta = (video - audio).abs();
            assert!(delta <= 0.050, "desvío A/V de {delta:.4} s");
            report.push(format!("pulso {i}: video={video:.6}s audio={audio:.6}s delta={:.3}ms", delta * 1000.0));
        }
        assert_eq!(result.frames_written, 300);
        let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../implementation/evidence/e2");
        std::fs::copy(&path, evidence.join("sync-export-720p.mp4")).unwrap();
        let text = format!(
            "{}\nPreset h264-720p; fuente [0.5,10.5); revisión 42; 300 frames; tolerancia A/V 50 ms, audio 20 ms, video 1 frame\n{}\n",
            tools.version,
            report.join("\n")
        );
        std::fs::write(evidence.join("sync-export-measurement.txt"), &text).unwrap();
        println!("{text}");
    }
}
