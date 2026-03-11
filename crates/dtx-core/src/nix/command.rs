//! Command analysis and automatic Nix package inference.
//!
//! Detects executables in service commands and maps them to Nix packages.

use std::collections::HashMap;

/// Maps common executable names to their Nix package names.
///
/// This allows dtx to automatically include required packages in flake.nix
/// based on the commands used in services.
pub fn get_package_mappings() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();

    // Node.js ecosystem
    m.insert("node", "nodejs");
    m.insert("nodejs", "nodejs");
    m.insert("npm", "nodejs");
    m.insert("npx", "nodejs");
    m.insert("yarn", "yarn");
    m.insert("pnpm", "pnpm");
    m.insert("bun", "bun");
    m.insert("deno", "deno");

    // Python ecosystem
    m.insert("python", "python3");
    m.insert("python3", "python3");
    m.insert("python2", "python2");
    m.insert("pip", "python3");
    m.insert("pip3", "python3");
    m.insert("poetry", "poetry");
    m.insert("uvicorn", "python3"); // Usually run via python -m
    m.insert("gunicorn", "python3");
    m.insert("flask", "python3");
    m.insert("django-admin", "python3");

    // Ruby ecosystem
    m.insert("ruby", "ruby");
    m.insert("gem", "ruby");
    m.insert("bundle", "bundler");
    m.insert("bundler", "bundler");
    m.insert("rails", "ruby");
    m.insert("rake", "ruby");

    // Go
    m.insert("go", "go");

    // Rust
    m.insert("cargo", "cargo");
    m.insert("rustc", "rustc");

    // Java/JVM
    m.insert("java", "jdk");
    m.insert("javac", "jdk");
    m.insert("mvn", "maven");
    m.insert("gradle", "gradle");

    // Databases
    m.insert("postgres", "postgresql");
    m.insert("postgresql", "postgresql");
    m.insert("pg_ctl", "postgresql");
    m.insert("psql", "postgresql");
    m.insert("pg_isready", "postgresql");
    m.insert("initdb", "postgresql");
    m.insert("mysql", "mysql");
    m.insert("mysqld", "mysql");
    m.insert("mariadb", "mariadb");
    m.insert("redis", "redis");
    m.insert("redis-server", "redis");
    m.insert("redis-cli", "redis");
    m.insert("mongod", "mongodb");
    m.insert("mongo", "mongodb");
    m.insert("mongodb", "mongodb");
    m.insert("sqlite3", "sqlite");

    // Message queues
    m.insert("rabbitmq", "rabbitmq-server");
    m.insert("rabbitmq-server", "rabbitmq-server");
    m.insert("rabbitmqctl", "rabbitmq-server");
    m.insert("kafka", "apacheKafka");
    m.insert("kafka-server-start", "apacheKafka");

    // Caching
    m.insert("memcached", "memcached");

    // Web servers
    m.insert("nginx", "nginx");
    m.insert("caddy", "caddy");
    m.insert("httpd", "apacheHttpd");

    // Container/orchestration
    m.insert("docker", "docker");
    m.insert("podman", "podman");
    m.insert("kubectl", "kubectl");

    // Dev tools
    m.insert("git", "git");
    m.insert("curl", "curl");
    m.insert("wget", "wget");
    m.insert("jq", "jq");
    m.insert("make", "gnumake");
    m.insert("cmake", "cmake");
    m.insert("gcc", "gcc");
    m.insert("g++", "gcc");
    m.insert("clang", "clang");

    // Misc services
    m.insert("minio", "minio");
    m.insert("vault", "vault");
    m.insert("consul", "consul");
    m.insert("etcd", "etcd");
    m.insert("grafana-server", "grafana");
    m.insert("prometheus", "prometheus");

    // PHP
    m.insert("php", "php");
    m.insert("composer", "phpPackages.composer");

    // Elixir/Erlang
    m.insert("elixir", "elixir");
    m.insert("mix", "elixir");
    m.insert("erl", "erlang");
    m.insert("rebar3", "rebar3");

    // .NET
    m.insert("dotnet", "dotnet-sdk");

    m
}

