//! V1 reads and media probes never run in the event loop. Completion is applied
//! as one command against the exact session revision captured at launch.
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tv2_domain::{
    asset::Asset,
    error::{DomainError, DomainResult},
    project::Revision,
};

pub struct EditorialImport {
    pub asset: Asset,
    pub import: tv2_v1compat::import::V1Import,
}
pub struct PreparedEditorialImport {
    pub data: EditorialImport,
    pub command: tv2_application::session::PreparedCommand,
    pub enrichment: bool,
    pub command_count: usize,
}

pub struct EditorialJob {
    pub project: String,
    pub revision: Revision,
    pub cancel: Arc<AtomicBool>,
    pub result: crossbeam_channel::Receiver<DomainResult<PreparedEditorialImport>>,
}

impl EditorialJob {
    pub fn start(
        snapshot: tv2_domain::Project,
        root: PathBuf,
        hint: Option<PathBuf>,
        tools: Option<tv2_media::FfmpegTools>,
    ) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let project = snapshot.project_id.clone();
        let revision = snapshot.revision;
        let token = cancel.clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        std::thread::Builder::new().name("editorial-import".into()).spawn(move || {
            let value = (|| {
                let data = prepare(root, hint, snapshot.assets.clone(), tools, &token)?;
                let enrichment = tv2_v1compat::import::is_source_bundle_enrichment(&data.import, &snapshot);
                let mut commands = Vec::new();
                if snapshot.asset(&data.asset.id).is_none() {
                    commands.push(tv2_domain::Command::ImportAsset { asset: data.asset.clone() });
                }
                commands.extend(tv2_v1compat::import::import_commands(&data.import, &snapshot, &data.asset.id));
                let command_count = commands.len();
                let envelope = tv2_application::CommandEnvelope::human(tv2_domain::Command::Batch {
                    label: if enrichment { "Vincular carpeta original V1" } else { "Importar proyecto V1" }.into(),
                    commands,
                })
                .with_base(revision)
                .with_actor(tv2_application::Actor::External { source: "import-v1".into() });
                let command = tv2_application::ProjectSession::new(snapshot).prepare_command(envelope)?;
                if token.load(Ordering::Acquire) {
                    return Err(DomainError::not_available("Importación cancelada"));
                }
                Ok(PreparedEditorialImport { data, command, enrichment, command_count })
            })();
            let _ = tx.send(value);
        })?;
        Ok(Self { project, revision, cancel, result })
    }
}

impl Drop for EditorialJob {
    fn drop(&mut self) {
        // The worker owns only inputs and a bounded result channel; no disk writes
        // or session references. Do not join a blocked filesystem read on the GUI.
        self.cancel.store(true, Ordering::Release);
    }
}

fn prepare(
    root: PathBuf,
    hint: Option<PathBuf>,
    assets: Vec<Asset>,
    tools: Option<tv2_media::FfmpegTools>,
    cancel: &Arc<AtomicBool>,
) -> DomainResult<EditorialImport> {
    let master_path = tv2_v1compat::import::find_master(&root).ok_or_else(|| DomainError::invalid("No hay master V1 en la carpeta"))?;
    let master = discovery_header(&master_path, cancel)?;
    let mut asset = assets.into_iter().find(|a| a.fingerprint.same_identity(&master.fingerprint));
    if asset.is_none() {
        let media = PathBuf::from(&master.media_path);
        let mut candidates: Vec<_> = hint.into_iter().collect();
        if media.is_absolute() {
            candidates.push(media.clone());
        }
        for base in [Some(root.as_path()), master_path.parent(), root.parent()].into_iter().flatten() {
            candidates.push(base.join(&media));
            if let Some(name) = media.file_name() {
                candidates.push(base.join(name));
            }
        }
        candidates.sort();
        candidates.dedup();
        let tools = tools.ok_or_else(|| DomainError::not_available("FFmpeg no disponible para verificar el medio V1"))?;
        for path in candidates {
            if cancel.load(Ordering::Acquire) {
                return Err(DomainError::not_available("Importación cancelada"));
            }
            if !path.is_file() {
                continue;
            }
            let abs = std::path::absolute(path)?;
            if let Ok(found) = tools.import_cancellable(&abs, abs.to_string_lossy().replace('\\', "/"), cancel.clone())
                && found.fingerprint.same_identity(&master.fingerprint)
            {
                asset = Some(found);
                break;
            }
        }
    }
    if cancel.load(Ordering::Acquire) {
        return Err(DomainError::not_available("Importación cancelada"));
    }
    let asset = asset
        .ok_or_else(|| DomainError::invalid(format!("Ningún medio coincide con el master: {}. Importa el original y repite.", master.media_path)))?;
    drop(master);
    let import = tv2_v1compat::import::read_v1_editorial_with_cancel(&root, &asset, &crate::paths::v1_marks_stores(), cancel)?;
    Ok(EditorialImport { asset, import })
}

// Media discovery only needs the identity header. Serde skips track records
// through a cancellable reader instead of allocating another complete master.
fn discovery_header(path: &std::path::Path, cancel: &AtomicBool) -> DomainResult<tv2_v1compat::master::V1Master> {
    use std::io::{BufRead, Read};
    #[derive(serde::Deserialize)]
    struct Header {
        schema: String,
        media: serde_json::Value,
    }
    struct Reader<'a> {
        file: std::fs::File,
        cancel: &'a AtomicBool,
    }
    impl Read for Reader<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::other("Importación cancelada"));
            }
            let len = buf.len().min(64 * 1024);
            self.file.read(&mut buf[..len])
        }
    }
    let mut reader = std::io::BufReader::new(Reader { file: std::fs::File::open(path)?, cancel });
    if reader.fill_buf()?.starts_with(&[239, 187, 191]) {
        reader.consume(3);
    }
    let header: Header = serde_json::from_reader(reader)?;
    Ok(tv2_v1compat::master::V1Master::parse(serde_json::json!({"schema":header.schema,"media":header.media}))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_asset_requires_no_probe_and_cancelled_import_cannot_complete() {
        let root = tempfile::tempdir().unwrap();
        let asset = tv2_domain::commands::tests_support::fake_video("a", 20);
        let raw = serde_json::json!({"schema":"editorial-master/1","project":{"name":"synthetic"},
            "media":{"path":"missing.mp4","duration":20,"fingerprint":asset.fingerprint},"tracks":{}});
        std::fs::write(root.path().join("synthetic.editorial.master.json"), raw.to_string()).unwrap();
        let token = Arc::new(AtomicBool::new(false));
        let value = prepare(root.path().into(), None, vec![asset.clone()], None, &token).unwrap();
        assert_eq!(value.asset.id, asset.id);
        assert_eq!(value.import.master.document, raw);
        token.store(true, Ordering::Release);
        assert!(prepare(root.path().into(), None, vec![asset], None, &token).is_err());
    }
}
