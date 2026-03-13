//! Resolve local nix file imports reachable from flake.nix.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rnix::SyntaxKind;

use super::parser::parse_nix;
use crate::error::NixError;

/// A resolved nix file with its path and content.
pub struct ResolvedNixFile {
    pub path: PathBuf,
    pub content: String,
}

/// Resolve all local nix files reachable from flake.nix via `imports = [...]` lists.
///
/// Follows relative paths (`./nix/dev.nix`), skips expressions (`inputs.foo.flakeModule`).
/// Recursively follows imports in resolved files.
pub fn resolve_flake_imports(flake_dir: &Path) -> Result<Vec<ResolvedNixFile>, NixError> {
    let flake_path = flake_dir.join("flake.nix");
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut results: Vec<ResolvedNixFile> = Vec::new();

    resolve_file(&flake_path, &mut visited, &mut results)?;

    Ok(results)
}

fn resolve_file(
    file_path: &Path,
    visited: &mut HashSet<PathBuf>,
    results: &mut Vec<ResolvedNixFile>,
) -> Result<(), NixError> {
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("cannot resolve import {}: {}", file_path.display(), e);
            return Ok(());
        }
    };

    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let content = match std::fs::read_to_string(&canonical) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("cannot read {}: {}", canonical.display(), e);
            return Ok(());
        }
    };

    let parsed = parse_nix(&content)?;
    let import_paths = extract_relative_imports(&parsed.syntax, &canonical);

    results.push(ResolvedNixFile {
        path: canonical,
        content,
    });

    for import_path in import_paths {
        resolve_file(&import_path, visited, results)?;
    }

    Ok(())
}

fn extract_relative_imports(syntax: &rnix::SyntaxNode, file_path: &Path) -> Vec<PathBuf> {
    let parent_dir = file_path.parent().unwrap_or(Path::new("."));
    let mut paths = Vec::new();

    for node in syntax.descendants() {
        if node.kind() != SyntaxKind::NODE_ATTRPATH_VALUE {
            continue;
        }

        let mut children = node.children();
        let Some(attrpath) = children.next() else {
            continue;
        };
        if attrpath.kind() != SyntaxKind::NODE_ATTRPATH {
            continue;
        }
        if attrpath.text().to_string().trim() != "imports" {
            continue;
        }

        // Find the value — skip non-node children
        let value = children.find(|c| c.kind() == SyntaxKind::NODE_LIST);
        let Some(list_node) = value else {
            continue;
        };

        for child in list_node.children() {
            if child.kind() == SyntaxKind::NODE_PATH {
                let text = child.text().to_string().trim().to_string();
                if text.starts_with("./") {
                    paths.push(parent_dir.join(&text));
                }
            }
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn no_imports() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "flake.nix", "{ outputs = inputs: { }; }");

        let result = resolve_flake_imports(dir.path()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].path.ends_with("flake.nix"));
    }

    #[test]
    fn local_imports() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "flake.nix",
            "{ imports = [ ./sub.nix ]; outputs = inputs: { }; }",
        );
        write_file(dir.path(), "sub.nix", "{ }");

        let result = resolve_flake_imports(dir.path()).unwrap();
        assert_eq!(result.len(), 2);

        let names: Vec<String> = result
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"flake.nix".to_string()));
        assert!(names.contains(&"sub.nix".to_string()));
    }

    #[test]
    fn nested_imports() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "flake.nix",
            "{ imports = [ ./a.nix ]; outputs = inputs: { }; }",
        );
        write_file(dir.path(), "a.nix", "{ imports = [ ./b.nix ]; }");
        write_file(dir.path(), "b.nix", "{ }");

        let result = resolve_flake_imports(dir.path()).unwrap();
        assert_eq!(result.len(), 3);

        let names: Vec<String> = result
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"flake.nix".to_string()));
        assert!(names.contains(&"a.nix".to_string()));
        assert!(names.contains(&"b.nix".to_string()));
    }

    #[test]
    fn external_skipped() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "flake.nix",
            "{ imports = [ inputs.foo.flakeModule ./local.nix ]; outputs = inputs: { }; }",
        );
        write_file(dir.path(), "local.nix", "{ }");

        let result = resolve_flake_imports(dir.path()).unwrap();
        assert_eq!(result.len(), 2);

        let names: Vec<String> = result
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"flake.nix".to_string()));
        assert!(names.contains(&"local.nix".to_string()));
    }

    #[test]
    fn missing_file() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "flake.nix",
            "{ imports = [ ./nonexistent.nix ]; outputs = inputs: { }; }",
        );

        let result = resolve_flake_imports(dir.path()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].path.ends_with("flake.nix"));
    }
}
