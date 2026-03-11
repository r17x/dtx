//! Natural language intent parser.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Error parsing natural language.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Failed to parse intent.
    #[error("Failed to parse intent: {0}")]
    Parse(String),

    /// Low confidence in parsing.
    #[error("Low confidence ({confidence}): {interpretation}")]
    LowConfidence {
        confidence: f32,
        interpretation: String,
    },

    /// Ambiguous command.
    #[error("Ambiguous command: {0}")]
    Ambiguous(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for ParseError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

/// A parsed natural language intent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedIntent {
    /// The operation to perform (start, stop, restart, status, logs).
    pub operation: String,
    /// Target resources (empty = all).
    pub targets: Vec<String>,
    /// Additional options.
    #[serde(default)]
    pub options: HashMap<String, String>,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
}

impl ParsedIntent {
    /// Create a new parsed intent.
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            targets: Vec::new(),
            options: HashMap::new(),
            confidence: 1.0,
        }
    }

    /// Add a target.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.targets.push(target.into());
        self
    }

    /// Add targets.
    pub fn with_targets(mut self, targets: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.targets.extend(targets.into_iter().map(Into::into));
        self
    }

    /// Add an option.
    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    /// Set confidence.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Check if this intent targets all resources.
    pub fn targets_all(&self) -> bool {
        self.targets.is_empty()
    }

    /// Check if confidence is above threshold.
    pub fn is_confident(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }
}

/// Intent parser using AI or pattern matching.
pub struct IntentParser {
    /// Known operation patterns.
    operation_patterns: HashMap<&'static str, Vec<&'static str>>,
    /// Known resource aliases.
    resource_aliases: HashMap<&'static str, &'static str>,
}

impl IntentParser {
    /// Create a new intent parser.
    pub fn new() -> Self {
        let mut operation_patterns = HashMap::new();

        operation_patterns.insert(
            "start",
            vec![
                "start", "run", "launch", "begin", "spin up", "boot", "bring up",
            ],
        );
        operation_patterns.insert(
            "stop",
            vec![
                "stop",
                "halt",
                "kill",
                "terminate",
                "shutdown",
                "shut down",
                "bring down",
            ],
        );
        operation_patterns.insert("restart", vec!["restart", "reboot", "bounce"]);
        operation_patterns.insert(
            "status",
            vec!["status", "state", "check", "health", "how is", "is running"],
        );
        operation_patterns.insert(
            "logs",
            vec![
                "logs", "log", "output", "show me", "view", "tail", "see logs",
            ],
        );

        let mut resource_aliases = HashMap::new();
        resource_aliases.insert("db", "postgres");
        resource_aliases.insert("database", "postgres");
        resource_aliases.insert("pg", "postgres");
        resource_aliases.insert("cache", "redis");
        resource_aliases.insert("web", "nginx");
        resource_aliases.insert("server", "api");
        resource_aliases.insert("everything", "");
        resource_aliases.insert("all", "");

        Self {
            operation_patterns,
            resource_aliases,
        }
    }

    /// Parse natural language input into an intent.
    pub fn parse(&self, input: &str) -> Result<ParsedIntent, ParseError> {
        let input_lower = input.to_lowercase();
        let words: Vec<&str> = input_lower.split_whitespace().collect();

        if words.is_empty() {
            return Err(ParseError::Parse("Empty input".to_string()));
        }

        // Find operation
        let (operation, confidence) = self.find_operation(&input_lower)?;

        // Find targets
        let targets = self.find_targets(&input_lower, &operation);

        // Parse options
        let options = self.parse_options(&input_lower);

        let intent = ParsedIntent {
            operation,
            targets,
            options,
            confidence,
        };

        Ok(intent)
    }

    /// Find the operation in the input.
    fn find_operation(&self, input: &str) -> Result<(String, f32), ParseError> {
        let mut best_match: Option<(&str, f32)> = None;

        for (operation, patterns) in &self.operation_patterns {
            for pattern in patterns {
                if input.contains(pattern) {
                    // Calculate confidence based on match quality
                    let confidence = if input.starts_with(pattern) {
                        0.95
                    } else if input.contains(&format!(" {} ", pattern)) {
                        0.85
                    } else {
                        0.7
                    };

                    if best_match.is_none() || confidence > best_match.unwrap().1 {
                        best_match = Some((operation, confidence));
                    }
                }
            }
        }

        match best_match {
            Some((op, conf)) => Ok((op.to_string(), conf)),
            None => Err(ParseError::Ambiguous(format!(
                "Could not determine operation from: {}",
                input
            ))),
        }
    }

