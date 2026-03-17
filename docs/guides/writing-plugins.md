# Writing Plugins

> Extend dtx with custom backends, middleware, and translators.

---

## Overview

Plugins can provide:
- **Backends**: New resource types (Kubernetes, VM, etc.)
- **Middleware**: Custom processing layers
- **Translators**: Convert between resource types

---

## Project Setup

```bash
cargo new --lib dtx-my-plugin
cd dtx-my-plugin
```

`Cargo.toml`:
```toml
[package]
name = "dtx-my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
dtx-core = "2.0"
dtx-plugin = "2.0"
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

---

## Plugin Entry Point

```rust
// src/lib.rs

use dtx_plugin::{dtx_plugin, Plugin, BackendPlugin, MiddlewarePlugin};

pub struct MyPlugin;

impl Plugin for MyPlugin {
    fn name(&self) -> &str { "my-plugin" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
    fn api_version(&self) -> u32 { 2 }

    fn backends(&self) -> Vec<Box<dyn BackendPlugin>> {
        vec![Box::new(MyBackend)]
    }

    fn middleware(&self) -> Vec<Box<dyn MiddlewarePlugin>> {
        vec![Box::new(MyMiddlewarePlugin)]
    }
}

impl MyPlugin {
    pub fn new() -> Self {
        Self
    }
}

// Generate the FFI entry point for dynamic loading
dtx_plugin!(MyPlugin);
```

---

## Plugin Macros

dtx provides three macros for generating FFI entry points:

| Macro | Use Case |
|-------|----------|
| `dtx_plugin!(Type)` | Full plugin providing backends and middleware |
| `dtx_backend_plugin!(Type)` | Backend-only plugin |
| `dtx_middleware_plugin!(Type)` | Middleware-only plugin |

### With Custom Constructor

If your plugin doesn't implement `Default`:

```rust
// Full plugin
dtx_plugin!(MyPlugin, MyPlugin::with_config(config));

// Backend only
dtx_backend_plugin!(MyBackend, MyBackend::new(settings));

// Middleware only
dtx_middleware_plugin!(MyMiddleware, MyMiddleware::new());
```

---

## Custom Backend

```rust
use dtx_core::resource::{Resource, ResourceKind};
use dtx_plugin::{BackendPlugin, PluginError};

pub struct MyBackend;

impl BackendPlugin for MyBackend {
    fn name(&self) -> &str {
        "my-backend"
    }

    fn resource_kind(&self) -> ResourceKind {
        ResourceKind::Custom(42)  // Unique ID for your backend
    }

    fn create_resource(
        &self,
        config: serde_json::Value,
    ) -> Result<Box<dyn Resource>, PluginError> {
        let config: MyResourceConfig = serde_json::from_value(config)?;
        Ok(Box::new(MyResource::new(config)))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyResourceConfig {
    pub id: String,
    pub setting: String,
}

pub struct MyResource {
    config: MyResourceConfig,
    state: ResourceState,
    event_bus: Arc<EventBus>,
}

#[async_trait]
impl Resource for MyResource {
    fn id(&self) -> &ResourceId { &self.config.id.clone().into() }
    fn kind(&self) -> ResourceKind { ResourceKind::Custom(42) }
    fn state(&self) -> &ResourceState { &self.state }

    async fn start(&mut self, ctx: &Context) -> Result<()> {
        // Your start logic
        self.state = ResourceState::Running { pid: None, started_at: Utc::now() };
        Ok(())
    }

    async fn stop(&mut self, ctx: &Context) -> Result<()> {
        // Your stop logic
        self.state = ResourceState::Stopped { exit_code: Some(0), .. };
        Ok(())
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}
```

---

## Custom Middleware Plugin

```rust
use dtx_core::middleware::Middleware;
use dtx_plugin::MiddlewarePlugin;

pub struct MyMiddlewarePlugin;

impl MiddlewarePlugin for MyMiddlewarePlugin {
    fn name(&self) -> &str {
        "my-middleware"
    }

    fn create_middleware(&self) -> Box<dyn Middleware> {
        Box::new(MyMiddleware::new())
    }
}

pub struct MyMiddleware {
    config: MyMiddlewareConfig,
}

#[async_trait]
impl Middleware for MyMiddleware {
    fn name(&self) -> &'static str { "my-middleware" }

    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
        // Your middleware logic
        next.run(op, ctx).await
    }
}
```

---

## Custom Translator

```rust
use dtx_core::translation::{Translator, TranslationError};
use dtx_plugin::TranslatorPlugin;
use std::any::{Any, TypeId};

pub struct MyTranslatorPlugin;

impl TranslatorPlugin for MyTranslatorPlugin {
    fn from_kind(&self) -> TypeId { TypeId::of::<ProcessConfig>() }
    fn to_kind(&self) -> TypeId { TypeId::of::<MyResourceConfig>() }

    fn translate(&self, from: &dyn Any) -> Result<Box<dyn Any>, TranslationError> {
        let process = from.downcast_ref::<ProcessConfig>()
            .ok_or_else(|| TranslationError::Incompatible("Expected ProcessConfig".into()))?;

        let my_config = MyResourceConfig {
            id: process.id.as_str().to_string(),
            setting: process.command.as_str().to_string(),
        };

        Ok(Box::new(my_config))
    }

    fn reverse(&self, to: &dyn Any) -> Result<Box<dyn Any>, TranslationError> {
        // Reverse translation
        todo!()
    }
}
```

---

## Plugin Manifest

`plugin.toml`:
```toml
[plugin]
name = "dtx-my-plugin"
version = "0.1.0"
api_version = "2"
authors = ["Your Name <you@example.com>"]
description = "My custom dtx plugin"
license = "MIT"

[provides]
backends = ["my-backend"]
middleware = ["my-middleware"]
translators = [["process", "my-resource"]]

[dependencies]
dtx-core = "2.0"
```

---

## Build & Install

```bash
# Build
cargo build --release

# Install (copy to plugin directory)
mkdir -p ~/.local/share/dtx/plugins
cp target/release/libdtx_my_plugin.so ~/.local/share/dtx/plugins/

# Or on macOS
cp target/release/libdtx_my_plugin.dylib ~/.local/share/dtx/plugins/
```

---

## Using the Plugin

`.dtx/config.yaml`:
```yaml
plugins:
  - my-plugin

resources:
  my-service:
    kind: my-backend
    setting: "value"
```

---

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_loads() {
        let plugin = MyPlugin::new();
        assert_eq!(plugin.name(), "my-plugin");
        assert_eq!(plugin.api_version(), 2);
    }

    #[test]
    fn test_backend_creates_resource() {
        let backend = MyBackend;
        let config = serde_json::json!({
            "id": "test",
            "setting": "value"
        });

        let resource = backend.create_resource(config).unwrap();
        assert_eq!(resource.kind(), ResourceKind::Custom(42));
    }
}
```

---

## Publishing

1. Create a GitHub repository
2. Add to dtx plugin registry (coming soon)
3. Users install via: `dtx plugin install github:username/dtx-my-plugin`

---

## Best Practices

1. **Version your API**: Bump version on breaking changes
2. **Handle errors**: Don't panic, return proper errors
3. **Document**: Include README with usage examples
4. **Test**: Comprehensive tests for all functionality
5. **Log**: Use tracing for debugging
