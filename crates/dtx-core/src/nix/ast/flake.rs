//! AST-based flake.nix manipulation.
//!
//! Provides type-safe generation and modification of flake.nix files
//! using rnix-parser, preserving user formatting and comments.

use crate::error::NixError;
use crate::model::Service;
use std::collections::HashSet;

/// AST-based flake.nix manipulation.
///
/// Preserves user formatting, comments, and modifications when editing.
pub struct FlakeAst {
    content: String,
}

impl FlakeAst {
    /// Parse existing flake.nix content.
    pub fn parse(content: &str) -> Result<Self, NixError> {
        super::parser::parse_nix(content)?;
        Ok(Self {
            content: content.to_string(),
        })
    }

    /// Create new flake for a devShell from services.
    pub fn new_devshell(services: &[Service], project_name: &str) -> Self {
        let packages = Self::collect_packages(services);
        let packages_str = packages
            .iter()
            .map(|p| format!("              {}", p))
            .collect::<Vec<_>>()
            .join("\n");

        let packages_section = if packages.is_empty() {
            "              # Add nixpkgs to your services".to_string()
        } else {
            packages_str
        };

        let content = format!(
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
{packages_section}
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
"#
        );

        // Parse the generated content (should always succeed)
        Self::parse(&content).expect("Generated flake should be valid")
    }

    /// Add a package to the devShell packages list.
    pub fn add_package(&mut self, package: &str) -> Result<(), NixError> {
        // Find the packages list - look for "process-compose" which should be the last item
        if let Some(pos) = self.content.find("process-compose") {
            // Insert before process-compose
            let indent = "              ";

            // Find the start of the process-compose line
            let line_start = self.content[..pos]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(pos);

            let mut new_content = self.content.clone();
            new_content.insert_str(line_start, &format!("{}{}\n", indent, package));

            // Re-parse to validate
            if !super::parser::validate_nix(&new_content) {
                return Err(NixError::ParseError("Failed to add package".to_string()));
            }

            self.content = new_content;
            Ok(())
        } else {
            Err(NixError::ParseError(
                "Could not find packages list".to_string(),
            ))
        }
    }

    /// Remove a package from the devShell packages list.
    pub fn remove_package(&mut self, package: &str) -> Result<bool, NixError> {
        // Find the package line with indentation
        let search_patterns = [
            format!("\n              {}\n", package),
            format!("\n              {}", package),
        ];

        for pattern in &search_patterns {
            if let Some(pos) = self.content.find(pattern) {
                let mut new_content = self.content.clone();
                new_content.replace_range(pos..pos + pattern.len(), "\n");

                // Re-parse to validate
                if !super::parser::validate_nix(&new_content) {
                    return Err(NixError::ParseError("Failed to remove package".to_string()));
                }

                self.content = new_content;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// List packages currently in the flake.
    pub fn list_packages(&self) -> Vec<String> {
        let mut packages = Vec::new();
        let mut in_packages = false;

        for line in self.content.lines() {
            let trimmed = line.trim();

            // Detect start of packages list
            if trimmed.contains("packages = with pkgs; [") {
                in_packages = true;
                continue;
            }

            // Detect end of packages list
            if in_packages && trimmed.contains("];") {
                break;
            }

            // Extract package names
            if in_packages {
                // Skip comments
                if trimmed.starts_with('#') {
                    continue;
                }

                // Package name should be alphanumeric with dashes/underscores
                if !trimmed.is_empty()
                    && trimmed
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    packages.push(trimmed.to_string());
                }
            }
        }

        packages
    }

    /// Get the content as a string slice.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// Check if the AST is valid.
    pub fn is_valid(&self) -> bool {
        super::parser::validate_nix(&self.content)
    }

    /// Collect unique packages from services.
    fn collect_packages(services: &[Service]) -> HashSet<String> {
        services
            .iter()
            .filter(|s| s.enabled)
            .filter_map(|s| s.package.clone())
            .collect()
    }
}

impl std::fmt::Display for FlakeAst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_devshell() {
        let services = vec![
            Service::new("db".to_string(), "postgres".to_string())
                .with_package("postgresql".to_string()),
            Service::new("cache".to_string(), "redis-server".to_string())
                .with_package("redis".to_string()),
        ];

        let flake = FlakeAst::new_devshell(&services, "test-project");
        let content = flake.to_string();

        assert!(content.contains("postgresql"));
        assert!(content.contains("redis"));
        assert!(content.contains("process-compose"));
        assert!(content.contains("test-project"));
        assert!(flake.is_valid());
    }

    #[test]
    fn test_add_package() {
        let services = vec![];
        let mut flake = FlakeAst::new_devshell(&services, "test");

        flake.add_package("nodejs").unwrap();
        let content = flake.to_string();

        assert!(content.contains("nodejs"));
        assert!(flake.is_valid());
    }

    #[test]
    fn test_remove_package() {
        let services =
            vec![Service::new("db".to_string(), "pg".to_string())
                .with_package("postgresql".to_string())];
        let mut flake = FlakeAst::new_devshell(&services, "test");

        assert!(flake.to_string().contains("postgresql"));

        let removed = flake.remove_package("postgresql").unwrap();
        assert!(removed);
        assert!(!flake.to_string().contains("postgresql"));
        assert!(flake.is_valid());
    }

    #[test]
    fn test_list_packages() {
        let services = vec![
            Service::new("db".to_string(), "postgres".to_string())
                .with_package("postgresql".to_string()),
            Service::new("cache".to_string(), "redis-server".to_string())
                .with_package("redis".to_string()),
        ];

        let flake = FlakeAst::new_devshell(&services, "test");
        let packages = flake.list_packages();

        assert!(packages.contains(&"postgresql".to_string()));
        assert!(packages.contains(&"redis".to_string()));
        assert!(packages.contains(&"process-compose".to_string()));
    }

    #[test]
    fn test_parse_existing() {
        let content = r#"{
  outputs = { nixpkgs, ... }: {
    devShells.default = nixpkgs.legacyPackages.x86_64-linux.mkShell {
      packages = [ nixpkgs.hello ];
    };
  };
}"#;

        let result = FlakeAst::parse(content);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_flake() {
        let flake = FlakeAst::new_devshell(&[], "empty-project");
        let content = flake.to_string();

        assert!(content.contains("# Add nixpkgs"));
        assert!(content.contains("process-compose"));
        assert!(flake.is_valid());
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

        let flake = FlakeAst::new_devshell(&services, "test");
        let content = flake.to_string();

        assert!(content.contains("postgresql"));
        assert!(!content.contains("redis"));
    }
}
