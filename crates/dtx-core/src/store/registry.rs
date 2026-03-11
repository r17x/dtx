//! Registry of known projects stored at `~/.config/dtx/known-projects.yaml`.
//!
//! Tracks which project directories have been initialized with dtx,
//! enabling multi-project coordination features like cross-project
//! port conflict detection and project listing.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryData {
    #[serde(default)]
    projects: Vec<String>, // paths as strings
}

/// Registry of known dtx projects.
///
/// Stores project paths in `~/.config/dtx/known-projects.yaml`.
/// Used for multi-project coordination (e.g., cross-project port
/// conflict detection, global project listing).
pub struct ProjectRegistry {
    path: PathBuf,
    data: RegistryData,
}

impl ProjectRegistry {
    /// Load the registry from `~/.config/dtx/known-projects.yaml`.
    ///
    /// Creates an empty registry if the file does not exist.
    pub fn load() -> std::io::Result<Self> {
        let path = crate::config::project::global_dtx_dir()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")
            })?
            .join("known-projects.yaml");
        let data = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_yaml::from_str(&content).unwrap_or_default()
        } else {
            RegistryData::default()
        };
        Ok(Self { path, data })
    }

    /// Register a project directory as known.
    ///
    /// No-op if the path is already registered.
    pub fn register(&mut self, project_root: &Path) {
        let path_str = project_root.to_string_lossy().to_string();
        if !self.data.projects.contains(&path_str) {
            self.data.projects.push(path_str);
        }
    }

    /// Remove a project directory from the registry.
    pub fn unregister(&mut self, project_root: &Path) {
        let path_str = project_root.to_string_lossy().to_string();
        self.data.projects.retain(|p| p != &path_str);
    }

    /// List all known project directories.
    pub fn known_projects(&self) -> Vec<PathBuf> {
        self.data.projects.iter().map(PathBuf::from).collect()
    }

    /// Persist the registry to disk.
    ///
    /// Creates parent directories if they do not exist.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml::to_string(&self.data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&self.path, yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn register_and_list() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("known-projects.yaml");
        let mut registry = ProjectRegistry {
            path: registry_path,
            data: RegistryData::default(),
        };

        let project_a = Path::new("/home/user/project-a");
        let project_b = Path::new("/home/user/project-b");

        registry.register(project_a);
        registry.register(project_b);
        assert_eq!(registry.known_projects().len(), 2);

        // Duplicate registration is a no-op
        registry.register(project_a);
        assert_eq!(registry.known_projects().len(), 2);
    }

    #[test]
    fn unregister_removes_project() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("known-projects.yaml");
        let mut registry = ProjectRegistry {
            path: registry_path,
            data: RegistryData::default(),
        };

        let project = Path::new("/home/user/project");
        registry.register(project);
        assert_eq!(registry.known_projects().len(), 1);

        registry.unregister(project);
        assert_eq!(registry.known_projects().len(), 0);
    }

    #[test]
    fn save_and_reload() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("known-projects.yaml");

        let mut registry = ProjectRegistry {
            path: registry_path.clone(),
            data: RegistryData::default(),
        };
        registry.register(Path::new("/tmp/my-project"));
        registry.save().unwrap();

        // Reload from disk
        let content = std::fs::read_to_string(&registry_path).unwrap();
        let data: RegistryData = serde_yaml::from_str(&content).unwrap();
        assert_eq!(data.projects, vec!["/tmp/my-project".to_string()]);
    }
}
