//! Config port — abstracts configuration loading and persistence.
//!
//! The configuration model is split by concern; everything is re-exported
//! here so consumers keep using `crate::ports::{AppConfig, ConfigPort, ...}`.

mod ai_integrations;
mod app;
mod env_tools;
mod layout;
mod logging;
mod notifications;
mod pane;
mod port;
mod projects;
mod runtime_naming;
#[cfg(test)]
mod tests;

pub use ai_integrations::*;
pub use app::*;
pub use env_tools::*;
pub use layout::*;
pub use logging::*;
pub use notifications::*;
pub use pane::*;
pub use port::*;
pub use projects::*;
pub use runtime_naming::*;
