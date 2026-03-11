//! Metric types for observability.

use std::sync::atomic::{AtomicU64, Ordering};

/// A counter metric that can only increase.
#[derive(Default)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Create a new counter with value 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment by 1.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment by n.
    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Get the current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// A gauge metric that can increase or decrease.
#[derive(Default)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    /// Create a new gauge with value 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the value.
    pub fn set(&self, value: u64) {
        self.value.store(value, Ordering::Relaxed);
    }

    /// Increment by 1.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement by 1.
    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get the current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// A histogram metric with buckets.
pub struct Histogram {
    buckets: Vec<f64>,
    counts: Vec<AtomicU64>,
    sum: AtomicU64,
    count: AtomicU64,
}

impl Histogram {
    /// Create a histogram with the given buckets.
    pub fn new(buckets: Vec<f64>) -> Self {
        let counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            buckets,
            counts,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Create a histogram with default buckets suitable for latency.
    pub fn default_buckets() -> Self {
        Self::new(vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ])
    }

    /// Observe a value.
    pub fn observe(&self, value: f64) {
        for (i, bucket) in self.buckets.iter().enumerate() {
            if value <= *bucket {
                self.counts[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        // Store sum as microseconds to avoid floating point atomics
        self.sum
            .fetch_add((value * 1_000_000.0) as u64, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the total count of observations.
    pub fn get_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get the sum of all observations.
    pub fn get_sum(&self) -> f64 {
        self.sum.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// Get bucket counts.
    pub fn get_buckets(&self) -> Vec<(f64, u64)> {
        self.buckets
            .iter()
            .zip(self.counts.iter())
            .map(|(b, c)| (*b, c.load(Ordering::Relaxed)))
            .collect()
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::default_buckets()
    }
}

/// Label set for metrics.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Labels {
    labels: Vec<(String, String)>,
}

impl Labels {
    /// Create an empty label set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a label.
    pub fn add(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push((key.into(), value.into()));
        self
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Format labels for Prometheus.
    pub fn to_prometheus(&self) -> String {
        if self.labels.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = self
            .labels
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, v.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();
        format!("{{{}}}", parts.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_ops() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.inc_by(5);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn gauge_ops() {
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0);

        gauge.inc();
        assert_eq!(gauge.get(), 1);

        gauge.dec();
        assert_eq!(gauge.get(), 0);

        gauge.set(42);
        assert_eq!(gauge.get(), 42);
    }

    #[test]
    fn histogram_ops() {
        let histogram = Histogram::new(vec![0.1, 0.5, 1.0]);

        histogram.observe(0.05);
        histogram.observe(0.3);
        histogram.observe(0.8);

        assert_eq!(histogram.get_count(), 3);
        assert!((histogram.get_sum() - 1.15).abs() < 0.001);

        let buckets = histogram.get_buckets();
        assert_eq!(buckets[0], (0.1, 1)); // 0.05 <= 0.1
        assert_eq!(buckets[1], (0.5, 2)); // 0.05, 0.3 <= 0.5
        assert_eq!(buckets[2], (1.0, 3)); // all <= 1.0
    }

    #[test]
    fn labels_prometheus_format() {
        let labels = Labels::new()
            .add("operation", "start")
            .add("success", "true");

        assert_eq!(
            labels.to_prometheus(),
            "{operation=\"start\",success=\"true\"}"
        );
    }

    #[test]
    fn labels_empty() {
        let labels = Labels::new();
        assert!(labels.is_empty());
        assert_eq!(labels.to_prometheus(), "");
    }

    #[test]
    fn labels_escaping() {
        let labels = Labels::new().add("path", "/api/v1");
        assert_eq!(labels.to_prometheus(), "{path=\"/api/v1\"}");

        let labels = Labels::new().add("msg", "hello \"world\"");
        assert_eq!(labels.to_prometheus(), "{msg=\"hello \\\"world\\\"\"}");
    }
}
