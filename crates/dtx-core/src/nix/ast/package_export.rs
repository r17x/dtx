//! Extract inline writeShellApplication definitions to the `packages` block.
//!
//! Given detected scripts (from `script_detection`), this module rewrites the nix source
//! to move each inline definition into `packages = { ... }` and replaces the original
//! site with a reference (e.g. `self'.packages.vault-bootstrap`).

use std::collections::HashMap;
use std::path::PathBuf;
use once_cell::sync::Lazy;
use regex::Regex;

use super::parser::parse_nix;
use super::script_detection::DetectedScript;
use crate::error::NixError;

/// Result of exporting detected scripts as packages.
pub struct ExportResult {
    /// Files that were modified.
    pub modified_files: Vec<PathBuf>,
    /// Names of packages that were exported.
    pub exported_packages: Vec<String>,
    /// Non-fatal warnings (e.g. skipped duplicates).
    pub warnings: Vec<String>,
}

/// Export detected inline scripts as entries in the `packages` block of their source files.
///
/// For each script, the inline `writeShellApplication { ... }` expression is moved to a let
/// binding inside `packages = let ... in { ... }` (or `packages = { ... }` if no let block
/// exists), and the original site is replaced with a reference matching any existing pattern
/// found in the file (e.g. `self'.packages.<name>`).
pub fn export_scripts_as_packages(scripts: &[DetectedScript]) -> Result<ExportResult, NixError> {
    if scripts.is_empty() {
        return Ok(ExportResult {
            modified_files: vec![],
            exported_packages: vec![],
            warnings: vec![],
        });
    }

    // Group scripts by source file
    let mut by_file: HashMap<PathBuf, Vec<&DetectedScript>> = HashMap::new();
    for script in scripts {
        by_file
            .entry(script.source_file.clone())
            .or_default()
            .push(script);
    }

    let mut result = ExportResult {
        modified_files: vec![],
        exported_packages: vec![],
        warnings: vec![],
    };

    for (file_path, file_scripts) in &by_file {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| NixError::IoError(format!("{}: {}", file_path.display(), e)))?;

        match process_file(&content, file_scripts) {
            Ok((modified, exported, warnings)) => {
                if exported.is_empty() {
                    result.warnings.extend(warnings);
                    continue;
                }

                // Validate the modified content parses correctly
                if let Err(e) = parse_nix(&modified) {
                    result.warnings.push(format!(
                        "{}: modified content failed validation ({}), skipping",
                        file_path.display(),
                        e
                    ));
                    continue;
                }

                std::fs::write(file_path, &modified)
                    .map_err(|e| NixError::IoError(format!("{}: {}", file_path.display(), e)))?;

                result.modified_files.push(file_path.clone());
                result.exported_packages.extend(exported);
                result.warnings.extend(warnings);
            }
            Err(e) => {
                result
                    .warnings
                    .push(format!("{}: {}", file_path.display(), e));
            }
        }
    }

    Ok(result)
}

/// Process a single file: extract scripts to packages block, return (modified_content, exported_names, warnings).
fn process_file(
    content: &str,
    scripts: &[&DetectedScript],
) -> Result<(String, Vec<String>, Vec<String>), NixError> {
    let mut warnings = Vec::new();
    let mut exported = Vec::new();

    // Detect existing reference pattern (e.g. `self'.packages`)
    let ref_prefix = detect_reference_prefix(content);

    // Find the packages block
    let pkg_block = find_packages_block(content);

    // Check which packages already exist
    let existing_packages = match &pkg_block {
        Some(block) => extract_existing_package_names(&content[block.inner_start..block.inner_end]),
        None => vec![],
    };

    // Sort scripts by byte range start descending so we can replace without invalidating offsets
    let mut sorted_scripts: Vec<&DetectedScript> = scripts.to_vec();
    sorted_scripts.sort_by(|a, b| b.attr_byte_range.0.cmp(&a.attr_byte_range.0));

    let mut modified = content.to_string();
    // Track insertions to the packages block — collect them and insert in one pass
    let mut package_entries: Vec<(String, String)> = Vec::new(); // (name, expr_text)

    for script in &sorted_scripts {
        // Check if already exported
        if existing_packages.contains(&script.name)
            || package_entries.iter().any(|(n, _)| n == &script.name)
        {
            warnings.push(format!(
                "'{}' already exists in packages block, skipping",
                script.name
            ));
            continue;
        }

        // Build the reference expression
        let reference = build_reference(&script.name, ref_prefix.as_deref());

        // Replace the inline expression with the reference
        let (start, end) = script.attr_byte_range;
        let replacement = build_replacement(&script.attr_text, &reference);
        modified.replace_range(start..end, &replacement);

        package_entries.push((script.name.clone(), script.expr_text.clone()));
        exported.push(script.name.clone());
    }

    // Insert all new package entries into the packages block
    if !package_entries.is_empty() {
        if let Some(block) = find_packages_block(&modified) {
            let indent = detect_indent(&modified, &block);
            // Insert before the closing brace of the packages block
            let mut insertion = String::new();
            for (name, expr) in &package_entries {
                insertion.push_str(&format!("\n{}{} = {};", indent, name, expr));
            }
            modified.insert_str(block.inner_end, &insertion);
        } else {
            // No packages block — warn
            warnings
                .push("no packages block found, could not insert package definitions".to_string());
            return Ok((content.to_string(), vec![], warnings));
        }
    }

    Ok((modified, exported, warnings))
}

