//! docker-compose.yml importer.

use std::collections::HashMap;

use serde::Deserialize;

use super::error::{ImportError, ImportResult};
use super::types::{ImportFormat, ImportedConfig, ImportedResource, Importer};

/// Importer for docker-compose.yml v3.x format.
#[derive(Debug, Clone, Default)]
pub struct DockerComposeImporter;

impl DockerComposeImporter {
    /// Create a new docker-compose importer.
    pub fn new() -> Self {
        Self
    }
}

impl Importer for DockerComposeImporter {
    fn format(&self) -> ImportFormat {
        ImportFormat::DockerCompose
    }

    fn import(&self, content: &str) -> ImportResult<ImportedConfig> {
        let dc: DockerComposeFile = serde_yaml::from_str(content).map_err(ImportError::from)?;

        let mut config = ImportedConfig::new();

        for (name, service) in dc.services {
            let mut resource = ImportedResource::new(&name);

            resource.image = service.image;

            if let Some(cmd) = service.command {
                resource.command = Some(match cmd {
                    CommandDef::String(s) => s,
                    CommandDef::Array(arr) => arr.join(" "),
                });
            }

            if let Some(dir) = service.working_dir {
                resource.working_dir = Some(dir);
            }

            if let Some(env) = service.environment {
                match env {
                    EnvironmentDef::Map(map) => {
                        resource.environment = map;
                    }
                    EnvironmentDef::List(list) => {
                        for item in list {
                            if let Some((key, value)) = item.split_once('=') {
                                resource
                                    .environment
                                    .insert(key.to_string(), value.to_string());
                            }
                        }
                    }
                }
            }

            if let Some(deps) = service.depends_on {
                match deps {
                    DependsOnDef::List(list) => {
                        resource.depends_on = list;
                    }
                    DependsOnDef::Map(map) => {
                        resource.depends_on = map.into_keys().collect();
                    }
                }
            }

            if let Some(ports) = service.ports {
                if let Some(first) = ports.first() {
                    if let Some(port) = parse_port(first) {
                        resource.port = Some(port);
                    }
                }
            }

            if let Some(hc) = service.healthcheck {
                if let Some(test) = hc.test {
                    resource.health_check = Some(match test {
                        HealthcheckTest::String(s) => s,
                        HealthcheckTest::Array(arr) => arr.join(" "),
                    });
                }
            }

            resource.restart = service.restart;

            config.add_resource(resource);
        }

        Ok(config)
    }
}

/// Parse port from docker-compose port format.
fn parse_port(port: &str) -> Option<u16> {
    // Format can be: "3000", "3000:3000", "127.0.0.1:3000:3000"
    let parts: Vec<&str> = port.split(':').collect();
    match parts.len() {
        1 => parts[0].parse().ok(),
        2 => parts[0].parse().ok(),
        3 => parts[1].parse().ok(),
        _ => None,
    }
}

// Serde structures for docker-compose.yml

#[derive(Debug, Deserialize)]
struct DockerComposeFile {
    #[serde(default)]
    services: HashMap<String, ServiceDef>,
}

#[derive(Debug, Deserialize)]
struct ServiceDef {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    command: Option<CommandDef>,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    environment: Option<EnvironmentDef>,
    #[serde(default)]
    depends_on: Option<DependsOnDef>,
    #[serde(default)]
    ports: Option<Vec<String>>,
    #[serde(default)]
    healthcheck: Option<HealthcheckDef>,
    #[serde(default)]
    restart: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CommandDef {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EnvironmentDef {
    Map(HashMap<String, String>),
    List(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DependsOnDef {
    List(Vec<String>),
    Map(HashMap<String, DependsOnCondition>),
}

#[derive(Debug, Deserialize)]
struct DependsOnCondition {
    /// Condition type - field used for serde deserialization.
    #[serde(default)]
    #[allow(dead_code)]
    condition: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HealthcheckDef {
    #[serde(default)]
    test: Option<HealthcheckTest>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HealthcheckTest {
    String(String),
    Array(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_basic() {
        let yaml = r#"
services:
  web:
    image: nginx:latest
    ports:
      - "80:80"
  db:
    image: postgres:16
"#;
        let importer = DockerComposeImporter::new();
        let result = importer.import(yaml).unwrap();

        assert_eq!(result.resources.len(), 2);

        let web = result.resources.iter().find(|r| r.name == "web").unwrap();
        assert_eq!(web.image, Some("nginx:latest".to_string()));
        assert_eq!(web.port, Some(80));
    }

    #[test]
    fn import_with_environment() {
        let yaml = r#"
services:
  api:
    image: node:20
    environment:
      NODE_ENV: production
      PORT: "3000"
"#;
        let importer = DockerComposeImporter::new();
        let result = importer.import(yaml).unwrap();

        let api = result.resources.iter().find(|r| r.name == "api").unwrap();
        assert_eq!(
            api.environment.get("NODE_ENV"),
            Some(&"production".to_string())
        );
    }
}
