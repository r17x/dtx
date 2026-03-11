//! Import functionality for reading external configuration formats.
//!
//! Supports:
//! - process-compose.yaml (v1.x)
//! - docker-compose.yml (v3.x)
//! - Procfile (Heroku-style)

mod docker_compose;
mod error;
mod process_compose;
mod procfile;
mod types;

pub use docker_compose::DockerComposeImporter;
pub use error::{ImportError, ImportResult};
pub use process_compose::ProcessComposeImporter;
pub use procfile::ProcfileImporter;
pub use types::{ImportFormat, ImportedConfig, ImportedResource, Importer};
