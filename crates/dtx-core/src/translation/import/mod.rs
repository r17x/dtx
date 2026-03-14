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

/// Detects inline writeShellApplication definitions in nix files, exports them
/// as packages, and rewrites commands to use the package basenames.
///
/// Returns the number of scripts exported, and the names of exported packages.
pub fn export_custom_scripts(
    config: &mut ImportedConfig,
    flake_dir: &std::path::Path,
) -> (usize, Vec<String>) {
    use crate::nix::{
        detect_scripts, export_scripts_as_packages, extract_executable, resolve_flake_imports,
    };

    // 1. Collect unresolved basenames — commands still containing /nix/store/
    let unresolved: Vec<String> = config
        .resources
        .iter()
        .filter_map(|r| r.command.as_ref())
        .filter(|cmd| cmd.contains("/nix/store/"))
        .filter_map(|cmd| extract_executable(cmd))
        .collect();

    if unresolved.is_empty() {
        return (0, vec![]);
    }

    // 2. Resolve flake imports to find all nix files
    let nix_files = match resolve_flake_imports(flake_dir) {
        Ok(files) => files,
        Err(e) => {
            tracing::warn!("failed to resolve flake imports: {}", e);
            return (0, vec![]);
        }
    };

    // 3. Detect inline scripts matching our unresolved basenames
    let scripts = detect_scripts(&unresolved, &nix_files);
    if scripts.is_empty() {
        return (0, vec![]);
    }

    // 4. Export scripts as packages (modifies nix files)
    let result = match export_scripts_as_packages(&scripts) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("failed to export scripts as packages: {}", e);
            return (0, vec![]);
        }
    };

    // 5. Add warnings from export
    config.warnings.extend(result.warnings);

    // 6. Rewrite commands: replace /nix/store/.../bin/<name> with just <name>
    //    and mark the resource with the nix package it needs
    for resource in &mut config.resources {
        if let Some(ref mut cmd) = resource.command {
            for pkg_name in &result.exported_packages {
                let new_cmd = rewrite_store_path_for_package(cmd, pkg_name);
                if new_cmd != *cmd {
                    *cmd = new_cmd;
                    resource.nix_packages.push(pkg_name.clone());
                }
            }
        }
    }

    (result.exported_packages.len(), result.exported_packages)
}

/// Rewrite nix store paths in a command whose basename matches `package_name`.
/// Preserves original whitespace by rebuilding from token boundaries.
fn rewrite_store_path_for_package(command: &str, package_name: &str) -> String {
    use crate::nix::command::is_nix_store_path;

    let mut result = String::with_capacity(command.len());
    let mut chars = command.char_indices().peekable();

    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() {
            result.push(c);
            chars.next();
        } else {
            // Find end of token
            let start = i;
            while let Some(&(_, tc)) = chars.peek() {
                if tc.is_whitespace() {
                    break;
                }
                chars.next();
            }
            let end = chars.peek().map(|&(j, _)| j).unwrap_or(command.len());
            let token = &command[start..end];

            if is_nix_store_path(token) {
                if let Some(basename) = token.rsplit('/').next() {
                    if basename == package_name {
                        result.push_str(basename);
                        continue;
                    }
                }
            }
            result.push_str(token);
        }
    }
    result
}
