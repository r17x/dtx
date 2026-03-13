use std::path::{Path, PathBuf};

use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use regex::Regex;

use crate::error::{CodeError, Result};

#[derive(Clone, Debug, serde::Serialize)]
pub struct SearchMatch {
    pub file: PathBuf,
    pub line: usize,
    pub matched_text: String,
    pub context: String,
}

pub fn search_pattern(
    root: &Path,
    pattern: &str,
    glob_filter: Option<&str>,
    context_lines: usize,
) -> Result<Vec<SearchMatch>> {
    let regex = Regex::new(pattern).map_err(|e| CodeError::Parse(e.to_string()))?;

    let mut builder = WalkBuilder::new(root);
    if let Some(glob) = glob_filter {
        let mut overrides = OverrideBuilder::new(root);
        if overrides.add(glob).is_ok() {
            if let Ok(ov) = overrides.build() {
                builder.overrides(ov);
            }
        }
    }
    let walker = builder.build();

    let mut matches = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            if let Some(m) = regex.find(line) {
                let start = i.saturating_sub(context_lines);
                let end = (i + context_lines + 1).min(lines.len());
                matches.push(SearchMatch {
                    file: entry.path().to_path_buf(),
                    line: i + 1,
                    matched_text: m.as_str().to_string(),
                    context: lines[start..end].join("\n"),
                });
            }
        }
    }
    Ok(matches)
}
