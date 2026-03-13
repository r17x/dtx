use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Enum,
    Interface,
    Trait,
    Impl,
    Module,
    Constant,
    Variable,
    TypeAlias,
    Import,
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Module => "module",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::TypeAlias => "type_alias",
            Self::Import => "import",
        };
        f.write_str(s)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub name_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub children: Vec<Symbol>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SymbolOverview {
    pub file: PathBuf,
    pub symbols: Vec<Symbol>,
}