/// Extracts the executable name from a command string.
///
/// Handles various command formats:
/// - Simple: `node app.js` → `node`
/// - With path: `/usr/bin/node app.js` → `node`
/// - With env: `NODE_ENV=prod node app.js` → `node`
/// - Shell wrapper: `bash -c "node app.js"` → `node`
///
/// # Examples
///
/// ```
/// use dtx_core::nix::command::extract_executable;
///
/// assert_eq!(extract_executable("node app.js"), Some("node".to_string()));
/// assert_eq!(extract_executable("python3 -m flask run"), Some("python3".to_string()));
/// assert_eq!(extract_executable("/usr/bin/redis-server"), Some("redis-server".to_string()));
/// ```
pub fn extract_executable(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    // Split by whitespace
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let mut idx = 0;

    // Skip environment variables (KEY=value format)
    while idx < parts.len() && parts[idx].contains('=') && !parts[idx].starts_with('-') {
        idx += 1;
    }

    if idx >= parts.len() {
        return None;
    }

    let candidate = parts[idx];

    // Handle shell wrappers: bash -c "actual command", sh -c "..."
    if (candidate == "bash" || candidate == "sh" || candidate == "zsh")
        && parts.get(idx + 1) == Some(&"-c")
    {
        // Extract the quoted command
        let rest = parts[idx + 2..].join(" ");
        let inner = rest.trim_matches(|c| c == '"' || c == '\'');
        return extract_executable(inner);
    }

    // Handle nix-shell -p pkg --run "command"
    if candidate == "nix-shell" {
        // Find --run and extract command after it
        if let Some(run_idx) = parts.iter().position(|&p| p == "--run") {
            if run_idx + 1 < parts.len() {
                let rest = parts[run_idx + 1..].join(" ");
                let inner = rest.trim_matches(|c| c == '"' || c == '\'');
                return extract_executable(inner);
            }
        }
        return None;
    }

    // Extract basename from path
    let basename = candidate.rsplit('/').next().unwrap_or(candidate);

    Some(basename.to_string())
}

/// Checks if a command is a local/path-based binary that doesn't need a nix package.
///
/// Returns `true` for:
/// - Relative paths: `./bin/app`, `../build/server`
/// - Absolute paths: `/usr/local/bin/app`, `/opt/tool`
/// - Home directory: `~/bin/app`
pub fn is_local_binary(command: &str) -> bool {
    let executable = match extract_executable(command) {
        Some(e) => e,
        None => return false,
    };

    // Check the original command for path indicators
    let cmd_trimmed = command.trim();
    let _first_word = cmd_trimmed.split_whitespace().next().unwrap_or("");

    // Skip env vars to get actual command
    let actual_cmd = cmd_trimmed
        .split_whitespace()
        .find(|s| !s.contains('='))
        .unwrap_or("");

    actual_cmd.starts_with("./")
        || actual_cmd.starts_with("../")
        || actual_cmd.starts_with('/')
        || actual_cmd.starts_with("~/")
        || executable.starts_with('.')
}

/// Result of package inference for a command.
#[derive(Debug, Clone, PartialEq)]
pub enum PackageInference {
    /// Found a matching nix package.
    Found(String),
    /// Command is a local binary, no package needed.
    LocalBinary,
    /// Unknown command, couldn't infer package.
    Unknown(String),
}

/// Infers the Nix package needed for a command using built-in mappings.
///
/// For dynamic/configurable mappings, use `PackageMappings::load()`.
///
/// Returns `None` if no mapping exists for the executable.
///
/// # Examples
///
/// ```
/// use dtx_core::nix::command::infer_package;
///
/// assert_eq!(infer_package("node app.js"), Some("nodejs".to_string()));
/// assert_eq!(infer_package("postgres -D /data"), Some("postgresql".to_string()));
/// assert_eq!(infer_package("./custom-binary"), None);
/// ```
pub fn infer_package(command: &str) -> Option<String> {
    let executable = extract_executable(command)?;
    let mappings = get_package_mappings();
    mappings.get(executable.as_str()).map(|s| s.to_string())
}

/// Infers package using dynamic mappings (loads user/project config).
///
/// This is the recommended function for production use as it respects
/// user and project-level configuration overrides.
pub fn infer_package_with_config(command: &str) -> Option<String> {
    let executable = extract_executable(command)?;
    let mappings = super::mappings::PackageMappings::load();
    mappings.get_package(&executable).cloned()
}

