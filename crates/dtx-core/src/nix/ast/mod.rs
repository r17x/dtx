//! AST-based Nix file manipulation using rnix-parser.
//!
//! This module provides type-safe AST operations for flake.nix files,
//! preserving user formatting, comments, and modifications.

mod flake;
mod imports;
mod package_export;
mod parser;
mod script_detection;

pub use flake::FlakeAst;
pub use imports::{resolve_flake_imports, ResolvedNixFile};
pub use package_export::{export_scripts_as_packages, ExportResult};
pub use parser::{parse_nix, validate_flake_nix, validate_nix};
pub use script_detection::{detect_scripts, DetectedScript, ScriptContext};
