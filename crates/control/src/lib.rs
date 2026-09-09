//! Local MCP transport and permission-scoped controller. No shell, SQL, file
//! access or arbitrary project replacement is exposed to clients.
mod engine;
mod persistence;
mod schema;
mod transport;
pub use engine::{ControlEngine, ControlEvent, HostAction, HostState, Permissions, ProposalSummary, Selection, TransportOperation};
pub use transport::{ControlService, PendingRequest};
pub const MCP_VERSION: &str = "2025-11-25";
#[cfg(test)]
mod tests;
