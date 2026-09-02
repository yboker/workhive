//! Production clock adapter backed by [`std::time::SystemTime`].
//!
//! `SystemClock` implements [`workhive_core::ports::ClockPort`] using
//! the OS wall clock. All production code that needs "now" should
//! receive a `&dyn ClockPort` through dependency injection rather
//! than calling `SystemTime::now()` directly, so tests can substitute
//! a `FakeClock` from `workhive_testkit`.

use std::time::SystemTime;

use workhive_core::ports::ClockPort;

/// Production wall-clock implementation.
///
/// Returns `0` if the system clock reports a time before the Unix
/// epoch (should never happen in practice; guarded against `duration_since`
/// returning `Err`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now_unix_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_returns_positive_unix_ms() {
        let ms = SystemClock.now_unix_ms();
        // Must be after 2020-01-01 (1_577_836_800_000 ms since epoch).
        assert!(
            ms > 1_577_836_800_000,
            "SystemClock returned implausible value: {ms}"
        );
    }
}
