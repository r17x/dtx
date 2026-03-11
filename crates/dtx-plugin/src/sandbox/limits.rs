//! Resource limits for sandboxed plugins.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Resource limits for sandboxed plugins.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory in bytes.
    pub max_memory: usize,

    /// Maximum CPU fuel (abstract units).
    pub max_fuel: u64,

    /// Maximum execution time per call.
    pub max_time: Duration,

    /// Maximum number of open file handles.
    pub max_files: usize,

    /// Maximum number of network connections.
    pub max_connections: usize,

    /// Maximum size of single allocation.
    pub max_allocation: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024,      // 64MB
            max_fuel: 1_000_000_000,           // ~1B instructions
            max_time: Duration::from_secs(30), // 30 seconds
            max_files: 100,
            max_connections: 10,
            max_allocation: 16 * 1024 * 1024, // 16MB single alloc
        }
    }
}

impl ResourceLimits {
    /// Create restrictive limits for untrusted code.
    pub fn restrictive() -> Self {
        Self {
            max_memory: 16 * 1024 * 1024,     // 16MB
            max_fuel: 100_000_000,            // 100M instructions
            max_time: Duration::from_secs(5), // 5 seconds
            max_files: 10,
            max_connections: 2,
            max_allocation: 1024 * 1024, // 1MB single alloc
        }
    }

    /// Create generous limits for trusted code.
    pub fn generous() -> Self {
        Self {
            max_memory: 512 * 1024 * 1024,      // 512MB
            max_fuel: 10_000_000_000,           // 10B instructions
            max_time: Duration::from_secs(300), // 5 minutes
            max_files: 1000,
            max_connections: 100,
            max_allocation: 64 * 1024 * 1024, // 64MB single alloc
        }
    }
}

/// Resource usage tracking.
#[derive(Clone, Debug, Default)]
pub struct ResourceUsage {
    /// Current memory usage.
    pub memory_current: usize,
    /// Peak memory usage.
    pub memory_peak: usize,
    /// Fuel consumed.
    pub fuel_consumed: u64,
    /// Time elapsed.
    pub time_elapsed: Duration,
    /// Open file handles.
    pub files_open: usize,
    /// Open network connections.
    pub connections_open: usize,
}

impl ResourceUsage {
    /// Check if memory limit exceeded.
    pub fn memory_exceeded(&self, limits: &ResourceLimits) -> bool {
        self.memory_current > limits.max_memory
    }

    /// Check if time limit exceeded.
    pub fn time_exceeded(&self, limits: &ResourceLimits) -> bool {
        self.time_elapsed > limits.max_time
    }

    /// Record memory allocation.
    pub fn record_allocation(&mut self, size: usize) {
        self.memory_current += size;
        self.memory_peak = self.memory_peak.max(self.memory_current);
    }

    /// Record memory deallocation.
    pub fn record_deallocation(&mut self, size: usize) {
        self.memory_current = self.memory_current.saturating_sub(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_memory, 64 * 1024 * 1024);
        assert_eq!(limits.max_fuel, 1_000_000_000);
    }

    #[test]
    fn restrictive_limits() {
        let limits = ResourceLimits::restrictive();
        assert_eq!(limits.max_memory, 16 * 1024 * 1024);
        assert_eq!(limits.max_time, Duration::from_secs(5));
    }

    #[test]
    fn generous_limits() {
        let limits = ResourceLimits::generous();
        assert_eq!(limits.max_memory, 512 * 1024 * 1024);
        assert_eq!(limits.max_time, Duration::from_secs(300));
    }

    #[test]
    fn usage_tracking() {
        let mut usage = ResourceUsage::default();
        usage.record_allocation(1024);
        assert_eq!(usage.memory_current, 1024);
        assert_eq!(usage.memory_peak, 1024);

        usage.record_allocation(2048);
        assert_eq!(usage.memory_current, 3072);
        assert_eq!(usage.memory_peak, 3072);

        usage.record_deallocation(1024);
        assert_eq!(usage.memory_current, 2048);
        assert_eq!(usage.memory_peak, 3072);
    }
}
