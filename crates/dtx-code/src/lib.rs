pub mod edit;
pub mod error;
pub mod index;
pub mod language;
pub mod parser;
pub mod patterns;
pub mod references;
pub mod rename;
pub mod search;
pub mod symbol;
pub mod workspace;

pub use edit::{
    content_hash, file_content_hash, insert_after_symbol, insert_at_line, insert_before_symbol,
    replace_lines, replace_symbol_body,
};
pub use error::{CodeError, Result};
pub use index::FileIndex;
pub use language::detect;
pub use parser::parse_source;
pub use references::{find_references, find_referencing_symbols, Reference};
pub use rename::{rename_symbol, RenameChange, RenameResult};
pub use search::{search_pattern, SearchMatch};
pub use symbol::{Symbol, SymbolKind, SymbolOverview};
pub use workspace::{DirEntryInfo, EntryKind, SymbolMatch, WorkspaceIndex};