/// Detected reference prefix like `self'.packages`.
fn detect_reference_prefix(content: &str) -> Option<String> {
    // Look for patterns like `<something>.command = <prefix>.<package-name>;`
    // where the prefix looks like a module path (e.g. self'.packages)
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\.command\s*=\s*([a-zA-Z_'][a-zA-Z_'.]*)\.[a-zA-Z_][a-zA-Z0-9_-]*\s*;")
            .unwrap()
    });
    let re = &*RE;

    for cap in re.captures_iter(content) {
        let prefix = &cap[1];
        // Validate it looks like a package reference (contains "packages")
        if prefix.contains("packages") {
            return Some(prefix.to_string());
        }
    }
    None
}

/// Build a reference expression for a package.
fn build_reference(name: &str, prefix: Option<&str>) -> String {
    match prefix {
        Some(p) => format!("{}.{}", p, name),
        None => name.to_string(),
    }
}

/// Build the replacement text for an attribute assignment.
///
/// Handles two cases:
/// - Direct: `"name".command = <expr>;` → `"name".command = <reference>;`
/// - Nested: `name = { command = <expr>; ... };` → replace only the command value
fn build_replacement(attr_text: &str, reference: &str) -> String {
    // Check if this is a nested attr set (e.g. `garage = { command = pkgs.writeShell...; };`)
    // by looking for `= {` before the writeShell part
    let ws_pos = attr_text
        .find("writeShellApplication")
        .or_else(|| attr_text.find("writeShellScriptBin"));

    let Some(ws_pos) = ws_pos else {
        return attr_text.to_string();
    };

    // Find `command =` before the writeShell call
    let before_ws = &attr_text[..ws_pos];
    static CMD_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"command\s*=\s*\S*\s*$").unwrap());
    let command_eq_re = &*CMD_RE;

    if let Some(cmd_match) = command_eq_re.find(before_ws) {
        // Check if there's a `= {` before `command =` (nested attr set)
        let before_command = &attr_text[..cmd_match.start()];
        let is_nested = before_command.contains("= {") || before_command.contains("=\n");

        if is_nested {
            // Nested: replace from `command = <writeShell...>` to end of that expression
            // Find the end of the writeShell expression (matching braces)
            let expr_end = find_expression_end(attr_text, ws_pos);
            // Include trailing semicolon
            let end_with_semi = skip_ws_and_semi(attr_text, expr_end);
            let mut result = String::new();
            result.push_str(&attr_text[..cmd_match.start()]);
            result.push_str("command = ");
            result.push_str(reference);
            result.push(';');
            if end_with_semi < attr_text.len() {
                result.push_str(&attr_text[end_with_semi..]);
            }
            return result;
        }
    }

    // Direct: find `= <prefix>.writeShell...` and replace the value
    // The attr_text is something like: `"vault-bootstrap".command = pkgs.writeShellApplication { ... };`
    // We want to replace everything after `= ` up to the final `;`
    if let Some(eq_pos) = before_ws.rfind('=') {
        let mut result = String::new();
        result.push_str(&attr_text[..=eq_pos]);
        result.push(' ');
        result.push_str(reference);
        result.push(';');
        return result;
    }

    attr_text.to_string()
}

/// Find the end of a brace-delimited expression starting near `start`.
fn find_expression_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0;
    let mut i = start;
    let mut found_open = false;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                found_open = true;
            }
            b'}' => {
                depth -= 1;
                if found_open && depth == 0 {
                    return i + 1;
                }
            }
            b'"' => {
                // Skip string contents
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1; // skip escaped char
                    }
                    i += 1;
                }
            }
            b'\'' if i + 1 < bytes.len() && bytes[i + 1] == b'\'' => {
                // Skip multi-line string ''...''
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'\'' && bytes[i + 1] == b'\'') {
                    i += 1;
                }
                if i + 1 < bytes.len() {
                    i += 1; // skip closing ''
                }
            }
            _ => {}
        }
        i += 1;
    }

    text.len()
}