/// Infers package with detailed result including local binary detection.
///
/// # Examples
///
/// ```
/// use dtx_core::nix::command::{infer_package_detailed, PackageInference};
///
/// assert_eq!(
///     infer_package_detailed("node app.js"),
///     PackageInference::Found("nodejs".to_string())
/// );
/// assert_eq!(
///     infer_package_detailed("./my-binary"),
///     PackageInference::LocalBinary
/// );
/// assert_eq!(
///     infer_package_detailed("unknown-tool"),
///     PackageInference::Unknown("unknown-tool".to_string())
/// );
/// ```
pub fn infer_package_detailed(command: &str) -> PackageInference {
    // Check if it's a local binary first
    if is_local_binary(command) {
        return PackageInference::LocalBinary;
    }

    // Try to infer package
    if let Some(pkg) = infer_package(command) {
        return PackageInference::Found(pkg);
    }

    // Unknown - extract executable name for reporting
    let executable = extract_executable(command).unwrap_or_else(|| command.to_string());
    PackageInference::Unknown(executable)
}

/// Infers packages for multiple services.
///
/// Returns a list of (service_name, inferred_package) tuples.
pub fn infer_packages_for_services(services: &[crate::model::Service]) -> Vec<(String, String)> {
    services
        .iter()
        .filter(|s| s.enabled)
        .filter(|s| s.package.is_none()) // Only infer if not explicitly set
        .filter_map(|s| infer_package(&s.command).map(|pkg| (s.name.clone(), pkg)))
        .collect()
}

/// Analysis result for a service's package requirements.
#[derive(Debug, Clone)]
pub struct ServicePackageAnalysis {
    /// Service name.
    pub service_name: String,
    /// The command being analyzed.
    pub command: String,
    /// Analysis result.
    pub result: PackageAnalysisResult,
}

/// Result of analyzing a service's package requirements.
#[derive(Debug, Clone, PartialEq)]
pub enum PackageAnalysisResult {
    /// Has explicit package set.
    Explicit(String),
    /// Package was auto-detected from command.
    AutoDetected(String),
    /// Local binary, no package needed.
    LocalBinary,
    /// Unknown command - user should set package or confirm it's available.
    NeedsAttention(String),
}

/// Analyzes all services and returns detailed package information.
///
/// This is useful for CLI tools or UI to show users what packages will be
/// included and which services might need attention.
pub fn analyze_service_packages(services: &[crate::model::Service]) -> Vec<ServicePackageAnalysis> {
    services
        .iter()
        .filter(|s| s.enabled)
        .map(|s| {
            let result = if let Some(ref pkg) = s.package {
                PackageAnalysisResult::Explicit(pkg.clone())
            } else {
                match infer_package_detailed(&s.command) {
                    PackageInference::Found(pkg) => PackageAnalysisResult::AutoDetected(pkg),
                    PackageInference::LocalBinary => PackageAnalysisResult::LocalBinary,
                    PackageInference::Unknown(exe) => PackageAnalysisResult::NeedsAttention(exe),
                }
            };

            ServicePackageAnalysis {
                service_name: s.name.clone(),
                command: s.command.clone(),
                result,
            }
        })
        .collect()
}

