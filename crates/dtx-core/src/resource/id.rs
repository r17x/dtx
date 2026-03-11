//! ResourceId - Unique identifier for a resource.
//!
//! A simple wrapper around String for type safety. Unlike ServiceName,
//! ResourceId has minimal validation (just non-empty).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a resource.
///
/// ResourceId is a lightweight wrapper around String for type safety.
/// It ensures the ID is non-empty but otherwise allows any valid string.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceId(String);

impl ResourceId {
    /// Create a new ResourceId from any string-like type.
    ///
    /// # Panics
    /// Panics if the input is empty. Use `try_new` for fallible construction.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        assert!(!id.is_empty(), "ResourceId cannot be empty");
        Self(id)
    }

    /// Try to create a new ResourceId, returning None if empty.
    pub fn try_new(id: impl Into<String>) -> Option<Self> {
        let id = id.into();
        if id.is_empty() {
            None
        } else {
            Some(Self(id))
        }
    }

    /// Access the inner string.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner String.
    #[inline]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ResourceId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ResourceId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl AsRef<str> for ResourceId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn resource_id_equality() {
        let a = ResourceId::new("api");
        let b = ResourceId::new("api");
        assert_eq!(a, b);
    }

    #[test]
    fn resource_id_inequality() {
        let a = ResourceId::new("api");
        let b = ResourceId::new("web");
        assert_ne!(a, b);
    }

    #[test]
    fn resource_id_hash() {
        let mut set = HashSet::new();
        set.insert(ResourceId::new("api"));
        assert!(set.contains(&ResourceId::new("api")));
        assert!(!set.contains(&ResourceId::new("web")));
    }

    #[test]
    fn resource_id_display() {
        let id = ResourceId::new("my-service");
        assert_eq!(id.to_string(), "my-service");
    }

    #[test]
    fn resource_id_serde() {
        let id = ResourceId::new("api");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"api\"");

        let parsed: ResourceId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn resource_id_from_str() {
        let id: ResourceId = "api".into();
        assert_eq!(id.as_str(), "api");
    }

    #[test]
    fn resource_id_from_string() {
        let id: ResourceId = String::from("api").into();
        assert_eq!(id.as_str(), "api");
    }

    #[test]
    fn resource_id_try_new_empty() {
        assert!(ResourceId::try_new("").is_none());
    }

    #[test]
    fn resource_id_try_new_valid() {
        assert!(ResourceId::try_new("api").is_some());
    }

    #[test]
    #[should_panic(expected = "ResourceId cannot be empty")]
    fn resource_id_new_empty_panics() {
        let _ = ResourceId::new("");
    }
}
