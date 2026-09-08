//! Localización de FFmpeg/FFprobe, sondeo de medios y fingerprint V1.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tv2_domain::asset::{Asset, AssetKind, AudioStreamInfo, Fingerprint, MediaProbe, VideoStreamInfo, is_image_codec};
use tv2_domain::error::{DomainError, DomainResult};
use tv2_domain::ids::AssetId;
use tv2_domain::time::{Rational, Ticks};

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Aplica banderas para no abrir consolas auxiliares en Windows.
pub fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(Stdio::null());
    cmd
}

#[derive(Clone, Debug)]
pub struct FfmpegTools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub origin: String,
    pub version: String,
    /// Capacidades (codificadores/muxers) consultadas una sola vez.
    caps: std::sync::Arc<parking_lot::Mutex<Option<Capabilities>>>,
}

/// Codificadores y muxers realmente presentes en el build, más la comprobación en
/// caliente de los codificadores por hardware (NVENC), que figuran en la lista
/// aunque no haya GPU compatible.
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub encoders: std::collections::BTreeSet<String>,
    pub muxers: std::collections::BTreeSet<String>,
    /// Resultado de la prueba de codificación por codificador de hardware (`Ok` o causa).
    pub hardware_checks: std::collections::BTreeMap<String, Result<(), String>>,
}

impl Capabilities {
    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders.contains(name)
    }
    pub fn has_muxer(&self, name: &str) -> bool {
        self.muxers.contains(name)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("no se encontró ffprobe/ffmpeg ({0})")]
    Missing(String),
    #[error("{0}")]
    Failed(String),
}

