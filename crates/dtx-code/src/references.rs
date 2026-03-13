use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use regex::Regex;

use crate::error::{CodeError, Result};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Reference {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub context: String,
}

pub fn find_references(
    root: &Path,
    symbol_name: &str,
    scope_path: Option<&Path>,
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
                });
            }
        }
    }
    Ok(refs)
}
