use std::time::{Duration, Instant};

use prometheus::{
    CounterVec, Encoder, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};

use crate::stats::BucketStatsCache;

pub struct MetricsRegistry {
    registry: Registry,
    http_requests_total: CounterVec,
    http_duration: HistogramVec,
    storage_duration: HistogramVec,
    uptime: prometheus::Gauge,
    start_time: Instant,
}

impl MetricsRegistry {
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        #[cfg(target_os = "linux")]
        {
            let pc = prometheus::process_collector::ProcessCollector::for_self();
            let _ = registry.register(Box::new(pc));
        }

        let http_requests_total = CounterVec::new(
            Opts::new("maxio_http_requests_total", "Total HTTP requests"),
            &["method", "route", "status"],
        )?;
        registry.register(Box::new(http_requests_total.clone()))?;

        let http_duration = HistogramVec::new(
            HistogramOpts::new(
                "maxio_http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.025, 0.1, 0.5, 1.0, 5.0, 10.0]),
            &["method", "route"],
        )?;
        registry.register(Box::new(http_duration.clone()))?;

        let storage_duration = HistogramVec::new(
            HistogramOpts::new(
                "maxio_storage_operation_duration_seconds",
                "Storage operation duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.025, 0.1, 0.5, 1.0, 5.0, 30.0]),
            &["operation"],
        )?;
        registry.register(Box::new(storage_duration.clone()))?;

        let uptime =
            prometheus::Gauge::new("maxio_uptime_seconds", "Server uptime in seconds")?;
        registry.register(Box::new(uptime.clone()))?;

        Ok(Self {
            registry,
            http_requests_total,
            http_duration,
            storage_duration,
            uptime,
            start_time: Instant::now(),
        })
    }

    pub fn record_http(&self, method: &str, route: &str, status: &str, elapsed: Duration) {
        self.http_requests_total
            .with_label_values(&[method, route, status])
            .inc();
        self.http_duration
            .with_label_values(&[method, route])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_storage_op(&self, operation: &str, elapsed: Duration) {
        self.storage_duration
            .with_label_values(&[operation])
            .observe(elapsed.as_secs_f64());
    }

    pub fn update_uptime(&self) {
        self.uptime.set(self.start_time.elapsed().as_secs_f64());
    }

    /// Encode all metrics to Prometheus text format. Bucket stats are read
    /// from the cache and written as transient gauges at scrape time so that
    /// deleted buckets never leave stale label sets in the output.
    pub fn gather_text(&self, stats: &BucketStatsCache) -> String {
        let encoder = TextEncoder::new();
        let mut all_families = self.registry.gather();

        let bucket_data = stats.get_all();
        if !bucket_data.is_empty() {
            let tmp = Registry::new();
            let obj_gauge = GaugeVec::new(
                Opts::new(
                    "maxio_bucket_objects_total",
                    "Number of objects per bucket",
                ),
                &["bucket"],
            )
            .unwrap();
            let size_gauge = GaugeVec::new(
                Opts::new(
                    "maxio_bucket_size_bytes",
                    "Total size in bytes per bucket",
                ),
                &["bucket"],
            )
            .unwrap();
            let _ = tmp.register(Box::new(obj_gauge.clone()));
            let _ = tmp.register(Box::new(size_gauge.clone()));

            for stat in &bucket_data {
                obj_gauge
                    .with_label_values(&[&stat.name])
                    .set(stat.object_count as f64);
                size_gauge
                    .with_label_values(&[&stat.name])
                    .set(stat.size_bytes as f64);
            }
            all_families.extend(tmp.gather());
        }

        let mut buf = Vec::new();
        let _ = encoder.encode(&all_families, &mut buf);
        String::from_utf8(buf).unwrap_or_default()
    }
}
