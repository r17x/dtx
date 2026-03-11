//! Plugin discovery and loading.
//!
//! The plugin loader discovers plugins in a directory, validates their
//! manifests, and loads them either statically or dynamically.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::error::{PluginError, Result};
use crate::manifest::{PluginManifest, PluginType};
use crate::traits::{BackendPlugin, MiddlewarePlugin};

/// A loaded plugin with its manifest and implementation.
pub struct LoadedPlugin {
    /// The plugin's manifest.
    pub manifest: PluginManifest,

    /// The plugin implementation, if loaded.
    kind: LoadedPluginKind,

    /// Path to the plugin directory.
    pub path: PathBuf,
}

/// The kind of loaded plugin implementation.
#[allow(dead_code)] // Translator variant is a placeholder for Phase 6
enum LoadedPluginKind {
    /// Plugin manifest discovered but not yet loaded.
    Discovered,

    /// Backend plugin implementation.
    Backend(Box<dyn BackendPlugin>),

    /// Middleware plugin implementation.
    Middleware(Box<dyn MiddlewarePlugin>),

    /// Translator plugin (placeholder for Phase 6).
    Translator,

    /// Dynamic library handle (when using `dynamic` feature).
    #[cfg(feature = "dynamic")]
    Dynamic {
        _library: libloading::Library,
        backend: Option<Box<dyn BackendPlugin>>,
        middleware: Option<Box<dyn MiddlewarePlugin>>,
    },
}

impl LoadedPlugin {
    /// Check if the plugin is actually loaded (not just discovered).
    pub fn is_loaded(&self) -> bool {
        !matches!(self.kind, LoadedPluginKind::Discovered)
    }

    /// Get the backend plugin, if this is a backend plugin and it's loaded.
    pub fn as_backend(&self) -> Option<&dyn BackendPlugin> {
        match &self.kind {
            LoadedPluginKind::Backend(b) => Some(b.as_ref()),
            #[cfg(feature = "dynamic")]
            LoadedPluginKind::Dynamic { backend, .. } => backend.as_ref().map(|b| b.as_ref()),
            _ => None,
        }
    }

    /// Get the middleware plugin, if this is a middleware plugin and it's loaded.
    pub fn as_middleware(&self) -> Option<&dyn MiddlewarePlugin> {
        match &self.kind {
            LoadedPluginKind::Middleware(m) => Some(m.as_ref()),
            #[cfg(feature = "dynamic")]
            LoadedPluginKind::Dynamic { middleware, .. } => middleware.as_ref().map(|m| m.as_ref()),
            _ => None,
        }
    }
}

/// Plugin loader for discovering and loading plugins.
///
/// The loader scans a plugins directory for subdirectories containing
/// `plugin.toml` manifests. Plugins can be loaded statically (by registering
/// them programmatically) or dynamically (using the `dynamic` feature).
///
/// # Example
///
/// ```ignore
/// use dtx_plugin::PluginLoader;
///
/// let mut loader = PluginLoader::new("./plugins");
///
/// // Discover all plugins
/// let manifests = loader.discover()?;
///
/// // Load a specific plugin
/// let plugin = loader.load("my-backend")?;
///
/// // Get a backend plugin
/// if let Some(backend) = loader.get_backend("docker") {
///     let resource = backend.create_resource(config)?;
/// }
/// ```
pub struct PluginLoader {
    /// Directory containing plugins.
    plugins_dir: PathBuf,

    /// Loaded plugins by name.
    loaded: HashMap<String, LoadedPlugin>,

    /// Statically registered backend plugins.
    static_backends: HashMap<String, Box<dyn BackendPlugin>>,

    /// Statically registered middleware plugins.
    static_middleware: HashMap<String, Box<dyn MiddlewarePlugin>>,
}

impl PluginLoader {
    /// Create a new plugin loader for the given directory.
    pub fn new(plugins_dir: impl Into<PathBuf>) -> Self {
        Self {
            plugins_dir: plugins_dir.into(),
            loaded: HashMap::new(),
            static_backends: HashMap::new(),
            static_middleware: HashMap::new(),
        }
    }

    /// Register a static backend plugin.
    ///
    /// Static plugins don't require dynamic loading and are compiled directly
    /// into the application.
    pub fn register_backend(&mut self, plugin: Box<dyn BackendPlugin>) {
        let name = plugin.name().to_string();
        info!(name = %name, "Registering static backend plugin");
        self.static_backends.insert(name, plugin);
    }

    /// Register a static middleware plugin.
    pub fn register_middleware(&mut self, plugin: Box<dyn MiddlewarePlugin>) {
        let name = plugin.name().to_string();
        info!(name = %name, "Registering static middleware plugin");
        self.static_middleware.insert(name, plugin);
    }

