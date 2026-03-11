//! NixClient - public API for Nix integration with tiered search.
//!
//! The client implements a three-tier search strategy to solve the version
//! mismatch problem between `nix search nixpkgs` and user's pinned flake.lock.

use super::backend::{CliBackend, NixBackend};
use super::cache::PackageCache;
use super::lockfile::FlakeLock;
use super::models::{Package, PackageInfo, SearchResult, SearchTier};
use crate::error::NixError;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Client for Nix package operations with tiered search.
///
/// ## Search Strategy
///
/// 1. **Tier 1**: Evaluate against user's flake (if flake.nix exists)
/// 2. **Tier 2**: Evaluate against pinned nixpkgs revision (from flake.lock)
/// 3. **Tier 3**: Search latest nixpkgs (CLI fallback, with warning)
pub struct NixClient {
    backend: Arc<dyn NixBackend>,
    cache: Arc<Mutex<PackageCache>>,
    project_path: Option<PathBuf>,
}

impl NixClient {
    /// Creates a new NixClient with default cache TTL (1 hour).
    pub fn new() -> Self {
        Self {
            backend: Arc::new(CliBackend::new()),
            cache: Arc::new(Mutex::new(PackageCache::default())),
            project_path: None,
        }
    }

    /// Creates a NixClient with custom cache TTL.
    pub fn with_cache_ttl(ttl_secs: u64) -> Self {
        Self {
            backend: Arc::new(CliBackend::new()),
            cache: Arc::new(Mutex::new(PackageCache::new(ttl_secs))),
            project_path: None,
        }
    }

    /// Sets the project path for flake-aware search.
    ///
    /// When set, the client will attempt to use the project's flake.nix
    /// and flake.lock for version-accurate package searches.
    pub fn with_project_path(mut self, path: impl AsRef<Path>) -> Self {
        self.project_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Sets the project path (non-builder style).
    pub fn set_project_path(&mut self, path: impl AsRef<Path>) {
        self.project_path = Some(path.as_ref().to_path_buf());
    }

    /// Tiered package search.
    ///
    /// Returns packages matching the query along with tier information.
    pub async fn search_with_tier(&self, query: &str) -> Result<SearchResult, NixError> {
        // Check cache first
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(query) {
                debug!(query = %query, "Cache hit");
                return Ok(SearchResult {
                    packages: cached,
                    tier: SearchTier::Cached,
                    version_warning: false,
                });
            }
        }

        // Tier 1: Try flake evaluation
        if let Some(ref project_path) = self.project_path {
            if project_path.join("flake.nix").exists() {
                debug!(tier = 1, "Attempting flake evaluation search");
                match self.search_via_flake(project_path, query).await {
                    Ok(packages) if !packages.is_empty() => {
                        info!(tier = 1, count = packages.len(), "Flake search succeeded");
                        self.cache_results(query, &packages).await;
                        return Ok(SearchResult {
                            packages,
                            tier: SearchTier::FlakeEval,
                            version_warning: false,
                        });
                    }
                    Ok(_) => debug!("Flake search returned empty, trying tier 2"),
                    Err(e) => debug!(error = %e, "Flake search failed, trying tier 2"),
                }
            }
        }

        // Tier 2: Try flake.lock pinned revision
        if let Some(flake_ref) = self.get_pinned_nixpkgs_ref() {
            debug!(tier = 2, flake_ref = %flake_ref, "Attempting pinned revision search");
            match self.backend.search(query, Some(&flake_ref)).await {
                Ok(packages) if !packages.is_empty() => {
                    info!(tier = 2, count = packages.len(), "Pinned search succeeded");
                    self.cache_results(query, &packages).await;
                    return Ok(SearchResult {
                        packages,
                        tier: SearchTier::PinnedRevision,
                        version_warning: false,
                    });
                }
                Ok(_) => debug!("Pinned search returned empty, trying tier 3"),
                Err(e) => debug!(error = %e, "Pinned search failed, trying tier 3"),
            }
        }

        // Tier 3: CLI fallback with warning
        warn!(
            "Searching latest nixpkgs - results may differ from your pinned version. \
             Consider adding a flake.nix to your project."
        );
        debug!(tier = 3, "Using CLI fallback search");

        let packages = self.backend.search(query, None).await?;
        self.cache_results(query, &packages).await;

        Ok(SearchResult {
            packages,
            tier: SearchTier::LatestNixpkgs,
            version_warning: true,
        })
    }

    /// Searches for packages matching the query.
    ///
    /// This is a convenience method that returns just the packages.
    /// Use `search_with_tier` for tier information.
    pub async fn search(&self, query: &str) -> Result<Vec<Package>, NixError> {
        let result = self.search_with_tier(query).await?;
        Ok(result.packages)
    }

