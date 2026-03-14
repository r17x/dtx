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
pub(crate) fn finalize_edit(
    workspace: &WorkspaceIndex,
    abs: &Path,
    new_content: &str,
) -> Result<()> {
    std::fs::write(abs, new_content)?;
    workspace.invalidate(abs);
    Ok(())
}

/// Compute byte offsets of each line start in content. Returns 1 entry per line.
pub(crate) fn compute_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
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

pub fn insert_at_line(
    workspace: &WorkspaceIndex,
    path: &Path,
    line: usize,
    content: &str,
) -> Result<()> {
    let abs = workspace.resolve_path(path);
    let file_content = std::fs::read_to_string(&abs)?;

    if file_content.is_empty() {
        if line != 1 {
            return Err(CodeError::InvalidLine(line, 0));
        }
        return finalize_edit(workspace, &abs, content);
    }

    let line_starts = compute_line_starts(&file_content);
    let line_count = line_starts.len();

    if line == 0 || line > line_count + 1 {
        return Err(CodeError::InvalidLine(line, line_count));
    }

    let mut new_content = String::with_capacity(file_content.len() + content.len());
    if line == line_count + 1 {
        // Append at end
        new_content.push_str(&file_content);
        if !file_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(content);
    } else {
        let offset = line_starts[line - 1];
        new_content.push_str(&file_content[..offset]);
        new_content.push_str(content);
        new_content.push_str(&file_content[offset..]);
    }

    finalize_edit(workspace, &abs, &new_content)
}

pub fn replace_lines(
    workspace: &WorkspaceIndex,
    path: &Path,
    start_line: usize,
    end_line: usize,
    new_content: &str,
) -> Result<()> {
    let abs = workspace.resolve_path(path);
    let file_content = std::fs::read_to_string(&abs)?;

    let line_starts = compute_line_starts(&file_content);
    let line_count = line_starts.len();

    if start_line == 0 || end_line < start_line || end_line > line_count {
        return Err(CodeError::InvalidLineRange(
            start_line, end_line, line_count,
        ));
    }

    let start_offset = line_starts[start_line - 1];
    let end_offset = if end_line < line_count {
        line_starts[end_line]
    } else {
        file_content.len()
    };

    let mut result = String::with_capacity(file_content.len() + new_content.len());
    result.push_str(&file_content[..start_offset]);
    result.push_str(new_content);
    result.push_str(&file_content[end_offset..]);

    finalize_edit(workspace, &abs, &result)
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
