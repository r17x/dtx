use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use regex::Regex;

use crate::error::{CodeError, Result};
use crate::symbol::{Symbol, SymbolKind};
use crate::workspace::WorkspaceIndex;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Reference {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub containing_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub containing_symbol_kind: Option<SymbolKind>,
}

pub fn find_references(
    root: &Path,
    symbol_name: &str,
    scope_path: Option<&Path>,
    cap: Option<usize>,
) -> Result<Vec<Reference>> {
    let pattern = Regex::new(&format!(r"\b{}\b", regex::escape(symbol_name)))
        .map_err(|e| CodeError::Parse(e.to_string()))?;

    let walk_root = scope_path.unwrap_or(root);
    let walker = WalkBuilder::new(walk_root).build();

    let mut refs = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if crate::language::detect(entry.path()).is_none() {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            if let Some(m) = pattern.find(line) {
                let start = i.saturating_sub(1);
                let end = (i + 2).min(lines.len());
                let context = lines[start..end].join("\n");
                refs.push(Reference {
                    file: entry.path().to_path_buf(),
                    line: i + 1,
                    column: m.start() + 1,
                    context,
                    containing_symbol: None,
                    containing_symbol_kind: None,
                });
                if cap.is_some_and(|c| refs.len() >= c) {
                    return Ok(refs);
                }
            }
        }
    }
    Ok(refs)
}

pub fn find_referencing_symbols(
    workspace: &WorkspaceIndex,
    symbol_name: &str,
    scope_path: Option<&Path>,
    cap: Option<usize>,
) -> Result<Vec<Reference>> {
    let mut refs = find_references(workspace.root(), symbol_name, scope_path, cap)?;

    for r in &mut refs {
        if let Ok(entry) = workspace.get_or_parse(&r.file) {
            let zero_based_line = r.line - 1;
            if let Some((name_path, kind)) = find_innermost_symbol(&entry.symbols, zero_based_line)
            {
                r.containing_symbol = Some(name_path);
                r.containing_symbol_kind = Some(kind);
            }
        }
    }

    Ok(refs)
}

fn find_innermost_symbol(symbols: &[Symbol], line: usize) -> Option<(String, SymbolKind)> {
    for sym in symbols {
        if sym.start_line <= line && line <= sym.end_line {
            // Check children for a more specific match
            if let Some(inner) = find_innermost_symbol(&sym.children, line) {
                return Some(inner);
            }
            return Some((sym.name_path.clone(), sym.kind.clone()));
        }
    }
    None
}