/// Skip whitespace and a trailing semicolon.
fn skip_ws_and_semi(text: &str, pos: usize) -> usize {
    let rest = &text[pos..];
    let trimmed = rest.trim_start();
    let ws_len = rest.len() - trimmed.len();
    if trimmed.starts_with(';') {
        pos + ws_len + 1
    } else {
        pos
    }
}

/// Location of a `packages = ... { <entries> }` block.
struct PackagesBlock {
    /// Byte offset of the character after the opening `{` of the entries block.
    inner_start: usize,
    /// Byte offset of the closing `}` of the entries block.
    inner_end: usize,
}

/// Find the `packages =` block, handling both `packages = { ... }` and `packages = let ... in { ... }`.
fn find_packages_block(content: &str) -> Option<PackagesBlock> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*packages\s*=").unwrap());
    let re = &*RE;
    let m = re.find(content)?;
    let after_eq = m.end();

    // Skip whitespace
    let rest = content[after_eq..].trim_start();
    let offset = after_eq + (content.len() - after_eq - content[after_eq..].trim_start().len());

    // Check for `let ... in` before the opening brace
    let brace_search_start = if rest.starts_with("let") {
        // Find `in` followed by `{`
        let rest_from_let = &content[offset..];
        // Find matching `in` — we need to skip nested let/in
        if let Some(in_match) = find_top_level_in(rest_from_let) {
            offset + in_match
        } else {
            offset
        }
    } else {
        offset
    };

    // Find the opening `{`
    let remaining = &content[brace_search_start..];
    let trimmed = remaining.trim_start();
    let ws = remaining.len() - trimmed.len();

    if !trimmed.starts_with('{') {
        return None;
    }

    let open_brace = brace_search_start + ws;
    let inner_start = open_brace + 1;

    // Find matching closing brace
    let close_brace = find_matching_brace(content, open_brace)?;

    Some(PackagesBlock {
        inner_start,
        inner_end: close_brace,
    })
}

/// Find the top-level `in` keyword position (skipping nested let/in pairs).
fn find_top_level_in(text: &str) -> Option<usize> {
    let mut depth = 0;
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip strings
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'\'' && bytes[i + 1] == b'\'' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'\'' && bytes[i + 1] == b'\'') {
                i += 1;
            }
            i += 2;
            continue;
        }

        // Check for keywords (must be at word boundary)
        if is_word_boundary(text, i) {
            if text[i..].starts_with("let") && is_word_end(text, i + 3) {
                depth += 1;
                i += 3;
                continue;
            }
            if text[i..].starts_with("in") && is_word_end(text, i + 2) {
                if depth <= 1 {
                    return Some(i + 2); // position after "in"
                }
                depth -= 1;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    None
}

fn is_word_boundary(text: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let prev = text.as_bytes()[pos - 1];
    !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'\'' && prev != b'-'
}

fn is_word_end(text: &str, pos: usize) -> bool {
    if pos >= text.len() {
        return true;
    }
    let next = text.as_bytes()[pos];
    !next.is_ascii_alphanumeric() && next != b'_' && next != b'\'' && next != b'-'
}

/// Find the matching closing brace for an opening brace at `pos`.
fn find_matching_brace(content: &str, pos: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if bytes[pos] != b'{' {
        return None;
    }
    let mut depth = 1;
    let mut i = pos + 1;

    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' if i + 1 < bytes.len() && bytes[i + 1] == b'\'' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'\'' && bytes[i + 1] == b'\'') {
                    i += 1;
                }
                i += 1; // skip second '
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract names of existing entries in a packages block body.
fn extract_existing_package_names(block_body: &str) -> Vec<String> {
    // Match both unquoted (`foo =`) and quoted (`"foo" =`) attribute names
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?m)^\s*(?:"([^"]+)"|([a-zA-Z_][a-zA-Z0-9_-]*))\s*="#).unwrap()
    });
    RE.captures_iter(block_body)
        .filter_map(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .map(|m| m.as_str().to_string())
        })
        .collect()
}

/// Detect indentation used inside the packages block.
fn detect_indent(content: &str, block: &PackagesBlock) -> String {
    let body = &content[block.inner_start..block.inner_end];
    // Find the last non-empty line's indentation
    for line in body.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            let indent_len = line.len() - line.trim_start().len();
            return line[..indent_len].to_string();
        }
    }
    // Fallback: 4 spaces
    "    ".to_string()
}

