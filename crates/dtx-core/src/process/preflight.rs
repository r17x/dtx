//! Pre-flight checks before starting services.
//!
//! Validates that all dependencies are available before attempting to start services.
//! This prevents confusing failures and provides actionable error messages.

use crate::model::Service;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Result of pre-flight checks.
#[derive(Debug)]
pub struct PreflightResult {
    /// Checks that passed.
    pub passed: Vec<PreflightCheck>,
    /// Checks that failed.
    pub failed: Vec<PreflightCheck>,
}

impl PreflightResult {
    /// Returns true if all checks passed.
    pub fn is_ok(&self) -> bool {
        self.failed.is_empty()
    }

    /// Returns a summary of failures for display.
    pub fn failure_summary(&self) -> String {
        if self.failed.is_empty() {
            return String::new();
        }

        let mut lines = vec!["Pre-flight checks failed:".to_string(), String::new()];

        for check in &self.failed {
            lines.push(format!("  ✗ {}", check.description));
            if let Some(hint) = &check.fix_hint {
                lines.push(format!("    → {}", hint));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    }
}

/// A single pre-flight check.
#[derive(Debug, Clone)]
pub struct PreflightCheck {
    /// Human-readable description of what's being checked.
    pub description: String,
    /// The type of check.
    pub check_type: CheckType,
    /// Services that require this check to pass.
    pub required_by: Vec<String>,
    /// Hint for how to fix if the check fails.
    pub fix_hint: Option<String>,
    /// Whether the check passed.
    pub passed: bool,
}

/// Type of pre-flight check.
#[derive(Debug, Clone)]
pub enum CheckType {
    /// Check if a binary exists in PATH.
    BinaryExists(String),
    /// Check if Docker daemon is running.
    DockerRunning,
    /// Check if a port is available.
    PortAvailable(u16),
    /// Check if a file exists.
    FileExists(String),
    /// Check if a Python module is importable.
    PythonModule(String),
    /// Check if nix is available.
    NixAvailable,
}

/// Analyzes services and generates required pre-flight checks.
pub fn analyze_services(services: &[Service]) -> Vec<PreflightCheck> {
    let mut checks = Vec::new();
    let mut seen_binaries: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen_scripts: HashMap<String, Vec<String>> = HashMap::new();
    let mut needs_docker = Vec::new();
    let mut needs_nix = false;
    let mut used_ports: HashMap<u16, Vec<String>> = HashMap::new();
    // Track binaries that will be provided by nix packages
    let mut nix_provided_binaries: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for service in services {
        if !service.enabled {
            continue;
        }

        // Check if service has a nix package (explicit or inferred)
        let has_nix_package = service.package.is_some()
            || crate::nix::command::infer_package(&service.command).is_some();

        // Extract command target and determine if it's a local script or PATH binary
        if let Some((cmd_type, target)) = extract_command_target(&service.command) {
            match cmd_type {
                CommandTarget::LocalScript => {
                    seen_scripts
                        .entry(target)
                        .or_default()
                        .push(service.name.clone());
                }
                CommandTarget::PathBinary => {
                    // If nix will provide this binary, mark it
                    if has_nix_package {
                        nix_provided_binaries.insert(target.clone());
                    }
                    seen_binaries
                        .entry(target)
                        .or_default()
                        .push(service.name.clone());
                }
            }
        }

        // Check for docker commands
        if service.command.starts_with("docker ") || service.command.contains("docker run") {
            needs_docker.push(service.name.clone());
        }

        // Check for nix commands
        if service.command.starts_with("nix ") {
            needs_nix = true;
        }

        // Track ports
        if let Some(port) = service.port {
            used_ports
                .entry(port)
                .or_default()
                .push(service.name.clone());
        }
    }

    // Generate local script checks (file exists)
    for (script_path, required_by) in seen_scripts {
        checks.push(PreflightCheck {
            description: format!("Script {} exists", script_path),
            check_type: CheckType::FileExists(script_path.clone()),
            required_by,
            fix_hint: Some(format!("Create the script at {}", script_path)),
            passed: false,
        });
    }

    // Generate binary checks (skip binaries provided by nix)
    for (binary, required_by) in seen_binaries {
        // Skip common shell builtins
        if is_shell_builtin(&binary) {
            continue;
        }

        // Skip binaries that will be provided by nix packages
        if nix_provided_binaries.contains(&binary) {
            continue;
        }

        let fix_hint = get_binary_fix_hint(&binary);

        checks.push(PreflightCheck {
            description: format!("{} binary available", binary),
            check_type: CheckType::BinaryExists(binary),
            required_by,
            fix_hint,
            passed: false,
        });
    }

    // Docker check
    if !needs_docker.is_empty() {
        checks.push(PreflightCheck {
            description: "Docker daemon running".to_string(),
            check_type: CheckType::DockerRunning,
            required_by: needs_docker,
            fix_hint: Some("Start Docker Desktop or OrbStack".to_string()),
            passed: false,
        });
    }

    // Nix check
    if needs_nix {
        checks.push(PreflightCheck {
            description: "Nix available".to_string(),
            check_type: CheckType::NixAvailable,
            required_by: vec!["nix commands".to_string()],
            fix_hint: Some("Install Nix: https://nixos.org/download".to_string()),
            passed: false,
        });
    }

    // Port availability checks
    for (port, required_by) in used_ports {
        checks.push(PreflightCheck {
            description: format!("Port {} available", port),
            check_type: CheckType::PortAvailable(port),
            required_by,
            fix_hint: Some(format!(
                "Kill process using port {}: lsof -ti:{} | xargs kill",
                port, port
            )),
            passed: false,
        });
    }

    checks
}

/// Run all pre-flight checks.
pub async fn run_preflight(checks: Vec<PreflightCheck>) -> PreflightResult {
    run_preflight_with_path(checks, None).await
}

/// Run all pre-flight checks with optional custom PATH.
///
/// When `nix_path` is provided, binary existence checks will use that PATH
/// instead of the system PATH. This allows pre-flight checks to pass when
/// binaries are provided by a Nix flake.
pub async fn run_preflight_with_path(
    checks: Vec<PreflightCheck>,
    nix_path: Option<&str>,
) -> PreflightResult {
    let mut passed = Vec::new();
    let mut failed = Vec::new();

    for mut check in checks {
        check.passed = run_single_check_with_path(&check.check_type, nix_path).await;

        if check.passed {
            passed.push(check);
        } else {
            failed.push(check);
        }
    }

    PreflightResult { passed, failed }
}

/// Run a single pre-flight check with optional custom PATH.
async fn run_single_check_with_path(check_type: &CheckType, nix_path: Option<&str>) -> bool {
    match check_type {
        CheckType::BinaryExists(binary) => check_binary_exists_with_path(binary, nix_path).await,
        CheckType::DockerRunning => check_docker_running().await,
        CheckType::PortAvailable(port) => check_port_available(*port).await,
        CheckType::FileExists(path) => Path::new(path).exists(),
        CheckType::PythonModule(module) => check_python_module_with_path(module, nix_path).await,
        CheckType::NixAvailable => check_binary_exists_with_path("nix", None).await,
    }
}

/// Check if a binary exists in PATH, with optional custom PATH.
async fn check_binary_exists_with_path(binary: &str, nix_path: Option<&str>) -> bool {
    // If nix_path provided, check directly in those directories first
    // (avoids issues with `which` not being in the modified PATH)
    if let Some(path) = nix_path {
        for dir in path.split(':') {
            let bin_path = std::path::Path::new(dir).join(binary);
            if bin_path.exists() && bin_path.is_file() {
                tracing::debug!(binary = binary, path = %bin_path.display(), "Found binary in Nix PATH");
                return true;
            }
        }
    }

    // Fall back to `which` for system PATH
    let result = Command::new("which")
        .arg(binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match result {
        Ok(status) => status.success(),
        Err(e) => {
            tracing::debug!(binary = binary, error = %e, "which command failed");
            false
        }
    }
}

/// Check if Docker daemon is running.
async fn check_docker_running() -> bool {
    Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

/// Check if a port is available.
async fn check_port_available(port: u16) -> bool {
    // Try to bind to the port
    tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .is_ok()
}

/// Check if a Python module is importable, with optional custom PATH.
async fn check_python_module_with_path(module: &str, nix_path: Option<&str>) -> bool {
    let mut cmd = Command::new("python3");
    cmd.args(["-c", &format!("import {}", module)])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // If nix_path is provided, use it to find python3
    if let Some(path) = nix_path {
        cmd.env("PATH", path);
    }

    cmd.status().await.is_ok_and(|s| s.success())
}

/// Type of command target.
enum CommandTarget {
    /// A local script (starts with ./ or /)
    LocalScript,
    /// A binary expected in PATH
    PathBinary,
}

/// Extract the command target from a command string.
/// Returns the type (local script vs PATH binary) and the target path/name.
fn extract_command_target(command: &str) -> Option<(CommandTarget, String)> {
    let command = command.trim();

    // Handle env prefix
    let first_token = if command.starts_with("env ") {
        command
            .split_whitespace()
            .find(|s| !s.contains('=') && *s != "env")?
    } else {
        command.split_whitespace().next()?
    };

    // Check if it's a local script (starts with ./ or is an absolute path)
    if first_token.starts_with("./") || first_token.starts_with('/') {
        // It's a local script - return the full path
        Some((CommandTarget::LocalScript, first_token.to_string()))
    } else if first_token.contains('/') {
        // Has a path but doesn't start with ./ or / - treat as local script
        Some((CommandTarget::LocalScript, first_token.to_string()))
    } else {
        // It's a PATH binary
        Some((CommandTarget::PathBinary, first_token.to_string()))
    }
}

/// Extract the binary name from a command string (for backwards compatibility).
#[allow(dead_code)]
fn extract_binary(command: &str) -> Option<String> {
    let command = command.trim();

    // Handle env prefix
    let command = if command.starts_with("env ") {
        command
            .split_whitespace()
            .find(|s| !s.contains('=') && *s != "env")?
    } else {
        command.split_whitespace().next()?
    };

    // Handle path prefixes
    let binary = if command.contains('/') {
        command.rsplit('/').next()?
    } else {
        command
    };

    Some(binary.to_string())
}

/// Check if a binary is a shell builtin or keyword.
fn is_shell_builtin(binary: &str) -> bool {
    matches!(
        binary,
        // Shell builtins
        "cd" | "echo"
            | "export"
            | "source"
            | "."
            | "["
            | "[["
            | "test"
            | "true"
            | "false"
            | "pwd"
            | "read"
            | "set"
            | "unset"
            | "shift"
            | "exit"
            | "return"
            | "break"
            | "continue"
            | "trap"
            | "eval"
            | "exec"
            | "type"
            | "hash"
            | "alias"
            | "unalias"
            | "wait"
            | "jobs"
            | "fg"
            | "bg"
            | "kill"
            | "times"
            | "umask"
            | "getopts"
            | "local"
            | "declare"
            | "typeset"
            | "readonly"
            | "let"
            | "printf"
            // Shell keywords (control flow)
            | "if"
            | "then"
            | "else"
            | "elif"
            | "fi"
            | "case"
            | "esac"
            | "for"
            | "while"
            | "until"
            | "do"
            | "done"
            | "in"
            | "select"
            | "function"
            | "time"
            | "coproc"
            // Common utilities that are safe to skip
            | "sh"
            | "bash"
            | "zsh"
    )
}

/// Get a fix hint for a missing binary.
fn get_binary_fix_hint(binary: &str) -> Option<String> {
    match binary {
        "docker" => Some("Install Docker Desktop or OrbStack".to_string()),
        "python" | "python3" => Some("Install Python or use nix".to_string()),
        "cargo" => Some("Install Rust: https://rustup.rs".to_string()),
        "node" | "npm" => Some("Install Node.js or use nix".to_string()),
        "yt-dlp" => Some("brew install yt-dlp".to_string()),
        "ffmpeg" | "ffprobe" => Some("brew install ffmpeg".to_string()),
        "nix" => Some("Install Nix: https://nixos.org/download".to_string()),
        "process-compose" => Some("Install process-compose or use nix".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_binary_simple() {
        assert_eq!(
            extract_binary("docker run nginx"),
            Some("docker".to_string())
        );
        assert_eq!(
            extract_binary("python3 script.py"),
            Some("python3".to_string())
        );
        assert_eq!(extract_binary("./run.sh"), Some("run.sh".to_string()));
    }

    #[test]
    fn test_extract_binary_with_path() {
        assert_eq!(
            extract_binary("/usr/bin/python3 script.py"),
            Some("python3".to_string())
        );
    }

    #[test]
    fn test_extract_binary_with_env() {
        assert_eq!(
            extract_binary("env FOO=bar python3 script.py"),
            Some("python3".to_string())
        );
    }

    #[test]
    fn test_is_shell_builtin() {
        assert!(is_shell_builtin("cd"));
        assert!(is_shell_builtin("echo"));
        assert!(!is_shell_builtin("docker"));
    }

    #[test]
    fn test_is_shell_keyword() {
        assert!(is_shell_builtin("while"));
        assert!(is_shell_builtin("for"));
        assert!(is_shell_builtin("if"));
        assert!(is_shell_builtin("case"));
        assert!(is_shell_builtin("sh"));
        assert!(is_shell_builtin("bash"));
    }
}