impl FfmpegTools {
    /// Orden de búsqueda: `TRANSCRIPTOR_FFMPEG_DIR`, carpeta del ejecutable
    /// (`third-party/ffmpeg`, `ffmpeg`), raíz del workspace en desarrollo
    /// (`packaging/third-party/ffmpeg`) y por último PATH.
    pub fn locate() -> Result<FfmpegTools, ProbeError> {
        let mut candidates: Vec<(PathBuf, String)> = Vec::new();
        if let Ok(dir) = std::env::var("TRANSCRIPTOR_FFMPEG_DIR") {
            candidates.push((PathBuf::from(dir), "TRANSCRIPTOR_FFMPEG_DIR".into()));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push((dir.join("third-party").join("ffmpeg"), "junto al ejecutable".into()));
            candidates.push((dir.join("ffmpeg"), "junto al ejecutable".into()));
            // desarrollo: target/{debug,release}/Transcriptor.exe → raíz del workspace
            let mut up = dir.to_path_buf();
            for _ in 0..4 {
                if let Some(p) = up.parent() {
                    up = p.to_path_buf();
                    let c = up.join("packaging").join("third-party").join("ffmpeg");
                    if c.is_dir() {
                        candidates.push((c, "workspace de desarrollo".into()));
                        break;
                    }
                }
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push((cwd.join("packaging").join("third-party").join("ffmpeg"), "directorio actual".into()));
        }
        let exe = |name: &str| if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        for (dir, origin) in candidates {
            let ffmpeg = dir.join(exe("ffmpeg"));
            let ffprobe = dir.join(exe("ffprobe"));
            if ffmpeg.is_file() && ffprobe.is_file() {
                let version = version_of(&ffmpeg).unwrap_or_default();
                return Ok(FfmpegTools { ffmpeg, ffprobe, origin, version, caps: Default::default() });
            }
        }
        // PATH
        let ffmpeg = PathBuf::from(exe("ffmpeg"));
        if let Ok(version) = version_of(&ffmpeg) {
            return Ok(FfmpegTools { ffmpeg, ffprobe: PathBuf::from(exe("ffprobe")), origin: "PATH".into(), version, caps: Default::default() });
        }
        Err(ProbeError::Missing("coloca ffmpeg.exe y ffprobe.exe en third-party/ffmpeg junto al ejecutable o define TRANSCRIPTOR_FFMPEG_DIR".into()))
    }

    pub fn ffmpeg_cmd(&self) -> Command {
        let mut c = Command::new(&self.ffmpeg);
        quiet(&mut c);
        c.arg("-hide_banner").arg("-nostdin").arg("-loglevel").arg("error");
        c
    }

    pub fn ffprobe_cmd(&self) -> Command {
        let mut c = Command::new(&self.ffprobe);
        quiet(&mut c);
        c
    }

    /// Capacidades del build (`-encoders`, `-muxers`); se calculan una vez por proceso.
    pub fn capabilities(&self) -> Capabilities {
        let mut guard = self.caps.lock();
        if let Some(c) = guard.as_ref() {
            return c.clone();
        }
        let list = |flag: &str| -> std::collections::BTreeSet<String> {
            let mut c = Command::new(&self.ffmpeg);
            quiet(&mut c);
            let out = c.args(["-hide_banner", flag]).stdout(Stdio::piped()).stderr(Stdio::null()).output();
            let Ok(out) = out else { return Default::default() };
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines()
                .skip_while(|l| !l.trim_start().starts_with("--"))
                .skip(1)
                .filter_map(|l| {
                    let mut it = l.split_whitespace();
                    let flags = it.next()?;
                    let name = it.next()?;
                    let ok = if flag == "-muxers" { flags.contains('E') } else { flags.starts_with('V') || flags.starts_with('A') };
                    ok.then(|| name.to_string())
                })
                .collect()
        };
        let caps = Capabilities { encoders: list("-encoders"), muxers: list("-muxers"), hardware_checks: Default::default() };
        *guard = Some(caps.clone());
        caps
    }

    /// Prueba real (0,2 s de video sintético) de un codificador por hardware; el
    /// resultado se memoriza. Devuelve la causa si no funciona en este equipo.
    pub fn check_hardware_encoder(&self, encoder: &str) -> Result<(), String> {
        {
            let guard = self.caps.lock();
            if let Some(c) = guard.as_ref()
                && let Some(r) = c.hardware_checks.get(encoder)
            {
                return r.clone();
            }
        }
        let caps = self.capabilities();
        let result = if !caps.has_encoder(encoder) {
            Err(format!("el build de FFmpeg no incluye {encoder}"))
        } else {
            let mut cmd = self.ffmpeg_cmd();
            cmd.args(["-f", "lavfi", "-i", "color=size=256x144:rate=30:duration=0.2", "-c:v", encoder, "-f", "null", "-"]);
            cmd.stdout(Stdio::null()).stderr(Stdio::piped());
            match cmd.output() {
                Ok(out) if out.status.success() => Ok(()),
                Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(200).collect()),
                Err(e) => Err(e.to_string()),
            }
        };
        let mut guard = self.caps.lock();
        if let Some(c) = guard.as_mut() {
            c.hardware_checks.insert(encoder.to_string(), result.clone());
        }
        result
    }

    /// Sondeo completo (equivalente a `medios.inspeccionar`).
    pub fn probe(&self, path: &Path) -> Result<MediaProbe, ProbeError> {
        self.probe_cancellable(path, Arc::new(AtomicBool::new(false)))
    }

    pub fn probe_cancellable(&self, path: &Path, cancel: Arc<AtomicBool>) -> Result<MediaProbe, ProbeError> {
        if !path.exists() {
            return Err(ProbeError::Failed(format!("no existe el archivo: {}", path.display())));
        }
        let mut cmd = self.ffprobe_cmd();
        cmd.args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"]).arg(path).stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = crate::process::CancellableChild::spawn(&mut cmd, cancel)
            .and_then(|child| child.output())
            .map_err(|e| ProbeError::Missing(e.to_string()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(ProbeError::Failed(format!("ffprobe no pudo leer el archivo: {}", err.trim().chars().take(300).collect::<String>())));
        }
        let raw: RawProbe = serde_json::from_slice(&out.stdout).map_err(|e| ProbeError::Failed(format!("salida de ffprobe inválida: {e}")))?;
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(normalize(raw, size, path))
    }

    /// Fingerprint compatible con V1 (`medios.fingerprint`).
    pub fn fingerprint(&self, path: &Path, probe: &MediaProbe) -> DomainResult<Fingerprint> {
        let meta = std::fs::metadata(path)?;
        let size = meta.len();
        let mtime_ns = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i128);
        Ok(Fingerprint { size, mtime_ns, hash_muestreado: sampled_hash(path, size)?, inventario_sha256: inventory_hash(probe) })
    }

