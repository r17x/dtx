use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::edit::{compute_line_starts, finalize_edit};
use crate::error::{CodeError, Result};
use crate::references::find_references;
use crate::workspace::WorkspaceIndex;

#[derive(Clone, Debug, Serialize)]
pub struct RenameChange {
    pub file: PathBuf,
    pub line: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenameResult {
    pub old_name: String,
    pub new_name: String,
    pub files_modified: Vec<PathBuf>,
    pub occurrences_replaced: usize,
    pub changes: Vec<RenameChange>,
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn rename_symbol(
    workspace: &WorkspaceIndex,
    path: &Path,
    name_path: &str,
    new_name: &str,
    dry_run: bool,
) -> Result<RenameResult> {
    if new_name.is_empty() {
        return Err(CodeError::RenameFailed("new name must not be empty".into()));
    }

    let matches = workspace.find_symbol(name_path, Some(path), None, false, None)?;
    if matches.is_empty() {
        return Err(CodeError::SymbolNotFound(name_path.to_string()));
    }
    let old_name = &matches[0].symbol.name;

    let refs = find_references(workspace.root(), old_name, None, None)?;
    if refs.is_empty() {
        return Ok(RenameResult {
            old_name: old_name.clone(),
            new_name: new_name.to_string(),
            files_modified: Vec::new(),
            occurrences_replaced: 0,
            changes: Vec::new(),
        });
    }

    let mut by_file: HashMap<PathBuf, Vec<(usize, usize)>> = HashMap::new();
    for r in refs {
        by_file.entry(r.file).or_default().push((r.line, r.column));
    }

    let mut files_modified = Vec::new();
    let mut total_replaced = 0;
    let mut changes = Vec::new();

    for (file_path, locations) in by_file {
        let content = std::fs::read_to_string(&file_path)?;
        let line_starts = compute_line_starts(&content);
        let line_count = line_starts.len();

        let mut byte_offsets: Vec<usize> = Vec::new();
        for &(line_1based, col_1based) in &locations {
            if line_1based == 0 || line_1based > line_count {
                continue;
            }
            let byte_offset = line_starts[line_1based - 1] + (col_1based - 1);
            if !content[byte_offset..].starts_with(old_name) {
                continue;
            }
            if byte_offset > 0 {
                if let Some(c) = content[..byte_offset].chars().next_back() {
                    if is_word_char(c) {
                        continue;
                    }
                }
            }
            let after_end = byte_offset + old_name.len();
            if after_end < content.len() {
                if let Some(c) = content[after_end..].chars().next() {
                    if is_word_char(c) {
                        continue;
                    }
                }
            }
            byte_offsets.push(byte_offset);
        }

        if byte_offsets.is_empty() {
            continue;
        }

        byte_offsets.sort_unstable();
        byte_offsets.dedup();

        if dry_run {
            for &bo in &byte_offsets {
                let line_idx = line_starts.partition_point(|&s| s <= bo) - 1;
                let line_start = line_starts[line_idx];
                let line_end = if line_idx + 1 < line_starts.len() {
                    line_starts[line_idx + 1].saturating_sub(1)
                } else {
                    content.len()
                };
                let before = content[line_start..line_end].to_string();
                let col = bo - line_start;
                let mut after_line = String::with_capacity(before.len());
                after_line.push_str(&before[..col]);
                after_line.push_str(new_name);
                after_line.push_str(&before[col + old_name.len()..]);
                changes.push(RenameChange {
                    file: file_path.clone(),
                    line: line_idx + 1,
                    before,
                    after: after_line,
                });
            }
        }

        if !dry_run {
            let size_diff = new_name.len() as isize - old_name.len() as isize;
            let new_len =
                (content.len() as isize + size_diff * byte_offsets.len() as isize) as usize;
            let mut result = String::with_capacity(new_len);
            let mut prev = 0;
            for &bo in &byte_offsets {
                result.push_str(&content[prev..bo]);
                result.push_str(new_name);
                prev = bo + old_name.len();
            }
            result.push_str(&content[prev..]);

            let abs = workspace.resolve_path(&file_path)?;
            let _ = finalize_edit(workspace, &abs, &result, None)?;
        }

        total_replaced += byte_offsets.len();
        files_modified.push(file_path);
    }

    files_modified.sort();
    changes.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

    Ok(RenameResult {
        old_name: old_name.clone(),
        new_name: new_name.to_string(),
        files_modified,
        occurrences_replaced: total_replaced,
        changes,
    })
}
