//! Fake clock for deterministic time in unit and integration tests.
//!
//! [`FakeClock`] is the canonical test double for
//! [`workhive_core::ports::ClockPort`]. Tests construct a `FakeClock`
//! at a known timestamp and advance it explicitly — this guarantees
//! that all time-dependent computations (elapsed, freshness, metric
//! evaluation) produce reproducible results without any wall-clock
//! dependency.
//!
//! ## Usage
//!
//! ```rust
//! use workhive_testkit::fakes::FakeClock;
//! use workhive_core::ports::ClockPort;
//!
//! let clock = FakeClock::new(1_700_000_000_000);
//! assert_eq!(clock.now_unix_ms(), 1_700_000_000_000);
//!
//! clock.advance_ms(5_000);
//! assert_eq!(clock.now_unix_ms(), 1_700_000_005_000);
//! ```

use std::cell::Cell;

use workhive_core::ports::ClockPort;

/// A deterministic, single-threaded fake clock.
///
/// The internal counter is stored in a [`Cell`] so that
/// `&FakeClock` (shared reference) can still advance time — matching
/// how tests typically hold fakes via a shared reference across
/// multiple calls.
pub struct FakeClock {
    now_ms: Cell<i64>,
}

impl FakeClock {
    /// Create a fake clock initialised to `start_unix_ms`.
    ///
    /// Panics if `start_unix_ms < 0` to prevent accidentally
    /// constructing a clock at an impossible time.
    pub fn new(start_unix_ms: i64) -> Self {
        assert!(
            start_unix_ms >= 0,
            "FakeClock: start_unix_ms must be non-negative"
        );
        Self {
            now_ms: Cell::new(start_unix_ms),
        }
    }

    /// Advance the clock by `delta_ms` milliseconds.
    ///
    /// `delta_ms` may be 0 (no-op) but must not be negative — tests
    /// that need to represent clock-skew must do so explicitly through
    /// a separate `FakeClock` instance.
    pub fn advance_ms(&self, delta_ms: i64) {
        assert!(
            delta_ms >= 0,
            "FakeClock: delta_ms must be non-negative; got {delta_ms}"
        );
        self.now_ms.set(self.now_ms.get().saturating_add(delta_ms));
    }

    /// Set the clock to an absolute value. Useful for test setup
    /// that wants to pin a known reference point mid-test.
    ///
    /// Panics if `unix_ms < 0`.
    pub fn set_ms(&self, unix_ms: i64) {
        assert!(
            unix_ms >= 0,
            "FakeClock: set_ms value must be non-negative; got {unix_ms}"
        );
        self.now_ms.set(unix_ms);
    }
}

impl ClockPort for FakeClock {
    fn now_unix_ms(&self) -> i64 {
        self.now_ms.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_drives_metric_evaluation_deterministically() {
        let clock = FakeClock::new(1_700_000_000_000);
        assert_eq!(clock.now_unix_ms(), 1_700_000_000_000);

        clock.advance_ms(5_000);
        assert_eq!(clock.now_unix_ms(), 1_700_000_005_000);

        clock.advance_ms(0);
        assert_eq!(
            clock.now_unix_ms(),
            1_700_000_005_000,
            "zero advance must not change the clock"
        );
    }

    #[test]
    fn fake_clock_set_ms_overrides_current_value() {
        let clock = FakeClock::new(1_000);
        clock.set_ms(9_000);
        assert_eq!(clock.now_unix_ms(), 9_000);
    }

    #[test]
    fn fake_clock_advance_saturates_at_i64_max() {
        let clock = FakeClock::new(i64::MAX);
        clock.advance_ms(1);
        assert_eq!(clock.now_unix_ms(), i64::MAX, "must not overflow");
    }
}
