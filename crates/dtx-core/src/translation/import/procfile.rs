//! Procfile importer.

use super::error::{ImportError, ImportResult};
use super::types::{ImportFormat, ImportedConfig, ImportedResource, Importer};

/// Importer for Heroku-style Procfile format.
#[derive(Debug, Clone, Default)]
pub struct ProcfileImporter;

impl ProcfileImporter {
    /// Create a new Procfile importer.
    pub fn new() -> Self {
        Self
    }
}

impl Importer for ProcfileImporter {
    fn format(&self) -> ImportFormat {
        ImportFormat::Procfile
    }

    fn import(&self, content: &str) -> ImportResult<ImportedConfig> {
        let mut config = ImportedConfig::new();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse "name: command"
            let (name, command) = line.split_once(':').ok_or_else(|| {
                ImportError::parse(line_num + 1, "expected 'name: command' format")
            })?;

            let name = name.trim();
            let command = command.trim();

            if name.is_empty() {
                return Err(ImportError::parse(line_num + 1, "empty process name"));
            }

            if command.is_empty() {
                return Err(ImportError::parse(line_num + 1, "empty command"));
            }

            let mut resource = ImportedResource::new(name);
            resource.command = Some(command.to_string());
            resource.source_line = Some(line_num + 1);

            // Infer port from common patterns
            if name == "web" {
                resource.port = Some(3000);
            }

            config.add_resource(resource);
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_basic() {
        let content = r#"
web: npm start
worker: npm run worker
"#;
        let importer = ProcfileImporter::new();
        let result = importer.import(content).unwrap();

        assert_eq!(result.resources.len(), 2);

        let web = result.resources.iter().find(|r| r.name == "web").unwrap();
        assert_eq!(web.command, Some("npm start".to_string()));
        assert_eq!(web.port, Some(3000));
    }

    #[test]
    fn import_with_comments() {
        let content = r#"
# This is a comment
web: bundle exec rails server
# Another comment
worker: bundle exec sidekiq
"#;
        let importer = ProcfileImporter::new();
        let result = importer.import(content).unwrap();

        assert_eq!(result.resources.len(), 2);
    }

    #[test]
    fn import_invalid_format() {
        let content = "web npm start";
        let importer = ProcfileImporter::new();
        let result = importer.import(content);

        assert!(result.is_err());
    }
}
