use std::path::{Path, PathBuf};

use dashmap::DashMap;
use ignore::{DirEntry, WalkBuilder};

use crate::error::{CodeError, Result};
use crate::index::FileIndex;
use crate::symbol::{Symbol, SymbolOverview};

fn walk_source_files(root: &Path) -> impl Iterator<Item = DirEntry> {
    WalkBuilder::new(root)
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
        .filter(|e| crate::language::detect(e.path()).is_some())
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DirEntryInfo {
    pub name: String,
    pub entry_type: EntryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl DirEntryInfo {
    fn from_metadata(name: String, metadata: &std::fs::Metadata) -> Self {
        Self {
            name,
            entry_type: if metadata.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            },
            size: if metadata.is_file() {
                Some(metadata.len())
            } else {
                None
            },
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SymbolMatch {
    pub file: PathBuf,
    pub symbol: Symbol,
    pub body: Option<String>,
}

pub struct WorkspaceIndex {
    root: PathBuf,
    root_canonical: PathBuf,
    cache: DashMap<PathBuf, FileIndex>,
}

impl WorkspaceIndex {
    pub fn new(root: PathBuf) -> Self {
        let root_canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
        Self {
            root,
            root_canonical,
            cache: DashMap::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = walk_source_files(&self.root)
            .filter_map(|e| {
                e.path()
                    .strip_prefix(&self.root)
                    .ok()
                    .map(Path::to_path_buf)
            })
            .collect();
        files.sort();
        files
    }

    pub fn resolve_path(&self, path: &Path) -> Result<PathBuf> {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let canonical = resolved
            .canonicalize()
            .map_err(|_| CodeError::FileNotFound(resolved.display().to_string()))?;
        if !canonical.starts_with(&self.root_canonical) {
            return Err(CodeError::PathTraversal(path.display().to_string()));
        }
        Ok(canonical)
    }

    pub fn get_or_parse(
        &self,
        path: &Path,
    ) -> Result<dashmap::mapref::one::Ref<'_, PathBuf, FileIndex>> {
        let abs = self.resolve_path(path)?;

        let current_mtime = std::fs::metadata(&abs)
            .and_then(|m| m.modified())
            .map_err(|_| CodeError::FileNotFound(abs.display().to_string()))?;

        let needs_reparse = match self.cache.get(&abs) {
            Some(entry) => entry.mtime != current_mtime,
            None => true,
        };

        if needs_reparse {
            let lang = crate::language::detect(&abs)
                .ok_or_else(|| CodeError::UnsupportedLanguage(abs.display().to_string()))?;
            let source = std::fs::read_to_string(&abs)?;
            let symbols = crate::parser::parse_source(&source, lang);
            self.cache.insert(
                abs.clone(),
                FileIndex {
                    path: abs.clone(),
                    language: lang,
                    mtime: current_mtime,
                    symbols,
                },
            );
        }

        self.cache
            .get(&abs)
            .ok_or_else(|| CodeError::FileNotFound(abs.display().to_string()))
    }

    pub fn invalidate(&self, path: &Path) {
        if let Ok(abs) = self.resolve_path(path) {
            self.cache.remove(&abs);
        }
    }

    pub fn symbols_in_file(&self, path: &Path) -> Result<Vec<Symbol>> {
        let entry = self.get_or_parse(path)?;
        Ok(entry.symbols.clone())
    }

    pub fn find_symbol(
        &self,
        name_path_pattern: &str,
        path: Option<&Path>,
        depth: Option<usize>,
        include_body: bool,
        source: Option<&str>,
    ) -> Result<Vec<SymbolMatch>> {
        let mut results = Vec::new();

        if let Some(p) = path {
            let abs = self.resolve_path(p)?;
            let src = match source {
                Some(s) => s.to_string(),
                None => std::fs::read_to_string(&abs)?,
            };
            let entry = self.get_or_parse(&abs)?;
            collect_matching_symbols(
                &abs,
                &entry.symbols,
                name_path_pattern,
                depth,
                include_body,
                &src,
                &mut results,
            );
        } else {
            for entry in walk_source_files(&self.root) {
                let file_path = entry.path().to_path_buf();
                // get_or_parse reads the file internally; only read again for body extraction
                let idx = match self.get_or_parse(&file_path) {
                    Ok(idx) => idx,
                    Err(_) => continue,
                };
                // Quick check: skip file if no symbols match pattern (avoids re-read)
                let has_match = idx.symbols.iter().any(|s| {
                    s.name_path.contains(name_path_pattern) || s.name.contains(name_path_pattern)
                });
                if !has_match {
                    continue;
                }
                let src = match std::fs::read_to_string(&file_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                collect_matching_symbols(
                    &file_path,
                    &idx.symbols,
                    name_path_pattern,
                    depth,
                    include_body,
                    &src,
                    &mut results,
                );
            }
        }

        Ok(results)
    }

    /// Find files matching a glob pattern (e.g., "*.rs", "**/test_*").
    /// Returns relative paths sorted alphabetically.
    pub fn find_files(&self, glob_pattern: &str) -> Result<Vec<PathBuf>> {
        use ignore::overrides::OverrideBuilder;

        let mut builder = OverrideBuilder::new(&self.root);
        builder
            .add(glob_pattern)
            .map_err(|e| CodeError::Parse(format!("Invalid glob pattern: {e}")))?;
        let overrides = builder
            .build()
            .map_err(|e| CodeError::Parse(format!("Invalid glob pattern: {e}")))?;

        let walker = WalkBuilder::new(&self.root).overrides(overrides).build();

        let mut files: Vec<PathBuf> = walker
            .flatten()
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
            .filter_map(|e| {
                e.path()
                    .strip_prefix(&self.root)
                    .ok()
                    .map(Path::to_path_buf)
            })
            .collect();

        files.sort();
        Ok(files)
    }

    /// List directory contents.
    pub fn list_dir(&self, path: &Path, recursive: bool) -> Result<Vec<DirEntryInfo>> {
        self.list_dir_with_depth(path, recursive, None)
    }

    pub fn list_dir_with_depth(
        &self,
        path: &Path,
        recursive: bool,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirEntryInfo>> {
        let abs = self.resolve_path(path)?;

        let mut entries = Vec::new();
        if recursive {
            // Use WalkBuilder to respect .gitignore
            let mut builder = WalkBuilder::new(&abs);
            if let Some(depth) = max_depth {
                builder.max_depth(Some(depth + 1)); // +1 because root counts as depth 0
            }
            for entry in builder.build().flatten() {
                if entry.path() == abs {
                    continue;
                }
                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let name = entry
                    .path()
                    .strip_prefix(&self.root)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .to_string();
                entries.push(DirEntryInfo::from_metadata(name, &metadata));
            }
        } else {
            let read_dir = std::fs::read_dir(&abs)
                .map_err(|_| CodeError::FileNotFound(abs.display().to_string()))?;
            for entry in read_dir.flatten() {
                let metadata = entry.metadata()?;
                let name = entry.file_name().to_string_lossy().to_string();
                entries.push(DirEntryInfo::from_metadata(name, &metadata));
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    pub fn get_overview(&self, path: &Path, depth: Option<usize>) -> Result<SymbolOverview> {
        let abs = self.resolve_path(path)?;
        let entry = self.get_or_parse(&abs)?;
        let symbols = match depth {
            Some(d) => truncate_depth(&entry.symbols, d),
            None => entry.symbols.clone(),
        };
        Ok(SymbolOverview { file: abs, symbols })
    }
}

fn collect_matching_symbols(
    file: &Path,
    symbols: &[Symbol],
    pattern: &str,
    depth: Option<usize>,
    include_body: bool,
    source: &str,
    results: &mut Vec<SymbolMatch>,
) {
    for s in symbols {
        if s.name_path.contains(pattern) || s.name.contains(pattern) {
            let body = if include_body {
                Some(source[s.start_byte..s.end_byte].to_string())
            } else {
                None
            };
            let symbol = match depth {
                Some(d) => {
                    let mut trimmed = s.clone();
                    trimmed.children = truncate_depth(&s.children, d);
                    trimmed
                }
                None => s.clone(),
            };
            results.push(SymbolMatch {
                file: file.to_path_buf(),
                symbol,
                body,
            });
        }
        collect_matching_symbols(
            file,
            &s.children,
            pattern,
            depth,
            include_body,
            source,
            results,
        );
    }
}

fn truncate_depth(symbols: &[Symbol], depth: usize) -> Vec<Symbol> {
    symbols
        .iter()
        .map(|s| {
            let mut s = s.clone();
            if depth == 0 {
                s.children = Vec::new();
            } else {
                s.children = truncate_depth(&s.children, depth - 1);
            }
            s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_workspace() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("create tempdir");
        let root = dir.path().to_path_buf();

        let rust_file = root.join("lib.rs");
        let mut f = std::fs::File::create(&rust_file).expect("create file");
        f.write_all(
            br#"
struct Foo {
    x: i32,
}

impl Foo {
    fn new() -> Self {
        Self { x: 0 }
    }

    fn get_x(&self) -> i32 {
        self.x
    }
}

fn standalone() -> bool {
    true
}
"#,
        )
        .expect("write");

        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).expect("mkdir");
        let py_file = sub.join("main.py");
        let mut f = std::fs::File::create(&py_file).expect("create file");
        f.write_all(
            br#"
class Greeter:
    def greet(self):
        return "hello"

def standalone_fn():
    pass
"#,
        )
        .expect("write");

        (dir, root)
    }

    #[test]
    fn symbols_in_file() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let symbols = ws.symbols_in_file(&root.join("lib.rs")).expect("parse");
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"standalone"));
    }

    #[test]
    fn find_symbol_by_name() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let matches = ws
            .find_symbol("standalone", None, None, false, None)
            .expect("find");
        assert!(matches.len() >= 2, "should find in both files");
    }

    #[test]
    fn find_symbol_with_body() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let matches = ws
            .find_symbol("standalone", Some(&path), None, true, None)
            .expect("find");
        assert_eq!(matches.len(), 1);
        assert!(matches[0].body.is_some());
        assert!(matches[0].body.as_ref().unwrap().contains("true"));
    }

    #[test]
    fn get_overview_with_depth() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let overview = ws
            .get_overview(&root.join("lib.rs"), Some(0))
            .expect("overview");
        for sym in &overview.symbols {
            assert!(sym.children.is_empty(), "depth 0 should have no children");
        }
    }

    #[test]
    fn cache_invalidation() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");

        let _ = ws.get_or_parse(&path).expect("initial parse");
        ws.invalidate(&path);

        // Should re-parse after invalidation
        let entry = ws.get_or_parse(&path).expect("re-parse");
        assert!(!entry.symbols.is_empty());
    }

    #[test]
    fn replace_symbol_body_roundtrip() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");

        crate::edit::replace_symbol_body(
            &ws,
            &path,
            "standalone",
            "fn standalone() -> bool {\n    false\n}",
            None,
        )
        .expect("replace");

        let content = std::fs::read_to_string(&path).expect("read");
        assert!(content.contains("false"));
        assert!(!content.contains("true"));
    }

    #[test]
    fn search_pattern_finds_matches() {
        let (_dir, root) = create_test_workspace();
        let matches =
            crate::search::search_pattern(&root, r"standalone", None, 1, None).expect("search");
        assert!(matches.len() >= 2);
    }

    #[test]
    fn list_files_finds_supported() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root);
        let files = ws.list_files();
        assert!(files
            .iter()
            .any(|f| f.extension().is_some_and(|e| e == "rs")));
        assert!(files
            .iter()
            .any(|f| f.extension().is_some_and(|e| e == "py")));
    }

    #[test]
    fn rename_symbol_single_file() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let result = crate::rename::rename_symbol(&ws, &path, "standalone", "helper", false)
            .expect("rename");
        assert_eq!(result.old_name, "standalone");
        assert_eq!(result.new_name, "helper");
        assert!(result.occurrences_replaced >= 1);
        let content = std::fs::read_to_string(&path).expect("read");
        assert!(content.contains("fn helper()"));
        assert!(!content.contains("fn standalone()"));
    }

    #[test]
    fn rename_symbol_not_found() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let result = crate::rename::rename_symbol(&ws, &path, "nonexistent", "new_name", false);
        assert!(result.is_err());
    }

    #[test]
    fn find_references_across_files() {
        let (_dir, root) = create_test_workspace();
        // "self" appears in both Rust and Python files
        let refs = crate::references::find_references(&root, "self", None, None).expect("refs");
        assert!(!refs.is_empty());
    }

    #[test]
    fn insert_at_line_beginning() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        crate::edit::insert_at_line(&ws, &path, 1, "// header\n", None).expect("insert");
        let content = std::fs::read_to_string(&path).expect("read");
        assert!(content.starts_with("// header\n"));
    }

    #[test]
    fn insert_at_line_middle() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let original = std::fs::read_to_string(&path).expect("read");
        let original_lines: Vec<&str> = original.lines().collect();
        crate::edit::insert_at_line(&ws, &path, 3, "// inserted\n", None).expect("insert");
        let content = std::fs::read_to_string(&path).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[2], "// inserted");
        assert_eq!(lines.len(), original_lines.len() + 1);
    }

    #[test]
    fn insert_at_line_end() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let original = std::fs::read_to_string(&path).expect("read");
        let line_count = original.lines().count();
        crate::edit::insert_at_line(&ws, &path, line_count + 1, "// footer\n", None)
            .expect("insert");
        let content = std::fs::read_to_string(&path).expect("read");
        assert!(content.contains("// footer"));
    }

    #[test]
    fn insert_at_line_invalid() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let original = std::fs::read_to_string(&path).expect("read");
        let line_count = original.lines().count();
        let result = crate::edit::insert_at_line(&ws, &path, line_count + 100, "nope\n", None);
        assert!(result.is_err());
    }

    #[test]
    fn replace_lines_single() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        crate::edit::replace_lines(&ws, &path, 2, 2, "// replaced\n", None).expect("replace");
        let content = std::fs::read_to_string(&path).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[1], "// replaced");
    }

    #[test]
    fn replace_lines_range() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let original = std::fs::read_to_string(&path).expect("read");
        let original_line_count = original.lines().count();
        crate::edit::replace_lines(&ws, &path, 2, 4, "// single replacement\n", None)
            .expect("replace");
        let content = std::fs::read_to_string(&path).expect("read");
        let new_line_count = content.lines().count();
        // Replaced 3 lines with 1 line
        assert!(new_line_count < original_line_count);
        assert!(content.contains("// single replacement"));
    }

    #[test]
    fn replace_lines_invalid_range() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let path = root.join("lib.rs");
        let result = crate::edit::replace_lines(&ws, &path, 5, 3, "nope\n", None);
        assert!(result.is_err());
    }

    #[test]
    fn find_referencing_symbols_enriches_context() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        // "self" appears inside methods - should report containing symbol
        let refs =
            crate::references::find_referencing_symbols(&ws, "self", None, None).expect("refs");
        assert!(!refs.is_empty());
        // At least some references should have containing_symbol set
        let enriched: Vec<_> = refs
            .iter()
            .filter(|r| r.containing_symbol.is_some())
            .collect();
        assert!(
            !enriched.is_empty(),
            "should find containing symbols for at least some refs"
        );
    }

    #[test]
    fn find_files_glob() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root);
        let rs_files = ws.find_files("*.rs").expect("glob");
        assert!(!rs_files.is_empty());
        assert!(rs_files
            .iter()
            .all(|f| f.extension().is_some_and(|e| e == "rs")));

        let py_files = ws.find_files("**/*.py").expect("glob");
        assert!(!py_files.is_empty());
    }

    #[test]
    fn find_files_no_match() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root);
        let files = ws.find_files("*.xyz").expect("glob");
        assert!(files.is_empty());
    }

    #[test]
    fn list_dir_non_recursive() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let entries = ws.list_dir(&root, false).expect("list");
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"lib.rs"));
        assert!(names.contains(&"sub"));
    }

    #[test]
    fn list_dir_recursive() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let entries = ws.list_dir(&root, true).expect("list");
        assert!(entries.iter().any(|e| e.name.contains("main.py")));
    }

    #[test]
    fn list_dir_not_found() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root.clone());
        let result = ws.list_dir(&root.join("nonexistent"), false);
        assert!(result.is_err());
    }
}
