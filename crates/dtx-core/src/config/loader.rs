//! Configuration loading with hierarchical merging.
//!
//! Precedence (highest to lowest):
//! 1. Project: .dtx/config.yaml
//! 2. Global: ~/.config/dtx/config.yaml
//! 3. System: /etc/dtx/config.yaml

use std::path::{Path, PathBuf};
use tracing::debug;

use super::schema::{DtxConfig, GlobalConfig, SchemaError};

/// Configuration levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigLevel {
    /// System-wide configuration (/etc/dtx/config.yaml).
    System,
    /// User global configuration (~/.config/dtx/config.yaml).
    Global,
    /// Project-specific configuration (.dtx/config.yaml).
    Project,
}

impl ConfigLevel {
    /// Get default path for this level.
    pub fn default_path(&self) -> Option<PathBuf> {
        match self {
            Self::System => Some(PathBuf::from("/etc/dtx/config.yaml")),
            Self::Global => dirs::config_dir().map(|p| p.join("dtx/config.yaml")),
            Self::Project => None,
        }
    }

    /// Get all levels in order of precedence (lowest to highest).
    pub fn all() -> &'static [ConfigLevel] {
        &[Self::System, Self::Global, Self::Project]
    }
}

/// Configuration loader with hierarchical merging.
pub struct ConfigLoader {
    system_path: Option<PathBuf>,
    global_path: Option<PathBuf>,
    project_path: Option<PathBuf>,
}

impl ConfigLoader {
    /// Create a loader with default paths.
    pub fn new() -> Self {
        Self {
            system_path: ConfigLevel::System.default_path(),
            global_path: ConfigLevel::Global.default_path(),
            project_path: None,
        }
    }

