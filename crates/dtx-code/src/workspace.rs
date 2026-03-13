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

#[derive(Clone, Debug, serde::Serialize)]
pub struct SymbolMatch {
    pub file: PathBuf,
    pub symbol: Symbol,
    pub body: Option<String>,
}

pub struct WorkspaceIndex {
    root: PathBuf,
    cache: DashMap<PathBuf, FileIndex>,
}

impl WorkspaceIndex {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
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

    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    pub fn get_or_parse(
        &self,
        path: &Path,
    ) -> Result<dashmap::mapref::one::Ref<'_, PathBuf, FileIndex>> {
        let abs = self.resolve_path(path);

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
        self.cache.remove(&self.resolve_path(path));
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
            let abs = self.resolve_path(p);
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
                let src = match std::fs::read_to_string(&file_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let idx = match self.get_or_parse(&file_path) {
                    Ok(idx) => idx,
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

    pub fn get_overview(&self, path: &Path, depth: Option<usize>) -> Result<SymbolOverview> {
        let abs = self.resolve_path(path);
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
        )
        .expect("replace");

        let content = std::fs::read_to_string(&path).expect("read");
        assert!(content.contains("false"));
        assert!(!content.contains("true"));
    }

    #[test]
    fn search_pattern_finds_matches() {
        let (_dir, root) = create_test_workspace();
        let matches = crate::search::search_pattern(&root, r"standalone", None, 1).expect("search");
        assert!(matches.len() >= 2);
    }

    #[test]
    fn list_files_finds_supported() {
        let (_dir, root) = create_test_workspace();
        let ws = WorkspaceIndex::new(root);
        let files = ws.list_files();
        assert!(files
            .iter()
            .any(|f| f.extension().map_or(false, |e| e == "rs")));
        assert!(files
            .iter()
            .any(|f| f.extension().map_or(false, |e| e == "py")));
    }

    #[test]
    fn find_references_across_files() {
        let (_dir, root) = create_test_workspace();
        // "self" appears in both Rust and Python files
        let refs = crate::references::find_references(&root, "self", None).expect("refs");
        assert!(!refs.is_empty());
    }
}
