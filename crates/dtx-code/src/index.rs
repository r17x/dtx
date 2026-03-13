use std::path::PathBuf;
use std::time::SystemTime;

use ast_grep_language::SupportLang;

use crate::symbol::Symbol;

pub struct FileIndex {
    pub path: PathBuf,
    pub language: SupportLang,
    pub mtime: SystemTime,
    pub symbols: Vec<Symbol>,
}
