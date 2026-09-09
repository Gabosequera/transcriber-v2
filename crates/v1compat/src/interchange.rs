//! Editable NLE handoffs from the validated V1 montage profile. These formats
//! reference the original media; they do not describe the rendered child file.
use crate::{
    V1Result, invalid,
    montaje::{Piece, V1Montaje},
};
use tv2_domain::{Asset, Project, Rational, Sequence, SequenceId, Ticks};

struct Handoff<'a> {
    sequence: &'a Sequence,
    asset: &'a Asset,
    pieces: Vec<Piece>,
    rate: Rational,
}
fn profile<'a>(project: &'a Project, id: &SequenceId) -> V1Result<Handoff<'a>> {
    let document = crate::export::montage_document(project, id)?;
    let sequence = project.sequence(id).ok_or_else(|| invalid("secuencia inexistente"))?;
    let fingerprint = crate::master::parse_fingerprint(&document["media"])?;
    let asset = project.assets.iter().find(|a| a.fingerprint.same_identity(&fingerprint)).ok_or_else(|| invalid("medio fuente inexistente"))?;
    let video = asset.probe.video.as_ref().ok_or_else(|| invalid("intercambio V1 requiere fuente de video"))?;
    let streams: std::collections::BTreeSet<_> = sequence.clips.iter().filter_map(|c| c.audio_stream).collect();
    if streams != asset.probe.audio.iter().map(|a| a.audio_index).collect() {
        return Err(invalid("intercambio simple debe conservar todas las pistas de audio originales; no se activan pistas omitidas"));
    }
    if video.variable_frame_rate {
        return Err(invalid("intercambio requiere fuente CFR; no se inventa timecode para VFR"));
    }
    let rate = sequence.frame_rate.reduced();
    if rate != video.frame_rate.reduced() {
        return Err(invalid("frame rate de secuencia diferente a fuente; conformado no representable"));
    }
    let montage = V1Montaje::parse(document, Some(&asset.fingerprint))?;
    let pieces = montage.flatten(false);
    if pieces.is_empty() {
        return Err(invalid("montaje vacío no tiene eventos para NLE"));
    }
    for piece in &pieces {
        for value in [piece.source_ini, piece.source_fin, piece.seq_ini, piece.seq_fin] {
            frame(crate::secs_to_ticks(value), rate)?;
        }
    }
    Ok(Handoff { sequence, asset, pieces, rate })
}
fn frame(time: Ticks, rate: Rational) -> V1Result<i64> {
    let numerator = time.0 as i128 * rate.num as i128;
    let denominator = tv2_domain::FLICKS_PER_SECOND as i128 * rate.den as i128;
    if numerator % denominator != 0 {
        return Err(invalid("borde fuera de fotograma: NLE export no redondea decisiones editoriales"));
    }
    i64::try_from(numerator / denominator).map_err(|_| invalid("timecode excede representación"))
}
fn clean_line(text: &str) -> V1Result<&str> {
    if text.chars().any(|c| c.is_control()) { Err(invalid("nombre contiene controles no representables en EDL/XML")) } else { Ok(text) }
}
fn tc(time: f64, rate: Rational) -> V1Result<String> {
    let frames = frame(crate::secs_to_ticks(time), rate)?;
    let nominal = rate.as_f64().round() as i64;
    if nominal <= 0 || nominal > 120 {
        return Err(invalid("frame rate no representable en CMX3600"));
    }
    let seconds = frames / nominal;
    if seconds >= 24 * 3600 {
        return Err(invalid("CMX3600 no admite timecode de 24 horas o más"));
    }
    Ok(format!("{:02}:{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60, frames % nominal))
}
/// CMX3600 non-drop timecode. Mono/stereo only, one audio stream; a multistream
/// source must use FCPXML, which records the actual audio inventory.
pub fn export_edl(project: &Project, sequence: &SequenceId) -> V1Result<String> {
    let handoff = profile(project, sequence)?;
    if handoff.pieces.len() > 999 {
        return Err(invalid("CMX3600 admite como máximo 999 eventos; usa FCPXML"));
    }
    if handoff.asset.probe.audio.len() > 1 || handoff.asset.probe.audio.first().is_some_and(|a| a.channels > 2) {
        return Err(invalid("CMX3600 no representa todas las pistas/canales de esta fuente; usa FCPXML"));
    }
    let mut text = format!("TITLE: {}\nFCM: NON-DROP FRAME\n\n", clean_line(&handoff.sequence.name)?);
    for (index, piece) in handoff.pieces.iter().enumerate() {
        let times = format!(
            "{} {} {} {}",
            tc(piece.source_ini, handoff.rate)?,
            tc(piece.source_fin, handoff.rate)?,
            tc(piece.seq_ini, handoff.rate)?,
            tc(piece.seq_fin, handoff.rate)?
        );
        text.push_str(&format!("{:03}  AX       V     C        {times}\n", index + 1));
        if let Some(audio) = handoff.asset.probe.audio.first() {
            text.push_str(&format!("{:03}  AX       {:<5} C        {times}\n", index + 1, if audio.channels == 1 { "A" } else { "AA" }));
        }
        text.push_str(&format!("* FROM CLIP NAME: {}\n* SOURCE FILE: {}\n\n", clean_line(&piece.clip_id)?, clean_line(&handoff.asset.path)?));
    }
    Ok(text)
}
fn xml(text: &str) -> V1Result<String> {
    clean_line(text)?;
    Ok(text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;"))
}
fn uri(path: &str) -> V1Result<String> {
    let path = path.replace('\\', "/");
    if path.chars().any(|c| c.is_control()) || path.split('/').any(|p| p == ".." || p == ".") {
        return Err(invalid("localizador fuente requiere ruta absoluta normalizada"));
    }
    let absolute = path.starts_with('/')
        || (path.len() > 2 && path.as_bytes()[0].is_ascii_alphabetic() && path.as_bytes()[1] == b':' && path.as_bytes()[2] == b'/');
    if !absolute {
        return Err(invalid("FCPXML requiere localizador fuente absoluto; resuelve la base del proyecto antes de exportar"));
    }
    let mut escaped = String::new();
    for byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(byte) {
            escaped.push(*byte as char);
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(if escaped.starts_with("//") {
        format!("file:{escaped}")
    } else if escaped.starts_with('/') {
        format!("file://{escaped}")
    } else {
        format!("file:///{escaped}")
    })
}
fn rational_time(time: Ticks, rate: Rational) -> V1Result<String> {
    let frames = frame(time, rate)?;
    Ok(format!("{}/{}s", frames as i128 * rate.den as i128, rate.num))
}
/// FCPXML 1.9, full source inventory and rational frame duration. Unknown
/// composition/effect profiles reject before serialization through the V1 adapter.
pub fn export_fcpxml(project: &Project, sequence: &SequenceId) -> V1Result<String> {
    let h = profile(project, sequence)?;
    let rate = h.rate;
    let video = h.asset.probe.video.as_ref().unwrap();
    let path = uri(&h.asset.path)?;
    // Media duration may include an audio tail off frame; preserve that precise
    // duration in flicks while all editorial clip boundaries remain frame exact.
    let asset_duration = format!("{}/{}s", h.asset.duration().0, tv2_domain::FLICKS_PER_SECOND);
    let last = h.pieces.last().unwrap();
    let duration = rational_time(crate::secs_to_ticks(last.seq_fin), rate)?;
    let mut text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.9\">\n  <resources>\n    <format id=\"r1\" name=\"FFVideoFormatRateUndefined\" frameDuration=\"{}/{}s\" width=\"{}\" height=\"{}\"/>\n    <asset id=\"r2\" name=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"1\" hasAudio=\"{}\" format=\"r1\"{}>\n      <media-rep kind=\"original-media\" src=\"{}\"/>\n    </asset>\n  </resources>\n  <library><event name=\"Transcriptor\"><project name=\"{}\">\n    <sequence format=\"r1\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"NDF\"><spine>\n",
        rate.den,
        rate.num,
        video.width,
        video.height,
        xml(&h.asset.name)?,
        asset_duration,
        u8::from(!h.asset.probe.audio.is_empty()),
        if h.asset.probe.audio.is_empty() {
            String::new()
        } else {
            format!(
                " audioSources=\"{}\" audioChannels=\"{}\" audioRate=\"{}\"",
                h.asset.probe.audio.len(),
                h.asset.probe.audio.iter().map(|s| s.channels).sum::<u32>(),
                h.asset.probe.audio[0].sample_rate
            )
        },
        xml(&path)?,
        xml(&h.sequence.name)?,
        duration
    );
    if h.asset.probe.audio.iter().any(|a| a.sample_rate != h.asset.probe.audio[0].sample_rate) {
        return Err(invalid("FCPXML simple requiere frecuencia de muestreo uniforme"));
    }
    for piece in &h.pieces {
        text.push_str(&format!(
            "      <asset-clip ref=\"r2\" name=\"{}\" offset=\"{}\" start=\"{}\" duration=\"{}\"/>\n",
            xml(&piece.clip_id)?,
            rational_time(crate::secs_to_ticks(piece.seq_ini), rate)?,
            rational_time(crate::secs_to_ticks(piece.source_ini), rate)?,
            rational_time(crate::secs_to_ticks(piece.source_fin - piece.source_ini), rate)?
        ));
    }
    text.push_str("    </spine></sequence>\n  </project></event></library>\n</fcpxml>\n");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn project() -> (Project, SequenceId) {
        let mut p = Project::new("nle");
        let mut asset = tv2_domain::commands::tests_support::fake_video("source", 12);
        asset.path = "C:/Media/source & café.mp4".into();
        let montage=V1Montaje::parse(json!({"schema":"editorial-montaje/1","media":asset.fingerprint,"duration_source":12.0,"clips":[{"clip_id":"clip-000001","source_ini":1.0,"source_fin":3.0,"seq_ini":0.0}]}),Some(&asset.fingerprint)).unwrap();
        let sequence = montage.to_sequence(&asset.id, Rational::new(30, 1), 640, 360, 1);
        let id = sequence.id.clone();
        p.sequences.push(sequence);
        p.assets.push(asset);
        (p, id)
    }
    #[test]
    fn edl_and_fcpxml_reference_source_and_escape_uri() {
        let (p, id) = project();
        let edl = export_edl(&p, &id).unwrap();
        assert!(edl.contains("00:00:01:00 00:00:03:00 00:00:00:00 00:00:02:00"));
        let xml = export_fcpxml(&p, &id).unwrap();
        assert!(xml.contains("file:///C:/Media/source%20%26%20caf%C3%A9.mp4"));
        assert!(xml.contains("duration=\"60/30s\""));
    }
    #[test]
    fn export_rejects_audio_loss_and_relative_paths() {
        let (mut p, id) = project();
        let audio = p.assets[0].probe.audio[0].clone();
        p.assets[0].probe.audio.push(audio);
        assert!(export_edl(&p, &id).is_err());
        let (mut p, id) = project();
        p.assets[0].path = "relative.mp4".into();
        assert!(export_fcpxml(&p, &id).is_err());
        assert!(tc(24.0 * 3600.0, Rational::new(30, 1)).is_err());
    }
    #[test]
    fn native_single_source_profile_exports_and_does_not_enable_omitted_audio() {
        let (mut p, id) = project();
        p.sequence_mut(&id).unwrap().extra.clear();
        assert!(export_edl(&p, &id).is_ok());
        p.sequence_mut(&id).unwrap().clips.retain(|c| c.audio_stream.is_none());
        p.sequence_mut(&id).unwrap().tracks.retain(|t| t.kind == tv2_domain::TrackKind::Video);
        assert!(export_fcpxml(&p, &id).is_err());
    }
}
