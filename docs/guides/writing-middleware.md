# Writing Middleware

> Create custom middleware for dtx.

---

## Overview

Middleware wraps operations, adding behavior before and after execution.

```
Request → [Auth] → [Logging] → [Metrics] → Handler → Response
                                              ↑
                                     Your middleware
```

---

## Basic Middleware

```rust
use async_trait::async_trait;
use dtx_core::{
    Context, Result,
    middleware::{Middleware, Next, Operation, Response},
};

pub struct MyMiddleware {
    // Configuration
}

#[async_trait]
impl Middleware for MyMiddleware {
    fn name(&self) -> &'static str {
        "my-middleware"
    }

    async fn handle(
        &self,
        op: Operation,
        ctx: Context,
        next: Next<'_>,
    ) -> Result<Response> {
        // Before: runs before the operation
        println!("Before: {:?}", op);

        // Call the next middleware/handler
        let result = next.run(op, ctx).await;

        // After: runs after the operation
        println!("After: {:?}", result.is_ok());

        result
    }
}
```

---

## Modifying Operations

Transform the operation before passing it on:

```rust
async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
    // Transform operation
    let modified_op = match op {
        Operation::Start { id } => {
            // Add logging
            tracing::info!("Starting {}", id);
            Operation::Start { id }
        }
        other => other,
    };

    next.run(modified_op, ctx).await
}
```

---

## Modifying Context

Add metadata to the context:

```rust
async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
    // Add request metadata
    let ctx = ctx
        .with_metadata("middleware", self.name())
        .with_metadata("timestamp", Utc::now().to_rfc3339());

    next.run(op, ctx).await
}
```

---

## Short-Circuiting

Return early without calling next:

```rust
async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
    // Check permission
    if !self.check_permission(&op, &ctx) {
        return Err(Error::Unauthorized);
    }

    // Check cache
    if let Some(cached) = self.cache.get(&op) {
        return Ok(cached);
    }

    next.run(op, ctx).await
}
```

---

## Error Handling

Handle or transform errors:

```rust
async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
    match next.run(op.clone(), ctx).await {
        Ok(response) => Ok(response),
        Err(error) => {
            // Log error
            tracing::error!(error = %error, "Operation failed");

            // Optionally transform error
            if error.is_retryable() {
                Err(error.context("Will be retried"))
            } else {
                Err(error)
            }
        }
    }
}
```

---

## Stateful Middleware

Track state across requests:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub struct CountingMiddleware {
    requests: AtomicU64,
    errors: AtomicU64,
}

impl CountingMiddleware {
    pub fn new() -> Self {
        Self {
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (
            self.requests.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
        )
    }
}

#[async_trait]
impl Middleware for CountingMiddleware {
    fn name(&self) -> &'static str { "counting" }

    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
        self.requests.fetch_add(1, Ordering::Relaxed);

        let result = next.run(op, ctx).await;

        if result.is_err() {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }

        result
    }
}
```

---

## Configurable Middleware

Make middleware configurable:

```rust
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst: u32,
}

pub struct RateLimitMiddleware {
    config: RateLimitConfig,
    limiter: RateLimiter,
}

impl RateLimitMiddleware {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            limiter: RateLimiter::new(config.requests_per_second, config.burst),
            config,
        }
    }
}

// Usage:
let middleware = RateLimitMiddleware::new(RateLimitConfig {
    requests_per_second: 100,
    burst: 10,
});
```

---

## Using Middleware

Register middleware with the orchestrator:

```rust
use dtx_core::middleware::MiddlewareStack;

let stack = MiddlewareStack::new()
    .layer(LoggingMiddleware::new())        // First (outermost)
    .layer(MyMiddleware::new())             // Second
    .layer(MetricsMiddleware::new());       // Third (innermost)

let orchestrator = Orchestrator::new(event_bus, stack);
```

Order matters: First added = first to run.

---

## Testing Middleware

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use dtx_core::middleware::{Handler, MiddlewareStackBuilder};

    struct TestHandler;

    #[async_trait]
    impl Handler for TestHandler {
        async fn handle(&self, _op: Operation, _ctx: Context) -> Result<Response> {
            Ok(Response::ok())
        }
    }

    #[tokio::test]
    async fn test_my_middleware() {
        let chain = MiddlewareStackBuilder::new()
            .layer(MyMiddleware::new())
            .build(TestHandler);

        let result = chain.execute(
            Operation::Start { id: ResourceId::new("test") },
            Context::new(),
        ).await;

        assert!(result.is_ok());
    }
}
```

---

## Best Practices

1. **Keep it focused**: One middleware, one concern
2. **Handle errors gracefully**: Don't panic, return errors
3. **Be efficient**: Middleware runs on every operation
4. **Use tracing**: Instrument with spans for debugging
5. **Make it configurable**: Accept config in constructor
6. **Test thoroughly**: Test all code paths
