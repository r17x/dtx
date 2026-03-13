//! Detect inline `writeShellApplication` / `writeShellScriptBin` definitions in nix files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use regex::Regex;
use rnix::SyntaxKind;

use super::imports::ResolvedNixFile;
use super::parser::parse_nix;

/// A detected inline script definition in a nix file.
#[derive(Debug, Clone)]
pub struct DetectedScript {
    /// Script name (e.g. "vault-bootstrap")
    pub name: String,
    /// Source file path
    pub source_file: PathBuf,
    /// Context where the script was found
    pub context: ScriptContext,
    /// The full writeShellApplication/writeShellScriptBin expression text
    pub expr_text: String,
    /// The full attribute assignment text (e.g. `"vault-bootstrap".command = pkgs.writeShellApplication { ... };`)
    pub attr_text: String,
    /// Byte range of the attribute in the source file (start, end)
    pub attr_byte_range: (usize, usize),
}

/// Where the script definition was found.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptContext {
    /// Inside a process-compose block
    ProcessCompose { profile: String },
    /// Other context
    Other,
}

/// Detect inline writeShellApplication/writeShellScriptBin definitions matching the given basenames.
///
/// Scans resolved nix files for function applications of `writeShellApplication` or
/// `writeShellScriptBin`, matches them against `basenames`, and returns structured detection
/// results. Scripts already inside `packages = { ... }` blocks are skipped since they are
/// already exported.
pub fn detect_scripts(basenames: &[String], nix_files: &[ResolvedNixFile]) -> Vec<DetectedScript> {
    let mut results = Vec::new();
    let basenames_set: HashSet<&str> = basenames.iter().map(|s| s.as_str()).collect();

    if basenames_set.is_empty() {
        return results;
    }

    for nix_file in nix_files {
        // Fast filter: skip files that don't contain any relevant strings
        let has_wsa = nix_file.content.contains("writeShellApplication");
        let has_wssb = nix_file.content.contains("writeShellScriptBin");
        if !has_wsa && !has_wssb {
            continue;
        }

        let has_any_basename = basenames
            .iter()
            .any(|b| nix_file.content.contains(b.as_str()));
        if !has_any_basename {
            continue;
        }

        detect_in_file(
            &nix_file.content,
            &nix_file.path,
            &basenames_set,
            &mut results,
        );
    }

    results
}

/// Hybrid detection: use regex to find writeShell* calls, then AST for accurate ranges.
fn detect_in_file(
    content: &str,
    file_path: &Path,
    basenames: &HashSet<&str>,
    results: &mut Vec<DetectedScript>,
) {
    let parsed = match parse_nix(content) {
        Ok(p) => p,
        Err(_) => return,
    };

    // Walk AST looking for NODE_APPLY nodes that are writeShellApplication/writeShellScriptBin calls
    for node in parsed.syntax.descendants() {
        if node.kind() != SyntaxKind::NODE_APPLY {
            continue;
        }

        let children: Vec<_> = node.children().collect();
        if children.len() < 2 {
            continue;
        }

        let func_node = &children[0];
        let func_text = func_node.text().to_string();

        let is_wsa = func_text.ends_with("writeShellApplication");
        let is_wssb = func_text.ends_with("writeShellScriptBin");

        if !is_wsa && !is_wssb {
            continue;
        }

        // Extract the script name
        let script_name = if is_wsa {
            // writeShellApplication { name = "foo"; ... }
            extract_name_from_attrset(&children[1])
        } else {
            // writeShellScriptBin "name" "script-text"
            extract_string_arg(&children[1])
        };

        let Some(script_name) = script_name else {
            continue;
        };

        if !basenames.contains(script_name.as_str()) {
            continue;
        }

        // Check if this is inside a `packages = { ... }` block — skip if so
        if is_inside_packages_block(&node) {
            continue;
        }

        // Find containing NODE_ATTRPATH_VALUE
        let (attr_text, attr_byte_range) =
            find_containing_attr(&node, content).unwrap_or_else(|| {
                let range = node.text_range();
                let start: usize = range.start().into();
                let end: usize = range.end().into();
                (content[start..end].to_string(), (start, end))
            });

        let expr_text = node.text().to_string();

        // Detect context
        let context = detect_context(&node);

        results.push(DetectedScript {
            name: script_name,
            source_file: file_path.to_path_buf(),
            context,
            expr_text,
            attr_text,
            attr_byte_range,
        });
    }
}