    /// Set custom system config path.
    pub fn with_system_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.system_path = Some(path.into());
        self
    }

    /// Set custom global config path.
    pub fn with_global_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.global_path = Some(path.into());
        self
    }

    /// Set project config path explicitly.
    pub fn with_project_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.project_path = Some(path.into());
        self
    }

    /// Discover project path by walking up directory tree.
    pub fn discover_project(&mut self, start: &Path) -> Option<PathBuf> {
        let mut current = start.to_path_buf();

        if let Ok(canonical) = current.canonicalize() {
            current = canonical;
        }

        loop {
            let config_path = current.join(".dtx/config.yaml");
            if config_path.exists() {
                debug!(?config_path, "Found project config");
                self.project_path = Some(config_path.clone());
                return Some(config_path);
            }

            if !current.pop() {
                break;
            }
        }

        debug!("No project config found");
        None
    }

    /// Discover project from current working directory.
    pub fn discover_project_cwd(&mut self) -> Option<PathBuf> {
        std::env::current_dir()
            .ok()
            .and_then(|cwd| self.discover_project(&cwd))
    }

    /// Load merged configuration from all levels.
    pub fn load(&self) -> Result<DtxConfig, SchemaError> {
        let mut config = DtxConfig::new();

        if let Some(path) = &self.system_path {
            if path.exists() {
                debug!(?path, "Loading system config");
                let system = DtxConfig::load(path)?;
                config = merge_config(config, system);
            }
        }

        if let Some(path) = &self.global_path {
            if path.exists() {
                debug!(?path, "Loading global config");
                let global = DtxConfig::load(path)?;
                config = merge_config(config, global);
            }
        }

        if let Some(path) = &self.project_path {
            if path.exists() {
                debug!(?path, "Loading project config");
                let project = DtxConfig::load(path)?;
                config = merge_config(config, project);
            }
        }

        Ok(config)
    }

    /// Load config at a specific level only.
    pub fn load_level(&self, level: ConfigLevel) -> Result<Option<DtxConfig>, SchemaError> {
        let path = match level {
            ConfigLevel::System => self.system_path.as_ref(),
            ConfigLevel::Global => self.global_path.as_ref(),
            ConfigLevel::Project => self.project_path.as_ref(),
        };

        match path {
            Some(p) if p.exists() => {
                debug!(?p, ?level, "Loading config at level");
                Ok(Some(DtxConfig::load(p)?))
            }
            _ => Ok(None),
        }
    }

    /// Save config at a specific level.
    pub fn save_level(&self, config: &DtxConfig, level: ConfigLevel) -> Result<(), SchemaError> {
        let path = match level {
            ConfigLevel::System => self.system_path.as_ref(),
            ConfigLevel::Global => self.global_path.as_ref(),
            ConfigLevel::Project => self.project_path.as_ref(),
        };

        match path {
            Some(p) => {
                debug!(?p, ?level, "Saving config at level");
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                config.save(p)
            }
            None => Err(SchemaError::Validation(format!(
                "no path configured for {:?} level",
                level
            ))),
        }
    }

    /// Get the path for a specific level.
    pub fn path_for_level(&self, level: ConfigLevel) -> Option<&PathBuf> {
        match level {
            ConfigLevel::System => self.system_path.as_ref(),
            ConfigLevel::Global => self.global_path.as_ref(),
            ConfigLevel::Project => self.project_path.as_ref(),
        }
    }

    /// Check if a config exists at the given level.
    pub fn exists_at_level(&self, level: ConfigLevel) -> bool {
        self.path_for_level(level)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Get all levels where config exists.
    pub fn existing_levels(&self) -> Vec<ConfigLevel> {
        ConfigLevel::all()
            .iter()
            .filter(|&&level| self.exists_at_level(level))
            .copied()
            .collect()
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Merge two configs (overlay values override base values).
fn merge_config(base: DtxConfig, overlay: DtxConfig) -> DtxConfig {
    DtxConfig {
        version: if overlay.version.is_empty() {
            base.version
        } else {
            overlay.version
        },
        project: if overlay.project.name.is_empty() {
            base.project
        } else {
            overlay.project
        },
        settings: merge_settings(base.settings, overlay.settings),
        resources: {
            let mut merged = base.resources;
            merged.extend(overlay.resources);
            merged
        },
        defaults: overlay.defaults.or(base.defaults),
        nix: merge_nix_config(base.nix, overlay.nix),
        ai: overlay.ai.or(base.ai),
    }
}

/// Merge settings (overlay overrides base).
fn merge_settings(base: GlobalConfig, overlay: GlobalConfig) -> GlobalConfig {
    GlobalConfig {
        log_level: overlay.log_level,
        health_check_interval: overlay.health_check_interval,
        shutdown_timeout: overlay.shutdown_timeout,
        log_dir: overlay.log_dir.or(base.log_dir),
        use_uds: overlay.use_uds,
        auto_resolve_ports: overlay.auto_resolve_ports,
    }
}

/// Merge global nix configurations.
fn merge_nix_config(
    base: Option<super::schema::GlobalNixConfig>,
    overlay: Option<super::schema::GlobalNixConfig>,
) -> Option<super::schema::GlobalNixConfig> {
    match (base, overlay) {
        (None, None) => None,
        (Some(b), None) => Some(b),
        (None, Some(o)) => Some(o),
        (Some(mut b), Some(o)) => {
            b.mappings.extend(o.mappings);
            Some(b)
        }
    }
}

/// Helper to create a config loader and load merged config in one step.
pub fn load_config() -> Result<DtxConfig, SchemaError> {
    let mut loader = ConfigLoader::new();
    loader.discover_project_cwd();
    loader.load()
}

/// Helper to load config from a specific project directory.
pub fn load_config_from(project_dir: &Path) -> Result<DtxConfig, SchemaError> {
    let mut loader = ConfigLoader::new();
    loader.discover_project(project_dir);
    loader.load()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_discovery() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("myproject");
        std::fs::create_dir_all(project.join(".dtx")).unwrap();
        std::fs::write(project.join(".dtx/config.yaml"), "project:\n  name: test").unwrap();

        let mut loader = ConfigLoader::new();
        let found = loader.discover_project(&project);
        assert!(found.is_some());
        assert!(found.unwrap().ends_with("config.yaml"));
    }

    #[test]
    fn test_config_merging() {
        let base = DtxConfig {
            settings: GlobalConfig {
                log_level: "info".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let overlay = DtxConfig {
            settings: GlobalConfig {
                log_level: "debug".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let merged = merge_config(base, overlay);
        assert_eq!(merged.settings.log_level, "debug");
    }

    #[test]
    fn test_save_level() {
        let temp = tempdir().unwrap();
        let project_config = temp.path().join(".dtx/config.yaml");

        let loader = ConfigLoader::new().with_project_path(&project_config);

        let config = DtxConfig::with_project_name("saved-project");
        loader.save_level(&config, ConfigLevel::Project).unwrap();

        assert!(project_config.exists());

        let loaded = DtxConfig::load(&project_config).unwrap();
        assert_eq!(loaded.project.name, "saved-project");
    }
}
