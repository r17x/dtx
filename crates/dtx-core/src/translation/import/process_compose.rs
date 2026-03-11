//! process-compose.yaml importer.

use std::collections::HashMap;

use serde::Deserialize;

use super::error::{ImportError, ImportResult};
use super::types::{ImportFormat, ImportedConfig, ImportedResource, Importer};

/// Importer for process-compose.yaml v1.x format.
#[derive(Debug, Clone, Default)]
pub struct ProcessComposeImporter;

impl ProcessComposeImporter {
    /// Create a new process-compose importer.
    pub fn new() -> Self {
        Self
    }
}

impl Importer for ProcessComposeImporter {
    fn format(&self) -> ImportFormat {
        ImportFormat::ProcessCompose
    }

    fn import(&self, content: &str) -> ImportResult<ImportedConfig> {
        let pc: ProcessComposeFile = serde_yaml::from_str(content).map_err(ImportError::from)?;

        let mut config = ImportedConfig::new();

        for (name, process) in pc.processes {
            let mut resource = ImportedResource::new(&name);

            resource.command = Some(process.command);

            if let Some(dir) = process.working_dir {
                resource.working_dir = Some(dir);
            }

            if let Some(env) = process.environment {
                for item in env.into_list() {
                    if let Some((key, value)) = item.split_once('=') {
                        resource
                            .environment
                            .insert(key.to_string(), value.to_string());
                    }
                }
            }

            if let Some(deps) = process.depends_on {
                for (dep_name, _condition) in deps {
                    resource.depends_on.push(dep_name);
                }
            }

            if let Some(probe) = process.readiness_probe {
                if let Some(exec) = probe.exec {
                    resource.health_check = Some(exec.command);
                }
            }

            if let Some(avail) = process.availability {
                resource.restart = avail.restart;
            }

            config.add_resource(resource);
        }

        Ok(config)
    }
}

// Serde structures for process-compose.yaml

#[derive(Debug, Deserialize)]
struct ProcessComposeFile {
    #[serde(default)]
    processes: HashMap<String, ProcessDef>,
}

#[derive(Debug, Deserialize)]
struct ProcessDef {
    command: String,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    environment: Option<Environment>,
    #[serde(default)]
    depends_on: Option<HashMap<String, DependencyDef>>,
    #[serde(default)]
    readiness_probe: Option<ProbeDef>,
    #[serde(default)]
    availability: Option<AvailabilityDef>,
}

/// Environment variables in process-compose can be either a list or map format.
/// We deserialize this using serde_yaml's tagged union support.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Environment {
    List(Vec<String>),
    Map(HashMap<String, serde_yaml::Value>),
}

impl Environment {
    /// Convert to a list of KEY=VALUE strings.
    fn into_list(self) -> Vec<String> {
        match self {
            Environment::List(list) => list,
            Environment::Map(map) => map
                .into_iter()
                .map(|(k, v)| {
                    let value_str = match v {
                        serde_yaml::Value::String(s) => s,
                        serde_yaml::Value::Number(n) => n.to_string(),
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        serde_yaml::Value::Null => "".to_string(),
                        other => {
                            tracing::warn!(
                                key = %k,
                                "Environment variable has complex type; converting with to_string"
                            );
                            format!("{}", serde_yaml::to_string(&other)
                                .unwrap_or_default()
                                .trim())
                        }
                    };
                    format!("{}={}", k, value_str)
                })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct DependencyDef {
    /// Condition type - field used for serde deserialization.
    #[serde(default)]
    #[allow(dead_code)]
    condition: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeDef {
    #[serde(default)]
    exec: Option<ExecDef>,
}

#[derive(Debug, Deserialize)]
struct ExecDef {
    command: String,
}

#[derive(Debug, Deserialize)]
struct AvailabilityDef {
    #[serde(default)]
    restart: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_basic() {
        let yaml = r#"
processes:
  api:
    command: npm start
  db:
    command: postgres
"#;
        let importer = ProcessComposeImporter::new();
        let result = importer.import(yaml).unwrap();

        assert_eq!(result.resources.len(), 2);
    }

    #[test]
    fn import_with_depends_on() {
        let yaml = r#"
processes:
  api:
    command: npm start
    depends_on:
      db:
        condition: process_healthy
  db:
    command: postgres
"#;
        let importer = ProcessComposeImporter::new();
        let result = importer.import(yaml).unwrap();

        let api = result.resources.iter().find(|r| r.name == "api").unwrap();
        assert!(api.depends_on.contains(&"db".to_string()));
    }

    #[test]
    fn import_with_environment_map() {
        let yaml = r#"
processes:
  postgres:
    command: postgres
    environment:
      PGDATA: /var/lib/postgresql/data
      POSTGRES_USER: admin
"#;
        let importer = ProcessComposeImporter::new();
        let result = importer.import(yaml).unwrap();

        let pg = result.resources.iter().find(|r| r.name == "postgres").unwrap();
        assert_eq!(pg.environment.get("PGDATA"), Some(&"/var/lib/postgresql/data".to_string()));
        assert_eq!(pg.environment.get("POSTGRES_USER"), Some(&"admin".to_string()));
    }

    #[test]
    fn import_with_environment_list() {
        let yaml = r#"
processes:
  api:
    command: npm start
    environment:
      - NODE_ENV=production
      - PORT=3000
"#;
        let importer = ProcessComposeImporter::new();
        let result = importer.import(yaml).unwrap();

        let api = result.resources.iter().find(|r| r.name == "api").unwrap();
        assert_eq!(api.environment.get("NODE_ENV"), Some(&"production".to_string()));
        assert_eq!(api.environment.get("PORT"), Some(&"3000".to_string()));
    }
}
