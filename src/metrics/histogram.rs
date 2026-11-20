use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

pub const MIN_BOUND_SECONDS: f64 = 0.0005;
pub const MAX_BOUND_SECONDS: f64 = 60.0;
pub const BOUND_COUNT: usize = 24;
/// One slot per bound plus a trailing overflow slot for values above the last bound.
pub const SLOT_COUNT: usize = BOUND_COUNT + 1;

/// Log-spaced upper bounds (inclusive) from 0.5ms to 60s.
static BOUNDS: LazyLock<[f64; BOUND_COUNT]> = LazyLock::new(|| {
    let ratio = (MAX_BOUND_SECONDS / MIN_BOUND_SECONDS).powf(1.0 / (BOUND_COUNT - 1) as f64);
    std::array::from_fn(|i| {
        if i == BOUND_COUNT - 1 {
            MAX_BOUND_SECONDS
        } else {
            MIN_BOUND_SECONDS * ratio.powi(i as i32)
        }
    })
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

/// Cumulative latency histogram updated with atomics only (safe on request hot paths).
pub struct LatencyHistogram {
    slots: [AtomicU64; SLOT_COUNT],
}

impl LatencyHistogram {
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub fn record(&self, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        let slot = BOUNDS.partition_point(|bound| *bound < secs);
        self.slots[slot].fetch_add(1, Ordering::Relaxed);
    }

    pub fn counts(&self) -> HistogramCounts {
        HistogramCounts(std::array::from_fn(|i| {
            self.slots[i].load(Ordering::Relaxed)
        }))
    }
}

/// Plain copy of histogram slot counts; supports deltas, merging and percentile extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistogramCounts(pub [u64; SLOT_COUNT]);

impl HistogramCounts {
    pub fn total(&self) -> u64 {
        self.0.iter().sum()
    }

    /// Counts recorded since `earlier` (a previous reading of the same cumulative histogram).
    pub fn delta_since(&self, earlier: &Self) -> Self {
        Self(std::array::from_fn(|i| {
            self.0[i].saturating_sub(earlier.0[i])
        }))
    }

    pub fn merge(&mut self, other: &Self) {
        for (slot, extra) in self.0.iter_mut().zip(other.0.iter()) {
            *slot += extra;
        }
    }

    /// Quantile in seconds with linear interpolation inside the containing bucket.
    /// `None` when no samples were recorded. Values above the last bound clamp to it.
    pub fn quantile(&self, q: f64) -> Option<f64> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let rank = q.clamp(0.0, 1.0) * total as f64;
        let mut cumulative = 0.0;
        for (slot, count) in self.0.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let count = *count as f64;
            if cumulative + count >= rank {
                let lower = if slot == 0 { 0.0 } else { BOUNDS[slot - 1] };
                let upper = if slot < BOUND_COUNT {
                    BOUNDS[slot]
                } else {
                    MAX_BOUND_SECONDS
                };
                let fraction = ((rank - cumulative) / count).clamp(0.0, 1.0);
                return Some(lower + fraction * (upper - lower));
            }
            cumulative += count;
        }
        Some(MAX_BOUND_SECONDS)
    }

    pub fn percentiles(&self) -> Option<Percentiles> {
        Some(Percentiles {
            p50: self.quantile(0.50)?,
            p95: self.quantile(0.95)?,
            p99: self.quantile(0.99)?,
        })
    }
}
