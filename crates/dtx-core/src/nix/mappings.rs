//! Dynamic command-to-package mapping system.
//!
//! Supports layered configuration:
//! 1. Built-in defaults (compiled)
//! 2. User-level overrides (~/.config/dtx/mappings.toml)
//! 3. Project-level overrides (.dtx/mappings.toml)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Package mapping configuration file format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MappingsConfig {
    /// Command-to-package mappings.
    /// Key: executable name, Value: nix package name
    #[serde(default)]
    pub mappings: HashMap<String, String>,

    /// Commands that should be treated as local binaries (no package needed).
    #[serde(default)]
    pub local_binaries: Vec<String>,

    /// Commands to ignore (don't warn about).
    #[serde(default)]
    pub ignore: Vec<String>,
}

impl MappingsConfig {
    /// Loads config from a TOML file.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }

        match std::fs::read_to_string(path) {
            Ok(content) => match toml_parse(&content) {
                Ok(config) => {
                    debug!(?path, "Loaded mappings config");
                    Some(config)
                }
                Err(e) => {
                    debug!(?path, error = %e, "Failed to parse mappings config");
                    None
                }
            },
            Err(e) => {
                debug!(?path, error = %e, "Failed to read mappings config");
                None
            }
        }
    }

    /// Merges another config into this one (other takes precedence).
    pub fn merge(&mut self, other: MappingsConfig) {
        self.mappings.extend(other.mappings);
        self.local_binaries.extend(other.local_binaries);
        self.ignore.extend(other.ignore);
    }

    /// Parses a mappings config from a TOML string.
    ///
    /// Wraps the internal parser for use by external callers (e.g., web validation).
    pub fn parse(content: &str) -> Result<Self, String> {
        toml_parse(content)
    }

    /// Creates an example config for users.
    pub fn example() -> String {
        r#"# DTX Command-to-Package Mappings
# Place this file at:
#   - ~/.config/dtx/mappings.toml (user-level)
#   - .dtx/mappings.toml (project-level)

# Custom command-to-package mappings
# Format: "command" = "package"
[mappings]
# Example: map 'mycompany-cli' to a custom package
# mycompany-cli = "mycompany.cli"

# Node.js alternatives
bun = "bun"
deno = "deno"

# Your custom tools
# my-tool = "my-tool-pkg"

# Commands that are local binaries (no package needed)
# These won't trigger "needs attention" warnings
local_binaries = [
    # "./scripts/*",  # Patterns not supported yet, list explicitly
]

# Commands to ignore completely (no warnings)
ignore = [
    # "legacy-tool",
]
"#
        .to_string()
    }
}

/// Simple TOML parser for mappings config.
/// Using manual parsing to avoid adding toml crate dependency.
fn toml_parse(content: &str) -> Result<MappingsConfig, String> {
    let mut config = MappingsConfig::default();
    let mut in_mappings = false;

    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Section headers
        if line == "[mappings]" {
            in_mappings = true;
            continue;
        }

        // Handle array assignments
        if line.starts_with("local_binaries") {
            in_mappings = false;
            // Parse inline array if present
            if let Some(arr) = parse_string_array(line) {
                config.local_binaries = arr;
            }
            continue;
        }

        if line.starts_with("ignore") {
            in_mappings = false;
            // Parse inline array if present
            if let Some(arr) = parse_string_array(line) {
                config.ignore = arr;
            }
            continue;
        }

        // Parse key-value pairs in [mappings] section
        if in_mappings {
            if let Some((key, value)) = parse_key_value(line) {
                config.mappings.insert(key, value);
            }
        }
    }

    Ok(config)
}

/// Parses a TOML key = "value" line.
fn parse_key_value(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }

    let key = parts[0].trim().trim_matches('"');
    let value = parts[1].trim().trim_matches('"');

    if key.is_empty() || value.is_empty() {
        return None;
    }

    Some((key.to_string(), value.to_string()))
}

