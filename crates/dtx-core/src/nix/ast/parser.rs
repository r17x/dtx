//! Nix parser wrapper using rnix.

use crate::error::NixError;
use rnix::{Root, SyntaxKind, SyntaxNode};
use rowan::ast::AstNode;

/// Parsed Nix file with AST root.
pub struct ParsedNix {
    /// The syntax tree root node.
    pub syntax: SyntaxNode,
}

/// Parse Nix source and return AST root.
pub fn parse_nix(content: &str) -> Result<ParsedNix, NixError> {
    let parsed = Root::parse(content);

    if !parsed.errors().is_empty() {
        let error_msgs: Vec<String> = parsed.errors().iter().map(|e| format!("{:?}", e)).collect();
        return Err(NixError::ParseError(error_msgs.join(", ")));
    }

    // Get the syntax tree from the parse result
    let root = parsed.tree();

    Ok(ParsedNix {
        syntax: root.syntax().clone(),
    })
}

/// Check if a Nix expression is valid.
pub fn validate_nix(content: &str) -> bool {
    Root::parse(content).errors().is_empty()
}

/// Validate content as a flake.nix file.
///
/// Beyond syntax checking, this verifies structural requirements:
/// - Top-level expression must be an attribute set (`{ ... }`)
/// - Must contain an `outputs` attribute
pub fn validate_flake_nix(content: &str) -> Result<(), String> {
    let parsed = Root::parse(content);

    if !parsed.errors().is_empty() {
        let error_msgs: Vec<String> = parsed.errors().iter().map(|e| format!("{:?}", e)).collect();
        return Err(format!("Syntax error: {}", error_msgs.join(", ")));
    }

    let root = parsed.tree();
    let syntax = root.syntax();

    // Find the top-level expression (skip whitespace/comments)
    let top_expr = syntax.children().find(|n| {
        n.kind() != SyntaxKind::TOKEN_WHITESPACE && n.kind() != SyntaxKind::TOKEN_COMMENT
    });

    let Some(top_expr) = top_expr else {
        return Err("Empty file — expected an attribute set".to_string());
    };

    // Top-level must be an attribute set: NODE_ATTR_SET
    if top_expr.kind() != SyntaxKind::NODE_ATTR_SET {
        return Err(format!(
            "Top-level expression must be an attribute set ({{ ... }}), found {:?}",
            top_expr.kind()
        ));
    }

    // Check for `outputs` attribute
    let has_outputs = top_expr.descendants().any(|n| {
        if n.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
            // Check if any attrpath child contains "outputs"
            n.children().any(|child| {
                child.kind() == SyntaxKind::NODE_ATTRPATH
                    && child.text().to_string().starts_with("outputs")
            })
        } else {
            false
        }
    });

    if !has_outputs {
        return Err("Missing required `outputs` attribute".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_nix() {
        let content = r#"{ pkgs }: pkgs.mkShell { buildInputs = [ pkgs.hello ]; }"#;
        assert!(parse_nix(content).is_ok());
    }

    #[test]
    fn test_parse_invalid_nix() {
        let content = r#"{ pkgs: pkgs.mkShell"#; // Missing closing bracket
        assert!(parse_nix(content).is_err());
    }

    #[test]
    fn test_validate_nix_valid() {
        assert!(validate_nix("{ x = 1; }"));
    }

    #[test]
    fn test_validate_nix_invalid() {
        assert!(!validate_nix("{ x = ; }"));
    }

    #[test]
    fn test_validate_flake_valid() {
        let content = r#"{
  outputs = { nixpkgs, ... }: { };
}"#;
        assert!(validate_flake_nix(content).is_ok());
    }

    #[test]
    fn test_validate_flake_full() {
        let content = r#"{
  description = "test";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = inputs: { };
}"#;
        assert!(validate_flake_nix(content).is_ok());
    }

    #[test]
    fn test_validate_flake_rejects_gibberish() {
        let result = validate_flake_nix("hello world foo bar");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("attribute set"));
    }

    #[test]
    fn test_validate_flake_rejects_missing_outputs() {
        let result = validate_flake_nix("{ description = \"test\"; }");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("outputs"));
    }

    #[test]
    fn test_validate_flake_rejects_syntax_error() {
        let result = validate_flake_nix("{ x = ; }");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Syntax error"));
    }

    #[test]
    fn test_parse_flake() {
        let content = r#"{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { nixpkgs, ... }: {
    devShells.default = nixpkgs.legacyPackages.x86_64-linux.mkShell {
      packages = [ ];
    };
  };
}"#;
        assert!(parse_nix(content).is_ok());
    }
}
