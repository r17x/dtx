//! Package search cache.

use super::models::Package;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Cache entry with timestamp.
struct CacheEntry {
    packages: Vec<Package>,
    timestamp: Instant,
}

/// In-memory cache for package search results.
pub struct PackageCache {
    entries: HashMap<String, CacheEntry>,
    ttl: Duration,
}

impl PackageCache {
    /// Creates a new cache with the specified TTL in seconds.
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Gets cached results for a query if not expired.
    pub fn get(&self, query: &str) -> Option<Vec<Package>> {
        self.entries
            .get(query)
            .filter(|entry| entry.timestamp.elapsed() < self.ttl)
            .map(|entry| entry.packages.clone())
    }

    /// Inserts results into the cache.
    pub fn insert(&mut self, query: String, packages: Vec<Package>) {
        self.entries.insert(
            query,
            CacheEntry {
                packages,
                timestamp: Instant::now(),
            },
        );
    }

    /// Clears all cache entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Removes expired entries.
    #[allow(dead_code)]
    pub fn remove_expired(&mut self) {
        self.entries
            .retain(|_, entry| entry.timestamp.elapsed() < self.ttl);
    }
}

impl Default for PackageCache {
    fn default() -> Self {
        Self::new(3600) // 1 hour default TTL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_package() -> Package {
        Package {
            attr_path: "legacyPackages.x86_64-linux.test".to_string(),
            name: "test".to_string(),
            pname: "test".to_string(),
            version: "1.0".to_string(),
            description: "Test package".to_string(),
        }
    }

    #[test]
    fn test_cache_hit() {
        let mut cache = PackageCache::new(3600);
        let packages = vec![test_package()];

        cache.insert("test".to_string(), packages.clone());

        let cached = cache.get("test").unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].pname, "test");
    }

    #[test]
    fn test_cache_miss() {
        let cache = PackageCache::new(3600);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_cache_expiry() {
        let mut cache = PackageCache::new(0); // Immediate expiry
        cache.insert("test".to_string(), vec![test_package()]);

        // Sleep briefly to ensure expiry
        std::thread::sleep(std::time::Duration::from_millis(10));

        assert!(cache.get("test").is_none());
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = PackageCache::new(3600);
        cache.insert("test1".to_string(), vec![test_package()]);
        cache.insert("test2".to_string(), vec![test_package()]);

        cache.clear();

        assert!(cache.get("test1").is_none());
        assert!(cache.get("test2").is_none());
    }

    #[test]
    fn test_cache_default() {
        let cache = PackageCache::default();
        assert_eq!(cache.ttl, Duration::from_secs(3600));
    }
}