    /// Construye el asset completo de un archivo.
    pub fn import(&self, path: &Path, portable_path: String) -> DomainResult<Asset> {
        self.import_cancellable(path, portable_path, Arc::new(AtomicBool::new(false)))
    }

    pub fn import_cancellable(&self, path: &Path, portable_path: String, cancel: Arc<AtomicBool>) -> DomainResult<Asset> {
        if cancel.load(Ordering::Acquire) {
            return Err(DomainError::new(tv2_domain::error::ErrorCode::Cancelled, "importación cancelada"));
        }
        let probe =
            self.probe_cancellable(path, cancel.clone()).map_err(|e| DomainError::process(e.to_string()).with("path", path.display().to_string()))?;
        if cancel.load(Ordering::Acquire) {
            return Err(DomainError::new(tv2_domain::error::ErrorCode::Cancelled, "importación cancelada"));
        }
        let fingerprint = self.fingerprint(path, &probe)?;
        let kind = probe.kind();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "medio".into());
        if kind == AssetKind::Image && probe.video.is_none() {
            return Err(DomainError::unsupported("el archivo no contiene video, audio ni imagen reconocibles"));
        }
        Ok(Asset {
            id: AssetId::random(),
            kind,
            name,
            path: portable_path,
            probe,
            fingerprint,
            image_duration: if kind == AssetKind::Image { Some(Ticks::from_seconds(5)) } else { None },
            missing: false,
            extra: Default::default(),
        })
    }
}

