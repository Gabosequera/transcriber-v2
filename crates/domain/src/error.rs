//! Errores con código estable, mensaje humano (español), contexto y acción.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// El objeto referido no existe en el proyecto.
    NotFound,
    /// Argumento inválido (rango, ID, tipo, tamaño).
    Invalid,
    /// La operación colisionaría con otro clip de la misma pista.
    Overlap,
    /// Rango fuera del medio / de la secuencia.
    OutOfRange,
    /// Precondición no satisfecha (pista bloqueada, tipo incompatible…).
    Precondition,
    /// La acción no está disponible para este tipo de objeto (p. ej. aceptar un bloque).
    NotAvailable,
    /// La revisión base del comando ya no es la vigente.
    StaleRevision,
    /// Conflicto con una edición externa del archivo.
    ExternalConflict,
    /// Nada que deshacer/rehacer.
    Empty,
    /// Error de E/S al persistir.
    Io,
    /// Fallo de un proceso externo (ffmpeg/ffprobe/worker).
    Process,
    /// Capacidad no disponible en esta instalación.
    Unsupported,
    /// Trabajo cancelado por el usuario.
    Cancelled,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DomainError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

impl DomainError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        DomainError { code, message: message.into(), context: Vec::new(), action: None }
    }
    pub fn not_found(what: &str, id: impl fmt::Display) -> Self {
        DomainError::new(ErrorCode::NotFound, format!("{what} «{id}» no existe en el proyecto")).with("id", id.to_string())
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::Invalid, message)
    }
    pub fn overlap(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::Overlap, message)
    }
    pub fn out_of_range(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::OutOfRange, message)
    }
    pub fn precondition(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::Precondition, message)
    }
    pub fn not_available(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::NotAvailable, message)
    }
    pub fn stale(expected: u64, actual: u64) -> Self {
        DomainError::new(ErrorCode::StaleRevision, format!("el proyecto cambió: revisión base {expected}, vigente {actual}"))
            .with("base_revision", expected.to_string())
            .with("current_revision", actual.to_string())
            .with_action("vuelve a consultar el estado y reintenta sobre la revisión vigente")
    }
    pub fn io(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::Io, message)
    }
    pub fn process(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::Process, message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        DomainError::new(ErrorCode::Unsupported, message)
    }
    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.context.push((key.to_string(), value.into()));
        self
    }
    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)?;
        if let Some(a) = &self.action {
            write!(f, " · {a}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DomainError {}

impl From<std::io::Error> for DomainError {
    fn from(e: std::io::Error) -> Self {
        DomainError::io(e.to_string())
    }
}

impl From<serde_json::Error> for DomainError {
    fn from(e: serde_json::Error) -> Self {
        DomainError::invalid(format!("JSON inválido: {e}"))
    }
}

pub type DomainResult<T> = Result<T, DomainError>;
