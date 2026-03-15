#[derive(Debug, thiserror::Error)]
pub enum CodeError {
    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Invalid line {0}: file has {1} lines")]
    InvalidLine(usize, usize),

    #[error("Invalid line range {0}-{1}: file has {2} lines")]
    InvalidLineRange(usize, usize, usize),

    #[error("Content hash mismatch: expected {expected}, actual {actual}")]
    ContentMismatch { expected: String, actual: String },

    #[error("Rename failed: {0}")]
    RenameFailed(String),

    #[error("Path traversal blocked: {0}")]
    PathTraversal(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CodeError>;
