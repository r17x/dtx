//! Import types and traits.

use std::collections::HashMap;
use std::path::Path;

use super::error::ImportResult;

/// Supported import formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// process-compose.yaml (v1.x)
    ProcessCompose,
    /// docker-compose.yml (v3.x)
    DockerCompose,
    /// Procfile (Heroku-style)
    Procfile,
    /// Auto-detect format
    Auto,
}

impl ImportFormat {
    /// Detect format from file path.
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let name_lower = name.to_lowercase();

        if name_lower == "procfile" {
            return Some(Self::Procfile);
        }

        // Check docker-compose first (more specific)
        if name_lower.starts_with("docker-compose")
            || name_lower == "compose.yaml"
            || name_lower == "compose.yml"
        {
            return Some(Self::DockerCompose);
        }

        // Check process-compose
        if name_lower.starts_with("process-compose")
            && (name_lower.ends_with(".yaml")
                || name_lower.ends_with(".yml")
                || name_lower.ends_with(".json"))
        {
            return Some(Self::ProcessCompose);
        }

        None
    }

    /// Detect format from content.
    pub fn from_content(content: &str) -> Option<Self> {
        let content = content.trim();

        // JSON process-compose: starts with `{` and contains `"processes"`
        if content.trim_start().starts_with('{') && content.contains("\"processes\"") {
            return Some(Self::ProcessCompose);
        }

        // Procfile: lines like "web: command"
        if content.lines().all(|line| {
            let line = line.trim();
            line.is_empty() || line.starts_with('#') || line.contains(':')
        }) && !content.contains("version:")
            && !content.contains("services:")
            && !content.contains("processes:")
        {
            return Some(Self::Procfile);
        }

        // process-compose: has "processes:" key
        if content.contains("processes:") {
            return Some(Self::ProcessCompose);
        }

        // docker-compose: has "services:" key
        if content.contains("services:") {
            return Some(Self::DockerCompose);
        }

        None
    }
}

/// Trait for configuration importers.
pub trait Importer {
    /// Get the format this importer handles.
    fn format(&self) -> ImportFormat;

    /// Import configuration from string content.
    fn import(&self, content: &str) -> ImportResult<ImportedConfig>;

    /// Import configuration from file path.
    fn import_file(&self, path: &Path) -> ImportResult<ImportedConfig> {
        let content = std::fs::read_to_string(path)?;
        self.import(&content)
    }
}

/// Imported configuration result.
#[derive(Debug, Clone, Default)]
pub struct ImportedConfig {
    /// Project name (if detected).
    pub project_name: Option<String>,

    /// Imported resources.
    pub resources: Vec<ImportedResource>,

    /// Warnings generated during import.
    pub warnings: Vec<String>,
}

impl ImportedConfig {
    /// Create a new empty imported config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a resource.
    pub fn add_resource(&mut self, resource: ImportedResource) {
        self.resources.push(resource);
    }

    /// Add a warning.
    pub fn add_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }
}

/// An imported resource definition.
#[derive(Debug, Clone)]
pub struct ImportedResource {
    /// Resource name.
    pub name: String,

    /// Command to execute.
    pub command: Option<String>,

    /// Working directory.
    pub working_dir: Option<String>,

    /// Environment variables.
    pub environment: HashMap<String, String>,

    /// Dependencies.
    pub depends_on: Vec<String>,

    /// Port (if specified).
    pub port: Option<u16>,

    /// Container image (if container).
    pub image: Option<String>,

    /// Health check command.
    pub health_check: Option<String>,

    /// Restart policy.
    pub restart: Option<String>,

    /// Source line number (for error reporting).
    pub source_line: Option<usize>,

    /// Nix flake packages this resource needs (set by export_custom_scripts).
    pub nix_packages: Vec<String>,
}

impl ImportedResource {
    /// Create a new imported resource.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: None,
            working_dir: None,
            environment: HashMap::new(),
            depends_on: Vec::new(),
            port: None,
            image: None,
            health_check: None,
            restart: None,
            source_line: None,
            nix_packages: Vec::new(),
        }
    }

    /// Set command.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set working directory.
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Add environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    /// Add dependency.
    pub fn depends_on(mut self, dep: impl Into<String>) -> Self {
        self.depends_on.push(dep.into());
        self
    }

    /// Set port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set image.
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = Some(image.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_path_process_compose_json() {
        let path = Path::new("process-compose.json");
        assert_eq!(
            ImportFormat::from_path(path),
            Some(ImportFormat::ProcessCompose)
        );
    }

    #[test]
    fn from_path_process_compose_yaml() {
        let path = Path::new("process-compose.yaml");
        assert_eq!(
            ImportFormat::from_path(path),
            Some(ImportFormat::ProcessCompose)
        );
    }

    #[test]
    fn from_content_json_processes() {
        let content = r#"{ "processes": { "api": { "command": "npm start" } } }"#;
        assert_eq!(
            ImportFormat::from_content(content),
            Some(ImportFormat::ProcessCompose)
        );
    }

    #[test]
    fn from_content_json_no_processes() {
        let content = r#"{ "services": { "web": {} } }"#;
        // JSON with "services" but no "processes" should not match ProcessCompose
        assert_ne!(
            ImportFormat::from_content(content),
            Some(ImportFormat::ProcessCompose)
        );
    }
}
