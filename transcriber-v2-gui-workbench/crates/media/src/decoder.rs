//! Decodificador de video por proceso FFmpeg: `-ss` exacto (transcodificación),
//! filtro `fps` a la frecuencia de secuencia, escala al tamaño pedido y salida
//! `rawvideo` RGBA por pipe. Un hilo lector llena un canal acotado (backpressure);
//! al soltar el decodificador el proceso se termina.
//!
//! El fotograma `n` corresponde al tiempo fuente `start + n / rate`.

use crate::compositor::Frame;
use crate::ffmpeg::FfmpegTools;
use crossbeam_channel::{Receiver, bounded};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tv2_domain::error::{DomainError, DomainResult};
use tv2_domain::time::{Rational, Ticks};

pub const DECODER_QUEUE: usize = 6;

/// El decodificador no entregó un fotograma dentro del plazo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeTimeout;

pub struct VideoDecoder {
    child: crate::process::CancellableChild,
    rx: Receiver<Frame>,
    pub width: u32,
    pub height: u32,
    pub rate: Rational,
    pub start: Ticks,
    /// Índice del siguiente fotograma que devolverá `next`.
    next_index: i64,
    finished: bool,
    stop: Arc<AtomicBool>,
    stderr_tail: Arc<parking_lot::Mutex<String>>,
}