#[cfg(test)]
mod tests {
    use super::super::script_detection::ScriptContext;
    use super::*;
    use std::path::{Path, PathBuf};

    fn make_script(
        name: &str,
        source_file: &Path,
        expr_text: &str,
        attr_text: &str,
        byte_range: (usize, usize),
    ) -> DetectedScript {
        DetectedScript {
            name: name.to_string(),
            source_file: source_file.to_path_buf(),
            context: ScriptContext::ProcessCompose {
                profile: "dev".to_string(),
            },
            expr_text: expr_text.to_string(),
            attr_text: attr_text.to_string(),
            attr_byte_range: byte_range,
        }
    }

    #[test]
    fn export_to_existing_packages() {
        let content = r#"{
  packages = {
    existing = pkgs.hello;
  };
  process-compose."dev" = {
    settings.processes = {
      foo.command = pkgs.writeShellApplication {
        name = "foo";
        text = "echo foo";
      };
    };
  };
}"#;
        let attr_start = content
            .find("foo.command = pkgs.writeShellApplication")
            .unwrap();
        let attr_end = content[attr_start..].find("};").unwrap() + attr_start + 2;
        let expr_text = r#"pkgs.writeShellApplication {
        name = "foo";
        text = "echo foo";
      }"#;

        let script = make_script(
            "foo",
            &PathBuf::from("/dev/null"),
            expr_text,
            &content[attr_start..attr_end],
            (attr_start, attr_end),
        );

        let (modified, exported, warnings) = process_file(content, &[&script]).unwrap();

        assert_eq!(exported, vec!["foo"]);
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
        // The packages block should now contain `foo`
        assert!(modified.contains("foo = pkgs.writeShellApplication"));
        // The original location should have a reference, not the inline expr
        assert!(!modified[modified.find("settings.processes").unwrap()..]
            .contains("writeShellApplication"));
        // Should still parse
        assert!(
            parse_nix(&modified).is_ok(),
            "modified content should be valid nix:\n{}",
            modified
        );
    }

    #[test]
    fn detect_and_replicate_pattern() {
        let content = r#"{
  packages = {
    caddy-dev = pkgs.writeShellApplication {
      name = "caddy-dev";
      text = "exec caddy";
    };
  };
  process-compose."dev" = {
    settings.processes = {
      caddy.command = self'.packages.caddy-dev;
      foo.command = pkgs.writeShellApplication {
        name = "foo";
        text = "echo foo";
      };
    };
  };
}"#;
        let prefix = detect_reference_prefix(content);
        assert_eq!(prefix, Some("self'.packages".to_string()));

        let attr_start = content
            .find("foo.command = pkgs.writeShellApplication")
            .unwrap();
        let attr_end = content[attr_start..].find("};").unwrap() + attr_start + 2;
        let expr_text = r#"pkgs.writeShellApplication {
        name = "foo";
        text = "echo foo";
      }"#;

        let script = make_script(
            "foo",
            &PathBuf::from("/dev/null"),
            expr_text,
            &content[attr_start..attr_end],
            (attr_start, attr_end),
        );

        let (modified, exported, _) = process_file(content, &[&script]).unwrap();
        assert_eq!(exported, vec!["foo"]);
        // Should use the detected prefix
        assert!(
            modified.contains("foo.command = self'.packages.foo;"),
            "expected self'.packages.foo reference in:\n{}",
            modified
        );
        assert!(
            parse_nix(&modified).is_ok(),
            "modified content should be valid nix:\n{}",
            modified
        );
    }

    #[test]
    fn skip_already_exported() {
        let content = r#"{
  packages = {
    foo = pkgs.writeShellApplication {
      name = "foo";
      text = "echo foo";
    };
  };
  process-compose."dev" = {
    settings.processes = {
      bar.command = pkgs.writeShellApplication {
        name = "foo";
        text = "echo foo";
      };
    };
  };
}"#;
        let attr_start = content.find("bar.command").unwrap();
        let attr_end = content[attr_start..].find("};").unwrap() + attr_start + 2;

        let script = make_script(
            "foo",
            &PathBuf::from("/dev/null"),
            "pkgs.writeShellApplication { name = \"foo\"; text = \"echo foo\"; }",
            &content[attr_start..attr_end],
            (attr_start, attr_end),
        );

        let (_, exported, warnings) = process_file(content, &[&script]).unwrap();
        assert!(exported.is_empty(), "should skip already-exported package");
        assert!(
            warnings.iter().any(|w| w.contains("already exists")),
            "should warn about duplicate: {:?}",
            warnings
        );
    }

    #[test]
    fn multiple_scripts_same_file() {
        let content = r#"{
  packages = {
    existing = pkgs.hello;
  };
  process-compose."dev" = {
    settings.processes = {
      alpha.command = pkgs.writeShellApplication {
        name = "alpha";
        text = "echo alpha";
      };
      beta.command = pkgs.writeShellApplication {
        name = "beta";
        text = "echo beta";
      };
      gamma.command = pkgs.writeShellApplication {
        name = "gamma";
        text = "echo gamma";
      };
    };
  };
}"#;
        let mut scripts = Vec::new();

        for name in &["alpha", "beta", "gamma"] {
            let search = format!("{}.command = pkgs.writeShellApplication", name);
            let attr_start = content.find(&search).unwrap();
            let attr_end = content[attr_start..].find("};").unwrap() + attr_start + 2;
            let expr_search = "pkgs.writeShellApplication";
            let expr_start = content[attr_start..].find(expr_search).unwrap() + attr_start;
            let expr_end = content[expr_start..].find('}').unwrap() + expr_start + 1;

            scripts.push(make_script(
                name,
                &PathBuf::from("/dev/null"),
                &content[expr_start..expr_end],
                &content[attr_start..attr_end],
                (attr_start, attr_end),
            ));
        }

        let script_refs: Vec<&DetectedScript> = scripts.iter().collect();
        let (modified, exported, warnings) = process_file(content, &script_refs).unwrap();

        assert_eq!(exported.len(), 3, "expected 3 exported, got {:?}", exported);
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
        // All three should be in the packages block
        for name in &["alpha", "beta", "gamma"] {
            assert!(
                modified.contains(&format!("{} = pkgs.writeShellApplication", name)),
                "packages block should contain '{}' in:\n{}",
                name,
                modified
            );
        }
        assert!(
            parse_nix(&modified).is_ok(),
            "modified content should be valid nix:\n{}",
            modified
        );
    }

    #[test]
    fn validates_output() {
        let content = r#"{
  packages = {
    existing = pkgs.hello;
  };
  things = {
    foo.command = pkgs.writeShellApplication {
      name = "foo";
      text = "echo hello";
    };
  };
}"#;
        let attr_start = content
            .find("foo.command = pkgs.writeShellApplication")
            .unwrap();
        let attr_end = content[attr_start..].find("};").unwrap() + attr_start + 2;
        let expr_start = content[attr_start..]
            .find("pkgs.writeShellApplication")
            .unwrap()
            + attr_start;
        let expr_end = content[expr_start..].find('}').unwrap() + expr_start + 1;

        let script = make_script(
            "foo",
            &PathBuf::from("/dev/null"),
            &content[expr_start..expr_end],
            &content[attr_start..attr_end],
            (attr_start, attr_end),
        );

        let (modified, _, _) = process_file(content, &[&script]).unwrap();
        let parse_result = parse_nix(&modified);
        assert!(
            parse_result.is_ok(),
            "output must be valid nix, got error: {}\ncontent:\n{}",
            parse_result
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default(),
            modified
        );
    }

    #[test]
    fn export_with_tempfile() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("dev.nix");
        let content = r#"{
  packages = {
    existing = pkgs.hello;
  };
  process-compose."dev" = {
    settings.processes = {
      foo.command = pkgs.writeShellApplication {
        name = "foo";
        text = "echo foo";
      };
    };
  };
}"#;
        std::fs::write(&file_path, content).unwrap();

        let attr_start = content
            .find("foo.command = pkgs.writeShellApplication")
            .unwrap();
        let attr_end = content[attr_start..].find("};").unwrap() + attr_start + 2;
        let expr_start = content[attr_start..]
            .find("pkgs.writeShellApplication")
            .unwrap()
            + attr_start;
        let expr_end = content[expr_start..].find('}').unwrap() + expr_start + 1;

        let script = make_script(
            "foo",
            &file_path,
            &content[expr_start..expr_end],
            &content[attr_start..attr_end],
            (attr_start, attr_end),
        );

        let result = export_scripts_as_packages(&[script]).unwrap();

        assert_eq!(result.exported_packages, vec!["foo"]);
        assert_eq!(result.modified_files, vec![file_path.clone()]);
        assert!(result.warnings.is_empty());

        let modified = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            parse_nix(&modified).is_ok(),
            "written file should be valid nix:\n{}",
            modified
        );
    }
}