    /// Search packages within user's flake context (Tier 1).
    async fn search_via_flake(
        &self,
        project_path: &Path,
        query: &str,
    ) -> Result<Vec<Package>, NixError> {
        let system = FlakeLock::current_system();
        let path_str = project_path.display();

        // Build Nix expression that searches within the flake's packages
        let expr = format!(
            r#"
            let
              flake = builtins.getFlake "path:{}";
              pkgs = flake.legacyPackages.{} or (flake.outputs.packages.{} or {{}});
              matching = builtins.filter
                (name: builtins.match ".*{}.*" name != null)
                (builtins.attrNames pkgs);
              take = n: list: builtins.genList (i: builtins.elemAt list i) (if builtins.length list < n then builtins.length list else n);
              getInfo = name: {{
                inherit name;
                version = pkgs.${{name}}.version or "";
                description = pkgs.${{name}}.meta.description or "";
              }};
            in builtins.map getInfo (take 50 (builtins.sort (a: b: a < b) matching))
            "#,
            path_str, system, system, query
        );

        let output = self.backend.eval(&expr).await?;

        let results: Vec<serde_json::Value> =
            serde_json::from_str(&output).map_err(|e| NixError::ParseError(e.to_string()))?;

        Ok(results
            .into_iter()
            .filter_map(|v| {
                let name = v.get("name")?.as_str()?.to_string();
                Some(Package {
                    attr_path: format!("legacyPackages.{}.{}", system, name),
                    name: name.clone(),
                    pname: name,
                    version: v
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: v
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            })
            .collect())
    }

    /// Get pinned nixpkgs flake reference from flake.lock.
    fn get_pinned_nixpkgs_ref(&self) -> Option<String> {
        let project_path = self.project_path.as_ref()?;
        let lock_path = project_path.join("flake.lock");

        if !lock_path.exists() {
            return None;
        }

        FlakeLock::parse_file(&lock_path)
            .ok()
            .and_then(|lock| lock.get_nixpkgs_flake_ref())
    }

    /// Cache search results.
    async fn cache_results(&self, query: &str, packages: &[Package]) {
        let mut cache = self.cache.lock().await;
        cache.insert(query.to_string(), packages.to_vec());
    }

    /// Validates that a package exists (uses tiered approach).
    pub async fn validate(&self, package: &str) -> Result<bool, NixError> {
        // Try pinned first
        if let Some(flake_ref) = self.get_pinned_nixpkgs_ref() {
            if let Ok(valid) = self.backend.validate(package, Some(&flake_ref)).await {
                return Ok(valid);
            }
        }

        // Fall back to latest
        self.backend.validate(package, None).await
    }

    /// Gets detailed information about a package.
    pub async fn get_info(&self, package: &str) -> Result<PackageInfo, NixError> {
        // Try pinned first
        if let Some(flake_ref) = self.get_pinned_nixpkgs_ref() {
            if let Ok(info) = self.backend.get_info(package, Some(&flake_ref)).await {
                return Ok(info);
            }
        }

        // Fall back to latest
        self.backend.get_info(package, None).await
    }

    /// Clears the search cache.
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.lock().await;
        cache.clear();
    }

    /// Check if Nix is available.
    pub fn is_available(&self) -> bool {
        self.backend.is_available()
    }

    /// Get the backend name.
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }
}

impl Default for NixClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = NixClient::new();
        assert_eq!(client.backend_name(), "CLI");
    }

    #[test]
    fn test_client_with_project_path() {
        let client = NixClient::new().with_project_path("/tmp/test");
        assert!(client.project_path.is_some());
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_search() {
        let client = NixClient::new();
        let results = client.search("hello").await.unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_search_with_tier() {
        let client = NixClient::new();
        let result = client.search_with_tier("hello").await.unwrap();

        assert!(!result.packages.is_empty());
        // Without a project path, should be tier 3
        assert_eq!(result.tier, SearchTier::LatestNixpkgs);
        assert!(result.version_warning);
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_caching() {
        let client = NixClient::new();

        // First call
        let start = std::time::Instant::now();
        let results1 = client.search("curl").await.unwrap();
        let first_duration = start.elapsed();

        // Second call (should be cached)
        let start = std::time::Instant::now();
        let result2 = client.search_with_tier("curl").await.unwrap();
        let second_duration = start.elapsed();

        assert_eq!(results1.len(), result2.packages.len());
        assert_eq!(result2.tier, SearchTier::Cached);
        assert!(
            second_duration < first_duration / 5,
            "Cache hit was not significantly faster"
        );
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_validate() {
        let client = NixClient::new();

        assert!(client.validate("hello").await.unwrap());
        assert!(!client.validate("zzz-nonexistent-123").await.unwrap());
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_get_info() {
        let client = NixClient::new();
        let info = client.get_info("hello").await.unwrap();

        assert_eq!(info.name, "hello");
        assert!(!info.version.is_empty());
    }

    #[tokio::test]
    async fn test_clear_cache() {
        let client = NixClient::new();
        client.clear_cache().await;
        // Should not panic
    }
}
