//! Sandboxed plugin wrapper.

use std::sync::Arc;

use wasmtime::{Instance, Module, Store};

use super::capabilities::Capabilities;
use super::host::register_host_functions;
use super::limits::ResourceLimits;
use super::runtime::{SandboxState, WasmError, WasmRuntime};
use crate::traits::{BackendPlugin, MiddlewarePlugin, Plugin};

/// A sandboxed plugin running in WASM.
pub struct SandboxedPlugin {
    name: String,
    version: String,
    module: Module,
    runtime: Arc<WasmRuntime>,
    capabilities: Capabilities,
    limits: ResourceLimits,
}

impl SandboxedPlugin {
    /// Load a plugin from WASM bytes.
    pub fn from_bytes(
        name: String,
        bytes: &[u8],
        capabilities: Capabilities,
        limits: ResourceLimits,
    ) -> Result<Self, WasmError> {
        let mut runtime = WasmRuntime::new()?;
        register_host_functions(runtime.linker_mut())?;

        let module = runtime.load_module(bytes)?;

        Ok(Self {
            name,
            version: "0.0.0".to_string(),
            module,
            runtime: Arc::new(runtime),
            capabilities,
            limits,
        })
    }

    /// Load a plugin from WASM file.
    pub fn from_file(
        path: &std::path::Path,
        capabilities: Capabilities,
        limits: ResourceLimits,
    ) -> Result<Self, WasmError> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let bytes = std::fs::read(path)?;
        Self::from_bytes(name, &bytes, capabilities, limits)
    }

    /// Create an instance for execution.
    fn create_instance(&self) -> Result<(Store<SandboxState>, Instance), WasmError> {
        let mut store = self
            .runtime
            .create_store(self.capabilities.clone(), self.limits.clone());

        let instance = self
            .runtime
            .linker()
            .instantiate(&mut store, &self.module)?;

        Ok((store, instance))
    }

    /// Call a function in the plugin that returns i32.
    pub fn call_i32(&self, func_name: &str) -> Result<i32, WasmError> {
        let (mut store, instance) = self.create_instance()?;

        let func = instance
            .get_typed_func::<(), i32>(&mut store, func_name)
            .map_err(|e| {
                WasmError::Compilation(format!("function not found: {}: {}", func_name, e))
            })?;

        func.call(&mut store, ()).map_err(WasmError::from)
    }

    /// Call a function in the plugin that takes no args and returns nothing.
    pub fn call_void(&self, func_name: &str) -> Result<(), WasmError> {
        let (mut store, instance) = self.create_instance()?;

        let func = instance
            .get_typed_func::<(), ()>(&mut store, func_name)
            .map_err(|e| {
                WasmError::Compilation(format!("function not found: {}: {}", func_name, e))
            })?;

        func.call(&mut store, ()).map_err(WasmError::from)
    }

    /// Get capabilities.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Get limits.
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }
}

impl Plugin for SandboxedPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn api_version(&self) -> u32 {
        2
    }

    fn backends(&self) -> Vec<Box<dyn BackendPlugin>> {
        // WASM plugins can't provide backends directly
        // They register via host functions
        vec![]
    }

    fn middleware(&self) -> Vec<Box<dyn MiddlewarePlugin>> {
        vec![]
    }
}

/// Wrapper to run a native plugin in sandbox mode.
///
/// This is for testing - wraps a native plugin with capability checks.
pub struct SandboxWrapper<P> {
    inner: P,
    capabilities: Capabilities,
}

impl<P: Plugin> SandboxWrapper<P> {
    /// Create a new sandbox wrapper.
    pub fn new(plugin: P, capabilities: Capabilities) -> Self {
        Self {
            inner: plugin,
            capabilities,
        }
    }

    /// Get the inner plugin.
    pub fn inner(&self) -> &P {
        &self.inner
    }

    /// Get capabilities.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
}

impl<P: Plugin> Plugin for SandboxWrapper<P> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn version(&self) -> &str {
        self.inner.version()
    }

    fn api_version(&self) -> u32 {
        self.inner.api_version()
    }

    fn backends(&self) -> Vec<Box<dyn BackendPlugin>> {
        if self.capabilities.resources.manage {
            self.inner.backends()
        } else {
            vec![]
        }
    }

    fn middleware(&self) -> Vec<Box<dyn MiddlewarePlugin>> {
        self.inner.middleware()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_minimal_module() {
        let wasm = wat::parse_str("(module)").unwrap();
        let plugin = SandboxedPlugin::from_bytes(
            "test".to_string(),
            &wasm,
            Capabilities::minimal(),
            ResourceLimits::default(),
        )
        .unwrap();

        assert_eq!(plugin.name(), "test");
        assert_eq!(plugin.api_version(), 2);
    }

    #[test]
    fn call_exported_function() {
        // Module that exports an "add" function returning i32
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "get_value") (result i32)
                    i32.const 42
                )
            )
            "#,
        )
        .unwrap();

        let plugin = SandboxedPlugin::from_bytes(
            "test".to_string(),
            &wasm,
            Capabilities::minimal(),
            ResourceLimits::default(),
        )
        .unwrap();

        let result = plugin.call_i32("get_value").unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn sandbox_wrapper_filters_backends() {
        struct TestPlugin;

        impl Plugin for TestPlugin {
            fn name(&self) -> &str {
                "test"
            }
            fn version(&self) -> &str {
                "1.0.0"
            }
            fn api_version(&self) -> u32 {
                2
            }
            fn backends(&self) -> Vec<Box<dyn BackendPlugin>> {
                vec![]
            }
            fn middleware(&self) -> Vec<Box<dyn MiddlewarePlugin>> {
                vec![]
            }
        }

        let wrapped = SandboxWrapper::new(TestPlugin, Capabilities::minimal());
        assert_eq!(wrapped.name(), "test");
        assert!(wrapped.backends().is_empty());
    }
}