impl VideoDecoder {
    /// `keyframes_only`: `-skip_frame nokey` (skim ×8 de V1).
    pub fn open(
        tools: &FfmpegTools,
        path: &Path,
        start: Ticks,
        width: u32,
        height: u32,
        rate: Rational,
        keyframes_only: bool,
    ) -> DomainResult<VideoDecoder> {
        Self::open_cancellable(tools, path, start, width, height, rate, keyframes_only, Arc::new(AtomicBool::new(false)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open_cancellable(
        tools: &FfmpegTools,
        path: &Path,
        start: Ticks,
        width: u32,
        height: u32,
        rate: Rational,
        keyframes_only: bool,
        cancel: Arc<AtomicBool>,
    ) -> DomainResult<VideoDecoder> {
        let width = width.max(2) & !1;
        let height = height.max(2) & !1;
        let mut cmd = tools.ffmpeg_cmd();
        if keyframes_only {
            cmd.args(["-skip_frame", "nokey"]);
        }
        let ss = start.max(Ticks::ZERO).as_seconds_f64();
        // Preserve the frame covering the seek instant (especially VFR). The fps
        // filter discards negative preroll; accurate_seek would discard that frame.
        cmd.args(["-noaccurate_seek", "-ss", &format!("{ss:.6}")]);
        cmd.arg("-i").arg(path);
        cmd.args(["-an", "-sn", "-dn"]);
        let vf = format!("fps=fps={}/{}:start_time=0:round=up,scale={}:{}:flags=bilinear,format=rgba", rate.num, rate.den, width, height);
        cmd.args(["-vf", &vf, "-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child =
            crate::process::CancellableChild::spawn(&mut cmd, cancel).map_err(|e| DomainError::process(format!("no se pudo iniciar ffmpeg: {e}")))?;
        let mut stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");
        let (tx, rx) = bounded::<Frame>(DECODER_QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_reader = stop.clone();
        let frame_len = (width * height * 4) as usize;
        std::thread::Builder::new()
            .name("video-decoder".into())
            .spawn(move || {
                let mut buf = vec![0u8; frame_len];
                loop {
                    if stop_reader.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut filled = 0;
                    let mut eof = false;
                    while filled < frame_len {
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
                    if eof || filled < frame_len {
                        break;
                    }
                    let frame = Frame::from_rgba(width, height, buf.clone());
                    if tx.send(frame).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| DomainError::process(e.to_string()))?;
        let tail = Arc::new(parking_lot::Mutex::new(String::new()));
        let tail_w = tail.clone();
        std::thread::Builder::new()
            .name("video-decoder-stderr".into())
            .spawn(move || {
                let mut s = String::new();
                let mut r = stderr;
                let _ = r.read_to_string(&mut s);
                let keep: String = s.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
                *tail_w.lock() = keep;
            })
            .ok();
        Ok(VideoDecoder { child, rx, width, height, rate, start, next_index: 0, finished: false, stop, stderr_tail: tail })
    }

    /// Tiempo fuente del siguiente fotograma que devolverá `next`.
    pub fn next_time(&self) -> Ticks {
        self.start + Ticks::from_frames(self.next_index, self.rate)
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Siguiente fotograma, esperando como máximo `timeout`. `Ok(None)` = fin del
    /// stream. `Err(DecodeTimeout)` = tiempo agotado (el decodificador va atrasado).
    pub fn next(&mut self, timeout: Duration) -> Result<Option<(Ticks, Frame)>, DecodeTimeout> {
        if self.finished {
            return Ok(None);
        }
        match self.rx.recv_timeout(timeout) {
            Ok(frame) => {
                let t = self.next_time();
                self.next_index += 1;
                Ok(Some((t, frame)))
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Err(DecodeTimeout),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                self.finished = true;
                Ok(None)
            }
        }
    }

    /// Fotograma disponible sin esperar.
    pub fn try_next(&mut self) -> Option<(Ticks, Frame)> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(frame) => {
                let t = self.next_time();
                self.next_index += 1;
                Some((t, frame))
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.finished = true;
                None
            }
            Err(_) => None,
        }
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr_tail.lock().clone()
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Decodifica una imagen fija (PNG/JPEG) a RGBA con `image`; conserva alfa.
pub fn decode_image(path: &Path) -> DomainResult<Frame> {
    let img = image::open(path).map_err(|e| DomainError::process(format!("no se pudo abrir la imagen {}: {e}", path.display())))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(Frame::from_rgba(w, h, rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media").join(name)
    }

    /// Lee el número de fotograma pintado en la fixture comprobando el color
    /// de fondo de la barra de color (testsrc2) no sirve; en su lugar se
    /// verifica que el tiempo fuente devuelto es exacto y que a partir de un
    /// `-ss` fuera de keyframe (1,5 s; GOP de 2 s) se obtiene un fotograma.
    #[test]
    fn decodes_from_exact_offset_outside_keyframe() {
        let tools = FfmpegTools::locate().unwrap();
        let mut d = VideoDecoder::open(&tools, &fixture("fixture-a.mp4"), Ticks::from_millis(1500), 320, 180, Rational::new(30, 1), false).unwrap();
        let (t0, f0) = d.next(Duration::from_secs(10)).unwrap().unwrap();
        assert_eq!(t0, Ticks::from_millis(1500));
        assert_eq!((f0.width, f0.height), (320, 180));
        let (t1, _) = d.next(Duration::from_secs(10)).unwrap().unwrap();
        assert_eq!(t1 - t0, Rational::new(30, 1).frame_duration());
        // el fotograma no es negro: hay contenido decodificado
        let bright = f0.rgba.as_chunks::<4>().0.iter().filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 200).count();
        assert!(bright > 1000, "bright={bright}");
        let mut count = 2;
        while let Ok(Some(_)) = d.next(Duration::from_secs(10)) {
            count += 1;
        }
        // 12 s − 1,5 s = 10,5 s × 30 = 315 fotogramas
        assert!((313..=316).contains(&count), "count={count}");
    }

    #[test]
    fn vfr_seek_preserves_presented_frame_against_native_pts() {
        let tools = FfmpegTools::locate().unwrap();
        let path = fixture("fixture-vfr.mp4");
        let probe = tools
            .ffprobe_cmd()
            .args(["-v", "error", "-select_streams", "v:0", "-show_frames", "-show_entries", "frame=best_effort_timestamp_time", "-of", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let json: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let pts: Vec<f64> =
            json["frames"].as_array().unwrap().iter().map(|f| f["best_effort_timestamp_time"].as_str().unwrap().parse().unwrap()).collect();
        // Independent oracle: native frames without seek or fps conversion.
        let raw = tools
            .ffmpeg_cmd()
            .arg("-i")
            .arg(&path)
            .args(["-an", "-vf", "scale=64:36:flags=bilinear,format=rgba", "-fps_mode", "passthrough", "-f", "rawvideo", "-"])
            .output()
            .unwrap();
        assert!(raw.status.success());
        let native: Vec<&[u8]> = raw.stdout.as_chunks::<{ 64 * 36 * 4 }>().0.iter().map(|f| f.as_slice()).collect();
        assert_eq!(native.len(), pts.len());
        for seek in [0.0, 0.05, 0.15, 1.05, 1.11, 2.233333, 4.15, 5.75] {
            let mut decoder = VideoDecoder::open(&tools, &path, Ticks::from_seconds_f64(seek), 64, 36, Rational::new(30, 1), false).unwrap();
            for frame_number in 0..5 {
                let (time, frame) = decoder
                    .next(Duration::from_secs(10))
                    .unwrap()
                    .unwrap_or_else(|| panic!("EOF seek={seek} frame={frame_number}: {}", decoder.stderr_tail()));
                let t = time.as_seconds_f64();
                let expected = pts.iter().rposition(|p| *p <= t + 0.000001).unwrap();
                let actual = native.iter().position(|pixels| *pixels == frame.rgba.as_slice()).expect("exact native pixels");
                assert_eq!(actual, expected, "seek={seek} frame={frame_number} t={t} expected_pts={} actual_pts={}", pts[expected], pts[actual]);
            }
        }
    }

    #[test]
    fn rotation_matches_explicit_counterclockwise_transform() {
        let tools = FfmpegTools::locate().unwrap();
        let path = fixture("fixture-rot90.mp4");
        let probe = tools.probe(&path).unwrap();
        let video = probe.video.unwrap();
        assert_eq!(video.rotation, 90);
        assert_eq!(video.display_size(), (360, 640));
        let native = tools
            .ffmpeg_cmd()
            .arg("-noautorotate")
            .arg("-i")
            .arg(&path)
            .args([
                "-an",
                "-vf",
                "select=eq(n\\,45),transpose=cclock,scale=90:160:flags=bilinear,format=rgba",
                "-frames:v",
                "1",
                "-fps_mode",
                "passthrough",
                "-f",
                "rawvideo",
                "-",
            ])
            .output()
            .unwrap();
        assert!(native.status.success());
        let mut decoder = VideoDecoder::open(&tools, &path, Ticks::from_millis(1500), 90, 160, Rational::new(30, 1), false).unwrap();
        let (_, frame) = decoder.next(Duration::from_secs(10)).unwrap().unwrap();
        assert_eq!(native.stdout.len(), 90 * 160 * 4);
        assert_eq!(frame.rgba.as_slice(), native.stdout.as_slice());
    }

    #[test]
    fn image_with_alpha_decodes() {
        let f = decode_image(&fixture("overlay-alpha.png")).unwrap();
        assert_eq!((f.width, f.height), (320, 180));
        assert_eq!(f.pixel(5, 5)[3], 0);
        // caja roja al 60 % (fuera del texto) y texto blanco opaco
        assert_eq!(f.pixel(160, 55), [255, 0, 0, 153]);
        assert_eq!(f.pixel(60, 130), [255, 0, 0, 153]);
    }
}
