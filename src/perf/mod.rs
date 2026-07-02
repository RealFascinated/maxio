//! Request-phase timing logs. Enable with `RUST_LOG=maxio::perf=trace` (all phases)
//! or `RUST_LOG=maxio::perf=warn` (slow phases only, threshold 5ms).

use std::time::{Duration, Instant};

pub const TARGET: &str = "maxio::perf";

const SLOW_THRESHOLD: Duration = Duration::from_millis(5);

#[inline]
pub fn log(phase: &str, elapsed: Duration, detail: &str) {
    let us = elapsed.as_micros();
    if elapsed >= SLOW_THRESHOLD {
        tracing::warn!(target: TARGET, phase, elapsed_us = us, detail);
    } else {
        tracing::trace!(target: TARGET, phase, elapsed_us = us, detail);
    }
}

#[inline]
pub fn start() -> Instant {
    Instant::now()
}

#[inline]
pub fn done(phase: &str, started: Instant) {
    log(phase, started.elapsed(), "");
}

#[inline]
pub fn done_detail(phase: &str, started: Instant, detail: &str) {
    log(phase, started.elapsed(), detail);
}
