//! UI layer for workhive interactive flows.
//!
//! Architecture:
//! - `session`:  Single terminal lifecycle owner (TerminalSession).
//! - `events`:   Event translation from crossterm to typed UiActions.
//! - `screens`:  Individual interactive screens (confirm, text_input, select, select_with_preview).
//! - `severity`: Typed message severity / style helper used by all
//!   screens that need to distinguish blocking validation
//!   failures from cautionary hints and positive previews.
//! - `validated_input`: Narrow retry-state policy that preserves a
//!   rejected submission as the next prompt's initial buffer and clears
//!   it on acceptance/cancel, shared by validation-loop call sites.

pub mod events;
pub mod screens;
pub mod session;
pub mod severity;
pub mod validated_input;
