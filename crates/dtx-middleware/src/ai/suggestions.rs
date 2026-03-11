//! AI-powered configuration suggestions.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

use super::provider::{AIProvider, AIRequest};
use super::AIError;

/// Probe configuration suggestion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeConfigSuggestion {
    /// Probe type: exec, http_get, or tcp_socket.
    #[serde(rename = "type")]
    pub probe_type: String,
    /// Command for exec probes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Host for HTTP/TCP probes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Port for HTTP/TCP probes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Path for HTTP probes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Period between checks in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_secs: Option<u32>,
    /// Timeout per check in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u32>,
}

/// Dependency suggestion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DependencySuggestion {
    /// Source service.
    pub from: String,
    /// Target dependency.
    pub to: String,
    /// Condition (started, healthy, completed).
    pub condition: String,
    /// Reason for the suggestion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Service information for suggestions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// Service name.
    pub name: String,
    /// Command to run.
    pub command: String,
    /// Port if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Known package/technology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// AI-powered suggestions generator.
pub struct Suggestions {
    provider: Arc<dyn AIProvider>,
}

impl Suggestions {
    /// Create a new suggestions generator.
    pub fn new(provider: Arc<dyn AIProvider>) -> Self {
        Self { provider }
    }

    /// Suggest a health check configuration for a service.
    pub async fn suggest_health_check(
        &self,
        service: &ServiceInfo,
    ) -> Result<Option<ProbeConfigSuggestion>, AIError> {
        let prompt = format!(
            r#"Suggest a health check configuration for this service as JSON.

Service: {}
Command: {}
Port: {}
Package: {}

Return a JSON object with these fields:
- type: "exec", "http_get", or "tcp_socket"
- command: (for exec type only)
- host: (defaults to "127.0.0.1")
- port: (for http_get/tcp_socket)
- path: (for http_get, defaults to "/health" or "/")
- period_secs: (defaults to 10)
- timeout_secs: (defaults to 5)

Only return the JSON object, no explanation."#,
            service.name,
            service.command,
            service
                .port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            service.package.as_deref().unwrap_or("unknown")
        );

        let request = AIRequest::new(prompt)
            .with_system("You are a DevOps expert. Respond only with valid JSON.")
            .with_max_tokens(300)
            .with_temperature(0.1);

        let response = self.provider.complete(request).await?;

        // Try to parse JSON from response
        let text = response.text.trim();
        let json_text = if text.starts_with("```") {
            // Extract from code block
            text.lines()
                .skip(1)
                .take_while(|l| !l.starts_with("```"))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.to_string()
        };

        match serde_json::from_str(&json_text) {
            Ok(config) => {
                debug!(service = %service.name, "Health check suggestion generated");
                Ok(Some(config))
            }
            Err(e) => {
                debug!(error = %e, "Failed to parse health check suggestion");
                Ok(None)
            }
        }
    }

    /// Suggest dependencies between services.
    pub async fn suggest_dependencies(
        &self,
        services: &[ServiceInfo],
    ) -> Result<Vec<DependencySuggestion>, AIError> {
        if services.is_empty() {
            return Ok(Vec::new());
        }

        let service_list: Vec<String> = services
            .iter()
            .map(|s| {
                format!(
                    "- {}: {} (port: {}, pkg: {})",
                    s.name,
                    s.command,
                    s.port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    s.package.as_deref().unwrap_or("unknown")
                )
            })
            .collect();

        let prompt = format!(
            r#"Analyze these services and suggest dependencies as JSON array.

Services:
{}

Return a JSON array of dependency objects:
[
  {{"from": "service1", "to": "service2", "condition": "started", "reason": "why"}}
]

condition: "started" (basic), "healthy" (needs health check), "completed" (one-shot)
Only suggest dependencies that make sense. Empty array [] if none needed.
Only return the JSON array, no explanation."#,
            service_list.join("\n")
        );

        let request = AIRequest::new(prompt)
            .with_system("You are a DevOps expert. Respond only with valid JSON.")
            .with_max_tokens(500)
            .with_temperature(0.1);

        let response = self.provider.complete(request).await?;

        // Try to parse JSON from response
        let text = response.text.trim();
        let json_text = if text.starts_with("```") {
            text.lines()
                .skip(1)
                .take_while(|l| !l.starts_with("```"))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.to_string()
        };

        match serde_json::from_str(&json_text) {
            Ok(deps) => {
                debug!(count = services.len(), "Dependency suggestions generated");
                Ok(deps)
            }
            Err(e) => {
                debug!(error = %e, "Failed to parse dependency suggestions");
                Ok(Vec::new())
            }
        }
    }

    /// Suggest a fix for a configuration issue.
    pub async fn suggest_fix(&self, issue: &str, context: &str) -> Result<String, AIError> {
        let prompt = format!(
            r#"Suggest a fix for this configuration issue.

Issue: {}

Context:
{}

Provide a brief, actionable suggestion (1-2 sentences)."#,
            issue, context
        );

        let request = AIRequest::new(prompt)
            .with_system("You are a DevOps expert. Be concise and actionable.")
            .with_max_tokens(200)
            .with_temperature(0.3);

        let response = self.provider.complete(request).await?;
        Ok(response.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::NoopProvider;

    #[test]
    fn service_info() {
        let info = ServiceInfo {
            name: "postgres".to_string(),
            command: "postgres -D /data".to_string(),
            port: Some(5432),
            package: Some("postgresql".to_string()),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("postgres"));
        assert!(json.contains("5432"));
    }

    #[test]
    fn probe_config_suggestion() {
        let config = ProbeConfigSuggestion {
            probe_type: "http_get".to_string(),
            command: None,
            host: Some("127.0.0.1".to_string()),
            port: Some(8080),
            path: Some("/health".to_string()),
            period_secs: Some(10),
            timeout_secs: Some(5),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("http_get"));
        assert!(json.contains("/health"));
    }

    #[tokio::test]
    async fn suggestions_with_noop() {
        let provider = Arc::new(NoopProvider);
        let suggestions = Suggestions::new(provider);

        let service = ServiceInfo {
            name: "test".to_string(),
            command: "echo test".to_string(),
            port: None,
            package: None,
        };

        // NoopProvider returns empty, so no suggestion
        let result = suggestions.suggest_health_check(&service).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn suggest_dependencies_empty() {
        let provider = Arc::new(NoopProvider);
        let suggestions = Suggestions::new(provider);

        let result = suggestions.suggest_dependencies(&[]).await.unwrap();
        assert!(result.is_empty());
    }
}
