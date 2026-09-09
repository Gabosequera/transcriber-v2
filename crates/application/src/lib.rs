//! Capa de aplicación: la sesión de proyecto ejecuta comandos con envelope
//! (actor, revisión base, idempotencia), mantiene historial undo/redo por
//! revisiones auditables, persiste JSON de forma atómica y escribe un journal.

pub mod documents;
pub mod reconcile;
pub mod session;
pub mod store;

pub use session::{Actor, CommandEnvelope, CommandResult, DryRunResult, HistoryEntry, PROTOCOL_VERSION, ProjectSession};
pub use store::{JOURNAL_FILE, PROJECT_FILE, ProjectStore};
mod protection;
#[cfg(test)]
mod semantic_tests;
