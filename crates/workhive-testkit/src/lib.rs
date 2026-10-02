//! # workhive-testkit
//!
//! Test utilities, fake adapters, and fixture builders for workhive.
//!
//! This crate is intended as a dev-dependency only. It provides:
//!
//! - **Fake adapters** — in-memory implementations of port traits for unit testing
//! - **Fixture builders** — convenient constructors for test data
//!
//! ## Usage
//!
//! ```rust,ignore
//! use workhive_testkit::fakes::FakeGit;
//! let git = FakeGit::new();
//! // Use in tests that require a GitPort implementation
//! ```

pub mod fakes;
