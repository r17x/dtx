//! Config-as-storage: file-backed configuration store.
//!
//! Replaces the SQLite database with `.dtx/config.yaml` as the single source of truth.
//! Provides file locking for concurrent CLI access and atomic writes.

mod error;
pub mod registry;

pub use error::StoreError;
pub use registry::ProjectRegistry;

use crate::config::loader::ConfigLoader;
use crate::config::schema::{DtxConfig, ResourceConfig};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// File-backed configuration store.
///
/// Wraps `DtxConfig` with persistence operations including
/// file locking and atomic writes.
pub struct ConfigStore {
    config: DtxConfig,
    config_path: PathBuf,
    project_root: PathBuf,
}

impl ConfigStore {
    /// Discover project by walking up from CWD and load config.
    pub fn discover_and_load() -> Result<Self, StoreError> {
        let cwd = std::env::current_dir().map_err(StoreError::Io)?;
        Self::discover_and_load_from(&cwd)
    }

    /// Discover project by walking up from the given path and load config.
    pub fn discover_and_load_from(start: &Path) -> Result<Self, StoreError> {
        let mut loader = ConfigLoader::new();
        let config_path = loader
            .discover_project(start)
            .ok_or(StoreError::ProjectNotFound)?;

        let config = loader.load()?;
        let project_root = config_path
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(start)
            .to_path_buf();

        Ok(Self {
            config,
            config_path,
            project_root,
        })
    }

    /// Load from a specific config path.
    pub fn load(config_path: PathBuf) -> Result<Self, StoreError> {
        let config = DtxConfig::load(&config_path)?;
        let project_root = config_path
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(Path::new("."))
            .to_path_buf();

        Ok(Self {
            config,
            config_path,
            project_root,
        })
    }

    /// Initialize a new project at the given root.
    pub fn init(project_root: PathBuf, name: &str) -> Result<Self, StoreError> {
        let dtx_dir = project_root.join(".dtx");
        std::fs::create_dir_all(&dtx_dir).map_err(StoreError::Io)?;

        let config_path = dtx_dir.join("config.yaml");
        let config = DtxConfig::with_project_name(name);

        let store = Self {
            config,
            config_path,
            project_root,
        };
        store.save()?;

        Ok(store)
    }

    // === Persistence ===

    /// Save config to disk with file locking and atomic write.
    pub fn save(&self) -> Result<(), StoreError> {
        let lock = FileLock::acquire(&self.config_path.with_extension("yaml.lock"))?;
        let yaml = self.config.to_yaml()?;
        atomic_write(&self.config_path, &yaml)?;
        drop(lock);
        Ok(())
    }

    /// Reload config from disk.
    pub fn reload(&mut self) -> Result<(), StoreError> {
        self.config = DtxConfig::load(&self.config_path)?;
        Ok(())
    }

    /// Save with read-modify-write under lock (for concurrent CLI access).
    pub fn save_with<F>(&mut self, f: F) -> Result<(), StoreError>
    where
        F: FnOnce(&mut DtxConfig),
    {
        let lock = FileLock::acquire(&self.config_path.with_extension("yaml.lock"))?;
        // Re-read under lock to get latest
        if self.config_path.exists() {
            self.config = DtxConfig::load(&self.config_path)?;
        }
        f(&mut self.config);
        let yaml = self.config.to_yaml()?;
        atomic_write(&self.config_path, &yaml)?;
        drop(lock);
        Ok(())
    }

    // === Project metadata ===

    pub fn project_name(&self) -> &str {
        &self.config.project.name
    }

