//! Project configuration management.
//!
//! Unified configuration for dtx projects stored in `.dtx/config.toml`.
//!
//! # Directory Discovery
//!
//! dtx finds the project root by searching for `.dtx` directory:
//! 1. Start from current working directory
//! 2. Search upward through parent directories
//! 3. Stop when `.dtx` is found or filesystem root is reached
//! 4. Fall back to global `~/.dtx` for user-level config
//!
//! This is similar to how git finds `.git`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Main project configuration file name.
pub const CONFIG_FILE: &str = "config.toml";

/// DTX directory name.
pub const DTX_DIR: &str = ".dtx";

/// Global DTX directory in user's home.
pub const GLOBAL_DTX_DIR: &str = ".dtx";

/// Result of project discovery.
#[derive(Debug, Clone)]
pub struct DiscoveredProject {
    /// Path to the project root (parent of .dtx).
    pub root: PathBuf,
    /// Path to the .dtx directory.
    pub dtx_dir: PathBuf,
    /// Whether this is the global config.
    pub is_global: bool,
}

/// Finds the .dtx directory by searching upward from the given path.
///
/// Returns the project root directory (parent of .dtx) if found.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();

    // Canonicalize to handle symlinks and relative paths
    if let Ok(canonical) = current.canonicalize() {
        current = canonical;
    }

    loop {
        let dtx_path = current.join(DTX_DIR);
        if dtx_path.is_dir() {
            debug!(?current, "Found .dtx directory");
            return Some(current);
        }

        // Move to parent directory
        if !current.pop() {
            break;
        }
    }

    None
}

/// Finds the .dtx directory starting from current working directory.
pub fn find_project_root_cwd() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| find_project_root(&cwd))
}

/// Gets the global dtx config directory (~/.dtx).
pub fn global_dtx_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(GLOBAL_DTX_DIR))
}

/// Discovers the project to use, with fallback to global config.
///
/// Search order:
/// 1. Search upward from current directory for .dtx
/// 2. Fall back to ~/.dtx if no project found
pub fn discover_project() -> Option<DiscoveredProject> {
    // First, try to find a project by searching upward
    if let Some(root) = find_project_root_cwd() {
        return Some(DiscoveredProject {
            dtx_dir: root.join(DTX_DIR),
            root,
            is_global: false,
        });
    }

    // Fall back to global config
    if let Some(global) = global_dtx_dir() {
        if global.is_dir() {
            debug!("Using global .dtx directory");
            return Some(DiscoveredProject {
                dtx_dir: global.clone(),
                root: global,
                is_global: true,
            });
        }
    }

    None
}

/// Discovers project from a specific starting path.
pub fn discover_project_from(start: &Path) -> Option<DiscoveredProject> {
    if let Some(root) = find_project_root(start) {
        return Some(DiscoveredProject {
            dtx_dir: root.join(DTX_DIR),
            root,
            is_global: false,
        });
    }

    // Fall back to global config
    if let Some(global) = global_dtx_dir() {
        if global.is_dir() {
            return Some(DiscoveredProject {
                dtx_dir: global.clone(),
                root: global,
                is_global: true,
            });
        }
    }

    None
}

/// Complete project configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project metadata.
    #[serde(default)]
    pub project: ProjectMeta,

    /// Package mappings (command → package).
    #[serde(default)]
    pub mappings: MappingsSection,

    /// Service overrides and defaults.
    #[serde(default)]
    pub services: ServicesSection,

    /// Runtime settings.
    #[serde(default)]
    pub runtime: RuntimeSection,
}

/// Project metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectMeta {
    /// Project name.
    pub name: Option<String>,
    /// Project description.
    pub description: Option<String>,
}

/// Package mappings configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MappingsSection {
    /// Custom command-to-package mappings.
    #[serde(default)]
    pub packages: HashMap<String, String>,

    /// Commands treated as local binaries (no package needed).
    #[serde(default)]
    pub local: Vec<String>,

    /// Commands to ignore (suppress warnings).
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Service configuration section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServicesSection {
    /// Default environment variables for all services.
    #[serde(default)]
    pub default_env: HashMap<String, String>,

    /// Default working directory.
    pub default_working_dir: Option<String>,
}

/// Runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSection {
    /// Use Unix Domain Socket instead of TCP.
    #[serde(default = "default_true")]
    pub use_uds: bool,

    /// Auto-resolve port conflicts.
    #[serde(default = "default_true")]
    pub auto_resolve_ports: bool,

    /// Log level (trace, debug, info, warn, error).
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for RuntimeSection {
    fn default() -> Self {
        Self {
            use_uds: true,
            auto_resolve_ports: true,
            log_level: "info".to_string(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

impl ProjectConfig {
    /// Creates a new empty config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates config with project name.
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            project: ProjectMeta {
                name: Some(name.into()),
                description: None,
            },
            ..Default::default()
        }
    }

    /// Gets the .dtx directory path for a project.
    pub fn dtx_dir(project_path: &Path) -> PathBuf {
        project_path.join(DTX_DIR)
    }

    /// Gets the config file path for a project.
    pub fn config_path(project_path: &Path) -> PathBuf {
        Self::dtx_dir(project_path).join(CONFIG_FILE)
    }

    /// Loads config from a project directory.
    pub fn load(project_path: &Path) -> Result<Self, ConfigError> {
        let config_path = Self::config_path(project_path);

        if !config_path.exists() {
            debug!(?config_path, "No config file, using defaults");
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| ConfigError::ReadError(config_path.clone(), e.to_string()))?;

        Self::parse(&content).map_err(|e| ConfigError::ParseError(config_path, e))
    }

    /// Saves config to a project directory.
    pub fn save(&self, project_path: &Path) -> Result<(), ConfigError> {
        let dtx_dir = Self::dtx_dir(project_path);
        std::fs::create_dir_all(&dtx_dir)
            .map_err(|e| ConfigError::WriteError(dtx_dir.clone(), e.to_string()))?;

        let config_path = Self::config_path(project_path);
        let content = self.to_toml();

        std::fs::write(&config_path, &content)
            .map_err(|e| ConfigError::WriteError(config_path.clone(), e.to_string()))?;

        info!(?config_path, "Saved project config");
        Ok(())
    }

    /// Parses config from TOML string.
    pub fn parse(content: &str) -> Result<Self, String> {
        parse_config_toml(content)
    }

    /// Serializes config to TOML string.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();

        // Project section
        out.push_str("# DTX Project Configuration\n");
        out.push_str("# Edit this file or use `dtx config` commands\n\n");

        out.push_str("[project]\n");
        if let Some(ref name) = self.project.name {
            out.push_str(&format!("name = \"{}\"\n", name));
        }
        if let Some(ref desc) = self.project.description {
            out.push_str(&format!("description = \"{}\"\n", desc));
        }
        out.push('\n');

        // Mappings section
        out.push_str("[mappings]\n");
        out.push_str("# Custom command-to-package mappings\n");
        out.push_str("# Format: \"command\" = \"package\"\n");
        if !self.mappings.packages.is_empty() {
            out.push_str("[mappings.packages]\n");
            for (cmd, pkg) in &self.mappings.packages {
                out.push_str(&format!("\"{}\" = \"{}\"\n", cmd, pkg));
            }
        }
        if !self.mappings.local.is_empty() {
            out.push_str(&format!("local = {:?}\n", self.mappings.local));
        }
        if !self.mappings.ignore.is_empty() {
            out.push_str(&format!("ignore = {:?}\n", self.mappings.ignore));
        }
        out.push('\n');

        // Runtime section
        out.push_str("[runtime]\n");
        out.push_str(&format!("use_uds = {}\n", self.runtime.use_uds));
        out.push_str(&format!(
            "auto_resolve_ports = {}\n",
            self.runtime.auto_resolve_ports
        ));
        out.push_str(&format!("log_level = \"{}\"\n", self.runtime.log_level));

        out
    }

    /// Generates an example config file.
    pub fn example() -> String {
        r#"# DTX Project Configuration
# This file controls how dtx manages your development environment.
# Location: .dtx/config.toml

[project]
name = "my-project"
description = "My awesome project"

[mappings]
# Custom command-to-package mappings
# When a service uses a command, dtx auto-detects the nix package.
# Add custom mappings here for tools not in the built-in list.

[mappings.packages]
# "my-tool" = "my-nix-package"
# "company-cli" = "company.packages.cli"

# Commands that are local binaries (no nix package needed)
local = [
    # "./scripts/start.sh",
    # "./bin/server",
]

# Commands to ignore (no warnings for unknown commands)
ignore = [
    # "proprietary-tool",
]

[services]
# Default environment variables for all services
[services.default_env]
# NODE_ENV = "development"
# LOG_LEVEL = "debug"

# Default working directory (relative to project root)
# default_working_dir = "."

[runtime]
# Use Unix Domain Socket for process-compose (faster, recommended)
use_uds = true

# Automatically reassign ports when conflicts are detected
auto_resolve_ports = true

# Log level: trace, debug, info, warn, error
log_level = "info"
"#
        .to_string()
    }

    // === Mapping helpers ===

    /// Adds a package mapping.
    pub fn add_mapping(&mut self, command: impl Into<String>, package: impl Into<String>) {
        self.mappings
            .packages
            .insert(command.into(), package.into());
    }

    /// Removes a package mapping.
    pub fn remove_mapping(&mut self, command: &str) -> Option<String> {
        self.mappings.packages.remove(command)
    }

    /// Gets a package mapping.
    pub fn get_mapping(&self, command: &str) -> Option<&String> {
        self.mappings.packages.get(command)
    }

    /// Adds a command to the local binaries list.
    pub fn add_local(&mut self, command: impl Into<String>) {
        let cmd = command.into();
        if !self.mappings.local.contains(&cmd) {
            self.mappings.local.push(cmd);
        }
    }

    /// Adds a command to the ignore list.
    pub fn add_ignore(&mut self, command: impl Into<String>) {
        let cmd = command.into();
        if !self.mappings.ignore.contains(&cmd) {
            self.mappings.ignore.push(cmd);
        }
    }

    /// Checks if a command is in the local binaries list.
    pub fn is_local(&self, command: &str) -> bool {
        self.mappings.local.iter().any(|c| c == command)
    }

    /// Checks if a command is in the ignore list.
    pub fn is_ignored(&self, command: &str) -> bool {
        self.mappings.ignore.iter().any(|c| c == command)
    }
}

/// Configuration errors.
#[derive(Debug)]
pub enum ConfigError {
    /// Failed to read config file.
    ReadError(PathBuf, String),
    /// Failed to parse config file.
    ParseError(PathBuf, String),
    /// Failed to write config file.
    WriteError(PathBuf, String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::ReadError(path, e) => {
                write!(f, "Failed to read {}: {}", path.display(), e)
            }
            ConfigError::ParseError(path, e) => {
                write!(f, "Failed to parse {}: {}", path.display(), e)
            }
            ConfigError::WriteError(path, e) => {
                write!(f, "Failed to write {}: {}", path.display(), e)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Simple TOML parser for project config.
fn parse_config_toml(content: &str) -> Result<ProjectConfig, String> {
    let mut config = ProjectConfig::default();
    let mut current_section = "";

    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Section headers
        if line.starts_with('[') && line.ends_with(']') {
            current_section = &line[1..line.len() - 1];
            continue;
        }

        // Parse based on current section
        match current_section {
            "project" => {
                if let Some((key, value)) = parse_kv(line) {
                    match key.as_str() {
                        "name" => config.project.name = Some(value),
                        "description" => config.project.description = Some(value),
                        _ => {}
                    }
                }
            }
            "mappings" => {
                if line.starts_with("local") {
                    if let Some(arr) = parse_array(line) {
                        config.mappings.local = arr;
                    }
                } else if line.starts_with("ignore") {
                    if let Some(arr) = parse_array(line) {
                        config.mappings.ignore = arr;
                    }
                }
            }
            "mappings.packages" => {
                if let Some((key, value)) = parse_kv(line) {
                    config.mappings.packages.insert(key, value);
                }
            }
            "services.default_env" => {
                if let Some((key, value)) = parse_kv(line) {
                    config.services.default_env.insert(key, value);
                }
            }
            "services" => {
                if let Some((key, value)) = parse_kv(line) {
                    if key == "default_working_dir" {
                        config.services.default_working_dir = Some(value);
                    }
                }
            }
            "runtime" => {
                if let Some((key, value)) = parse_kv(line) {
                    match key.as_str() {
                        "use_uds" => config.runtime.use_uds = value == "true",
                        "auto_resolve_ports" => config.runtime.auto_resolve_ports = value == "true",
                        "log_level" => config.runtime.log_level = value,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(config)
}

fn parse_kv(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }
    let key = parts[0].trim().trim_matches('"');
    let value = parts[1].trim().trim_matches('"');
    Some((key.to_string(), value.to_string()))
}

fn parse_array(line: &str) -> Option<Vec<String>> {
    let start = line.find('[')?;
    let end = line.rfind(']')?;
    if start >= end {
        return None;
    }
    let arr_content = &line[start + 1..end];
    let items: Vec<String> = arr_content
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ProjectConfig::new();
        assert!(config.mappings.packages.is_empty());
        assert!(config.runtime.use_uds);
        assert!(config.runtime.auto_resolve_ports);
    }

    #[test]
    fn test_config_with_name() {
        let config = ProjectConfig::with_name("test-project");
        assert_eq!(config.project.name, Some("test-project".to_string()));
    }

    #[test]
    fn test_add_mapping() {
        let mut config = ProjectConfig::new();
        config.add_mapping("my-tool", "my-package");
        assert_eq!(
            config.get_mapping("my-tool"),
            Some(&"my-package".to_string())
        );
    }

    #[test]
    fn test_parse_config() {
        let content = r#"
[project]
name = "test"

[mappings.packages]
"custom-cli" = "custom-pkg"

[mappings]
local = ["./run.sh"]
ignore = ["legacy"]

[runtime]
use_uds = false
auto_resolve_ports = true
"#;

        let config = ProjectConfig::parse(content).unwrap();
        assert_eq!(config.project.name, Some("test".to_string()));
        assert_eq!(
            config.mappings.packages.get("custom-cli"),
            Some(&"custom-pkg".to_string())
        );
        assert_eq!(config.mappings.local, vec!["./run.sh"]);
        assert_eq!(config.mappings.ignore, vec!["legacy"]);
        assert!(!config.runtime.use_uds);
        assert!(config.runtime.auto_resolve_ports);
    }

    #[test]
    fn test_to_toml() {
        let mut config = ProjectConfig::with_name("test");
        config.add_mapping("foo", "bar");

        let toml = config.to_toml();
        assert!(toml.contains("name = \"test\""));
        assert!(toml.contains("[mappings]"));
    }

    #[test]
    fn test_find_project_root() {
        // Create a temp directory structure
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp
            .path()
            .join("my-project")
            .canonicalize()
            .unwrap_or_else(|_| {
                // Directory doesn't exist yet, create it first
                std::fs::create_dir_all(temp.path().join("my-project").join(".dtx")).unwrap();
                temp.path().join("my-project").canonicalize().unwrap()
            });
        let dtx_dir = project_root.join(".dtx");
        let subdir = project_root.join("src").join("deep");

        std::fs::create_dir_all(&dtx_dir).unwrap();
        std::fs::create_dir_all(&subdir).unwrap();

        // From project root should find itself
        let found = find_project_root(&project_root);
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().canonicalize().unwrap(),
            project_root.canonicalize().unwrap()
        );

        // From subdir should find project root
        let found = find_project_root(&subdir);
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().canonicalize().unwrap(),
            project_root.canonicalize().unwrap()
        );

        // From temp dir (no .dtx) should find nothing (temp has .dtx inside my-project, not at root)
        // Actually temp.path() will traverse up and find my-project/.dtx, so test differently
        let empty_temp = tempfile::tempdir().unwrap();
        let found = find_project_root(empty_temp.path());
        assert!(found.is_none());
    }

    #[test]
    fn test_discover_project() {
        // Create temp project
        let temp = tempfile::tempdir().unwrap();
        let dtx_dir = temp.path().join(".dtx");
        std::fs::create_dir_all(&dtx_dir).unwrap();

        // discover_project_from should find it
        let found = discover_project_from(temp.path());
        assert!(found.is_some());
        let project = found.unwrap();
        // Compare canonicalized paths to handle macOS /var -> /private/var
        assert_eq!(
            project.root.canonicalize().unwrap(),
            temp.path().canonicalize().unwrap()
        );
        assert!(!project.is_global);
    }

    #[test]
    fn test_global_dtx_dir() {
        let global = global_dtx_dir();
        // Should return Some if home dir exists
        if let Some(home) = dirs::home_dir() {
            assert!(global.is_some());
            assert_eq!(global.unwrap(), home.join(".dtx"));
        }
    }
}
