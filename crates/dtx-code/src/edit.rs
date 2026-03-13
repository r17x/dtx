use std::path::Path;

use crate::error::{CodeError, Result};
use crate::symbol::Symbol;
use crate::workspace::WorkspaceIndex;

/// Shared context for symbol-level edits: resolves path, reads file, finds symbol.
fn prepare_edit(
    workspace: &WorkspaceIndex,
    path: &Path,
    name_path: &str,
) -> Result<(std::path::PathBuf, String, Symbol)> {
    let abs = workspace.resolve_path(path);
    let content = std::fs::read_to_string(&abs)?;
    let idx = workspace.get_or_parse(&abs)?;
    let symbol = find_symbol_by_name_path(&idx.symbols, name_path)
        .ok_or_else(|| CodeError::SymbolNotFound(name_path.to_string()))?;
    Ok((abs, content, symbol))
}

/// Write new content and invalidate the workspace cache.
fn finalize_edit(workspace: &WorkspaceIndex, abs: &Path, new_content: &str) -> Result<()> {
    std::fs::write(abs, new_content)?;
    workspace.invalidate(abs);
    Ok(())
}

pub fn replace_symbol_body(
    workspace: &WorkspaceIndex,
    path: &Path,
    name_path: &str,
    new_body: &str,
) -> Result<()> {
    let (abs, content, symbol) = prepare_edit(workspace, path, name_path)?;
    let mut new_content = String::with_capacity(content.len());
    new_content.push_str(&content[..symbol.start_byte]);
    new_content.push_str(new_body);
    new_content.push_str(&content[symbol.end_byte..]);
    finalize_edit(workspace, &abs, &new_content)
}

pub fn insert_before_symbol(
    workspace: &WorkspaceIndex,
    path: &Path,
    name_path: &str,
    content_to_insert: &str,
) -> Result<()> {
    let (abs, content, symbol) = prepare_edit(workspace, path, name_path)?;
    let mut new_content = String::with_capacity(content.len() + content_to_insert.len());
    new_content.push_str(&content[..symbol.start_byte]);
    new_content.push_str(content_to_insert);
    new_content.push_str(&content[symbol.start_byte..]);
    finalize_edit(workspace, &abs, &new_content)
}

pub fn insert_after_symbol(
    workspace: &WorkspaceIndex,
    path: &Path,
    name_path: &str,
    content_to_insert: &str,
) -> Result<()> {
    let (abs, content, symbol) = prepare_edit(workspace, path, name_path)?;
    let mut new_content = String::with_capacity(content.len() + content_to_insert.len());
    new_content.push_str(&content[..symbol.end_byte]);
    new_content.push_str(content_to_insert);
    new_content.push_str(&content[symbol.end_byte..]);
    finalize_edit(workspace, &abs, &new_content)
}

fn find_symbol_by_name_path(symbols: &[Symbol], name_path: &str) -> Option<Symbol> {
    for s in symbols {
        if s.name_path == name_path {
            return Some(s.clone());
        }
        if let Some(found) = find_symbol_by_name_path(&s.children, name_path) {
            return Some(found);
        }
    }
    None
}
