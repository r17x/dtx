#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Memory not found: {0}")]
    NotFound(String),

    #[error("Invalid memory name: {0}")]
    InvalidName(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Frontmatter parse error: {0}")]
    Frontmatter(String),
}

pub type Result<T> = std::result::Result<T, MemoryError>;
