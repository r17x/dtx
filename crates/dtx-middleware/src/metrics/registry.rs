//! Central registry for all metrics.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::types::{Counter, Gauge, Histogram, Labels};

/// Central registry for all metrics.
///
/// Thread-safe registry that manages counters, gauges, and histograms.
#[derive(Default)]
pub struct MetricsRegistry {
    counters: RwLock<HashMap<(String, Labels), Arc<Counter>>>,
    gauges: RwLock<HashMap<(String, Labels), Arc<Gauge>>>,
    histograms: RwLock<HashMap<(String, Labels), Arc<Histogram>>>,
}

impl MetricsRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a counter with the given name and labels.
    pub fn counter(&self, name: &str, labels: Labels) -> Arc<Counter> {
        let key = (name.to_string(), labels);
        let mut counters = self.counters.write();
        counters
            .entry(key)
            .or_insert_with(|| Arc::new(Counter::default()))
            .clone()
    }

    /// Get or create a gauge with the given name and labels.
    pub fn gauge(&self, name: &str, labels: Labels) -> Arc<Gauge> {
        let key = (name.to_string(), labels);
        let mut gauges = self.gauges.write();
        gauges
            .entry(key)
            .or_insert_with(|| Arc::new(Gauge::default()))
            .clone()
    }

    /// Get or create a histogram with the given name and labels.
    pub fn histogram(&self, name: &str, labels: Labels) -> Arc<Histogram> {
        let key = (name.to_string(), labels);
        let mut histograms = self.histograms.write();
        histograms
            .entry(key)
            .or_insert_with(|| Arc::new(Histogram::default_buckets()))
            .clone()
    }

    /// Export all metrics in Prometheus text format.
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();

        // Export counters
        let counters = self.counters.read();
        for ((name, labels), counter) in counters.iter() {
            output.push_str(&format!(
                "{}{} {}\n",
                name,
                labels.to_prometheus(),
                counter.get()
            ));
        }

        // Export gauges
        let gauges = self.gauges.read();
        for ((name, labels), gauge) in gauges.iter() {
            output.push_str(&format!(
                "{}{} {}\n",
                name,
                labels.to_prometheus(),
                gauge.get()
            ));
        }

        // Export histograms
        let histograms = self.histograms.read();
        for ((name, labels), histogram) in histograms.iter() {
            // Bucket counts
            for (bucket, count) in histogram.get_buckets() {
                let bucket_labels = labels.clone().add("le", format!("{}", bucket));
                output.push_str(&format!(
                    "{}_bucket{} {}\n",
                    name,
                    bucket_labels.to_prometheus(),
                    count
                ));
            }
            // +Inf bucket
            let inf_labels = labels.clone().add("le", "+Inf");
            output.push_str(&format!(
                "{}_bucket{} {}\n",
                name,
                inf_labels.to_prometheus(),
                histogram.get_count()
            ));
            // Count and sum
            output.push_str(&format!(
                "{}_count{} {}\n",
                name,
                labels.to_prometheus(),
                histogram.get_count()
            ));
            output.push_str(&format!(
                "{}_sum{} {}\n",
                name,
                labels.to_prometheus(),
                histogram.get_sum()
            ));
        }

        output
    }

    /// Clear all metrics.
    pub fn clear(&self) {
        self.counters.write().clear();
        self.gauges.write().clear();
        self.histograms.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_counter() {
        let registry = MetricsRegistry::new();

        let counter = registry.counter("requests", Labels::new().add("method", "GET"));
        counter.inc();

        let same_counter = registry.counter("requests", Labels::new().add("method", "GET"));
        assert_eq!(same_counter.get(), 1);

        let diff_counter = registry.counter("requests", Labels::new().add("method", "POST"));
        assert_eq!(diff_counter.get(), 0);
    }

    #[test]
    fn registry_gauge() {
        let registry = MetricsRegistry::new();

        let gauge = registry.gauge("connections", Labels::new());
        gauge.set(10);

        let same_gauge = registry.gauge("connections", Labels::new());
        assert_eq!(same_gauge.get(), 10);
    }

    #[test]
    fn registry_histogram() {
        let registry = MetricsRegistry::new();

        let histogram = registry.histogram("latency", Labels::new());
        histogram.observe(0.1);
        histogram.observe(0.2);

        let same_histogram = registry.histogram("latency", Labels::new());
        assert_eq!(same_histogram.get_count(), 2);
    }

    #[test]
    fn registry_export_prometheus() {
        let registry = MetricsRegistry::new();

        registry
            .counter("requests", Labels::new().add("method", "GET"))
            .inc();
        registry.gauge("connections", Labels::new()).set(5);
        registry
            .histogram("latency", Labels::new().add("endpoint", "/api"))
            .observe(0.1);

        let output = registry.export_prometheus();

        assert!(output.contains("requests{method=\"GET\"} 1"));
        assert!(output.contains("connections 5"));
        assert!(output.contains("latency_count{endpoint=\"/api\"} 1"));
    }

    #[test]
    fn registry_clear() {
        let registry = MetricsRegistry::new();

        registry.counter("test", Labels::new()).inc();
        assert!(!registry.export_prometheus().is_empty());

        registry.clear();
        assert!(registry.export_prometheus().is_empty());
    }
}
