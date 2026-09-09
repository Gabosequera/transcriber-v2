//! Original analysis documents are immutable evidence; projected layers are views.
use crate::error::DomainResult;
use crate::{AssetId, DomainError, Project};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct MasterEvidence {
    pub asset_id: AssetId,
    pub source_digest: String,
    pub document: serde_json::Value,
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
        let mut canonical = self.document.clone();
        let obj = canonical.as_object_mut().ok_or_else(|| DomainError::invalid("master no es un objeto"))?;
        obj.remove("generated_at");
        obj.remove("chunks");
        if crate::digest::digest_json(&canonical) != self.source_digest {
            return Err(DomainError::precondition("digest del master incoherente"));
        }
        let duration = self.document["media"]["duration"].as_f64().ok_or_else(|| DomainError::invalid("master sin duración"))?;
        if !duration.is_finite() || duration <= 0.0 || duration > crate::Ticks::MAX.as_seconds_f64() {
            return Err(DomainError::out_of_range("duración de master inválida"));
        }
        Ok(())
    }
}
