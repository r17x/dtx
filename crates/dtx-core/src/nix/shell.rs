//! Nix shell wrapper for running commands in Nix development environment.

use crate::{CoreError, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// Wrapper for executing commands inside a Nix development shell.
pub struct NixShell {
    project_path: PathBuf,
}

impl NixShell {
    /// Creates a new NixShell wrapper for the given project path.
    ///
    /// # Arguments
    ///
    /// * `project_path` - Path to the project directory containing flake.nix
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::nix::NixShell;
    ///
    /// let shell = NixShell::new("/path/to/project");
    /// assert!(!shell.has_flake()); // Unless flake.nix exists at that path
    /// ```
    pub fn new(project_path: impl AsRef<Path>) -> Self {
        Self {
            project_path: project_path.as_ref().to_path_buf(),
        }
    }

    /// Returns the project path.
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    /// Checks if flake.nix exists in the project directory.
    pub fn has_flake(&self) -> bool {
        self.project_path.join("flake.nix").exists()
    }

    /// Runs a command inside the nix develop shell.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute
    ///
    /// # Returns
    ///
    /// The output of the command.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - flake.nix doesn't exist
    /// - nix is not installed
    /// - The command fails to execute
    pub async fn run(&self, command: &str) -> Result<std::process::Output> {
        if !self.has_flake() {
            return Err(CoreError::ProcessCompose(format!(
                "No flake.nix found in {}. Run 'dtx nix init' first.",
                self.project_path.display()
            )));
        }

        Command::new("nix")
            .args(["develop", "--command", "sh", "-c", command])
            .current_dir(&self.project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    CoreError::ProcessCompose(
                        "nix not found in PATH. Please install Nix first.".to_string(),
                    )
                } else {
                    CoreError::Io(e)
                }
            })
    }

    /// Spawns a long-running command inside the nix develop shell.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute
    ///
    /// # Returns
    ///
    /// A child process handle.
    pub async fn spawn(&self, command: &str) -> Result<tokio::process::Child> {
        if !self.has_flake() {
            return Err(CoreError::ProcessCompose(format!(
                "No flake.nix found in {}. Run 'dtx nix init' first.",
                self.project_path.display()
            )));
        }

        Command::new("nix")
            .args(["develop", "--command", "sh", "-c", command])
            .current_dir(&self.project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    CoreError::ProcessCompose(
                        "nix not found in PATH. Please install Nix first.".to_string(),
                    )
                } else {
                    CoreError::Io(e)
                }
            })
    }

    /// Lists packages available in the Nix shell.
    ///
    /// This attempts to evaluate the devShell's buildInputs.
    pub async fn list_packages(&self) -> Result<Vec<String>> {
        if !self.has_flake() {
            return Ok(vec![]);
        }

        let output = Command::new("nix")
            .args([
                "eval",
                "--json",
                ".#devShells.x86_64-linux.default.buildInputs",
                "--impure",
            ])
            .current_dir(&self.project_path)
            .output()
            .await
            .map_err(CoreError::Io)?;

        if !output.status.success() {
            // Fallback: try to extract from flake.nix content
            return Ok(vec![]);
        }

        let packages: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).unwrap_or_default();

        Ok(packages
            .iter()
            .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
            .map(String::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_new_shell() {
        let dir = tempdir().unwrap();
        let shell = NixShell::new(dir.path());

        assert_eq!(shell.project_path(), dir.path());
        assert!(!shell.has_flake());
    }

    #[test]
    fn test_has_flake_detection() {
        let dir = tempdir().unwrap();
        let shell = NixShell::new(dir.path());

        assert!(!shell.has_flake());

        fs::write(dir.path().join("flake.nix"), "{}").unwrap();
        assert!(shell.has_flake());
    }

    #[tokio::test]
    async fn test_run_without_flake_errors() {
        let dir = tempdir().unwrap();
        let shell = NixShell::new(dir.path());

        let result = shell.run("echo test").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No flake.nix found"));
    }

    #[tokio::test]
    #[ignore] // Requires nix installation
    async fn test_run_command_in_shell() {
        let dir = tempdir().unwrap();

        // Create a minimal valid flake
        let flake = r#"{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { nixpkgs, ... }: {
    devShells.x86_64-linux.default = nixpkgs.legacyPackages.x86_64-linux.mkShell {
      packages = [];
    };
  };
}"#;
        fs::write(dir.path().join("flake.nix"), flake).unwrap();

        let shell = NixShell::new(dir.path());
        let output = shell.run("echo hello").await.unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("hello"));
    }

    #[tokio::test]
    #[ignore] // Requires nix installation
    async fn test_spawn_command() {
        let dir = tempdir().unwrap();

        let flake = r#"{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { nixpkgs, ... }: {
    devShells.x86_64-linux.default = nixpkgs.legacyPackages.x86_64-linux.mkShell {};
  };
}"#;
        fs::write(dir.path().join("flake.nix"), flake).unwrap();

        let shell = NixShell::new(dir.path());
        let mut child = shell.spawn("sleep 1").await.unwrap();

        // Child should be running
        assert!(child.id().is_some());

        // Wait for completion
        let status = child.wait().await.unwrap();
        assert!(status.success());
    }
}
