//! Host functions exposed to sandboxed WASM plugins.

use wasmtime::{Caller, Linker};

use super::runtime::{SandboxState, WasmError};

/// Register host functions in the linker.
pub fn register_host_functions(linker: &mut Linker<SandboxState>) -> Result<(), WasmError> {
    register_log_functions(linker)?;
    register_env_functions(linker)?;
    register_event_functions(linker)?;
    register_resource_functions(linker)?;

    Ok(())
}

fn register_log_functions(linker: &mut Linker<SandboxState>) -> Result<(), WasmError> {
    // log_info(ptr: i32, len: i32)
    linker.func_wrap(
        "dtx",
        "log_info",
        |mut caller: Caller<'_, SandboxState>, ptr: i32, len: i32| {
            let msg = read_string(&mut caller, ptr, len)?;
            tracing::info!(plugin = "sandbox", "{}", msg);
            Ok(())
        },
    )?;

    // log_warn(ptr: i32, len: i32)
    linker.func_wrap(
        "dtx",
        "log_warn",
        |mut caller: Caller<'_, SandboxState>, ptr: i32, len: i32| {
            let msg = read_string(&mut caller, ptr, len)?;
            tracing::warn!(plugin = "sandbox", "{}", msg);
            Ok(())
        },
    )?;

    // log_error(ptr: i32, len: i32)
    linker.func_wrap(
        "dtx",
        "log_error",
        |mut caller: Caller<'_, SandboxState>, ptr: i32, len: i32| {
            let msg = read_string(&mut caller, ptr, len)?;
            tracing::error!(plugin = "sandbox", "{}", msg);
            Ok(())
        },
    )?;

    Ok(())
}

fn register_env_functions(linker: &mut Linker<SandboxState>) -> Result<(), WasmError> {
    // env_get(name_ptr: i32, name_len: i32, out_ptr: i32, out_len: i32) -> i32
    linker.func_wrap(
        "dtx",
        "env_get",
        |mut caller: Caller<'_, SandboxState>,
         name_ptr: i32,
         name_len: i32,
         out_ptr: i32,
         out_len: i32|
         -> Result<i32, wasmtime::Error> {
            let name = read_string(&mut caller, name_ptr, name_len)?;

            // Check capability
            if !caller.data().capabilities.can_read_env(&name) {
                return Err(WasmError::CapabilityDenied(format!("env:read:{}", name)).into());
            }

            match std::env::var(&name) {
                Ok(value) => {
                    let bytes = value.as_bytes();
                    if bytes.len() <= out_len as usize {
                        write_bytes(&mut caller, out_ptr, bytes)?;
                        Ok(bytes.len() as i32)
                    } else {
                        Ok(-(bytes.len() as i32)) // Negative = buffer too small
                    }
                }
                Err(_) => Ok(0), // Not found
            }
        },
    )?;

    Ok(())
}

fn register_event_functions(linker: &mut Linker<SandboxState>) -> Result<(), WasmError> {
    // event_publish(type_ptr: i32, type_len: i32, data_ptr: i32, data_len: i32) -> i32
    linker.func_wrap(
        "dtx",
        "event_publish",
        |mut caller: Caller<'_, SandboxState>,
         type_ptr: i32,
         type_len: i32,
         _data_ptr: i32,
         _data_len: i32|
         -> Result<i32, wasmtime::Error> {
            let event_type = read_string(&mut caller, type_ptr, type_len)?;

            // Check capability
            if !caller.data().capabilities.events.publish {
                return Err(WasmError::CapabilityDenied("event:publish".to_string()).into());
            }

            // TODO: Actually publish to EventBus
            tracing::debug!(plugin = "sandbox", event = %event_type, "event published");

            Ok(0) // Success
        },
    )?;

    Ok(())
}

fn register_resource_functions(linker: &mut Linker<SandboxState>) -> Result<(), WasmError> {
    // resource_start(id_ptr: i32, id_len: i32) -> i32
    linker.func_wrap(
        "dtx",
        "resource_start",
        |mut caller: Caller<'_, SandboxState>,
         id_ptr: i32,
         id_len: i32|
         -> Result<i32, wasmtime::Error> {
            // Check capability
            if !caller.data().capabilities.resources.manage {
                return Err(WasmError::CapabilityDenied("resource:manage".to_string()).into());
            }

            let id = read_string(&mut caller, id_ptr, id_len)?;

            // TODO: Actually start resource
            tracing::info!(plugin = "sandbox", resource = %id, "resource start requested");

            Ok(0)
        },
    )?;

    // resource_stop(id_ptr: i32, id_len: i32) -> i32
    linker.func_wrap(
        "dtx",
        "resource_stop",
        |mut caller: Caller<'_, SandboxState>,
         id_ptr: i32,
         id_len: i32|
         -> Result<i32, wasmtime::Error> {
            if !caller.data().capabilities.resources.manage {
                return Err(WasmError::CapabilityDenied("resource:manage".to_string()).into());
            }

            let id = read_string(&mut caller, id_ptr, id_len)?;
            tracing::info!(plugin = "sandbox", resource = %id, "resource stop requested");

            Ok(0)
        },
    )?;

    Ok(())
}

// --- Memory helpers ---

fn read_string(
    caller: &mut Caller<'_, SandboxState>,
    ptr: i32,
    len: i32,
) -> Result<String, wasmtime::Error> {
    let bytes = read_bytes(caller, ptr, len)?;
    String::from_utf8(bytes)
        .map_err(|e| wasmtime::Error::new(WasmError::Compilation(e.to_string())))
}

fn read_bytes(
    caller: &mut Caller<'_, SandboxState>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, wasmtime::Error> {
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| {
            wasmtime::Error::new(WasmError::Compilation("no memory export".to_string()))
        })?;

    let data = memory.data(&*caller);
    let start = ptr as usize;
    let end = start + len as usize;

    if end > data.len() {
        return Err(wasmtime::Error::new(WasmError::MemoryLimit));
    }

    Ok(data[start..end].to_vec())
}

fn write_bytes(
    caller: &mut Caller<'_, SandboxState>,
    ptr: i32,
    bytes: &[u8],
) -> Result<(), wasmtime::Error> {
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| {
            wasmtime::Error::new(WasmError::Compilation("no memory export".to_string()))
        })?;

    memory
        .write(caller, ptr as usize, bytes)
        .map_err(|e| wasmtime::Error::new(WasmError::Compilation(e.to_string())))
}
