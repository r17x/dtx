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

/// Sanitizes nix store paths in imported commands and health checks.
/// Part of the translation step — transforms raw imported data into
/// deterministic dtx resource definitions.
///
/// Takes the PATH from the user's devShell environment.
/// Returns the number of commands sanitized.
pub fn sanitize_nix_commands(config: &mut ImportedConfig, path_env: &str) -> usize {
    use crate::nix::command::sanitize_nix_store_paths;

    let mut count = 0;
    let mut warnings = Vec::new();

    for resource in &mut config.resources {
        if let Some(ref mut cmd) = resource.command {
            let (sanitized, w) = sanitize_nix_store_paths(cmd, path_env);
            if sanitized != *cmd {
                count += 1;
                *cmd = sanitized;
            }
            warnings.extend(w);
        }
        if let Some(ref mut hc) = resource.health_check {
            let (sanitized, w) = sanitize_nix_store_paths(hc, path_env);
            if sanitized != *hc {
                count += 1;
                *hc = sanitized;
            }
            warnings.extend(w);
        }
    }

    config.warnings.extend(warnings);
    count
}
