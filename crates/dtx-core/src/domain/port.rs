//! Port - A valid non-privileged port number.
//!
//! # Invariants
//!
//! A Port guarantees:
//! - Not zero (OS assignment)
//! - Not privileged (>= 1024)
//!
//! # Example
//!
//! ```rust
//! use dtx_core::domain::Port;
//!
//! // Valid ports
//! assert!(Port::try_from(3000u16).is_ok());
//! assert!(Port::try_from(8080u16).is_ok());
//!
//! // Invalid ports
//! assert!(Port::try_from(0u16).is_err());    // Zero
//! assert!(Port::try_from(80u16).is_err());   // Privileged
//! assert!(Port::try_from(443u16).is_err());  // Privileged
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

/// A valid non-privileged port. If you have a Port, it IS valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct Port(u16);

/// The minimum non-privileged port number.
pub const MIN_NON_PRIVILEGED_PORT: u16 = 1024;

/// Errors that can occur when parsing a port.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParsePortError {
    #[error("port 0 is reserved for OS assignment and cannot be explicitly used")]
    Zero,

    #[error("port {0} is privileged (< 1024) and requires root/admin privileges")]
    Privileged(u16),
}

impl Port {
    /// The minimum valid port value.
    pub const MIN: u16 = MIN_NON_PRIVILEGED_PORT;

    /// The maximum valid port value.
    pub const MAX: u16 = u16::MAX;

    /// Create a new port, returning None if invalid.
    #[inline]
    pub fn new(value: u16) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// Get the port number.
    #[inline]
    pub fn get(&self) -> u16 {
        self.0
    }

    /// Common default ports for development.
    pub const fn default_http() -> Self {
        Port(3000)
    }

    pub const fn default_api() -> Self {
        Port(8080)
    }

    pub const fn default_db() -> Self {
        Port(5432)
    }
}

impl TryFrom<u16> for Port {
    type Error = ParsePortError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value == 0 {
            return Err(ParsePortError::Zero);
        }
        if value < MIN_NON_PRIVILEGED_PORT {
            return Err(ParsePortError::Privileged(value));
        }
        Ok(Port(value))
    }
}

impl From<Port> for u16 {
    fn from(port: Port) -> u16 {
        port.0
    }
}

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ports() {
        let valid: [u16; 5] = [1024, 3000, 8080, 9000, 65535];

        for port in valid {
            assert!(Port::try_from(port).is_ok(), "should be valid: {}", port);
        }
    }

    #[test]
    fn invalid_zero() {
        assert!(matches!(Port::try_from(0u16), Err(ParsePortError::Zero)));
    }

    #[test]
    fn invalid_privileged() {
        let privileged: [u16; 4] = [1, 22, 80, 443];

        for port in privileged {
            assert!(
                matches!(Port::try_from(port), Err(ParsePortError::Privileged(_))),
                "should be privileged: {}",
                port
            );
        }
    }

    #[test]
    fn boundary_1023() {
        assert!(matches!(
            Port::try_from(1023u16),
            Err(ParsePortError::Privileged(1023))
        ));
    }

    #[test]
    fn boundary_1024() {
        assert!(Port::try_from(1024u16).is_ok());
    }

    #[test]
    fn default_ports() {
        assert_eq!(Port::default_http().get(), 3000);
        assert_eq!(Port::default_api().get(), 8080);
        assert_eq!(Port::default_db().get(), 5432);
    }

    #[test]
    fn serde_roundtrip() {
        let port = Port::try_from(3000u16).unwrap();
        let json = serde_json::to_string(&port).unwrap();
        assert_eq!(json, "3000");

        let parsed: Port = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, port);
    }

    #[test]
    fn serde_rejects_invalid() {
        let result: Result<Port, _> = serde_json::from_str("80");
        assert!(result.is_err());
    }
}