/// Extract `name = "value"` from an attribute set node.
fn extract_name_from_attrset(node: &rnix::SyntaxNode) -> Option<String> {
    if node.kind() != SyntaxKind::NODE_ATTR_SET {
        return None;
    }

    for child in node.descendants() {
        if child.kind() != SyntaxKind::NODE_ATTRPATH_VALUE {
            continue;
        }

        let mut children = child.children();
        let Some(attrpath) = children.next() else {
            continue;
        };
        if attrpath.kind() != SyntaxKind::NODE_ATTRPATH {
            continue;
        }
        if attrpath.text().to_string().trim() != "name" {
            continue;
        }

        // Find the value (a string literal)
        for value_child in child.children() {
            if value_child.kind() == SyntaxKind::NODE_STRING {
                return extract_string_content(&value_child);
            }
        }
    }

    None
}

/// Extract the string content from a NODE_STRING node, stripping quotes.
fn extract_string_content(node: &rnix::SyntaxNode) -> Option<String> {
    let text = node.text().to_string();
    // Strip surrounding quotes
    let trimmed = text.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        Some(trimmed[1..trimmed.len() - 1].to_string())
    } else {
        None
    }
}

/// Extract string from a direct string argument (for writeShellScriptBin "name" ...).
fn extract_string_arg(node: &rnix::SyntaxNode) -> Option<String> {
    // writeShellScriptBin "name" "text" is parsed as NODE_APPLY(NODE_APPLY(func, "name"), "text")
    // So the second child of the outer NODE_APPLY is "text", and the inner NODE_APPLY's
    // second child is "name". But we receive the first arg node directly.
    if node.kind() == SyntaxKind::NODE_STRING {
        return extract_string_content(node);
    }
    // It could also be a nested NODE_APPLY for curried application
    // writeShellScriptBin "name" "text" → APPLY(APPLY(writeShellScriptBin, "name"), "text")
    // In that case children[1] at the outer level is "text", and we need the inner apply's arg
    if node.kind() == SyntaxKind::NODE_APPLY {
        let children: Vec<_> = node.children().collect();
        if children.len() >= 2 && children[1].kind() == SyntaxKind::NODE_STRING {
            return extract_string_content(&children[1]);
        }
    }
    None
}

/// Check if a node is inside a `packages = { ... }` block by walking ancestors.
fn is_inside_packages_block(node: &rnix::SyntaxNode) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
            // Check if the attrpath starts with "packages"
            for child in parent.children() {
                if child.kind() == SyntaxKind::NODE_ATTRPATH {
                    let path_text = child.text().to_string();
                    let first_segment = path_text.split('.').next().unwrap_or("").trim();
                    if first_segment == "packages" {
                        return true;
                    }
                }
            }
        }
        current = parent.parent();
    }
    false
}

/// Find the closest ancestor NODE_ATTRPATH_VALUE that directly contains this expression.
fn find_containing_attr(
    node: &rnix::SyntaxNode,
    content: &str,
) -> Option<(String, (usize, usize))> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
            let range = parent.text_range();
            let start: usize = range.start().into();
            let end: usize = range.end().into();
            // Include the trailing semicolon if present
            let end_with_semi = skip_trailing_semicolon(content, end);
            let text = &content[start..end_with_semi];
            return Some((text.to_string(), (start, end_with_semi)));
        }
        current = parent.parent();
    }
    None
}

/// Advance past whitespace and a trailing semicolon.
fn skip_trailing_semicolon(content: &str, pos: usize) -> usize {
    let rest = &content[pos..];
    let trimmed = rest.trim_start();
    let whitespace_len = rest.len() - trimmed.len();
    if trimmed.starts_with(';') {
        pos + whitespace_len + 1
    } else {
        pos
    }
}

/// Detect whether the node is inside a process-compose block and extract profile name.
fn detect_context(node: &rnix::SyntaxNode) -> ScriptContext {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
            for child in parent.children() {
                if child.kind() == SyntaxKind::NODE_ATTRPATH {
                    let path_text = child.text().to_string();
                    if path_text.contains("process-compose") {
                        // Extract profile name: process-compose."dev" → "dev"
                        let profile = extract_profile_from_attrpath(&path_text);
                        return ScriptContext::ProcessCompose { profile };
                    }
                }
            }
        }
        current = parent.parent();
    }
    ScriptContext::Other
}

