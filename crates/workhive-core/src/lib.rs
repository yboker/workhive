//! # workhive-core
//!
//! Domain model, port traits, and error types for the workhive workspace manager.
//!
//! This crate is the leaf dependency — it has zero internal crate dependencies.
//! All external system interactions are abstracted behind port traits defined here.

pub mod control_plane;
pub mod domain;
pub mod errors;
pub mod i18n_keys;
pub mod ports;
pub mod usecases;