/// Returns services that need user attention (unknown commands without package).
pub fn get_services_needing_attention(
    services: &[crate::model::Service],
) -> Vec<ServicePackageAnalysis> {
    analyze_service_packages(services)
        .into_iter()
        .filter(|a| matches!(a.result, PackageAnalysisResult::NeedsAttention(_)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_command() {
        assert_eq!(extract_executable("node app.js"), Some("node".to_string()));
        assert_eq!(
            extract_executable("python3 -m flask run"),
            Some("python3".to_string())
        );
        assert_eq!(
            extract_executable("redis-server --port 6379"),
            Some("redis-server".to_string())
        );
    }

    #[test]
    fn test_extract_with_path() {
        assert_eq!(
            extract_executable("/usr/bin/node app.js"),
            Some("node".to_string())
        );
        assert_eq!(
            extract_executable("/nix/store/xxx/bin/postgres"),
            Some("postgres".to_string())
        );
    }

    #[test]
    fn test_extract_with_env_vars() {
        assert_eq!(
            extract_executable("NODE_ENV=production node app.js"),
            Some("node".to_string())
        );
        assert_eq!(
            extract_executable("FOO=bar BAZ=qux python3 script.py"),
            Some("python3".to_string())
        );
    }

    #[test]
    fn test_extract_shell_wrapper() {
        assert_eq!(
            extract_executable("bash -c \"node app.js\""),
            Some("node".to_string())
        );
        assert_eq!(
            extract_executable("sh -c 'redis-server'"),
            Some("redis-server".to_string())
        );
    }

    #[test]
    fn test_infer_package() {
        assert_eq!(infer_package("node app.js"), Some("nodejs".to_string()));
        assert_eq!(
            infer_package("python3 -m uvicorn main:app"),
            Some("python3".to_string())
        );
        assert_eq!(
            infer_package("postgres -D /data"),
            Some("postgresql".to_string())
        );
        assert_eq!(
            infer_package("redis-server --port 6379"),
            Some("redis".to_string())
        );
    }

    #[test]
    fn test_infer_unknown_command() {
        assert_eq!(infer_package("./my-custom-binary"), None);
        assert_eq!(infer_package("some-unknown-tool"), None);
    }

    #[test]
    fn test_infer_packages_for_services() {
        use crate::model::Service;

        let services = vec![
            // Has explicit package - should NOT be inferred
            Service::new("db".to_string(), "postgres -D /data".to_string())
                .with_package("postgresql_16".to_string()),
            // No package - should be inferred
            Service::new("api".to_string(), "node server.js".to_string()),
            // No package - should be inferred
            Service::new("cache".to_string(), "redis-server".to_string()),
            // Unknown command - no inference
            Service::new("custom".to_string(), "./run.sh".to_string()),
        ];

        let inferred = infer_packages_for_services(&services);

        assert_eq!(inferred.len(), 2);
        assert!(inferred
            .iter()
            .any(|(name, pkg)| name == "api" && pkg == "nodejs"));
        assert!(inferred
            .iter()
            .any(|(name, pkg)| name == "cache" && pkg == "redis"));
    }

    #[test]
    fn test_is_local_binary() {
        assert!(is_local_binary("./bin/server"));
        assert!(is_local_binary("../build/app"));
        assert!(is_local_binary("/usr/local/bin/custom"));
        assert!(is_local_binary("~/bin/mytool"));
        assert!(is_local_binary("FOO=bar ./run.sh"));

        assert!(!is_local_binary("node app.js"));
        assert!(!is_local_binary("python3 script.py"));
        assert!(!is_local_binary("unknown-tool"));
    }

    #[test]
    fn test_infer_package_detailed() {
        assert_eq!(
            infer_package_detailed("node app.js"),
            PackageInference::Found("nodejs".to_string())
        );
        assert_eq!(
            infer_package_detailed("./my-binary"),
            PackageInference::LocalBinary
        );
        assert_eq!(
            infer_package_detailed("/opt/app/server"),
            PackageInference::LocalBinary
        );
        assert_eq!(
            infer_package_detailed("unknown-tool --flag"),
            PackageInference::Unknown("unknown-tool".to_string())
        );
    }

    #[test]
    fn test_analyze_service_packages() {
        use crate::model::Service;

        let services = vec![
            Service::new("db".to_string(), "postgres".to_string())
                .with_package("postgresql".to_string()),
            Service::new("api".to_string(), "node app.js".to_string()),
            Service::new("worker".to_string(), "./worker.sh".to_string()),
            Service::new("unknown".to_string(), "proprietary-tool".to_string()),
        ];

        let analysis = analyze_service_packages(&services);

        assert_eq!(analysis.len(), 4);

        // Explicit
        assert!(matches!(
            &analysis[0].result,
            PackageAnalysisResult::Explicit(p) if p == "postgresql"
        ));

        // Auto-detected
        assert!(matches!(
            &analysis[1].result,
            PackageAnalysisResult::AutoDetected(p) if p == "nodejs"
        ));

        // Local binary
        assert!(matches!(
            &analysis[2].result,
            PackageAnalysisResult::LocalBinary
        ));

        // Needs attention
        assert!(matches!(
            &analysis[3].result,
            PackageAnalysisResult::NeedsAttention(e) if e == "proprietary-tool"
        ));
    }

    #[test]
    fn test_get_services_needing_attention() {
        use crate::model::Service;

        let services = vec![
            Service::new("api".to_string(), "node app.js".to_string()),
            Service::new("unknown1".to_string(), "proprietary-tool".to_string()),
            Service::new("local".to_string(), "./run.sh".to_string()),
            Service::new("unknown2".to_string(), "another-unknown".to_string()),
        ];

        let needs_attention = get_services_needing_attention(&services);

        assert_eq!(needs_attention.len(), 2);
        assert!(needs_attention.iter().any(|a| a.service_name == "unknown1"));
        assert!(needs_attention.iter().any(|a| a.service_name == "unknown2"));
    }
}
