//! Nix package integration.
//!
//! This module provides Nix package search and validation functionality,
//! as well as flake.nix and .envrc generation.
//!
//! ## Architecture
//!
//! The module uses a tiered search strategy to solve the version mismatch problem
//! between `nix search nixpkgs` (latest) and user's pinned `flake.lock`:
//!
//! - **Tier 1**: Evaluate against user's flake (if in project with flake.nix)
//! - **Tier 2**: Evaluate against pinned nixpkgs revision (from flake.lock)
//! - **Tier 3**: Search latest nixpkgs (CLI fallback, with warning)

pub mod ast;
pub mod backend;
mod cache;
mod client;
pub mod command;
pub mod devenv;
pub mod envrc;
pub mod flake;
mod lockfile;
pub mod mappings;
mod models;
#[cfg(feature = "native-nix")]
pub mod native;
pub mod shell;
pub mod sync;

pub use ast::FlakeAst;
pub use backend::{CliBackend, NixBackend};
pub use client::NixClient;
pub use command::{
    analyze_service_packages, extract_executable, get_services_needing_attention, infer_package,
    infer_package_detailed, infer_package_with_config, infer_packages_for_services,
    is_local_binary, PackageAnalysisResult, PackageInference, ServicePackageAnalysis,
};
pub use devenv::{dev_env_cache, DevEnvCache, DevEnvironment};
pub use envrc::EnvrcGenerator;
pub use flake::FlakeGenerator;
pub use lockfile::FlakeLock;
pub use mappings::{init_project_config, init_user_config, MappingsConfig, PackageMappings};
pub use models::{Package, PackageInfo, SearchResult, SearchTier};
#[cfg(feature = "native-nix")]
pub use native::{detect_system, NativeNixEvaluator, NixValue};
pub use shell::NixShell;
pub use sync::{find_flake_path, sync_add_package, sync_remove_package};