    /// Find target resources in the input.
    fn find_targets(&self, input: &str, operation: &str) -> Vec<String> {
        let mut targets = Vec::new();

        // Remove operation patterns from input
        let mut cleaned = input.to_string();
        if let Some(patterns) = self.operation_patterns.get(operation) {
            for pattern in patterns {
                cleaned = cleaned.replace(pattern, " ");
            }
        }

        // Common words to ignore
        let stop_words = [
            "the",
            "a",
            "an",
            "and",
            "or",
            "with",
            "in",
            "on",
            "for",
            "to",
            "from",
            "my",
            "please",
            "service",
            "services",
            "resource",
            "resources",
        ];

        // Extract potential targets
        for word in cleaned.split_whitespace() {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if word.is_empty() || stop_words.contains(&word) {
                continue;
            }

            // Check for aliases
            if let Some(resolved) = self.resource_aliases.get(word) {
                if !resolved.is_empty() && !targets.contains(&resolved.to_string()) {
                    targets.push(resolved.to_string());
                }
            } else if !targets.contains(&word.to_string()) {
                targets.push(word.to_string());
            }
        }

        targets
    }

    /// Parse options from input.
    fn parse_options(&self, input: &str) -> HashMap<String, String> {
        let mut options = HashMap::new();

        // Look for follow flag
        if input.contains("follow") || input.contains("-f") || input.contains("stream") {
            options.insert("follow".to_string(), "true".to_string());
        }

        // Look for line count
        for word in input.split_whitespace() {
            if let Ok(n) = word.parse::<u32>() {
                if n <= 1000 {
                    options.insert("lines".to_string(), n.to_string());
                }
            }
        }

        options
    }
}

impl Default for IntentParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_intent_builder() {
        let intent = ParsedIntent::new("start")
            .with_target("postgres")
            .with_target("redis")
            .with_option("timeout", "30")
            .with_confidence(0.9);

        assert_eq!(intent.operation, "start");
        assert_eq!(intent.targets, vec!["postgres", "redis"]);
        assert_eq!(intent.options.get("timeout"), Some(&"30".to_string()));
        assert_eq!(intent.confidence, 0.9);
    }

    #[test]
    fn parsed_intent_targets_all() {
        let intent = ParsedIntent::new("start");
        assert!(intent.targets_all());

        let intent = intent.with_target("api");
        assert!(!intent.targets_all());
    }

    #[test]
    fn parsed_intent_is_confident() {
        let intent = ParsedIntent::new("start").with_confidence(0.8);
        assert!(intent.is_confident(0.7));
        assert!(!intent.is_confident(0.9));
    }

    #[test]
    fn parse_start_command() {
        let parser = IntentParser::new();
        let intent = parser.parse("start the database").unwrap();

        assert_eq!(intent.operation, "start");
        assert!(intent.targets.contains(&"postgres".to_string()));
    }

    #[test]
    fn parse_stop_command() {
        let parser = IntentParser::new();
        let intent = parser.parse("stop everything").unwrap();

        assert_eq!(intent.operation, "stop");
        assert!(intent.targets_all());
    }

    #[test]
    fn parse_logs_with_follow() {
        let parser = IntentParser::new();
        let intent = parser.parse("show me the api logs follow").unwrap();

        assert_eq!(intent.operation, "logs");
        assert_eq!(intent.options.get("follow"), Some(&"true".to_string()));
    }

    #[test]
    fn parse_restart_command() {
        let parser = IntentParser::new();
        let intent = parser.parse("restart redis").unwrap();

        assert_eq!(intent.operation, "restart");
        assert!(intent.targets.contains(&"redis".to_string()));
    }

    #[test]
    fn parse_status_command() {
        let parser = IntentParser::new();
        let intent = parser.parse("check the status of postgres").unwrap();

        assert_eq!(intent.operation, "status");
        assert!(intent.targets.contains(&"postgres".to_string()));
    }

    #[test]
    fn parse_with_multiple_targets() {
        let parser = IntentParser::new();
        let intent = parser.parse("start postgres and redis").unwrap();

        assert_eq!(intent.operation, "start");
        assert!(intent.targets.contains(&"postgres".to_string()));
        assert!(intent.targets.contains(&"redis".to_string()));
    }

    #[test]
    fn parse_with_alias() {
        let parser = IntentParser::new();
        let intent = parser.parse("start the db").unwrap();

        assert_eq!(intent.operation, "start");
        assert!(intent.targets.contains(&"postgres".to_string()));
    }

    #[test]
    fn parse_empty_fails() {
        let parser = IntentParser::new();
        let result = parser.parse("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_ambiguous_fails() {
        let parser = IntentParser::new();
        let result = parser.parse("do something");
        assert!(result.is_err());
    }
}
