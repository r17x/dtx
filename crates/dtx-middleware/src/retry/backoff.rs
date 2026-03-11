//! Backoff strategies for retry operations.

use std::time::Duration;

/// Backoff strategy for calculating delay between retries.
pub trait BackoffStrategy: Send + Sync {
    /// Calculate delay before the next retry.
    ///
    /// `attempt` is 0-indexed (first retry is attempt 0).
    fn delay(&self, attempt: u32) -> Duration;

    /// Clone into a boxed trait object.
    fn clone_box(&self) -> Box<dyn BackoffStrategy>;
}

/// Fixed delay between retries.
#[derive(Clone)]
pub struct FixedBackoff {
    delay: Duration,
}

impl FixedBackoff {
    /// Create a new fixed backoff with the given delay.
    pub fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl BackoffStrategy for FixedBackoff {
    fn delay(&self, _attempt: u32) -> Duration {
        self.delay
    }

    fn clone_box(&self) -> Box<dyn BackoffStrategy> {
        Box::new(self.clone())
    }
}

/// Exponential backoff with optional jitter.
///
/// Delay = min(initial * multiplier^attempt, max) ± jitter
#[derive(Clone)]
pub struct ExponentialBackoff {
    initial: Duration,
    max: Duration,
    multiplier: f64,
    jitter: bool,
}

impl ExponentialBackoff {
    /// Create a new exponential backoff.
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max,
            multiplier: 2.0,
            jitter: true,
        }
    }

    /// Set the multiplier (default: 2.0).
    pub fn multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Enable or disable jitter (default: enabled).
    pub fn jitter(mut self, enabled: bool) -> Self {
        self.jitter = enabled;
        self
    }
}

impl BackoffStrategy for ExponentialBackoff {
    fn delay(&self, attempt: u32) -> Duration {
        let base = self.initial.as_secs_f64() * self.multiplier.powi(attempt as i32);
        let capped = base.min(self.max.as_secs_f64());

        let final_delay = if self.jitter {
            // Add random jitter ±25%
            let jitter_factor = (rand::random::<f64>() - 0.5) * 0.5;
            (capped * (1.0 + jitter_factor)).max(0.0)
        } else {
            capped
        };

        Duration::from_secs_f64(final_delay)
    }

    fn clone_box(&self) -> Box<dyn BackoffStrategy> {
        Box::new(self.clone())
    }
}

/// Linear backoff.
///
/// Delay = min(initial + increment * attempt, max)
#[derive(Clone)]
pub struct LinearBackoff {
    initial: Duration,
    increment: Duration,
    max: Duration,
}

impl LinearBackoff {
    /// Create a new linear backoff.
    pub fn new(initial: Duration, increment: Duration, max: Duration) -> Self {
        Self {
            initial,
            increment,
            max,
        }
    }
}

impl BackoffStrategy for LinearBackoff {
    fn delay(&self, attempt: u32) -> Duration {
        let delay = self.initial + self.increment * attempt;
        delay.min(self.max)
    }

    fn clone_box(&self) -> Box<dyn BackoffStrategy> {
        Box::new(self.clone())
    }
}

/// No delay between retries.
#[derive(Clone, Default)]
pub struct NoBackoff;

impl BackoffStrategy for NoBackoff {
    fn delay(&self, _attempt: u32) -> Duration {
        Duration::ZERO
    }

    fn clone_box(&self) -> Box<dyn BackoffStrategy> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_backoff() {
        let backoff = FixedBackoff::new(Duration::from_millis(100));

        assert_eq!(backoff.delay(0), Duration::from_millis(100));
        assert_eq!(backoff.delay(1), Duration::from_millis(100));
        assert_eq!(backoff.delay(10), Duration::from_millis(100));
    }

    #[test]
    fn exponential_backoff_no_jitter() {
        let backoff = ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10))
            .jitter(false);

        assert_eq!(backoff.delay(0), Duration::from_millis(100));
        assert_eq!(backoff.delay(1), Duration::from_millis(200));
        assert_eq!(backoff.delay(2), Duration::from_millis(400));
        assert_eq!(backoff.delay(3), Duration::from_millis(800));
    }

    #[test]
    fn exponential_backoff_caps() {
        let backoff =
            ExponentialBackoff::new(Duration::from_secs(1), Duration::from_secs(5)).jitter(false);

        // 1 * 2^10 = 1024, but capped at 5
        assert_eq!(backoff.delay(10), Duration::from_secs(5));
    }

    #[test]
    fn exponential_backoff_custom_multiplier() {
        let backoff = ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10))
            .multiplier(3.0)
            .jitter(false);

        assert_eq!(backoff.delay(0), Duration::from_millis(100));
        assert_eq!(backoff.delay(1), Duration::from_millis(300));
        assert_eq!(backoff.delay(2), Duration::from_millis(900));
    }

    #[test]
    fn exponential_backoff_with_jitter() {
        let backoff = ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10))
            .jitter(true);

        // With jitter, delay should be within ±25% of base
        let delay = backoff.delay(0);
        let base_ms = 100.0;
        let delay_ms = delay.as_millis() as f64;
        assert!(
            delay_ms >= base_ms * 0.75 && delay_ms <= base_ms * 1.25,
            "Delay {} not within jitter range",
            delay_ms
        );
    }

    #[test]
    fn linear_backoff() {
        let backoff = LinearBackoff::new(
            Duration::from_millis(100),
            Duration::from_millis(50),
            Duration::from_millis(500),
        );

        assert_eq!(backoff.delay(0), Duration::from_millis(100));
        assert_eq!(backoff.delay(1), Duration::from_millis(150));
        assert_eq!(backoff.delay(2), Duration::from_millis(200));
        assert_eq!(backoff.delay(10), Duration::from_millis(500)); // capped
    }

    #[test]
    fn no_backoff() {
        let backoff = NoBackoff;

        assert_eq!(backoff.delay(0), Duration::ZERO);
        assert_eq!(backoff.delay(100), Duration::ZERO);
    }

    #[test]
    fn clone_box() {
        let backoff: Box<dyn BackoffStrategy> =
            Box::new(FixedBackoff::new(Duration::from_millis(100)));
        let cloned = backoff.clone_box();

        assert_eq!(cloned.delay(0), Duration::from_millis(100));
    }
}