    pub fn project_description(&self) -> Option<&str> {
        self.config.project.description.as_deref()
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn dtx_dir(&self) -> PathBuf {
        self.project_root.join(".dtx")
    }

    pub fn set_project_name(&mut self, name: &str) {
        self.config.project.name = name.to_string();
    }

    pub fn set_project_description(&mut self, desc: Option<String>) {
        self.config.project.description = desc;
    }

    // === Resources ===

    pub fn list_resources(&self) -> impl Iterator<Item = (&str, &ResourceConfig)> {
        self.config.resources.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn list_enabled_resources(&self) -> impl Iterator<Item = (&str, &ResourceConfig)> {
        self.config
            .resources
            .iter()
            .filter(|(_, r)| r.enabled)
            .map(|(k, v)| (k.as_str(), v))
    }

    pub fn get_resource(&self, name: &str) -> Option<&ResourceConfig> {
        self.config.resources.get(name)
    }

    pub fn get_resource_mut(&mut self, name: &str) -> Option<&mut ResourceConfig> {
        self.config.resources.get_mut(name)
    }

    pub fn add_resource(&mut self, name: &str, config: ResourceConfig) -> Result<(), StoreError> {
        if self.config.resources.contains_key(name) {
            return Err(StoreError::DuplicateResource(name.to_string()));
        }
        self.config.resources.insert(name.to_string(), config);
        Ok(())
    }

    pub fn remove_resource(&mut self, name: &str) -> Result<ResourceConfig, StoreError> {
        self.config
            .resources
            .shift_remove(name)
            .ok_or_else(|| StoreError::ResourceNotFound(name.to_string()))
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), StoreError> {
        self.config
            .resources
            .get_mut(name)
            .ok_or_else(|| StoreError::ResourceNotFound(name.to_string()))?
            .enabled = enabled;
        Ok(())
    }

    pub fn resource_count(&self) -> usize {
        self.config.resources.len()
    }

    // === Change detection ===

    pub fn fingerprint(&self) -> u64 {
        let yaml = self.config.to_yaml().unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        yaml.hash(&mut hasher);
        hasher.finish()
    }

    // === Full config access ===

    pub fn config(&self) -> &DtxConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut DtxConfig {
        &mut self.config
    }
}

// === File locking ===

/// RAII file lock using POSIX flock.
struct FileLock {
    #[allow(dead_code)]
    file: std::fs::File,
}

impl FileLock {
    fn acquire(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(StoreError::Io)?;

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if ret != 0 {
                return Err(StoreError::Io(std::io::Error::last_os_error()));
            }
        }

        Ok(Self { file })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            unsafe {
                libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

/// Atomic file write: write to .tmp then rename.
fn atomic_write(path: &Path, content: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
    }
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, content).map_err(StoreError::Io)?;
    std::fs::rename(&tmp, path).map_err(StoreError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_config() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::init(dir.path().to_path_buf(), "test-project").unwrap();
        assert_eq!(store.project_name(), "test-project");
        assert!(dir.path().join(".dtx/config.yaml").exists());
    }

    #[test]
    fn add_and_remove_resource() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::init(dir.path().to_path_buf(), "test").unwrap();

        let config = ResourceConfig {
            command: Some("echo hello".to_string()),
            ..Default::default()
        };

        store.add_resource("api", config).unwrap();
        assert_eq!(store.resource_count(), 1);
        assert!(store.get_resource("api").is_some());

        // Duplicate should fail
        let dup = ResourceConfig::default();
        assert!(store.add_resource("api", dup).is_err());

        store.remove_resource("api").unwrap();
        assert_eq!(store.resource_count(), 0);

        // Remove non-existent should fail
        assert!(store.remove_resource("api").is_err());
    }

    #[test]
    fn save_and_reload() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::init(dir.path().to_path_buf(), "test").unwrap();

        store.add_resource("api", ResourceConfig {
            command: Some("npm start".to_string()),
            port: Some(3000),
            ..Default::default()
        }).unwrap();
        store.save().unwrap();

        // Reload and verify
        let mut store2 = ConfigStore::load(dir.path().join(".dtx/config.yaml")).unwrap();
        store2.reload().unwrap();
        assert_eq!(store2.resource_count(), 1);
        let api = store2.get_resource("api").unwrap();
        assert_eq!(api.port, Some(3000));
    }

    #[test]
    fn deterministic_serialization() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::init(dir.path().to_path_buf(), "test").unwrap();

        store.add_resource("api", ResourceConfig {
            command: Some("npm start".to_string()),
            port: Some(3000),
            ..Default::default()
        }).unwrap();
        store.add_resource("db", ResourceConfig {
            command: Some("postgres".to_string()),
            port: Some(5432),
            ..Default::default()
        }).unwrap();

        store.save().unwrap();
        let content1 = std::fs::read_to_string(dir.path().join(".dtx/config.yaml")).unwrap();

        store.save().unwrap();
        let content2 = std::fs::read_to_string(dir.path().join(".dtx/config.yaml")).unwrap();

        assert_eq!(content1, content2, "Serialization must be deterministic");
    }

    #[test]
    fn enabled_resources_filter() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::init(dir.path().to_path_buf(), "test").unwrap();

        store.add_resource("api", ResourceConfig {
            command: Some("npm start".to_string()),
            enabled: true,
            ..Default::default()
        }).unwrap();
        store.add_resource("worker", ResourceConfig {
            command: Some("worker run".to_string()),
            enabled: false,
            ..Default::default()
        }).unwrap();

        let enabled: Vec<_> = store.list_enabled_resources().collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].0, "api");
    }
}
