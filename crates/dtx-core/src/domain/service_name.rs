//! ServiceName - A valid service identifier.
//!
//! # Invariants
//!
//! A ServiceName guarantees:
//! - Length: 2-63 characters
//! - Pattern: `[a-z][a-z0-9-]*[a-z0-9]`
//! - Not a process-compose reserved word
//! - No consecutive hyphens
//!
//! # Example
//!
//! ```rust
//! use dtx_core::domain::ServiceName;
//!
//! // Valid names
//! assert!("api".parse::<ServiceName>().is_ok());
//! assert!("my-api".parse::<ServiceName>().is_ok());
//! assert!("web-server-01".parse::<ServiceName>().is_ok());
//!
//! // Invalid names
//! assert!("a".parse::<ServiceName>().is_err());           // Too short
//! assert!("My-Api".parse::<ServiceName>().is_err());      // Uppercase
//! assert!("my--api".parse::<ServiceName>().is_err());     // Consecutive hyphens
//! assert!("version".parse::<ServiceName>().is_err());     // Reserved
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A valid service name. If you have a ServiceName, it IS valid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServiceName(String);

/// Reserved words that cannot be used as service names.
/// These conflict with process-compose YAML structure.
const RESERVED_WORDS: &[&str] = &[
    "version",
    "services",
    "processes",
    "global",
    "log_level",
    "log_location",
    "log_format",
    "shell",
    "environment",
];

/// Errors that can occur when parsing a service name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseServiceNameError {
    #[error("service name too short (min 2 chars): '{0}'")]
    TooShort(String),

    #[error("service name too long (max 63 chars): '{0}'")]
    TooLong(String),

    #[error("service name must start with lowercase letter: '{0}'")]
    InvalidStart(String),

    #[error("service name must end with lowercase letter or digit: '{0}'")]
    InvalidEnd(String),

    #[error("service name contains invalid character '{1}' at position {2}: '{0}'")]
    InvalidChar(String, char, usize),

    #[error("service name '{0}' is reserved (conflicts with process-compose)")]
    Reserved(String),

    #[error("service name contains consecutive hyphens: '{0}'")]
    ConsecutiveHyphens(String),
}

impl ServiceName {
    /// Maximum length for a service name (DNS label limit).
    pub const MAX_LENGTH: usize = 63;

    /// Minimum length for a service name.
    pub const MIN_LENGTH: usize = 2;

    /// Access the inner string. Guaranteed to be valid.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner String.
    #[inline]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Check if a string would be a valid service name (without allocating).
    pub fn is_valid(s: &str) -> bool {
        Self::validate(s).is_ok()
    }

    /// Validate a string as a service name, returning the first error if invalid.
    fn validate(s: &str) -> Result<(), ParseServiceNameError> {
        // Length checks
        if s.len() < Self::MIN_LENGTH {
            return Err(ParseServiceNameError::TooShort(s.to_string()));
        }
        if s.len() > Self::MAX_LENGTH {
            return Err(ParseServiceNameError::TooLong(s.to_string()));
        }

        // Start character must be lowercase letter
        let first = s.chars().next().unwrap();
        if !first.is_ascii_lowercase() {
            return Err(ParseServiceNameError::InvalidStart(s.to_string()));
        }

        // End character must be lowercase letter or digit
        let last = s.chars().last().unwrap();
        if !last.is_ascii_lowercase() && !last.is_ascii_digit() {
            return Err(ParseServiceNameError::InvalidEnd(s.to_string()));
        }

        // All characters must be lowercase, digit, or hyphen
        for (i, c) in s.chars().enumerate() {
            if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' {
                return Err(ParseServiceNameError::InvalidChar(s.to_string(), c, i));
            }
        }

        // No consecutive hyphens
        if s.contains("--") {
            return Err(ParseServiceNameError::ConsecutiveHyphens(s.to_string()));
        }

        // Not a reserved word
        if RESERVED_WORDS.contains(&s) {
            return Err(ParseServiceNameError::Reserved(s.to_string()));
        }

        Ok(())
    }
}

impl FromStr for ServiceName {
    type Err = ParseServiceNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::validate(s)?;
        Ok(ServiceName(s.to_string()))
    }
}

impl TryFrom<String> for ServiceName {
    type Error = ParseServiceNameError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::validate(&s)?;
        Ok(ServiceName(s))
    }
}

impl From<ServiceName> for String {
    fn from(name: ServiceName) -> String {
        name.0
    }
}

impl fmt::Display for ServiceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ServiceName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_service_names() {
        let valid = [
            "api",
            "my-api",
            "web-server",
            "db01",
            "my-web-server-01",
            "a1", // minimum length with digit
        ];

        for name in valid {
            assert!(
                name.parse::<ServiceName>().is_ok(),
                "should be valid: {}",
                name
            );
        }
    }

    #[test]
    fn invalid_too_short() {
        assert!(matches!(
            "a".parse::<ServiceName>(),
            Err(ParseServiceNameError::TooShort(_))
        ));
    }

    #[test]
    fn invalid_uppercase() {
        // Uppercase first letter returns InvalidStart, not InvalidChar
        assert!(matches!(
            "MyApi".parse::<ServiceName>(),
            Err(ParseServiceNameError::InvalidStart(_))
        ));
        // Uppercase in the middle returns InvalidChar
        assert!(matches!(
            "myApi".parse::<ServiceName>(),
            Err(ParseServiceNameError::InvalidChar(_, 'A', 2))
        ));
    }

    #[test]
    fn invalid_consecutive_hyphens() {
        assert!(matches!(
            "my--api".parse::<ServiceName>(),
            Err(ParseServiceNameError::ConsecutiveHyphens(_))
        ));
    }

    #[test]
    fn invalid_reserved() {
        assert!(matches!(
            "version".parse::<ServiceName>(),
            Err(ParseServiceNameError::Reserved(_))
        ));
    }

    #[test]
    fn invalid_starts_with_digit() {
        assert!(matches!(
            "1api".parse::<ServiceName>(),
            Err(ParseServiceNameError::InvalidStart(_))
        ));
    }

    #[test]
    fn invalid_ends_with_hyphen() {
        assert!(matches!(
            "api-".parse::<ServiceName>(),
            Err(ParseServiceNameError::InvalidEnd(_))
        ));
    }

    #[test]
    fn invalid_space() {
        assert!(matches!(
            "my api".parse::<ServiceName>(),
            Err(ParseServiceNameError::InvalidChar(_, ' ', 2))
        ));
    }

    #[test]
    fn serde_roundtrip() {
        let name: ServiceName = "my-api".parse().unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"my-api\"");

        let parsed: ServiceName = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, name);
    }

    #[test]
    fn serde_rejects_invalid() {
        let result: Result<ServiceName, _> = serde_json::from_str("\"my api\"");
        assert!(result.is_err());
    }
}