/// Parses a TOML array like: key = ["a", "b", "c"]
fn parse_string_array(line: &str) -> Option<Vec<String>> {
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

/// Manages layered package mappings.
pub struct PackageMappings {
    /// Combined mappings from all layers.
    mappings: HashMap<String, String>,
    /// Commands treated as local binaries.
    local_binaries: Vec<String>,
    /// Commands to ignore.
    ignore: Vec<String>,
}

impl PackageMappings {
    /// Creates a new PackageMappings with built-in defaults.
    pub fn new() -> Self {
        Self {
            mappings: super::command::get_package_mappings()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            local_binaries: Vec::new(),
            ignore: Vec::new(),
        }
    }

    /// Loads mappings with user and project overrides.
    pub fn load() -> Self {
        let mut mappings = Self::new();

        // Load user-level config
        if let Some(user_config) = Self::user_config_path() {
            if let Some(config) = MappingsConfig::load_from_file(&user_config) {
                mappings.apply_config(config);
            }
        }

        // Load project-level config
        if let Some(project_config) = Self::project_config_path() {
            if let Some(config) = MappingsConfig::load_from_file(&project_config) {
                mappings.apply_config(config);
            }
        }

        mappings
    }

    /// Loads mappings for a specific project directory.
    pub fn load_for_project(project_path: &Path) -> Self {
        let mut mappings = Self::new();

        // Load user-level config
        if let Some(user_config) = Self::user_config_path() {
            if let Some(config) = MappingsConfig::load_from_file(&user_config) {
                mappings.apply_config(config);
            }
        }

        // Load project-level config
        let project_config = project_path.join(".dtx").join("mappings.toml");
        if let Some(config) = MappingsConfig::load_from_file(&project_config) {
            mappings.apply_config(config);
        }

        mappings
    }

    /// Applies a config, overriding existing values.
    fn apply_config(&mut self, config: MappingsConfig) {
        self.mappings.extend(config.mappings);
        self.local_binaries.extend(config.local_binaries);
        self.ignore.extend(config.ignore);
    }

    /// Gets the user config path (~/.config/dtx/mappings.toml).
    fn user_config_path() -> Option<PathBuf> {
        dirs_next().map(|p| p.join("dtx").join("mappings.toml"))
    }

    /// Gets the project config path (.dtx/mappings.toml).
    fn project_config_path() -> Option<PathBuf> {
        std::env::current_dir()
            .ok()
            .map(|p| p.join(".dtx").join("mappings.toml"))
    }

    /// Looks up a package for an executable.
    pub fn get_package(&self, executable: &str) -> Option<&String> {
        self.mappings.get(executable)
    }

    /// Checks if a command should be treated as a local binary.
    pub fn is_local_binary(&self, executable: &str) -> bool {
        self.local_binaries.iter().any(|lb| lb == executable)
    }

    /// Checks if a command should be ignored (no warnings).
    pub fn is_ignored(&self, executable: &str) -> bool {
        self.ignore.iter().any(|i| i == executable)
    }

    /// Returns the total number of mappings.
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    /// Checks if mappings are empty.
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// Adds a custom mapping (for runtime additions).
    pub fn add_mapping(&mut self, executable: String, package: String) {
        self.mappings.insert(executable, package);
    }

    /// Gets all mappings (for debugging/display).
    pub fn all_mappings(&self) -> &HashMap<String, String> {
        &self.mappings
    }
}

impl Default for PackageMappings {
    fn default() -> Self {
        Self::new()
    }
}

/// Gets the config directory path.
/// Returns ~/.config on Unix, appropriate path on other platforms.
fn dirs_next() -> Option<PathBuf> {
    // Simple implementation - just use ~/.config
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}

/// Initializes user config directory with example mappings file.
pub fn init_user_config() -> std::io::Result<PathBuf> {
    let config_dir = dirs_next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No home directory"))?
        .join("dtx");

    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("mappings.toml");
    if !config_path.exists() {
        std::fs::write(&config_path, MappingsConfig::example())?;
    }

    Ok(config_path)
}

/// Initializes project config directory with example mappings file.
pub fn init_project_config(project_path: &Path) -> std::io::Result<PathBuf> {
    let config_dir = project_path.join(".dtx");
    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("mappings.toml");
    if !config_path.exists() {
        std::fs::write(&config_path, MappingsConfig::example())?;
    }

    Ok(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_value() {
        assert_eq!(
            parse_key_value("node = \"nodejs\""),
            Some(("node".to_string(), "nodejs".to_string()))
        );
        assert_eq!(
            parse_key_value("my-tool = \"my-pkg\""),
            Some(("my-tool".to_string(), "my-pkg".to_string()))
        );
        assert_eq!(parse_key_value("invalid"), None);
    }

    #[test]
    fn test_parse_string_array() {
        assert_eq!(
            parse_string_array("local_binaries = [\"a\", \"b\", \"c\"]"),
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
        assert_eq!(parse_string_array("ignore = []"), Some(vec![]));
    }

    #[test]
    fn test_toml_parse() {
        let content = r#"
[mappings]
custom-tool = "custom-pkg"
another = "another-pkg"

local_binaries = ["./run.sh"]
ignore = ["legacy"]
"#;

        let config = toml_parse(content).unwrap();
        assert_eq!(
            config.mappings.get("custom-tool"),
            Some(&"custom-pkg".to_string())
        );
        assert_eq!(
            config.mappings.get("another"),
            Some(&"another-pkg".to_string())
        );
        assert_eq!(config.local_binaries, vec!["./run.sh"]);
        assert_eq!(config.ignore, vec!["legacy"]);
    }

    #[test]
    fn test_package_mappings_defaults() {
        let mappings = PackageMappings::new();
        assert!(mappings.len() > 50); // Should have many built-in mappings
        assert_eq!(mappings.get_package("node"), Some(&"nodejs".to_string()));
        assert_eq!(
            mappings.get_package("python3"),
            Some(&"python3".to_string())
        );
    }

    #[test]
    fn test_package_mappings_add_custom() {
        let mut mappings = PackageMappings::new();
        mappings.add_mapping("my-tool".to_string(), "my-package".to_string());
        assert_eq!(
            mappings.get_package("my-tool"),
            Some(&"my-package".to_string())
        );
    }

    #[test]
    fn test_mappings_config_example() {
        let example = MappingsConfig::example();
        assert!(example.contains("[mappings]"));
        assert!(example.contains("local_binaries"));
        assert!(example.contains("ignore"));
    }
}
