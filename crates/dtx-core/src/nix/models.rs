//! Nix package models.

use serde::{Deserialize, Serialize};

/// A Nix package from search results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Package {
    /// Full attribute path (e.g., "legacyPackages.x86_64-linux.postgresql")
    pub attr_path: String,
    /// Full attribute name for backwards compatibility
    pub name: String,
    /// Package name (e.g., "postgresql")
    pub pname: String,
    /// Package version
    pub version: String,
    /// Package description
    pub description: String,
}

/// Detailed package information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    /// Package name
    pub name: String,
    /// Package version
    pub version: String,
    /// Package description
    pub description: String,
    /// Homepage URL
    pub homepage: Option<String>,
    /// License identifier
    pub license: Option<String>,
}

/// Raw search result from `nix search --json`.
#[derive(Debug, Deserialize)]
pub struct NixSearchResult {
    /// Package name
    pub pname: String,
    /// Package version
    pub version: String,
    /// Package description
    #[serde(default)]
    pub description: String,
}

/// Search result with tier information.
///
/// Indicates which search tier produced the results and whether
/// there might be version mismatches.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Found packages.
    pub packages: Vec<Package>,
    /// Which tier found the results.
    pub tier: SearchTier,
    /// Whether results might differ from user's pinned version.
    pub version_warning: bool,
}

/// Which search tier produced results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchTier {
    /// Tier 1: Evaluated from user's flake.
    FlakeEval,
    /// Tier 2: Evaluated from pinned flake.lock revision.
    PinnedRevision,
    /// Tier 3: Searched latest nixpkgs (may not match user's version).
    LatestNixpkgs,
    /// From cache (tier unknown).
    Cached,
}

impl SearchTier {
    /// Get a human-readable description of this tier.
    pub fn description(&self) -> &'static str {
        match self {
            SearchTier::FlakeEval => "from your flake.nix",
            SearchTier::PinnedRevision => "from your pinned nixpkgs",
            SearchTier::LatestNixpkgs => "from latest nixpkgs (may differ from your version)",
            SearchTier::Cached => "from cache",
        }
    }
}

impl Default for SearchResult {
    fn default() -> Self {
        Self {
            packages: vec![],
            tier: SearchTier::LatestNixpkgs,
            version_warning: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_serialization() {
        let pkg = Package {
            attr_path: "legacyPackages.x86_64-linux.postgresql".to_string(),
            name: "nixpkgs#postgresql".to_string(),
            pname: "postgresql".to_string(),
            version: "15.4".to_string(),
            description: "A powerful, open source object-relational database system".to_string(),
        };

        let json = serde_json::to_string(&pkg).unwrap();
        assert!(json.contains("postgresql"));

        let deserialized: Package = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pname, "postgresql");
    }

    #[test]
    fn test_nix_search_result_deserialize() {
        let json = r#"{"pname": "redis", "version": "7.2.3", "description": "An open source in-memory data store"}"#;
        let result: NixSearchResult = serde_json::from_str(json).unwrap();

        assert_eq!(result.pname, "redis");
        assert_eq!(result.version, "7.2.3");
    }

    #[test]
    fn test_nix_search_result_missing_description() {
        let json = r#"{"pname": "test", "version": "1.0"}"#;
        let result: NixSearchResult = serde_json::from_str(json).unwrap();

        assert_eq!(result.description, "");
    }

    #[test]
    fn test_search_tier_description() {
        assert_eq!(SearchTier::FlakeEval.description(), "from your flake.nix");
        assert_eq!(
            SearchTier::PinnedRevision.description(),
            "from your pinned nixpkgs"
        );
        assert!(SearchTier::LatestNixpkgs
            .description()
            .contains("may differ"));
    }

    #[test]
    fn test_search_result_default() {
        let result = SearchResult::default();
        assert!(result.packages.is_empty());
        assert_eq!(result.tier, SearchTier::LatestNixpkgs);
        assert!(result.version_warning);
    }
}
