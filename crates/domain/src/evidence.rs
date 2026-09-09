//! Original analysis documents are immutable evidence; projected layers are views.
use crate::error::DomainResult;
use crate::{AssetId, DomainError, Project};
use serde::{Deserialize, Serialize};

/// Immutable shared analysis payload. Copies of sessions/history share both bytes
/// and the canonical digest; deserialization always starts a fresh trusted cache.
#[derive(Clone, Debug)]
pub struct EvidenceDocument(std::sync::Arc<EvidenceData>);
#[derive(Debug)]
struct EvidenceData {
    value: serde_json::Value,
    digest: std::sync::OnceLock<String>,
    full_digest: std::sync::OnceLock<String>,
    serialized: std::sync::OnceLock<SerializedEvidence>,
}

#[derive(Clone, Debug)]
pub struct SerializedEvidence {
    pub sha256: String,
    pub bytes: u64,
}
impl From<serde_json::Value> for EvidenceDocument {
    fn from(value: serde_json::Value) -> Self {
        Self(std::sync::Arc::new(EvidenceData { value, digest: Default::default(), full_digest: Default::default(), serialized: Default::default() }))
    }
}
impl std::ops::Deref for EvidenceDocument {
    type Target = serde_json::Value;
    fn deref(&self) -> &Self::Target {
        &self.0.value
    }
}
impl PartialEq for EvidenceDocument {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0) || self.0.value == other.0.value
    }
}
impl PartialEq<serde_json::Value> for EvidenceDocument {
    fn eq(&self, other: &serde_json::Value) -> bool {
        self.0.value == *other
    }
}
impl Serialize for EvidenceDocument {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.value.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for EvidenceDocument {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(serde_json::Value::deserialize(deserializer)?.into())
    }
}
impl EvidenceDocument {
    /// Storage fingerprint of serde JSON bytes, computed without a second JSON
    /// tree or a large String. Canonical/editorial digests remain unchanged.
    pub fn serialized_fingerprint(&self) -> &SerializedEvidence {
        use sha2::{Digest, Sha256};
        self.0.serialized.get_or_init(|| {
            struct Sink {
                hash: Sha256,
                bytes: u64,
            }
            impl std::io::Write for Sink {
                fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                    self.hash.update(bytes);
                    self.bytes += bytes.len() as u64;
                    Ok(bytes.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            let mut sink = Sink { hash: Sha256::new(), bytes: 0 };
            serde_json::to_writer(&mut sink, &self.0.value).expect("JSON Value serialization into an infallible hash sink");
            SerializedEvidence { sha256: hex::encode(sink.hash.finalize()), bytes: sink.bytes }
        })
    }
    pub fn full_digest(&self) -> &str {
        self.0.full_digest.get_or_init(|| crate::digest::digest_json(&self.0.value))
    }
    pub fn source_digest(&self) -> &str {
        self.0.digest.get_or_init(|| crate::digest::digest_object_without(&self.0.value, &["generated_at", "chunks"]))
    }
    pub fn shares_storage(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MasterEvidence {
    pub asset_id: AssetId,
    pub source_digest: String,
    pub document: EvidenceDocument,
    /// Immutable source files, shared across edits; absent in legacy projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bundle: Option<std::sync::Arc<SourceBundle>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBundle {
    master_path: String,
    documents: std::sync::Arc<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    files: std::collections::BTreeMap<String, SourceFile>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    directories: std::collections::BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_master_digest: Option<String>,
    #[serde(skip)]
    master: std::sync::OnceLock<Result<EvidenceDocument, String>>,
}
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub source: std::path::PathBuf,
    pub size: u64,
    pub sha256: String,
}
impl PartialEq for SourceBundle {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
            || self.master_path == other.master_path
                && self.documents == other.documents
                && self.files == other.files
                && self.directories == other.directories
                && self.external_master_digest == other.external_master_digest
    }
}
impl SourceBundle {
    /// Compare immutable captured content independently of storage relocation.
    /// This does not verify source bytes; normal bundle equality also compares
    /// locators and remains the rule for commands/replacement protection.
    pub fn same_content(&self, other: &Self) -> bool {
        self.master_path == other.master_path
            && self.documents == other.documents
            && self.directories == other.directories
            && self.external_master_digest == other.external_master_digest
            && self.files.len() == other.files.len()
            && self.files.iter().all(|(path, file)| other.files.get(path).is_some_and(|other| file.size == other.size && file.sha256 == other.sha256))
    }

    pub fn new(master_path: String, documents: std::collections::BTreeMap<String, String>) -> Self {
        Self {
            master_path,
            documents: std::sync::Arc::new(documents),
            files: Default::default(),
            directories: Default::default(),
            external_master_digest: None,
            master: Default::default(),
        }
    }
    pub fn with_files(mut self, files: std::collections::BTreeMap<String, SourceFile>) -> Self {
        self.files = files;
        self
    }
    pub fn with_directories(mut self, directories: std::collections::BTreeSet<String>) -> Self {
        self.directories = directories;
        self
    }
    pub fn directories(&self) -> &std::collections::BTreeSet<String> {
        &self.directories
    }
    pub fn with_external_master_digest(mut self, digest: String) -> Self {
        self.external_master_digest = Some(digest);
        self
    }
    pub fn external_master_digest(&self) -> Option<&str> {
        self.external_master_digest.as_deref()
    }
    /// Canonical master digest, distinct from the SHA-256 of original bytes.
    /// External descriptors must additionally be verified by the store on IO.
    pub fn master_digest(&self) -> DomainResult<&str> {
        if self.documents.contains_key(&self.master_path) {
            let digest = self.master_document()?.full_digest();
            if self.external_master_digest.as_deref().is_some_and(|external| external != digest) {
                return Err(DomainError::invalid("digest del master externo difiere del documento"));
            }
            Ok(digest)
        } else if self.files.contains_key(&self.master_path) {
            self.external_master_digest.as_deref().ok_or_else(|| DomainError::invalid("master externo sin digest canónico"))
        } else {
            Err(DomainError::invalid("carpeta original sin master"))
        }
    }
    /// Storage relocation shares original text and its parsed evidence cache.
    /// It never duplicates the captured documents across history snapshots.
    pub fn relocated(&self, files: std::collections::BTreeMap<String, SourceFile>) -> Self {
        Self {
            master_path: self.master_path.clone(),
            documents: self.documents.clone(),
            files,
            directories: self.directories.clone(),
            external_master_digest: self.external_master_digest.clone(),
            master: self.master.clone(),
        }
    }
    pub fn files(&self) -> &std::collections::BTreeMap<String, SourceFile> {
        &self.files
    }
    pub fn master_path(&self) -> &str {
        &self.master_path
    }
    pub fn documents(&self) -> &std::collections::BTreeMap<String, String> {
        &self.documents
    }
    fn master_document(&self) -> DomainResult<&EvidenceDocument> {
        self.master
            .get_or_init(|| {
                let raw = self.documents.get(&self.master_path).ok_or_else(|| "carpeta original sin master".to_string())?;
                serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}')).map(Into::into).map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| DomainError::invalid(e.clone()))
    }
}

impl MasterEvidence {
    /// A legacy master may acquire its missing original folder. The immutable
    /// document/identity/digest cannot change and an existing bundle cannot be
    /// removed or replaced. Callers must still validate the candidate bundle;
    /// storage IO additionally verifies external source bytes and canonical JSON.
    pub fn allows_source_enrichment(&self, candidate: &Self) -> bool {
        self.asset_id == candidate.asset_id
            && self.source_digest == candidate.source_digest
            && self.document == candidate.document
            && (self.source_bundle.is_none() || self.source_bundle == candidate.source_bundle)
    }

    pub fn validate(&self, project: &Project) -> DomainResult<()> {
        if let Some(bundle) = &self.source_bundle {
            for file in bundle.files().values() {
                let locator = file.source.to_string_lossy();
                if locator.starts_with("@project/")
                    && (file.sha256.len() != 64
                        || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                        || locator != format!("@project/source-bundles/{}", file.sha256))
                {
                    return Err(DomainError::invalid("localizador de evidencia fuera del almacén del proyecto"));
                }
            }
        }
        if let Some(bundle) = &self.source_bundle
            && bundle.master_digest()? != self.document.full_digest()
        {
            return Err(DomainError::precondition("el master de la carpeta original difiere de la evidencia"));
        }
        let a = project.asset(&self.asset_id).ok_or_else(|| DomainError::not_found("asset", &self.asset_id))?;
        if self.document["schema"] != "editorial-master/1" {
            return Err(DomainError::unsupported("schema de master desconocido"));
        }
        let fp: crate::Fingerprint = serde_json::from_value(self.document["media"]["fingerprint"].clone())?;
        if !fp.same_identity(&a.fingerprint) {
            return Err(DomainError::precondition("master de otro medio"));
        }
        if self.document.source_digest() != self.source_digest {
            return Err(DomainError::precondition("digest del master incoherente"));
        }
        let duration = self.document["media"]["duration"].as_f64().ok_or_else(|| DomainError::invalid("master sin duración"))?;
        if !duration.is_finite() || duration <= 0.0 || duration > crate::Ticks::MAX.as_seconds_f64() {
            return Err(DomainError::out_of_range("duración de master inválida"));
        }
        Ok(())
    }
}
