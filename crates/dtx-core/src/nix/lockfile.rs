//! Flake.lock parser for extracting pinned nixpkgs revisions.
//!
//! This module parses the flake.lock JSON file to extract the exact nixpkgs
//! revision the user has pinned, enabling version-accurate package searches.

use crate::error::NixError;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Parsed flake.lock file structure.
#[derive(Debug, Deserialize)]
pub struct FlakeLock {
    nodes: HashMap<String, LockNode>,
    root: String,
    version: u32,
}

/// A node in the flake.lock dependency graph.
#[derive(Debug, Deserialize)]
struct LockNode {
    inputs: Option<HashMap<String, InputRef>>,
    locked: Option<LockedInfo>,
    #[allow(dead_code)]
    original: Option<OriginalInfo>,
}

/// Reference to another input, either simple or path-based.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum InputRef {
    /// Simple reference to another node by name.
    Simple(String),
    /// Path reference following input indirection.
    Path(Vec<String>),
}

/// Locked (resolved) information for a flake input.
#[derive(Debug, Deserialize)]
struct LockedInfo {
    #[serde(rename = "lastModified")]
    #[allow(dead_code)]
    last_modified: Option<u64>,
    #[serde(rename = "narHash")]
    #[allow(dead_code)]
    nar_hash: Option<String>,
    owner: Option<String>,
    repo: Option<String>,
    rev: Option<String>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    lock_type: Option<String>,
}

/// Original (unresolved) information for a flake input.
#[derive(Debug, Deserialize)]
struct OriginalInfo {
    #[allow(dead_code)]
    owner: Option<String>,
    #[allow(dead_code)]
    repo: Option<String>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    original_type: Option<String>,
    #[serde(rename = "ref")]
    #[allow(dead_code)]
    git_ref: Option<String>,
}

impl FlakeLock {
    /// Parse flake.lock from a file path.
    pub fn parse_file(path: impl AsRef<Path>) -> Result<Self, NixError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| NixError::IoError(format!("Failed to read flake.lock: {}", e)))?;
        Self::parse(&content)
    }

    /// Parse flake.lock from a string.
    pub fn parse(content: &str) -> Result<Self, NixError> {
        serde_json::from_str(content)
            .map_err(|e| NixError::InvalidLockFile(format!("Invalid flake.lock JSON: {}", e)))
    }

    /// Get the pinned nixpkgs revision.
    ///
    /// Tries multiple strategies:
    /// 1. Direct "nixpkgs" node
    /// 2. Follow root inputs to find nixpkgs
    pub fn get_nixpkgs_rev(&self) -> Option<String> {
        // Try direct "nixpkgs" node
        if let Some(rev) = self.get_node_rev("nixpkgs") {
            return Some(rev);
        }

        // Try following root inputs
        if let Some(root_node) = self.nodes.get(&self.root) {
            if let Some(inputs) = &root_node.inputs {
                // Look for nixpkgs input
                if let Some(input_ref) = inputs.get("nixpkgs") {
                    let node_name = match input_ref {
                        InputRef::Simple(name) => name.clone(),
                        InputRef::Path(path) => path.first()?.clone(),
                    };
                    return self.get_node_rev(&node_name);
                }
            }
        }

        None
    }

    /// Get the revision for a specific node by name.
    fn get_node_rev(&self, name: &str) -> Option<String> {
        self.nodes
            .get(name)
            .and_then(|n| n.locked.as_ref())
            .and_then(|l| l.rev.clone())
    }

    /// Get the flake reference for nixpkgs at the pinned revision.
    ///
    /// Returns a flake URL like `github:NixOS/nixpkgs?rev=abc123`.
    pub fn get_nixpkgs_flake_ref(&self) -> Option<String> {
        let rev = self.get_nixpkgs_rev()?;

        // Get owner/repo from locked info or default to NixOS/nixpkgs
        let (owner, repo) = self
            .nodes
            .get("nixpkgs")
            .and_then(|n| n.locked.as_ref())
            .map(|l| {
                (
                    l.owner.clone().unwrap_or_else(|| "NixOS".to_string()),
                    l.repo.clone().unwrap_or_else(|| "nixpkgs".to_string()),
                )
            })
            .unwrap_or_else(|| ("NixOS".to_string(), "nixpkgs".to_string()));

        Some(format!("github:{}/{}?rev={}", owner, repo, rev))
    }

    /// Get the lock file version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Get the current system platform string.
    ///
    /// Returns the Nix system identifier for the current platform.
    pub fn current_system() -> &'static str {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "x86_64-linux";
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "aarch64-linux";
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "x86_64-darwin";
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "aarch64-darwin";
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
        )))]
        return "x86_64-linux"; // Default fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOCK: &str = r#"{
        "nodes": {
            "nixpkgs": {
                "locked": {
                    "lastModified": 1705883077,
                    "narHash": "sha256-example",
                    "owner": "NixOS",
                    "repo": "nixpkgs",
                    "rev": "abc123def456",
                    "type": "github"
                },
                "original": {
                    "owner": "NixOS",
                    "repo": "nixpkgs",
                    "ref": "nixpkgs-unstable",
                    "type": "github"
                }
            },
            "root": {
                "inputs": {
                    "nixpkgs": "nixpkgs"
                }
            }
        },
        "root": "root",
        "version": 7
    }"#;

    const FLAKE_PARTS_LOCK: &str = r#"{
        "nodes": {
            "flake-parts": {
                "inputs": {
                    "nixpkgs-lib": ["nixpkgs"]
                },
                "locked": {
                    "owner": "hercules-ci",
                    "repo": "flake-parts",
                    "rev": "flake-parts-rev",
                    "type": "github"
                }
            },
            "nixpkgs": {
                "locked": {
                    "owner": "NixOS",
                    "repo": "nixpkgs",
                    "rev": "pinned-nixpkgs-rev",
                    "type": "github"
                }
            },
            "root": {
                "inputs": {
                    "flake-parts": "flake-parts",
                    "nixpkgs": "nixpkgs"
                }
            }
        },
        "root": "root",
        "version": 7
    }"#;

    #[test]
    fn test_parse_lock() {
        let lock = FlakeLock::parse(SAMPLE_LOCK).unwrap();
        assert_eq!(lock.version, 7);
        assert_eq!(lock.root, "root");
    }

    #[test]
    fn test_get_nixpkgs_rev() {
        let lock = FlakeLock::parse(SAMPLE_LOCK).unwrap();
        let rev = lock.get_nixpkgs_rev().unwrap();
        assert_eq!(rev, "abc123def456");
    }

    #[test]
    fn test_get_nixpkgs_flake_ref() {
        let lock = FlakeLock::parse(SAMPLE_LOCK).unwrap();
        let flake_ref = lock.get_nixpkgs_flake_ref().unwrap();
        assert!(flake_ref.contains("abc123def456"));
        assert!(flake_ref.starts_with("github:NixOS/nixpkgs"));
    }

    #[test]
    fn test_flake_parts_lock() {
        let lock = FlakeLock::parse(FLAKE_PARTS_LOCK).unwrap();
        let rev = lock.get_nixpkgs_rev().unwrap();
        assert_eq!(rev, "pinned-nixpkgs-rev");
    }

    #[test]
    fn test_invalid_json() {
        let result = FlakeLock::parse("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_current_system() {
        let system = FlakeLock::current_system();
        // Should return a valid system string
        assert!(
            system == "x86_64-linux"
                || system == "aarch64-linux"
                || system == "x86_64-darwin"
                || system == "aarch64-darwin"
        );
    }
}
