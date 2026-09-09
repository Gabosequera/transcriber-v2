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
}
impl From<serde_json::Value> for EvidenceDocument {
    fn from(value: serde_json::Value) -> Self {
        Self(std::sync::Arc::new(EvidenceData { value, digest: Default::default() }))
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
    pub fn source_digest(&self) -> &str {
        self.0.digest.get_or_init(|| crate::digest::digest_object_without(&self.0.value, &["generated_at", "chunks"]))
    }
    pub fn shares_storage(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct MasterEvidence {
    pub asset_id: AssetId,
    pub source_digest: String,
    pub document: EvidenceDocument,
}

impl MasterEvidence {
    pub fn validate(&self, project: &Project) -> DomainResult<()> {
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
