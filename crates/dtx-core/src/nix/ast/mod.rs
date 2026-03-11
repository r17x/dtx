//! AST-based Nix file manipulation using rnix-parser.
//!
//! This module provides type-safe AST operations for flake.nix files,
//! preserving user formatting, comments, and modifications.

mod flake;
mod parser;

pub use flake::FlakeAst;
pub use parser::{parse_nix, validate_flake_nix, validate_nix};
