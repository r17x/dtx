//! Flake.nix generation for Nix development environments.

use super::command::infer_package;
use crate::model::Service;
use std::collections::HashSet;

/// Generator for flake.nix files.
pub struct FlakeGenerator;

impl FlakeGenerator {
    /// Generates a flake.nix file from a list of services.
    ///
    /// Automatically detects required packages from service commands when
    /// `package` is not explicitly set. For example, a service with command
    /// `node app.js` will automatically include `nodejs` in the flake.
    ///
    /// # Arguments
    ///
    /// * `services` - The services to extract packages from
    /// * `project_name` - Name of the project for the flake description
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::nix::FlakeGenerator;
    /// use dtx_core::model::Service;
    ///
    /// let services = vec![
    ///     // Explicit package
    ///     Service::new("db".to_string(), "postgres".to_string())
    ///         .with_package("postgresql".to_string()),
    ///     // Auto-detected from command
    ///     Service::new("api".to_string(), "node server.js".to_string()),
    /// ];
    ///
    /// let flake = FlakeGenerator::generate(&services, "my-project");
    /// assert!(flake.contains("postgresql"));
    /// assert!(flake.contains("nodejs")); // Auto-detected!
    /// assert!(flake.contains("process-compose"));
    /// ```
    pub fn generate(services: &[Service], project_name: &str) -> String {
        let packages = Self::collect_packages(services);
        let packages_str = packages
            .iter()
            .map(|p| format!("              {}", p))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"{{
  description = "{project_name} - managed by dtx";

  inputs = {{
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  }};

  outputs = inputs:
    inputs.flake-parts.lib.mkFlake {{ inherit inputs; }} {{
      systems = [ "aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux" ];

      perSystem = {{ pkgs, ... }}: {{
        devShells.default = pkgs.mkShell {{
          packages = with pkgs; [
{packages_str}
              process-compose
          ];

          shellHook = ''
            export PROJECT_ROOT="$PWD"
            export DATA_DIR="$PROJECT_ROOT/.data"
            mkdir -p "$DATA_DIR"

            echo "🦀 {project_name} development environment"
            echo "   Run 'dtx start' to start services"
          '';
        }};
      }};
    }};
}}
"#,
            project_name = project_name,
            packages_str = if packages.is_empty() {
                "              # Add nixpkgs to your services".to_string()
            } else {
                packages_str
            }
        )
    }

    /// Collects unique Nix packages from services.
    ///
    /// Uses explicit `package` when set, otherwise infers from command.
    fn collect_packages(services: &[Service]) -> HashSet<String> {
        services
            .iter()
            .filter(|s| s.enabled)
            .filter_map(|s| {
                // Use explicit package if set, otherwise infer from command
                s.package.clone().or_else(|| infer_package(&s.command))
            })
            .collect()
    }

    /// Generates a minimal flake for testing or empty projects.
    pub fn generate_minimal(project_name: &str) -> String {
        Self::generate(&[], project_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_flake_with_packages() {
        let services = vec![
            Service::new("db".to_string(), "postgres -D $PGDATA".to_string())
                .with_package("postgresql".to_string()),
            Service::new("cache".to_string(), "redis-server".to_string())
                .with_package("redis".to_string()),
        ];

        let flake = FlakeGenerator::generate(&services, "my-project");

        assert!(flake.contains("postgresql"));
        assert!(flake.contains("redis"));
        assert!(flake.contains("process-compose"));
        assert!(flake.contains("my-project"));
        assert!(flake.contains("nixpkgs-unstable"));
    }

    #[test]
    fn test_generate_empty_flake() {
        let flake = FlakeGenerator::generate(&[], "empty-project");

        assert!(flake.contains("process-compose"));
        assert!(flake.contains("empty-project"));
        assert!(flake.contains("Add nixpkgs to your services"));
    }

    #[test]
    fn test_minimal_flake() {
        let flake = FlakeGenerator::generate_minimal("test");

        assert!(flake.contains("test"));
        assert!(flake.contains("process-compose"));
    }

    #[test]
    fn test_duplicate_packages() {
        let services = vec![
            Service::new("db1".to_string(), "postgres1".to_string())
                .with_package("postgresql".to_string()),
            Service::new("db2".to_string(), "postgres2".to_string())
                .with_package("postgresql".to_string()),
        ];

        let flake = FlakeGenerator::generate(&services, "test");

        // Should only include postgresql once
        let count = flake.matches("postgresql").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_disabled_services_excluded() {
        let services = vec![
            Service::new("db".to_string(), "postgres".to_string())
                .with_package("postgresql".to_string()),
            Service::new("cache".to_string(), "redis".to_string())
                .with_package("redis".to_string())
                .disabled(),
        ];

        let flake = FlakeGenerator::generate(&services, "test");

        assert!(flake.contains("postgresql"));
        assert!(!flake.contains("redis"));
    }

    #[test]
    fn test_flake_structure() {
        let flake = FlakeGenerator::generate_minimal("test");

        // Verify essential flake structure
        assert!(flake.contains("description"));
        assert!(flake.contains("inputs"));
        assert!(flake.contains("outputs"));
        assert!(flake.contains("devShells.default"));
        assert!(flake.contains("mkShell"));
        assert!(flake.contains("shellHook"));
    }

    #[test]
    fn test_project_root_setup() {
        let flake = FlakeGenerator::generate_minimal("test");

        assert!(flake.contains("PROJECT_ROOT"));
        assert!(flake.contains("DATA_DIR"));
        assert!(flake.contains("mkdir -p"));
    }

    #[test]
    fn test_auto_detect_package_from_command() {
        // Service with no package but detectable command
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string()),
            Service::new("worker".to_string(), "python3 worker.py".to_string()),
        ];

        let flake = FlakeGenerator::generate(&services, "test");

        assert!(
            flake.contains("nodejs"),
            "Should auto-detect nodejs from 'node' command"
        );
        assert!(
            flake.contains("python3"),
            "Should auto-detect python3 from 'python3' command"
        );
    }

    #[test]
    fn test_explicit_package_overrides_inference() {
        // Explicit package should be used, not inferred
        let services = vec![
            Service::new("db".to_string(), "postgres -D /data".to_string())
                .with_package("postgresql_16".to_string()), // Explicit version
        ];

        let flake = FlakeGenerator::generate(&services, "test");

        assert!(
            flake.contains("postgresql_16"),
            "Should use explicit package"
        );
        // Should NOT contain the inferred "postgresql" (without version)
        let count = flake.matches("postgresql").count();
        assert_eq!(count, 1, "Should only have one postgresql entry");
    }

    #[test]
    fn test_mixed_explicit_and_inferred() {
        let services = vec![
            // Explicit
            Service::new("db".to_string(), "postgres".to_string())
                .with_package("postgresql".to_string()),
            // Inferred
            Service::new("api".to_string(), "node app.js".to_string()),
            // Unknown (no package)
            Service::new("custom".to_string(), "./run.sh".to_string()),
        ];

        let flake = FlakeGenerator::generate(&services, "test");

        assert!(flake.contains("postgresql"));
        assert!(flake.contains("nodejs"));
        assert!(!flake.contains("run.sh")); // Custom scripts don't add packages
    }
}
