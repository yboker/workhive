//! Typed fallback observer for platforms we do not support.
//!
//! Returns `ProcessObserverError::Unsupported` for every observation.
//! The merge policy treats this as "the OS layer is silent" and falls
//! through to tmux fallback, which is the correct degradation path.

use workhive_core::domain::PaneObservation;
use workhive_core::ports::{PaneProcessLookup, ProcessObserverError, ProcessObserverPort};

/// Process observer that reports `Unsupported` on every call.
///
/// Used on hosts where neither macOS `ps` nor Linux `/proc` is
/// available. The error path emits a `tracing::debug!` (not `warn!`)
/// because unsupported is the documented, non-failure degradation.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedProcessObserver;

impl UnsupportedProcessObserver {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessObserverPort for UnsupportedProcessObserver {
    fn observe(
        &self,
        _lookup: &PaneProcessLookup,
    ) -> Result<Option<PaneObservation>, ProcessObserverError> {
        tracing::debug!(
            "process_observer: platform unsupported; merge policy will fall back to tmux"
        );
        Err(ProcessObserverError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_observer_returns_unsupported_error() {
        let obs = UnsupportedProcessObserver::new();
        let lookup = PaneProcessLookup {
            tty: Some("/dev/ttys003".into()),
            shell_pid: Some(42),
        };
        let err = obs.observe(&lookup).unwrap_err();
        assert!(matches!(err, ProcessObserverError::Unsupported));
    }
}
