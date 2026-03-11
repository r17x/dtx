//! AI agent resource implementation (placeholder).
//!
//! This module provides a stub AgentResource implementation that returns
//! "not implemented" errors. It defines the type structure for future
//! implementation in Phase 8.3.

use std::any::Any;

use async_trait::async_trait;
use dtx_core::resource::{
    Context, HealthStatus, LogStream, Resource, ResourceError, ResourceId, ResourceKind,
    ResourceResult, ResourceState,
};
use serde::{Deserialize, Serialize};

/// Configuration for an AI agent resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResourceConfig {
    /// Unique identifier for this agent.
    pub id: ResourceId,
    /// Model identifier (e.g., "gpt-4", "claude-3-opus").
    pub model: String,
    /// Provider name (e.g., "openai", "anthropic", "ollama").
    pub provider: String,
    /// System prompt to configure agent behavior.
    pub system_prompt: Option<String>,
    /// Maximum tokens for responses.
    pub max_tokens: Option<u32>,
    /// Temperature for response generation (0.0 to 2.0).
    pub temperature: Option<f32>,
    /// API endpoint override (for self-hosted models).
    pub api_endpoint: Option<String>,
    /// Additional provider-specific options.
    pub extra_options: Option<serde_json::Value>,
}

impl AgentResourceConfig {
    /// Create a new agent resource configuration.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the agent
    /// * `model` - Model identifier (e.g., "gpt-4")
    /// * `provider` - Provider name (e.g., "openai")
    ///
    /// # Example
    ///
    /// ```
    /// use dtx_process::AgentResourceConfig;
    /// use dtx_core::resource::ResourceId;
    ///
    /// let config = AgentResourceConfig::new(
    ///     ResourceId::new("code-assistant"),
    ///     "claude-3-opus",
    ///     "anthropic",
    /// );
    /// ```
    pub fn new(id: ResourceId, model: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            id,
            model: model.into(),
            provider: provider.into(),
            system_prompt: None,
            max_tokens: None,
            temperature: None,
            api_endpoint: None,
            extra_options: None,
        }
    }

    /// Set the system prompt.
    #[must_use]
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set the maximum tokens.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set the temperature.
    #[must_use]
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set a custom API endpoint.
    #[must_use]
    pub fn with_api_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.api_endpoint = Some(endpoint.into());
        self
    }
}

/// An AI agent resource (placeholder implementation).
///
/// This is a stub that returns "not implemented" errors for all operations.
/// Full implementation is planned for Phase 8.3.
///
/// # Future Capabilities
///
/// The full implementation will support:
/// - OpenAI API-compatible providers
/// - Anthropic Claude models
/// - Self-hosted models via Ollama
/// - MCP (Model Context Protocol) integration
/// - Tool use and function calling
pub struct AgentResource {
    config: AgentResourceConfig,
    state: ResourceState,
}

impl AgentResource {
    /// Create a new agent resource with the given configuration.
    pub fn new(config: AgentResourceConfig) -> Self {
        Self {
            config,
            state: ResourceState::Pending,
        }
    }

    /// Get the agent configuration.
    pub fn config(&self) -> &AgentResourceConfig {
        &self.config
    }

    fn not_implemented() -> ResourceError {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Agent backend not yet implemented (planned for Phase 8.3)",
        ))
    }
}

#[async_trait]
impl Resource for AgentResource {
    fn id(&self) -> &ResourceId {
        &self.config.id
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::Agent
    }

    fn state(&self) -> &ResourceState {
        &self.state
    }

    async fn start(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn stop(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn kill(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn restart(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Unknown
    }

    fn logs(&self) -> Option<Box<dyn LogStream>> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_resource_new() {
        let config = AgentResourceConfig::new(ResourceId::new("test-agent"), "gpt-4", "openai")
            .with_system_prompt("You are a helpful assistant.")
            .with_max_tokens(4096)
            .with_temperature(0.7);

        let resource = AgentResource::new(config);

        assert_eq!(resource.id().as_str(), "test-agent");
        assert_eq!(resource.kind(), ResourceKind::Agent);
        assert_eq!(resource.state(), &ResourceState::Pending);
        assert_eq!(resource.config().model, "gpt-4");
        assert_eq!(resource.config().provider, "openai");
        assert_eq!(
            resource.config().system_prompt,
            Some("You are a helpful assistant.".to_string())
        );
        assert_eq!(resource.config().max_tokens, Some(4096));
        assert_eq!(resource.config().temperature, Some(0.7));
    }

    #[tokio::test]
    async fn agent_resource_start_returns_error() {
        let config =
            AgentResourceConfig::new(ResourceId::new("test-agent"), "claude-3-opus", "anthropic");
        let mut resource = AgentResource::new(config);
        let ctx = Context::new();

        let result = resource.start(&ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[tokio::test]
    async fn agent_resource_stop_returns_error() {
        let config = AgentResourceConfig::new(ResourceId::new("test-agent"), "gpt-4", "openai");
        let mut resource = AgentResource::new(config);
        let ctx = Context::new();

        let result = resource.stop(&ctx).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn agent_resource_health_returns_unknown() {
        let config = AgentResourceConfig::new(ResourceId::new("test-agent"), "gpt-4", "openai");
        let resource = AgentResource::new(config);

        let health = resource.health().await;

        assert_eq!(health, HealthStatus::Unknown);
    }
}
