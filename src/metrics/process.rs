use std::sync::Mutex;
use std::time::Instant;

use super::snapshot::ProcessSnapshot;

pub struct RawProcessMetrics {
    pub resident_memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub cpu_seconds_total: f64,
    pub open_fds: u64,
    pub max_fds: u64,
}

pub struct ProcessCpuTracker {
    last_sample: Mutex<Option<(f64, Instant)>>,
}

impl ProcessCpuTracker {
    pub fn new() -> Self {
        Self {
            last_sample: Mutex::new(None),
        }
    }

    pub fn usage_ratio(&self, cpu_seconds_total: f64, uptime_seconds: f64) -> f64 {
        let now = Instant::now();
        let mut last = self.last_sample.lock().unwrap();

        let raw_ratio = match *last {
            Some((prev_cpu, prev_at)) => {
                let elapsed = prev_at.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    (cpu_seconds_total - prev_cpu) / elapsed
                } else if uptime_seconds > 0.0 {
                    cpu_seconds_total / uptime_seconds
                } else {
                    0.0
                }
            }
            None if uptime_seconds > 0.0 => cpu_seconds_total / uptime_seconds,
            None => 0.0,
        };

        *last = Some((cpu_seconds_total, now));
        (raw_ratio / cpu_cores()).max(0.0)
    }

    pub fn snapshot(cpu_usage_percent: f64) -> Option<ProcessSnapshot> {
        read_raw_process_metrics().map(|raw| ProcessSnapshot {
            resident_memory_bytes: raw.resident_memory_bytes,
            virtual_memory_bytes: raw.virtual_memory_bytes,
            cpu_usage_percent,
            open_fds: raw.open_fds,
            max_fds: raw.max_fds,
        })
    }
}

pub fn read_raw_process_metrics() -> Option<RawProcessMetrics> {
    #[cfg(target_os = "linux")]
    {
        read_linux_process_metrics()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn cpu_cores() -> f64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0)
}

#[cfg(target_os = "linux")]
fn read_linux_process_metrics() -> Option<RawProcessMetrics> {
    const CLOCK_TICKS_PER_SEC: f64 = 100.0;

    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let resident_kb = parse_proc_status_kb(&status, "VmRSS:")?;
    let virtual_kb = parse_proc_status_kb(&status, "VmSize:")?;

    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let (utime, stime) = parse_proc_stat_cpu(&stat)?;
    let cpu_seconds_total = (utime + stime) as f64 / CLOCK_TICKS_PER_SEC;

    let open_fds = std::fs::read_dir("/proc/self/fd").ok()?.count() as u64;
    let max_fds = parse_proc_max_open_files().unwrap_or(0);

    Some(RawProcessMetrics {
        resident_memory_bytes: resident_kb * 1024,
        virtual_memory_bytes: virtual_kb * 1024,
        cpu_seconds_total,
        open_fds,
        max_fds,
    })
}

#[cfg(target_os = "linux")]
fn parse_proc_status_kb(status: &str, key: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let (label, value) = line.split_once(':')?;
        if label.trim() != key.trim_end_matches(':') {
            return None;
        }
        let kb = value.split_whitespace().next()?.parse().ok()?;
        Some(kb)
    })
}

#[cfg(target_os = "linux")]
fn parse_proc_stat_cpu(stat: &str) -> Option<(u64, u64)> {
    let close_paren = stat.rfind(')')?;
    let rest = stat[close_paren + 1..].trim_start();
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let utime = fields.get(11)?.parse().ok()?;
    let stime = fields.get(12)?.parse().ok()?;
    Some((utime, stime))
}

#[cfg(target_os = "linux")]
fn parse_proc_max_open_files() -> Option<u64> {
    let limits = std::fs::read_to_string("/proc/self/limits").ok()?;
    for line in limits.lines() {
        if line.starts_with("Max open files") {
            let soft = line.split_whitespace().nth(3)?;
            return soft.parse().ok();
        }
    }
    None
}
