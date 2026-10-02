//! Terminal status probe port.

use thiserror::Error;

use crate::domain::UnitId;

/// Request for visible terminal UI text from a runtime pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalStatusProbeRequest {
    pub group: String,
    pub target_unit: UnitId,
}

/// Captured terminal text for a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalStatusCapture {
    pub unit_id: UnitId,
    pub text: String,
}

/// Errors from the terminal status probe adapter.
#[derive(Debug, Error)]
pub enum TerminalStatusProbeError {
    #[error("terminal status probe unavailable: {reason}")]
    Unavailable { reason: String },
    #[error("terminal status capture failed for {unit_id}: {detail}")]
    CaptureFailed { unit_id: String, detail: String },
}

/// External boundary for reading visible terminal status text.
pub trait TerminalStatusProbePort {
    fn capture_many(
        &self,
        requests: &[TerminalStatusProbeRequest],
    ) -> Result<Vec<Option<TerminalStatusCapture>>, TerminalStatusProbeError>;
}
