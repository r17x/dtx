//! Auto-sync flake.nix when services are added or removed.
//!
//! Uses FlakeAst for incremental edits that preserve user content.
//! Only adds/removes packages that dtx tracks (from service `package` field).

use crate::error::NixError;
use crate::nix::ast::FlakeAst;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Find the flake.nix path for a project.
///
/// Checks `{project_path}/flake.nix` first, then `{project_path}/.dtx/flake.nix`.
pub fn find_flake_path(project_path: &Path) -> Option<PathBuf> {
    let root_flake = project_path.join("flake.nix");
    if root_flake.exists() {
        return Some(root_flake);
    }

    let dtx_flake = project_path.join(".dtx").join("flake.nix");
    if dtx_flake.exists() {
        return Some(dtx_flake);
    }

    None
}

/// Add a package to the project's flake.nix.
///
/// - If no flake exists, creates a new one at `{project_path}/flake.nix`.
/// - If a flake exists, parses it and adds the package (preserving user content).
/// - Returns `Ok(true)` if the flake was modified, `Ok(false)` if skipped (duplicate).
pub fn sync_add_package(
    project_path: &Path,
    project_name: &str,
    package: &str,
) -> Result<bool, NixError> {
    match find_flake_path(project_path) {
        Some(flake_path) => {
            let content = std::fs::read_to_string(&flake_path).map_err(|e| {
                NixError::IoError(format!("Failed to read {}: {}", flake_path.display(), e))
            })?;

            let mut ast = FlakeAst::parse(&content)?;

            // Check for duplicates — FlakeAst::add_package does NOT check
            let existing = ast.list_packages();
            if existing.contains(&package.to_string()) {
                return Ok(false);
            }

            ast.add_package(package)?;

            std::fs::write(&flake_path, ast.to_string()).map_err(|e| {
                NixError::IoError(format!("Failed to write {}: {}", flake_path.display(), e))
            })?;

            tracing::info!(package = %package, path = %flake_path.display(), "Added package to flake.nix");
            Ok(true)
        }
        None => {
            // Create new flake
            let mut ast = FlakeAst::new_devshell(&[], project_name);
            ast.add_package(package)?;

            let flake_path = project_path.join("flake.nix");
            std::fs::write(&flake_path, ast.to_string()).map_err(|e| {
                NixError::IoError(format!("Failed to write {}: {}", flake_path.display(), e))
            })?;

            tracing::info!(package = %package, path = %flake_path.display(), "Created flake.nix with package");
            Ok(true)
        }
    }
}

/// Remove a package from the project's flake.nix.
///
/// - If no flake exists, returns `Ok(false)`.
/// - If the package is still needed by another service (in `remaining_packages`), returns `Ok(false)`.
/// - Returns `Ok(true)` if the flake was modified.
pub fn sync_remove_package(
    project_path: &Path,
    package: &str,
    remaining_packages: &HashSet<String>,
) -> Result<bool, NixError> {
    // If another service still uses this package, don't remove it
    if remaining_packages.contains(package) {
        return Ok(false);
    }

    let flake_path = match find_flake_path(project_path) {
        Some(p) => p,
        None => return Ok(false),
    };

    let content = std::fs::read_to_string(&flake_path).map_err(|e| {
        NixError::IoError(format!("Failed to read {}: {}", flake_path.display(), e))
    })?;

    let mut ast = FlakeAst::parse(&content)?;
    let removed = ast.remove_package(package)?;

    if removed {
        std::fs::write(&flake_path, ast.to_string()).map_err(|e| {
            NixError::IoError(format!("Failed to write {}: {}", flake_path.display(), e))
        })?;
        tracing::info!(package = %package, path = %flake_path.display(), "Removed package from flake.nix");
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sync_add_creates_flake_when_missing() {
        let dir = TempDir::new().unwrap();
        let result = sync_add_package(dir.path(), "test-project", "nodejs").unwrap();
        assert!(result);

        let flake_path = dir.path().join("flake.nix");
        assert!(flake_path.exists());

        let content = std::fs::read_to_string(&flake_path).unwrap();
        assert!(content.contains("nodejs"));
        assert!(content.contains("process-compose"));
        assert!(content.contains("test-project"));
    }

    #[test]
    fn test_sync_add_to_existing_flake() {
        let dir = TempDir::new().unwrap();
        let flake_path = dir.path().join("flake.nix");

        // Create initial flake
        let ast = FlakeAst::new_devshell(&[], "test-project");
        std::fs::write(&flake_path, ast.to_string()).unwrap();

        let result = sync_add_package(dir.path(), "test-project", "redis").unwrap();
        assert!(result);

        let content = std::fs::read_to_string(&flake_path).unwrap();
        assert!(content.contains("redis"));
    }

    #[test]
    fn test_sync_add_skips_duplicate() {
        let dir = TempDir::new().unwrap();

        // First add
        sync_add_package(dir.path(), "test-project", "nodejs").unwrap();

        // Second add — should skip
        let result = sync_add_package(dir.path(), "test-project", "nodejs").unwrap();
        assert!(!result);
    }

    #[test]
    fn test_sync_remove_package() {
        let dir = TempDir::new().unwrap();

        // Add then remove
        sync_add_package(dir.path(), "test-project", "redis").unwrap();

        let remaining = HashSet::new();
        let result = sync_remove_package(dir.path(), "redis", &remaining).unwrap();
        assert!(result);

        let content = std::fs::read_to_string(dir.path().join("flake.nix")).unwrap();
        assert!(!content.contains("redis"));
    }

    #[test]
    fn test_sync_remove_skips_when_still_needed() {
        let dir = TempDir::new().unwrap();

        sync_add_package(dir.path(), "test-project", "redis").unwrap();

        let mut remaining = HashSet::new();
        remaining.insert("redis".to_string());

        let result = sync_remove_package(dir.path(), "redis", &remaining).unwrap();
        assert!(!result);

        // Package should still be there
        let content = std::fs::read_to_string(dir.path().join("flake.nix")).unwrap();
        assert!(content.contains("redis"));
    }

    #[test]
    fn test_sync_remove_no_flake() {
        let dir = TempDir::new().unwrap();
        let remaining = HashSet::new();
        let result = sync_remove_package(dir.path(), "redis", &remaining).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_find_flake_path_root() {
        let dir = TempDir::new().unwrap();
        let flake_path = dir.path().join("flake.nix");
        std::fs::write(&flake_path, "{}").unwrap();

        assert_eq!(find_flake_path(dir.path()), Some(flake_path));
    }

    #[test]
    fn test_find_flake_path_dtx_dir() {
        let dir = TempDir::new().unwrap();
        let dtx_dir = dir.path().join(".dtx");
        std::fs::create_dir_all(&dtx_dir).unwrap();
        let flake_path = dtx_dir.join("flake.nix");
        std::fs::write(&flake_path, "{}").unwrap();

        assert_eq!(find_flake_path(dir.path()), Some(flake_path));
    }

    #[test]
    fn test_find_flake_path_none() {
        let dir = TempDir::new().unwrap();
        assert_eq!(find_flake_path(dir.path()), None);
    }
}
