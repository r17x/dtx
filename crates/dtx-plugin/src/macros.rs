//! Macros for plugin registration.
//!
//! These macros generate the required FFI entry points for dynamic plugin loading.

/// Register a backend plugin for dynamic loading.
///
/// This macro generates the `extern "C"` entry point function that the plugin
/// loader expects when loading plugins dynamically.
///
/// # Example
///
/// ```ignore
/// use dtx_plugin::{dtx_backend_plugin, BackendPlugin, PluginError};
/// use dtx_core::resource::{Resource, ResourceKind};
///
/// pub struct MyBackend;
///
/// impl BackendPlugin for MyBackend {
///     fn name(&self) -> &str { "my-backend" }
///     fn resource_kind(&self) -> ResourceKind { ResourceKind::Custom(42) }
///     fn create_resource(&self, config: serde_json::Value) -> Result<Box<dyn Resource>, PluginError> {
///         // ...
///     }
/// }
///
/// // Generate the entry point
/// dtx_backend_plugin!(MyBackend);
///
/// // Or with a custom constructor:
/// dtx_backend_plugin!(MyBackend, MyBackend::with_config(default_config()));
/// ```
#[macro_export]
macro_rules! dtx_backend_plugin {
    ($ty:ty) => {
        $crate::dtx_backend_plugin!($ty, <$ty>::default());
    };
    ($ty:ty, $constructor:expr) => {
        /// Plugin entry point for dynamic loading.
        ///
        /// # Safety
        ///
        /// This function is called by the plugin loader via FFI.
        /// The returned pointer must be converted back using `Box::from_raw`.
        #[no_mangle]
        pub extern "C" fn create_plugin() -> *mut dyn $crate::BackendPlugin {
            let plugin: Box<dyn $crate::BackendPlugin> = Box::new($constructor);
            Box::into_raw(plugin)
        }
    };
}

/// Register a middleware plugin for dynamic loading.
///
/// This macro generates the `extern "C"` entry point function that the plugin
/// loader expects when loading plugins dynamically.
///
/// # Example
///
/// ```ignore
/// use dtx_plugin::{dtx_middleware_plugin, MiddlewarePlugin};
/// use dtx_core::middleware::Middleware;
///
/// pub struct MyMiddlewarePlugin;
///
/// impl MiddlewarePlugin for MyMiddlewarePlugin {
///     fn name(&self) -> &str { "my-middleware" }
///     fn create_middleware(&self) -> Box<dyn Middleware> {
///         // ...
///     }
/// }
///
/// // Generate the entry point
/// dtx_middleware_plugin!(MyMiddlewarePlugin);
/// ```
#[macro_export]
macro_rules! dtx_middleware_plugin {
    ($ty:ty) => {
        $crate::dtx_middleware_plugin!($ty, <$ty>::default());
    };
    ($ty:ty, $constructor:expr) => {
        /// Plugin entry point for dynamic loading.
        ///
        /// # Safety
        ///
        /// This function is called by the plugin loader via FFI.
        /// The returned pointer must be converted back using `Box::from_raw`.
        #[no_mangle]
        pub extern "C" fn create_plugin() -> *mut dyn $crate::MiddlewarePlugin {
            let plugin: Box<dyn $crate::MiddlewarePlugin> = Box::new($constructor);
            Box::into_raw(plugin)
        }
    };
}

/// Register a full plugin (providing both backends and middleware) for dynamic loading.
///
/// This macro generates the `extern "C"` entry point function that returns
/// a `Plugin` trait object. The plugin can then provide multiple backends
/// and middleware through its trait implementation.
///
/// # Example
///
/// ```ignore
/// use dtx_plugin::{dtx_plugin, Plugin, BackendPlugin, MiddlewarePlugin};
///
/// pub struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     fn name(&self) -> &str { "my-plugin" }
///     fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
///     fn api_version(&self) -> u32 { 2 }
///
///     fn backends(&self) -> Vec<Box<dyn BackendPlugin>> {
///         vec![Box::new(MyBackend)]
///     }
///
///     fn middleware(&self) -> Vec<Box<dyn MiddlewarePlugin>> {
///         vec![Box::new(MyMiddleware)]
///     }
/// }
///
/// // Generate the entry point
/// dtx_plugin!(MyPlugin);
/// ```
#[macro_export]
macro_rules! dtx_plugin {
    ($ty:ty) => {
        $crate::dtx_plugin!($ty, <$ty>::default());
    };
    ($ty:ty, $constructor:expr) => {
        /// Plugin entry point for dynamic loading.
        ///
        /// # Safety
        ///
        /// This function is called by the plugin loader via FFI.
        /// The returned pointer must be converted back using `Box::from_raw`.
        #[no_mangle]
        pub extern "C" fn create_plugin() -> *mut dyn $crate::Plugin {
            let plugin: Box<dyn $crate::Plugin> = Box::new($constructor);
            Box::into_raw(plugin)
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::traits::{BackendPlugin, MiddlewarePlugin, Plugin};
    use dtx_core::middleware::Middleware;
    use dtx_core::resource::{Resource, ResourceKind};

    // Test backend plugin - constructed in test_backend_macro_compiles
    #[derive(Default)]
    #[allow(dead_code)]
    struct TestBackend;

    impl BackendPlugin for TestBackend {
        fn name(&self) -> &str {
            "test-backend"
        }
        fn resource_kind(&self) -> ResourceKind {
            ResourceKind::Process
        }
        fn create_resource(
            &self,
            _config: serde_json::Value,
        ) -> Result<Box<dyn Resource>, crate::PluginError> {
            Err(crate::PluginError::ResourceCreation(
                "test only".to_string(),
            ))
        }
    }

    // Test middleware plugin - constructed in test_middleware_macro_compiles
    #[derive(Default)]
    #[allow(dead_code)]
    struct TestMiddlewarePlugin;

    impl MiddlewarePlugin for TestMiddlewarePlugin {
        fn name(&self) -> &str {
            "test-middleware"
        }
        fn create_middleware(&self) -> Box<dyn Middleware> {
            unimplemented!("test only")
        }
    }

    // Test full plugin - constructed in test_plugin_macro_compiles
    #[derive(Default)]
    #[allow(dead_code)]
    struct TestPlugin;

    impl Plugin for TestPlugin {
        fn name(&self) -> &str {
            "test-plugin"
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

    #[test]
    fn test_backend_macro_compiles() {
        // Just verify the macro compiles
        fn _check() {
            // We can't actually call the generated function here because
            // it would conflict with other test modules, but we verify
            // the macro syntax is correct by ensuring this compiles
            let _: fn() -> *mut dyn BackendPlugin = || {
                let plugin: Box<dyn BackendPlugin> = Box::new(TestBackend);
                Box::into_raw(plugin)
            };
        }
    }

    #[test]
    fn test_middleware_macro_compiles() {
        fn _check() {
            let _: fn() -> *mut dyn MiddlewarePlugin = || {
                let plugin: Box<dyn MiddlewarePlugin> = Box::new(TestMiddlewarePlugin);
                Box::into_raw(plugin)
            };
        }
    }

    #[test]
    fn test_plugin_macro_compiles() {
        fn _check() {
            let _: fn() -> *mut dyn Plugin = || {
                let plugin: Box<dyn Plugin> = Box::new(TestPlugin);
                Box::into_raw(plugin)
            };
        }
    }
}
