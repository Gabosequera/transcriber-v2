//! Bound heavy in-session receipt payloads while keeping every key replayable.
//! Cold copies are temporary acceleration storage; durable truth is the audit.
use crate::session::{CommandEnvelope, CommandResult};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    io::{Read, Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
};
use tv2_domain::{DomainError, error::DomainResult};

pub(crate) const HOT_RECEIPTS: usize = 1024;
type Receipt = (CommandEnvelope, CommandResult);
pub(crate) enum StoredReceipt {
    Hot(Box<Receipt>),
    Cold { owner: Arc<ReceiptSpill>, offset: u64, length: usize, sha256: String, storage: bool },
}

pub(crate) struct ReceiptSpill {
    root: tempfile::TempDir,
    file: Mutex<std::fs::File>,
}

fn owner(spill: &mut Option<Arc<ReceiptSpill>>) -> DomainResult<Arc<ReceiptSpill>> {
    if let Some(owner) = spill {
        return Ok(owner.clone());
    }
    let root = tempfile::Builder::new().prefix("tv2-receipts-").tempdir()?;
    let file = std::fs::OpenOptions::new().create_new(true).read(true).write(true).open(root.path().join("receipts.bin"))?;
    let owner = Arc::new(ReceiptSpill { root, file: Mutex::new(file) });
    *spill = Some(owner.clone());
    Ok(owner)
}

impl StoredReceipt {
    pub(crate) fn new(request: CommandEnvelope, result: CommandResult, count: usize, spill: &mut Option<Arc<ReceiptSpill>>) -> DomainResult<Self> {
        let (raw, storage) = if crate::storage_codec::command_has_large_evidence(&request.command) {
            let owner = owner(spill)?;
            let mut writer = crate::storage_codec::Writer::new(owner.root.path());
            let mut shadow = request.clone();
            shadow.command = writer.command(&request.command)?;
            (serde_json::to_vec(&(&shadow, &result))?, writer.used)
        } else {
            (serde_json::to_vec(&(&request, &result))?, false)
        };
        if raw.len() > 64 * 1024 * 1024 {
            return Err(DomainError::invalid("recibo individual supera 64 MiB"));
        }
        // Bound both entry count and encoded payload, at most 64 MiB hot.
        if !storage && count < HOT_RECEIPTS && raw.len() <= 64 * 1024 {
            return Ok(Self::Hot(Box::new((request, result))));
        }
        let owner = owner(spill)?;
        let sha256 = hex::encode(Sha256::digest(&raw));
        let offset = {
            let mut file = owner.file.lock().map_err(|_| DomainError::io("caché de recibos ocupada"))?;
            let offset = file.seek(SeekFrom::End(0))?;
            file.write_all(&raw)?;
            // This is reconstructible cache, not the durable audit commit.
            file.flush()?;
            offset
        };
        Ok(Self::Cold { owner, offset, length: raw.len(), sha256, storage })
    }

    pub(crate) fn load(&self) -> DomainResult<Cow<'_, Receipt>> {
        match self {
            Self::Hot(value) => Ok(Cow::Borrowed(value)),
            Self::Cold { owner, offset, length, sha256, storage } => {
                let mut raw = vec![0; *length];
                let mut file = owner.file.lock().map_err(|_| DomainError::io("caché de recibos ocupada"))?;
                file.seek(SeekFrom::Start(*offset))?;
                file.read_exact(&mut raw)?;
                if hex::encode(Sha256::digest(&raw)) != *sha256 {
                    return Err(DomainError::invalid("caché de recibo corrupta; reabre el proyecto para restaurar desde auditoría"));
                }
                let mut receipt: Receipt = serde_json::from_slice(&raw)?;
                if *storage {
                    crate::storage_codec::Reader::new(owner.root.path()).command(&mut receipt.0.command)?;
                }
                Ok(Cow::Owned(receipt))
            }
        }
    }

    pub(crate) fn result(&self) -> DomainResult<CommandResult> {
        match self {
            Self::Hot(receipt) => Ok(receipt.1.clone()),
            Self::Cold { owner, offset, length, sha256, .. } => {
                let mut bytes = vec![0; *length];
                let mut file = owner.file.lock().map_err(|_| DomainError::io("caché de recibos ocupada"))?;
                file.seek(SeekFrom::Start(*offset))?;
                file.read_exact(&mut bytes)?;
                if hex::encode(Sha256::digest(&bytes)) != *sha256 {
                    return Err(DomainError::invalid("caché de recibo corrupta"));
                }
                let receipt: Receipt = serde_json::from_slice(&bytes)?;
                Ok(receipt.1)
            }
        }
    }
}