/// Extract profile name from an attrpath like `process-compose."dev"`.
fn extract_profile_from_attrpath(path: &str) -> String {
    let re = Regex::new(r#"process-compose\s*\.\s*"([^"]+)""#).unwrap();
    if let Some(caps) = re.captures(path) {
        return caps[1].to_string();
    }
    // Try unquoted: process-compose.dev
    let parts: Vec<&str> = path.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        if part.trim().contains("process-compose") {
            if let Some(next) = parts.get(i + 1) {
                let profile = next.trim().trim_matches('"');
                if !profile.is_empty() {
                    return profile.to_string();
                }
            }
        }
    }
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(content: &str) -> ResolvedNixFile {
        ResolvedNixFile {
            path: PathBuf::from("/test/dev.nix"),
            content: content.to_string(),
        }
    }

    #[test]
    fn detect_write_shell_application() {
        let content = r#"{
  process-compose."dev" = {
    settings.processes = {
      "vault-bootstrap".command = pkgs.writeShellApplication {
        name = "vault-bootstrap";
        text = "echo hi";
      };
    };
  };
}"#;
        let files = vec![make_file(content)];
        let basenames = vec!["vault-bootstrap".to_string()];
        let results = detect_scripts(&basenames, &files);

        assert_eq!(results.len(), 1, "expected 1 result, got {:?}", results);
        assert_eq!(results[0].name, "vault-bootstrap");
        assert_eq!(
            results[0].context,
            ScriptContext::ProcessCompose {
                profile: "dev".to_string()
            }
        );
        assert!(results[0].expr_text.contains("writeShellApplication"));
        assert!(results[0].attr_text.contains("vault-bootstrap"));
    }

    #[test]
    fn detect_write_shell_script_bin() {
        let content = r#"{
  "pg-setup".command =
    let
      pgSetupCMD = pkgs.writeShellScriptBin "pg-setup" "echo hi";
    in
    "some-command";
}"#;
        let files = vec![make_file(content)];
        let basenames = vec!["pg-setup".to_string()];
        let results = detect_scripts(&basenames, &files);

        assert_eq!(results.len(), 1, "expected 1 result, got {:?}", results);
        assert_eq!(results[0].name, "pg-setup");
    }

    #[test]
    fn skip_already_in_packages() {
        let content = r#"{
  packages = {
    caddy-dev = pkgs.writeShellApplication {
      name = "caddy-dev";
      text = "echo hi";
    };
  };
}"#;
        let files = vec![make_file(content)];
        let basenames = vec!["caddy-dev".to_string()];
        let results = detect_scripts(&basenames, &files);

        assert_eq!(
            results.len(),
            0,
            "expected 0 results (inside packages block), got {:?}",
            results
        );
    }

    #[test]
    fn multiple_scripts() {
        let content = r#"{
  process-compose."dev" = {
    settings.processes = {
      node_modules.command = pkgs.writeShellApplication {
        name = "install";
        text = "bun install";
      };
      "vault-bootstrap".command = pkgs.writeShellApplication {
        name = "vault-bootstrap";
        text = "echo hi";
      };
      garage = {
        command = pkgs.writeShellApplication {
          name = "garage-server";
          text = "exec garage";
        };
      };
    };
  };
}"#;
        let files = vec![make_file(content)];
        let basenames = vec!["install".to_string(), "garage-server".to_string()];
        let results = detect_scripts(&basenames, &files);

        assert_eq!(results.len(), 2, "expected 2 results, got {:?}", results);
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"install"), "missing 'install': {:?}", names);
        assert!(
            names.contains(&"garage-server"),
            "missing 'garage-server': {:?}",
            names
        );
    }

    #[test]
    fn no_match() {
        let content = r#"{
  process-compose."dev" = {
    settings.processes = {
      foo.command = pkgs.writeShellApplication {
        name = "foo";
        text = "echo foo";
      };
    };
  };
}"#;
        let files = vec![make_file(content)];
        let basenames = vec!["nonexistent".to_string()];
        let results = detect_scripts(&basenames, &files);

        assert_eq!(results.len(), 0, "expected 0 results, got {:?}", results);
    }
}
