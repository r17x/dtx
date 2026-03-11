//! ShellCommand - A valid shell command string.
//!
//! # Invariants
//!
//! A ShellCommand guarantees:
//! - Non-empty (after trimming)
//! - No null bytes
//! - Balanced quotes (simple check)
//!
//! # Example
//!
//! ```rust
//! use dtx_core::domain::ShellCommand;
//!
//! // Valid commands
//! assert!("npm start".parse::<ShellCommand>().is_ok());
//! assert!("echo 'hello world'".parse::<ShellCommand>().is_ok());
//!
//! // Invalid commands
//! assert!("".parse::<ShellCommand>().is_err());           // Empty
//! assert!("echo 'hello".parse::<ShellCommand>().is_err()); // Unbalanced
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A valid shell command. If you have a ShellCommand, it IS valid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ShellCommand(String);

/// Errors that can occur when parsing a shell command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseShellCommandError {
    #[error("command cannot be empty")]
    Empty,

    #[error("command contains null byte at position {0}")]
    NullByte(usize),

    #[error("command has unbalanced single quotes")]
    UnbalancedSingleQuotes,

    #[error("command has unbalanced double quotes")]
    UnbalancedDoubleQuotes,
}

impl ShellCommand {
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

    /// Validate a string as a shell command.
    fn validate(s: &str) -> Result<(), ParseShellCommandError> {
        let trimmed = s.trim();

        // Non-empty
        if trimmed.is_empty() {
            return Err(ParseShellCommandError::Empty);
        }

        // No null bytes
        if let Some(pos) = trimmed.find('\0') {
            return Err(ParseShellCommandError::NullByte(pos));
        }

        // Balanced quotes (simple check - not handling escapes)
        let single_count = trimmed.matches('\'').count();
        if single_count % 2 != 0 {
            return Err(ParseShellCommandError::UnbalancedSingleQuotes);
        }

        let double_count = trimmed.matches('"').count();
        if double_count % 2 != 0 {
            return Err(ParseShellCommandError::UnbalancedDoubleQuotes);
        }

        Ok(())
    }
}

impl FromStr for ShellCommand {
    type Err = ParseShellCommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::validate(s)?;
        Ok(ShellCommand(s.trim().to_string()))
    }
}

impl TryFrom<String> for ShellCommand {
    type Error = ParseShellCommandError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::validate(&s)?;
        Ok(ShellCommand(s.trim().to_string()))
    }
}

impl From<ShellCommand> for String {
    fn from(cmd: ShellCommand) -> String {
        cmd.0
    }
}

impl fmt::Display for ShellCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ShellCommand {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_commands() {
        let valid = [
            "npm start",
            "echo 'hello world'",
            "python -m http.server 8000",
            "cargo run --release",
            "./start.sh",
            "NODE_ENV=production node server.js",
        ];

        for cmd in valid {
            assert!(
                cmd.parse::<ShellCommand>().is_ok(),
                "should be valid: {}",
                cmd
            );
        }
    }

    #[test]
    fn trims_whitespace() {
        let cmd: ShellCommand = "  npm start  ".parse().unwrap();
        assert_eq!(cmd.as_str(), "npm start");
    }

    #[test]
    fn invalid_empty() {
        assert!(matches!(
            "".parse::<ShellCommand>(),
            Err(ParseShellCommandError::Empty)
        ));
        assert!(matches!(
            "   ".parse::<ShellCommand>(),
            Err(ParseShellCommandError::Empty)
        ));
    }

    #[test]
    fn invalid_unbalanced_single_quotes() {
        assert!(matches!(
            "echo 'hello".parse::<ShellCommand>(),
            Err(ParseShellCommandError::UnbalancedSingleQuotes)
        ));
    }

    #[test]
    fn invalid_unbalanced_double_quotes() {
        assert!(matches!(
            "echo \"hello".parse::<ShellCommand>(),
            Err(ParseShellCommandError::UnbalancedDoubleQuotes)
        ));
    }

    #[test]
    fn serde_roundtrip() {
        let cmd: ShellCommand = "npm start".parse().unwrap();
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(json, "\"npm start\"");

        let parsed: ShellCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, cmd);
    }
}