    /// Discover all plugins in the plugins directory.
    ///
    /// This scans the plugins directory for subdirectories containing
    /// `plugin.toml` files and parses their manifests.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugins directory doesn't exist or
    /// if a manifest fails to parse.
    pub fn discover(&mut self) -> Result<Vec<PluginManifest>> {
        if !self.plugins_dir.exists() {
            debug!(path = ?self.plugins_dir, "Plugin directory does not exist, skipping discovery");
            return Ok(Vec::new());
        }

        let mut manifests = Vec::new();

        let entries = std::fs::read_dir(&self.plugins_dir).map_err(PluginError::ManifestRead)?;

        for entry in entries {
            let entry = entry.map_err(PluginError::ManifestRead)?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("plugin.toml");
            if !manifest_path.exists() {
                debug!(path = ?path, "Skipping directory without plugin.toml");
                continue;
            }

            match PluginManifest::from_file(&manifest_path) {
                Ok(manifest) => {
                    info!(
                        name = %manifest.name,
                        version = %manifest.version,
                        plugin_type = ?manifest.plugin_type,
                        "Discovered plugin"
                    );

                    // Store as discovered (not yet loaded)
                    let loaded = LoadedPlugin {
                        manifest: manifest.clone(),
                        kind: LoadedPluginKind::Discovered,
                        path: path.clone(),
                    };
                    self.loaded.insert(manifest.name.clone(), loaded);

                    manifests.push(manifest);
                }
                Err(e) => {
                    warn!(path = ?manifest_path, error = %e, "Failed to parse plugin manifest");
                }
            }
        }

        Ok(manifests)
    }

    /// Load a plugin by name.
    ///
    /// For static plugins, this looks up the registered plugin.
    /// For dynamic plugins (with `dynamic` feature), this loads the shared library.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin is not found, not discovered,
    /// or fails to load.
    pub fn load(&mut self, name: &str) -> Result<&LoadedPlugin> {
        // Check if already loaded
        if self
            .loaded
            .get(name)
            .map(|p| p.is_loaded())
            .unwrap_or(false)
        {
            return Ok(self.loaded.get(name).unwrap());
        }

        // Check static registrations first
        if let Some(backend) = self.static_backends.remove(name) {
            let manifest = PluginManifest {
                name: name.to_string(),
                version: "static".to_string(),
                description: None,
                authors: Vec::new(),
                plugin_type: PluginType::Backend,
                entry_point: String::new(),
                dependencies: Vec::new(),
            };

            let loaded = LoadedPlugin {
                manifest,
                kind: LoadedPluginKind::Backend(backend),
                path: PathBuf::new(),
            };

            self.loaded.insert(name.to_string(), loaded);
            return Ok(self.loaded.get(name).unwrap());
        }

        if let Some(middleware) = self.static_middleware.remove(name) {
            let manifest = PluginManifest {
                name: name.to_string(),
                version: "static".to_string(),
                description: None,
                authors: Vec::new(),
                plugin_type: PluginType::Middleware,
                entry_point: String::new(),
                dependencies: Vec::new(),
            };

            let loaded = LoadedPlugin {
                manifest,
                kind: LoadedPluginKind::Middleware(middleware),
                path: PathBuf::new(),
            };

            self.loaded.insert(name.to_string(), loaded);
            return Ok(self.loaded.get(name).unwrap());
        }

        // Check if discovered but not loaded
        let is_discovered = self.loaded.contains_key(name);
        if !is_discovered {
            return Err(PluginError::PluginNotFound(name.to_string()));
        }

        // For discovered plugins, attempt dynamic loading
        #[cfg(feature = "dynamic")]
        {
            self.load_dynamic(name)
        }

        #[cfg(not(feature = "dynamic"))]
        {
            // Without dynamic feature, we can only use static plugins
            // If the plugin was discovered but not registered statically, we can't load it
            Err(PluginError::DynamicNotAvailable)
        }
    }

