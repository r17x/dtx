//! ConfigStore error types.

use crate::config::schema::SchemaError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("resource '{0}' already exists")]
    DuplicateResource(String),

    #[error("resource '{0}' not found")]
    ResourceNotFound(String),

    #[error("project not found: no .dtx directory")]
    ProjectNotFound,

    #[error("config error: {0}")]
    Config(#[from] SchemaError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
