use std::path::PathBuf;

use anyhow::Result;

use dtx_code::WorkspaceIndex;
use dtx_core::config::project::find_project_root_cwd;

use crate::output::Output;

pub async fn symbols(out: &Output, file: PathBuf) -> Result<()> {
    let root = find_project_root_cwd()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let index = WorkspaceIndex::new(root);

    let overview = index
        .get_overview(&file, None)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    out.step("symbols").done(&format!("{}", file.display()));
    print_symbols(&overview.symbols, 0);
    Ok(())
}

pub async fn find(out: &Output, pattern: String, path: Option<PathBuf>) -> Result<()> {
    let root = find_project_root_cwd()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let index = WorkspaceIndex::new(root);

    let matches = index
        .find_symbol(&pattern, path.as_deref(), None, false, None)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    out.step("find")
        .done(&format!("{} matches for '{pattern}'", matches.len()));
    for m in &matches {
        println!(
            "  {}:{} {} ({})",
            m.file.display(),
            m.symbol.start_line + 1,
            m.symbol.name_path,
            m.symbol.kind,
        );
    }
    Ok(())
}

pub async fn search(out: &Output, pattern: String, glob: Option<String>) -> Result<()> {
    let root = find_project_root_cwd()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

    let matches = dtx_code::search_pattern(&root, &pattern, glob.as_deref(), 2)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    out.step("search")
        .done(&format!("{} matches for '{pattern}'", matches.len()));
    for m in &matches {
        println!("  {}:{} {}", m.file.display(), m.line, m.matched_text);
    }
    Ok(())
}

fn print_symbols(symbols: &[dtx_code::Symbol], depth: usize) {
    let indent = "  ".repeat(depth);
    for s in symbols {
        println!(
            "{indent}{} {} [L{}-L{}]",
            s.kind,
            s.name_path,
            s.start_line + 1,
            s.end_line + 1,
        );
        if !s.children.is_empty() {
            print_symbols(&s.children, depth + 1);
        }
    }
}
