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

pub struct EditorialJob {
    pub project: String,
    pub revision: Revision,
    pub cancel: Arc<AtomicBool>,
    pub result: crossbeam_channel::Receiver<DomainResult<EditorialImport>>,
}

impl EditorialJob {
    pub fn start(
        project: String,
        revision: Revision,
        root: PathBuf,
        hint: Option<PathBuf>,
        assets: Vec<Asset>,
        tools: Option<tv2_media::FfmpegTools>,
    ) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        std::thread::Builder::new().name("editorial-import".into()).spawn(move || {
            let value = prepare(root, hint, assets, tools, &token);
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
    let master = tv2_v1compat::master::V1Master::load(&master_path)?;
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
    let import = tv2_v1compat::import::read_v1_editorial(&root, &asset)?;
    Ok(EditorialImport { asset, import })
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
