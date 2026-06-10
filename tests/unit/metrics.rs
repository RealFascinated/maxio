use std::time::Duration;

use maxio::cache::MetricsLruCache;
use maxio::metrics::rolling::{RollingBytesWindow, RollingCountWindow, RollingLatencyWindow};

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

#[test]
fn metrics_lru_evicts_when_at_capacity() {
    let cache = MetricsLruCache::<&str, &str>::new(None, "test", 2);
    cache.insert("first", "1");
    cache.insert("second", "2");
    cache.get(&"first");
    cache.insert("third", "3");

    assert_eq!(cache.get(&"first"), Some("1"));
    assert_eq!(cache.get(&"second"), None);
    assert_eq!(cache.get(&"third"), Some("3"));
}

#[test]
fn metrics_lru_update_does_not_count_as_eviction() {
    let cache = MetricsLruCache::<&str, &str>::new(None, "test", 2);
    cache.insert("key", "v1");
    cache.insert("key", "v2");
    assert_eq!(cache.get(&"key"), Some("v2"));
}