fn version_of(ffmpeg: &Path) -> Result<String, ProbeError> {
    let mut c = Command::new(ffmpeg);
    quiet(&mut c);
    let out = c.arg("-version").stdout(Stdio::piped()).stderr(Stdio::null()).output().map_err(|e| ProbeError::Missing(e.to_string()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().next().unwrap_or("").to_string())
}

#[derive(Deserialize, Default)]
struct RawProbe {
    #[serde(default)]
    format: RawFormat,
    #[serde(default)]
    streams: Vec<RawStream>,
}

#[derive(Deserialize, Default)]
struct RawFormat {
    #[serde(default)]
    format_name: String,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    start_time: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawStream {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    pix_fmt: Option<String>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    #[serde(default)]
    avg_frame_rate: Option<String>,
    #[serde(default)]
    time_base: Option<String>,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    nb_frames: Option<String>,
    #[serde(default)]
    sample_rate: Option<String>,
    #[serde(default)]
    channels: Option<u32>,
    #[serde(default)]
    channel_layout: Option<String>,
    #[serde(default)]
    side_data_list: Vec<serde_json::Value>,
    #[serde(default)]
    tags: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    disposition: Option<serde_json::Map<String, serde_json::Value>>,
}

fn secs(s: &Option<String>) -> Option<Ticks> {
    s.as_deref().and_then(|v| v.parse::<f64>().ok()).filter(|f| f.is_finite()).map(Ticks::from_seconds_f64)
}

fn normalize(raw: RawProbe, size: u64, path: &Path) -> MediaProbe {
    let container = raw.format.format_name.split(',').next().unwrap_or("").to_string();
    let mut duration = secs(&raw.format.duration).unwrap_or(Ticks::ZERO);
    let mut start_time = secs(&raw.format.start_time).unwrap_or(Ticks::ZERO);
    let mut video = None;
    let mut audio = Vec::new();
    let mut aidx = 0;
    for s in &raw.streams {
        match s.codec_type.as_str() {
            "video" if video.is_none() => {
                // las carátulas adjuntas (mp3 con imagen) no son video
                let attached = s.disposition.as_ref().and_then(|d| d.get("attached_pic")).and_then(|v| v.as_i64()).unwrap_or(0) == 1;
                if attached {
                    continue;
                }
                let mut rotation = 0i32;
                for sd in &s.side_data_list {
                    if let Some(r) = sd.get("rotation").and_then(|v| v.as_i64()) {
                        rotation = ((r % 360) + 360) as i32 % 360;
                    }
                }
                if let Some(t) = &s.tags
                    && let Some(r) = t.get("rotate").and_then(|v| v.as_str()).and_then(|v| v.parse::<i64>().ok())
                {
                    rotation = ((r % 360) + 360) as i32 % 360;
                }
                let r_rate = s.r_frame_rate.as_deref().and_then(Rational::parse);
                let avg = s.avg_frame_rate.as_deref().and_then(Rational::parse);
                let frame_rate = avg.or(r_rate).unwrap_or(Rational::new(30, 1));
                let vfr = matches!((r_rate, avg), (Some(a), Some(b)) if a.reduced() != b.reduced());
                let st = secs(&s.start_time);
                if let Some(st) = st {
                    start_time = st;
                }
                let is_img = is_image_codec(&s.codec_name) || container == "image2" || container == "png_pipe" || container == "mjpeg";
                video = Some(VideoStreamInfo {
                    stream_index: s.index,
                    codec: s.codec_name.clone(),
                    width: s.width.unwrap_or(0),
                    height: s.height.unwrap_or(0),
                    rotation,
                    frame_rate,
                    avg_frame_rate: avg,
                    time_base: s.time_base.as_deref().and_then(Rational::parse).unwrap_or(Rational::new(1, 1000)),
                    start_time: st.unwrap_or(Ticks::ZERO),
                    pix_fmt: s.pix_fmt.clone().unwrap_or_default(),
                    nb_frames: if is_img { Some(1) } else { s.nb_frames.as_deref().and_then(|v| v.parse().ok()) },
                    duration: secs(&s.duration),
                    variable_frame_rate: vfr,
                });
            }
            "audio" => {
                audio.push(AudioStreamInfo {
                    stream_index: s.index,
                    audio_index: aidx,
                    codec: s.codec_name.clone(),
                    channels: s.channels.unwrap_or(0),
                    channel_layout: s.channel_layout.clone().unwrap_or_default(),
                    sample_rate: s.sample_rate.as_deref().and_then(|v| v.parse().ok()).unwrap_or(0),
                    start_time: secs(&s.start_time).unwrap_or(Ticks::ZERO),
                    duration: secs(&s.duration),
                    title: s.tags.as_ref().and_then(|t| t.get("title")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                });
                aidx += 1;
            }
            _ => {}
        }
    }
    if duration == Ticks::ZERO {
        duration = video.as_ref().and_then(|v| v.duration).or_else(|| audio.first().and_then(|a| a.duration)).unwrap_or(Ticks::ZERO);
    }
    let _ = path;
    MediaProbe { container, duration, start_time, video, audio, size }
}

const SAMPLE_BLOCK: u64 = 8 * 1024 * 1024;

/// SHA-256 de `str(size)` + tres bloques de 8 MiB (inicio, medio, fin), como
/// `medios._hash_muestreado`. Un archivo menor que un bloque se lee entero
/// tres veces (idéntico a V1).
pub fn sampled_hash(path: &Path, size: u64) -> DomainResult<String> {
    let mut h = Sha256::new();
    h.update(size.to_string().as_bytes());
    let mut f = std::fs::File::open(path)?;
    let block = SAMPLE_BLOCK.min(size);
    let offsets =
        [0u64, size.saturating_sub(SAMPLE_BLOCK / 2).min(size / 2).max(size / 2).saturating_sub(SAMPLE_BLOCK / 2), size.saturating_sub(SAMPLE_BLOCK)];
    // V1: (0, max(0, size//2 - B//2), max(0, size - B))
    let offsets = [offsets[0], (size / 2).saturating_sub(SAMPLE_BLOCK / 2), size.saturating_sub(SAMPLE_BLOCK)];
    let mut buf = vec![0u8; block as usize];
    for off in offsets {
        f.seek(SeekFrom::Start(off))?;
        let mut read = 0usize;
        while read < buf.len() {
            let n = f.read(&mut buf[read..])?;
            if n == 0 {
                break;
            }
            read += n;
        }
        h.update(&buf[..read]);
    }
    Ok(hex::encode(h.finalize()))
}

/// `inventario_sha256` de V1: SHA-256 de `json.dumps(inv, sort_keys=True)` con
/// separadores por defecto de Python (`", "` y `": "`) y `ensure_ascii=True`.
/// El inventario V1 usa segundos float; aquí se reconstruye con la misma forma
/// para que un medio importado en V2 comparta identidad con V1.
pub fn inventory_hash(probe: &MediaProbe) -> String {
    let text = v1_inventory_json(probe);
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// Reconstruye el texto exacto de `json.dumps(inv, sort_keys=True)` de V1.
pub fn v1_inventory_json(probe: &MediaProbe) -> String {
    fn py(f: f64) -> String {
        tv2_domain::digest::python_float_repr(f)
    }
    fn s(t: Ticks) -> String {
        py(t.as_seconds_f64())
    }
    let mut out = String::from("{");
    out.push_str(&format!("\"duracion\": {}, ", s(probe.duration)));
    out.push_str("\"pistas\": [");
    for (i, a) in probe.audio.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let dur = a.duration.unwrap_or(probe.duration);
        out.push_str(&format!(
            "{{\"canales\": {}, \"codec\": {}, \"duracion\": {}, \"idx\": {}, \"sample_rate\": {}, \"start_time\": {}}}",
            a.channels,
            py_str(&a.codec),
            s(dur),
            a.audio_index,
            a.sample_rate,
            s(a.start_time)
        ));
    }
    out.push_str("], ");
    out.push_str(&format!("\"t0\": {}, ", s(probe.start_time)));
    match &probe.video {
        None => out.push_str("\"video\": null"),
        Some(v) => {
            let (dw, dh) = v.display_size();
            let fps = (v.frame_rate.as_f64() * 1000.0).round() / 1000.0;
            out.push_str(&format!(
                "\"video\": {{\"codec\": {}, \"display_height\": {}, \"display_width\": {}, \"fps\": {}, \"height\": {}, \"pix_fmt\": {}, \"rotacion\": {}, \"width\": {}}}",
                py_str(&v.codec),
                dh,
                dw,
                py(fps),
                v.height,
                py_str(&v.pix_fmt),
                v.rotation,
                v.width
            ));
        }
    }
    out.push('}');
    out
}

fn py_str(s: &str) -> String {
    // ensure_ascii=True: escapa no-ASCII como \uXXXX
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_ascii() && (c as u32) >= 0x20 => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media").join(name)
    }

    #[test]
    fn locate_and_probe_fixture() {
        let tools = FfmpegTools::locate().expect("ffmpeg vendorizado");
        assert!(tools.version.contains("ffmpeg version"));
        let p = tools.probe(&fixture("fixture-a.mp4")).unwrap();
        assert_eq!(p.container, "mov");
        assert_eq!(p.duration, Ticks::from_seconds(12));
        let v = p.video.as_ref().unwrap();
        assert_eq!((v.width, v.height), (640, 360));
        assert_eq!(v.frame_rate, Rational::new(30, 1));
        assert_eq!(v.nb_frames, Some(360));
        assert_eq!(p.audio.len(), 1);
        assert_eq!(p.audio[0].sample_rate, 48000);
        assert_eq!(p.kind(), AssetKind::Video);
        let png = tools.probe(&fixture("overlay-alpha.png")).unwrap();
        assert_eq!(png.kind(), AssetKind::Image);
        assert_eq!(png.video.as_ref().unwrap().pix_fmt, "rgba");
        let wav = tools.probe(&fixture("tone-220.wav")).unwrap();
        assert_eq!(wav.kind(), AssetKind::Audio);
        assert_eq!(wav.audio[0].channels, 2);
        let fp = tools.fingerprint(&fixture("fixture-a.mp4"), &p).unwrap();
        assert_eq!(fp.hash_muestreado.len(), 64);
        assert_eq!(fp.inventario_sha256.len(), 64);
        assert!(fp.same_identity(&tools.fingerprint(&fixture("fixture-a.mp4"), &p).unwrap()));
    }

    #[test]
    fn inventory_json_has_python_shape() {
        let tools = FfmpegTools::locate().expect("ffmpeg vendorizado");
        let p = tools.probe(&fixture("fixture-a.mp4")).unwrap();
        let text = v1_inventory_json(&p);
        assert!(text.starts_with("{\"duracion\": 12.0, \"pistas\": [{\"canales\": 1, \"codec\": \"aac\", \"duracion\": 12.0, \"idx\": 0, \"sample_rate\": 48000, \"start_time\": 0.0}], \"t0\": 0.0, \"video\": {\"codec\": \"h264\", \"display_height\": 360, \"display_width\": 640, \"fps\": 30.0, \"height\": 360, \"pix_fmt\": \"yuv420p\", \"rotacion\": 0, \"width\": 640}}"), "{text}");
    }

    #[test]
    fn relink_moved_unicode_fixture_preserves_identity_and_rejects_wrong_media() {
        use tv2_domain::{Command, project::Project};
        let tools = FfmpegTools::locate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.mp4");
        let moved = dir.path().join("vídeo movido 日本語.mp4");
        std::fs::copy(fixture("fixture-a.mp4"), &original).unwrap();
        let asset = tools.import(&original, original.to_string_lossy().into_owned()).unwrap();
        let mut project = Project::new("Relink");
        Command::ImportAsset { asset: asset.clone() }.apply(&mut project).unwrap();
        std::fs::rename(&original, &moved).unwrap(); // solo copia temporal de V2
        project.asset_mut(&asset.id).unwrap().missing = true;
        let bad = tools.import(&fixture("fixture-sync.mp4"), "incorrecto.mp4".into()).unwrap();
        let before = project.clone();
        assert!(Command::RelinkAsset { asset_id: asset.id.clone(), path: "incorrecto.mp4".into(), asset: Some(bad) }.apply(&mut project).is_err());
        assert_eq!(before, project);
        assert!(Command::RelinkAsset { asset_id: asset.id.clone(), path: "sin-verificar.mp4".into(), asset: None }.apply(&mut project).is_err());
        let candidate = tools.import(&moved, moved.to_string_lossy().into_owned()).unwrap();
        assert!(candidate.fingerprint.same_identity(&asset.fingerprint));
        Command::RelinkAsset { asset_id: asset.id.clone(), path: moved.to_string_lossy().into_owned(), asset: Some(candidate) }
            .apply(&mut project)
            .unwrap();
        let relinked = project.asset(&asset.id).unwrap();
        assert!(!relinked.missing);
        assert_eq!(relinked.fingerprint, asset.fingerprint);
        assert_eq!(relinked.id, asset.id);
        let mut decoder = crate::VideoDecoder::open(&tools, &moved, Ticks::ZERO, 96, 54, Rational::new(30, 1), false).unwrap();
        let (_, frame) = decoder.next(std::time::Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!((frame.width, frame.height), (96, 54));
    }
}
