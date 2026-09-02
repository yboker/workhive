//! Wall-clock port for testable time access.
//!
//! All use-case code that needs the current wall-clock time MUST
//! call `ClockPort::now_unix_ms()` instead of calling
//! `std::time::SystemTime::now()` directly. This keeps every
//! time-dependent computation deterministic under tests.
//!
//! ## Implementations
//!
//! - [`crate::ports::ClockPort`] — the trait (here).
//! - `workhive_infra::SystemClock` — production implementation that
//!   delegates to `std::time::SystemTime::now()`.
//! - `workhive_testkit::fakes::FakeClock` — test double that returns
//!   a user-controlled fixed timestamp, advancing only when the test
//!   explicitly moves time forward.

/// A time source that returns the current wall-clock time as Unix
/// milliseconds.
///
/// Implementations must be cheap to call (no blocking I/O) and must
/// return a monotonically non-decreasing value within a single
/// process run. They are not required to survive clock skew or
/// leap-second adjustments.
pub trait ClockPort {
    /// Current wall-clock time in Unix milliseconds (ms since
    /// the Unix epoch 1970-01-01T00:00:00Z). Always ≥ 0.
    fn now_unix_ms(&self) -> i64;
}
