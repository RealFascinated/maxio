use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RollingLatencyWindow {
    window: Duration,
    samples: Mutex<VecDeque<(Instant, f64)>>,
}

impl RollingLatencyWindow {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            samples: Mutex::new(VecDeque::new()),
        }
    }

    pub fn record(&self, elapsed: Duration) {
        let now = Instant::now();
        let mut samples = self.samples.lock().unwrap();
        samples.push_back((now, elapsed.as_secs_f64()));
        Self::prune(&self.window, &mut samples, now);
    }

    pub fn average_seconds(&self) -> Option<f64> {
        let mut samples = self.samples.lock().unwrap();
        let now = Instant::now();
        Self::prune(&self.window, &mut samples, now);
        if samples.is_empty() {
            return None;
        }
        let sum: f64 = samples.iter().map(|(_, secs)| secs).sum();
        Some(sum / samples.len() as f64)
    }

    fn prune(window: &Duration, samples: &mut VecDeque<(Instant, f64)>, now: Instant) {
        let cutoff = now.checked_sub(*window).unwrap_or(now);
        while samples.front().is_some_and(|(t, _)| *t < cutoff) {
            samples.pop_front();
        }
    }
}

pub struct RollingBytesWindow {
    window: Duration,
    samples: Mutex<VecDeque<(Instant, u64)>>,
}

impl RollingBytesWindow {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            samples: Mutex::new(VecDeque::new()),
        }
    }

    pub fn record(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let now = Instant::now();
        let mut samples = self.samples.lock().unwrap();
        samples.push_back((now, bytes));
        Self::prune(&self.window, &mut samples, now);
    }

    pub fn bytes_per_sec(&self) -> f64 {
        let mut samples = self.samples.lock().unwrap();
        let now = Instant::now();
        Self::prune(&self.window, &mut samples, now);
        let total_bytes: u64 = samples.iter().map(|(_, bytes)| bytes).sum();
        let window_secs = self.window.as_secs_f64();
        if window_secs <= 0.0 {
            return 0.0;
        }
        total_bytes as f64 / window_secs
    }

    fn prune(window: &Duration, samples: &mut VecDeque<(Instant, u64)>, now: Instant) {
        let cutoff = now.checked_sub(*window).unwrap_or(now);
        while samples.front().is_some_and(|(t, _)| *t < cutoff) {
            samples.pop_front();
        }
    }
}

pub struct RollingCountWindow {
    window: Duration,
    events: Mutex<VecDeque<Instant>>,
}

impl RollingCountWindow {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            events: Mutex::new(VecDeque::new()),
        }
    }

    pub fn record(&self) {
        let now = Instant::now();
        let mut events = self.events.lock().unwrap();
        events.push_back(now);
        Self::prune(&self.window, &mut events, now);
    }

    pub fn ops_per_sec(&self) -> f64 {
        let mut events = self.events.lock().unwrap();
        let now = Instant::now();
        Self::prune(&self.window, &mut events, now);
        let window_secs = self.window.as_secs_f64();
        if window_secs <= 0.0 {
            return 0.0;
        }
        events.len() as f64 / window_secs
    }

    fn prune(window: &Duration, events: &mut VecDeque<Instant>, now: Instant) {
        let cutoff = now.checked_sub(*window).unwrap_or(now);
        while events.front().is_some_and(|t| *t < cutoff) {
            events.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_latency_averages_recent_samples() {
        let window = RollingLatencyWindow::new(Duration::from_secs(15));
        window.record(Duration::from_millis(100));
        window.record(Duration::from_millis(200));
        let avg = window.average_seconds().unwrap();
        assert!((avg - 0.15).abs() < 0.001);
    }

    #[test]
    fn rolling_bytes_window_computes_rate() {
        let window = RollingBytesWindow::new(Duration::from_secs(10));
        window.record(1_000);
        window.record(2_000);
        let rate = window.bytes_per_sec();
        assert!((rate - 300.0).abs() < 0.001);
    }

    #[test]
    fn rolling_count_window_computes_ops_per_sec() {
        let window = RollingCountWindow::new(Duration::from_secs(10));
        window.record();
        window.record();
        window.record();
        let rate = window.ops_per_sec();
        assert!((rate - 0.3).abs() < 0.001);
    }
}
