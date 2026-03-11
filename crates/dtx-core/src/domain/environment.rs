//! Environment - A collection of valid environment variables.
//!
//! # Invariants
//!
//! Each EnvVar guarantees:
//! - Key is non-empty
//! - Key matches pattern `[A-Z_][A-Z0-9_]*`
//! - Value contains no null bytes
//!
//! # Example
//!
//! ```rust
//! use dtx_core::domain::{Environment, EnvVar};
//!
//! // Parse from KEY=value format
//! let var: EnvVar = "NODE_ENV=production".parse().unwrap();
//! assert_eq!(var.key(), "NODE_ENV");
//! assert_eq!(var.value(), "production");
//!
//! // Build environment
//! let env = Environment::new()
//!     .with("NODE_ENV", "production")
//!     .with("PORT", "3000");
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// A valid environment variable key-value pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    key: String,
    value: String,
}

/// A collection of environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    vars: HashMap<String, String>,
}

/// Errors that can occur when parsing environment variables.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseEnvironmentError {
    #[error("environment variable key cannot be empty")]
    EmptyKey,

    #[error("environment variable key '{0}' is invalid (must match [A-Z_][A-Z0-9_]*)")]
    InvalidKey(String),

    #[error("environment variable value contains null byte")]
    NullByteInValue,

    #[error("environment variable format invalid, expected KEY=value: '{0}'")]
    InvalidFormat(String),
}

impl EnvVar {
    /// Create a new environment variable, validating key and value.
    pub fn new(
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ParseEnvironmentError> {
        let key = key.into();
        let value = value.into();

        Self::validate_key(&key)?;
        Self::validate_value(&value)?;

        Ok(EnvVar { key, value })
    }

    /// Get the key.
    #[inline]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Get the value.
    #[inline]
    pub fn value(&self) -> &str {
        &self.value
    }

    fn validate_key(key: &str) -> Result<(), ParseEnvironmentError> {
        if key.is_empty() {
            return Err(ParseEnvironmentError::EmptyKey);
        }

        let first = key.chars().next().unwrap();
        if !first.is_ascii_uppercase() && first != '_' {
            return Err(ParseEnvironmentError::InvalidKey(key.to_string()));
        }

        for c in key.chars().skip(1) {
            if !c.is_ascii_uppercase() && !c.is_ascii_digit() && c != '_' {
                return Err(ParseEnvironmentError::InvalidKey(key.to_string()));
            }
        }

        Ok(())
    }

    fn validate_value(value: &str) -> Result<(), ParseEnvironmentError> {
        if value.contains('\0') {
            return Err(ParseEnvironmentError::NullByteInValue);
        }
        Ok(())
    }
}

impl FromStr for EnvVar {
    type Err = ParseEnvironmentError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (key, value) = s
            .split_once('=')
            .ok_or_else(|| ParseEnvironmentError::InvalidFormat(s.to_string()))?;

        EnvVar::new(key, value)
    }
}

impl fmt::Display for EnvVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.key, self.value)
    }
}

impl Environment {
    /// Create a new empty environment.
    pub fn new() -> Self {
        Environment {
            vars: HashMap::new(),
        }
    }

    /// Add a variable (builder pattern).
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// Get a variable by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(|s| s.as_str())
    }

    /// Set a variable.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    /// Remove a variable.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.vars.remove(key)
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// Number of variables.
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// Iterate over variables.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Convert to HashMap.
    pub fn into_map(self) -> HashMap<String, String> {
        self.vars
    }

    /// Get a reference to the inner HashMap.
    pub fn as_map(&self) -> &HashMap<String, String> {
        &self.vars
    }

    /// Parse from a list of KEY=value strings.
    pub fn from_strings<I, S>(items: I) -> Result<Self, ParseEnvironmentError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut env = Environment::new();
        for item in items {
            let var: EnvVar = item.as_ref().parse()?;
            env.set(var.key, var.value);
        }
        Ok(env)
    }

    /// Create from a HashMap (used when loading from DB).
    pub fn from_map(map: HashMap<String, String>) -> Self {
        Environment { vars: map }
    }
}

impl FromIterator<EnvVar> for Environment {
    fn from_iter<T: IntoIterator<Item = EnvVar>>(iter: T) -> Self {
        let mut env = Environment::new();
        for var in iter {
            env.set(var.key, var.value);
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_env_var() {
        let var: EnvVar = "NODE_ENV=production".parse().unwrap();
        assert_eq!(var.key(), "NODE_ENV");
        assert_eq!(var.value(), "production");
    }

    #[test]
    fn valid_env_var_underscore_start() {
        let var: EnvVar = "_PRIVATE=secret".parse().unwrap();
        assert_eq!(var.key(), "_PRIVATE");
    }

    #[test]
    fn valid_env_var_empty_value() {
        let var: EnvVar = "EMPTY=".parse().unwrap();
        assert_eq!(var.value(), "");
    }

    #[test]
    fn invalid_lowercase_key() {
        assert!(matches!(
            "node_env=production".parse::<EnvVar>(),
            Err(ParseEnvironmentError::InvalidKey(_))
        ));
    }

    #[test]
    fn invalid_no_equals() {
        assert!(matches!(
            "NODEENV".parse::<EnvVar>(),
            Err(ParseEnvironmentError::InvalidFormat(_))
        ));
    }

    #[test]
    fn environment_builder() {
        let env = Environment::new()
            .with("NODE_ENV", "production")
            .with("PORT", "3000");

        assert_eq!(env.get("NODE_ENV"), Some("production"));
        assert_eq!(env.get("PORT"), Some("3000"));
        assert_eq!(env.len(), 2);
    }

    #[test]
    fn environment_from_strings() {
        let env = Environment::from_strings(["NODE_ENV=production", "PORT=3000"]).unwrap();

        assert_eq!(env.get("NODE_ENV"), Some("production"));
        assert_eq!(env.get("PORT"), Some("3000"));
    }

    #[test]
    fn serde_roundtrip() {
        let env = Environment::new()
            .with("NODE_ENV", "production")
            .with("PORT", "3000");

        let json = serde_json::to_string(&env).unwrap();
        let parsed: Environment = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.get("NODE_ENV"), Some("production"));
        assert_eq!(parsed.get("PORT"), Some("3000"));
    }
}
