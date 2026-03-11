//! WASM runtime for sandboxed plugin execution.

use std::time::Duration;

use wasmtime::{Config, Engine, Linker, Module, Store};

use super::capabilities::Capabilities;
use super::limits::ResourceLimits;

/// WASM runtime for sandboxed plugin execution.
pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<SandboxState>,
}

/// State shared with WASM guest.
pub struct SandboxState {
    /// Capabilities for this sandbox.
    pub capabilities: Capabilities,
    /// Resource limits.
    pub limits: ResourceLimits,
    /// Current memory usage.
    pub memory_used: usize,
    /// CPU time used.
    pub cpu_used: Duration,
}

impl WasmRuntime {
    /// Create a new WASM runtime.
    pub fn new() -> Result<Self, WasmError> {
        let mut config = Config::new();

        // Enable fuel metering for CPU limits
        config.consume_fuel(true);

        // Enable epoch interruption for timeouts
        config.epoch_interruption(true);

        // Memory limits
        config.max_wasm_stack(512 * 1024); // 512KB stack

        let engine = Engine::new(&config)?;
        let linker = Linker::new(&engine);

        Ok(Self { engine, linker })
    }

    /// Load a WASM module from bytes.
    pub fn load_module(&self, bytes: &[u8]) -> Result<Module, WasmError> {
        Module::new(&self.engine, bytes).map_err(WasmError::from)
    }

    /// Load a WASM module from file.
    pub fn load_module_file(&self, path: &std::path::Path) -> Result<Module, WasmError> {
        Module::from_file(&self.engine, path).map_err(WasmError::from)
    }

    /// Create a new store with limits.
    pub fn create_store(
        &self,
        capabilities: Capabilities,
        limits: ResourceLimits,
    ) -> Store<SandboxState> {
        let state = SandboxState {
            capabilities,
            limits: limits.clone(),
            memory_used: 0,
            cpu_used: Duration::ZERO,
        };

        let mut store = Store::new(&self.engine, state);

        // Set initial fuel (CPU budget)
        store.set_fuel(limits.max_fuel).ok();

        // Set epoch deadline for timeouts
        // Allow 1 epoch tick before trap - epochs are incremented externally for timeouts
        store.set_epoch_deadline(1);

        store
    }

    /// Get the engine reference.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the linker reference.
    pub fn linker(&self) -> &Linker<SandboxState> {
        &self.linker
    }

    /// Get a mutable linker reference for adding host functions.
    pub fn linker_mut(&mut self) -> &mut Linker<SandboxState> {
        &mut self.linker
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().expect("failed to create WASM runtime")
    }
}

/// Errors from WASM runtime.
#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    /// WASM engine error.
    #[error("WASM engine error: {0}")]
    Engine(#[from] wasmtime::Error),

    /// Module compilation failed.
    #[error("module compilation failed: {0}")]
    Compilation(String),

    /// Out of fuel (CPU limit exceeded).
    #[error("out of fuel (CPU limit exceeded)")]
    OutOfFuel,

    /// Memory limit exceeded.
    #[error("memory limit exceeded")]
    MemoryLimit,

    /// Capability denied.
    #[error("capability denied: {0}")]
    CapabilityDenied(String),

    /// Timeout.
    #[error("timeout")]
    Timeout,

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_creates_engine() {
        let runtime = WasmRuntime::new().unwrap();
        // Just verify the engine was created successfully
        let _engine = runtime.engine();
    }

    #[test]
    fn runtime_loads_empty_module() {
        let runtime = WasmRuntime::new().unwrap();
        // Minimal valid WASM module
        let wasm = wat::parse_str("(module)").unwrap();
        let module = runtime.load_module(&wasm).unwrap();
        assert_eq!(module.exports().count(), 0);
    }

    #[test]
    fn store_has_fuel() {
        let runtime = WasmRuntime::new().unwrap();
        let store = runtime.create_store(Capabilities::minimal(), ResourceLimits::default());
        // Store should have initial fuel set
        let fuel = store.get_fuel().unwrap();
        assert!(fuel > 0);
    }
}