    /// Load a plugin dynamically from a shared library.
    #[cfg(feature = "dynamic")]
    fn load_dynamic(&mut self, name: &str) -> Result<&LoadedPlugin> {
        let plugin = self
            .loaded
            .get(name)
            .ok_or_else(|| PluginError::PluginNotFound(name.to_string()))?;

        let manifest = plugin.manifest.clone();
        let plugin_dir = plugin.path.clone();

        // Find the shared library
        let lib_name = format!(
            "{}{}",
            if cfg!(target_os = "windows") {
                ""
            } else {
                "lib"
            },
            name.replace('-', "_")
        );
        let lib_ext = if cfg!(target_os = "windows") {
            "dll"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };

        let lib_path = plugin_dir.join(format!("{}.{}", lib_name, lib_ext));

        if !lib_path.exists() {
            return Err(PluginError::ManifestNotFound(lib_path));
        }

        // Load the library
        let library = unsafe { libloading::Library::new(&lib_path)? };

        // Get the entry point symbol
        let entry_point = &manifest.entry_point;

        // Note: Trait objects are not technically FFI-safe, but this is the standard
        // pattern for Rust plugin systems. The plugin and host must be compiled with
        // compatible Rust versions and use the same memory allocator.
        #[allow(improper_ctypes_definitions)]
        let loaded = match manifest.plugin_type {
            PluginType::Backend => {
                type CreateFn = unsafe extern "C" fn() -> *mut dyn BackendPlugin;
                let create: libloading::Symbol<CreateFn> =
                    unsafe { library.get(entry_point.as_bytes())? };

                let backend = unsafe { Box::from_raw(create()) };

                LoadedPlugin {
                    manifest,
                    kind: LoadedPluginKind::Dynamic {
                        _library: library,
                        backend: Some(backend),
                        middleware: None,
                    },
                    path: plugin_dir,
                }
            }
            PluginType::Middleware => {
                type CreateFn = unsafe extern "C" fn() -> *mut dyn MiddlewarePlugin;
                let create: libloading::Symbol<CreateFn> =
                    unsafe { library.get(entry_point.as_bytes())? };

                let middleware = unsafe { Box::from_raw(create()) };

                LoadedPlugin {
                    manifest,
                    kind: LoadedPluginKind::Dynamic {
                        _library: library,
                        backend: None,
                        middleware: Some(middleware),
                    },
                    path: plugin_dir,
                }
            }
            PluginType::Translator => {
                // Translator plugins not yet implemented
                LoadedPlugin {
                    manifest,
                    kind: LoadedPluginKind::Translator,
                    path: plugin_dir,
                }
            }
        };

        info!(name = %name, "Loaded dynamic plugin");
        self.loaded.insert(name.to_string(), loaded);
        Ok(self.loaded.get(name).unwrap())
    }

    /// Get a loaded backend plugin by name.
    pub fn get_backend(&self, name: &str) -> Option<&dyn BackendPlugin> {
        self.loaded.get(name).and_then(|p| p.as_backend())
    }

    /// Get a loaded middleware plugin by name.
    pub fn get_middleware(&self, name: &str) -> Option<&dyn MiddlewarePlugin> {
        self.loaded.get(name).and_then(|p| p.as_middleware())
    }

    /// Get the plugins directory.
    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    /// List all discovered plugin names.
    pub fn discovered_plugins(&self) -> impl Iterator<Item = &str> {
        self.loaded.keys().map(|s| s.as_str())
    }

    /// List all loaded plugin names.
    pub fn loaded_plugins(&self) -> impl Iterator<Item = &str> {
        self.loaded
            .iter()
            .filter(|(_, p)| p.is_loaded())
            .map(|(name, _)| name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_loader_new() {
        let loader = PluginLoader::new("/some/path");
        assert_eq!(loader.plugins_dir(), Path::new("/some/path"));
        assert_eq!(loader.discovered_plugins().count(), 0);
    }

    #[test]
    fn test_discover_empty_dir() {
        let dir = tempdir().unwrap();
        let mut loader = PluginLoader::new(dir.path());

        let manifests = loader.discover().unwrap();
        assert!(manifests.is_empty());
    }

    #[test]
    fn test_discover_nonexistent_dir() {
        let mut loader = PluginLoader::new("/nonexistent/path/to/plugins");

        // Should not error, just return empty
        let manifests = loader.discover().unwrap();
        assert!(manifests.is_empty());
    }

    #[test]
    fn test_discover_plugin() {
        let dir = tempdir().unwrap();

        // Create a plugin directory with manifest
        let plugin_dir = dir.path().join("my-plugin");
        std::fs::create_dir(&plugin_dir).unwrap();

        let manifest_content = r#"
            name = "my-plugin"
            version = "1.0.0"
            plugin_type = "backend"
            entry_point = "create_plugin"
        "#;
        std::fs::write(plugin_dir.join("plugin.toml"), manifest_content).unwrap();

        let mut loader = PluginLoader::new(dir.path());
        let manifests = loader.discover().unwrap();

        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].name, "my-plugin");
        assert_eq!(manifests[0].version, "1.0.0");
        assert_eq!(loader.discovered_plugins().count(), 1);
    }

    #[test]
    fn test_plugin_not_found() {
        let loader = PluginLoader::new("/some/path");
        assert!(loader.get_backend("nonexistent").is_none());
        assert!(loader.get_middleware("nonexistent").is_none());
    }
}
