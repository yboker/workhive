//! UI layer for workhive interactive flows.
//!
//! Architecture:
//! - `session`:  Single terminal lifecycle owner (TerminalSession).
//! - `events`:   Event translation from crossterm to typed UiActions.
//! - `screens`:  Individual interactive screens (confirm, text_input, select, select_with_preview).
//! - `validated_input`: Narrow retry-state policy that preserves a
//!   rejected submission as the next prompt's initial buffer and clears
//!   it on acceptance/cancel, shared by validation-loop call sites.

pub mod events;
pub mod screens;
pub mod session;
pub mod validated_input;
