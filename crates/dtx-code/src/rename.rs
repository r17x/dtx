use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::edit::{compute_line_starts, finalize_edit};
use crate::error::{CodeError, Result};
use crate::references::find_references;
use crate::workspace::WorkspaceIndex;

#[derive(Clone, Debug, Serialize)]
pub struct RenameResult {
    pub old_name: String,
    pub new_name: String,
    pub files_modified: Vec<PathBuf>,
    pub occurrences_replaced: usize,
}

pub fn rename_symbol(
    workspace: &WorkspaceIndex,
    path: &Path,
    name_path: &str,
    new_name: &str,
) -> Result<RenameResult> {
    if new_name.is_empty() {
        return Err(CodeError::RenameFailed("new name must not be empty".into()));
    }

    // Find the definition to get the actual leaf name
    let matches = workspace.find_symbol(name_path, Some(path), None, false, None)?;
    if matches.is_empty() {
        return Err(CodeError::SymbolNotFound(name_path.to_string()));
    }
    let old_name = &matches[0].symbol.name;

    // Find all references across the workspace
    let refs = find_references(workspace.root(), old_name, None)?;
    if refs.is_empty() {
        return Ok(RenameResult {
            old_name: old_name.clone(),
            new_name: new_name.to_string(),
            files_modified: Vec::new(),
            occurrences_replaced: 0,
        });
    }

    // Group references by file, consuming refs to avoid cloning PathBufs
    let mut by_file: HashMap<PathBuf, Vec<(usize, usize)>> = HashMap::new();
    for r in refs {
        by_file.entry(r.file).or_default().push((r.line, r.column));
    }

    let mut files_modified = Vec::new();
    let mut total_replaced = 0;

    for (file_path, locations) in by_file {
        let content = std::fs::read_to_string(&file_path)?;
        let line_starts = compute_line_starts(&content);
        let line_count = line_starts.len();

        // Compute verified byte offsets
        let mut byte_offsets: Vec<usize> = Vec::new();
        for &(line_1based, col_1based) in &locations {
            if line_1based == 0 || line_1based > line_count {
                continue;
            }
            let byte_offset = line_starts[line_1based - 1] + (col_1based - 1);
            if content[byte_offset..].starts_with(old_name) {
                byte_offsets.push(byte_offset);
            }
        }

        if byte_offsets.is_empty() {
            continue;
        }

        byte_offsets.sort_unstable();
        byte_offsets.dedup();

        // Single-pass forward replacement (O(filesize) instead of O(N*filesize))
        let size_diff = new_name.len() as isize - old_name.len() as isize;
        let new_len = (content.len() as isize + size_diff * byte_offsets.len() as isize) as usize;
        let mut result = String::with_capacity(new_len);
        let mut prev = 0;
        for &bo in &byte_offsets {
            result.push_str(&content[prev..bo]);
            result.push_str(new_name);
            prev = bo + old_name.len();
        }
        result.push_str(&content[prev..]);

        let abs = workspace.resolve_path(&file_path);
        finalize_edit(workspace, &abs, &result)?;
        total_replaced += byte_offsets.len();
        files_modified.push(file_path);
    }

    files_modified.sort();

    Ok(RenameResult {
        old_name: old_name.clone(),
        new_name: new_name.to_string(),
        files_modified,
        occurrences_replaced: total_replaced,
    })
}
