//! # workhive-infra
//!
//! Adapter implementations for workhive port traits.
//!
//! Each module provides a concrete implementation that talks to an external system
//! (git CLI, tmux CLI, filesystem, config files, etc.). These adapters implement
//! the port traits defined in `workhive-core`.
//!
//! # Runtime boundary
//!
//! The `tmux` module owns `TmuxPort`, `TmuxError`, and all tmux-specific DTOs.
//! These types are **not** re-exported by `workhive-core`. The `runtime` module
//! bridges `TmuxPort` → `WorkspaceRuntimePort` / `PaneRuntimePort` and is the
//! only translation point from `TmuxError` to `RuntimeError`.

pub mod activity_event_store;
pub mod clock;
pub mod config;
pub mod control_plane;
pub mod dependency;
pub mod discovery;
pub mod environment;
pub mod errors;
pub mod executable;
pub mod git;
pub mod host_probe;
pub mod i18n;
pub mod message_bus;
pub mod notification;
pub mod notification_outbox_store;
pub mod orchestration;
pub mod pane_activity;
pub mod pane_launch_identity;
pub mod pane_message;
pub mod pane_observer;
pub mod pane_registry;
pub mod pane_supervisor;
pub mod path_completion;
pub mod process_observer;
pub mod project_index;
pub mod project_init_lock;
pub mod replacement_shell;
pub mod runtime;
pub mod snapshot_legacy;
pub mod snapshot_runtime;
pub mod snapshot_store;
pub mod terminal_status_probe;
pub mod tmux;
pub mod tool_launch_instrumentation;
pub mod workspace_fs;
pub mod workspace_identity;
pub mod workspace_provision;
pub mod worktree_provenance;

#[cfg(test)]
mod message_bus_integration;

#[cfg(test)]
mod pane_sync_integration;

#[cfg(test)]
pub(crate) mod test_env {
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Run a closure with exclusive access to process-level environment variables.
    ///
    /// Sets `XDG_CONFIG_HOME` before the closure and restores the previous value
    /// afterward, even on panic. All tests that modify env vars MUST use this
    /// to avoid concurrent mutation of the shared process environment.
    pub fn with_xdg_config_home<F, R>(value: &str, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let old = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", value) };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));

        match old {
            Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }

        result.unwrap_or_else(|e| std::panic::resume_unwind(e))
    }

    /// Run a closure with `XDG_CONFIG_HOME` explicitly unset.
    pub fn without_xdg_config_home<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let old = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));

        match old {
            Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }

        result.unwrap_or_else(|e| std::panic::resume_unwind(e))
    }
}
