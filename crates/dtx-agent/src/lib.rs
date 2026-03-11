//! AI Agent backend for dtx.
//!
//! This crate provides AI agent support for the dtx orchestration system.
//! It enables running AI agents as managed resources alongside processes,
//! containers, and other dtx-managed entities.
//!
//! # Features
//!
//! - Multiple runtime backends (Claude, OpenAI, Ollama)
//! - Tool/function calling support
//! - Streaming responses
//! - Conversation history management
//! - Multiple execution modes (daemon, one-shot, worker, cron)
//!
//! # Runtime Support
//!
//! | Runtime | Status | Features |
//! |---------|--------|----------|
//! | Claude (Anthropic) | Full | Streaming, Tools, Vision |
//! | OpenAI | Full | Streaming, Tools, Vision |
//! | Ollama (Local) | Full | Streaming, Tools*, Vision* |
//! | llama.cpp | Planned | - |
//! | MCP | Planned | - |
//!
//! *Tool and vision support depends on the specific model
//!
//! # Example
//!
//! ```ignore
//! use dtx_agent::{AgentResource, AgentConfig};
//! use dtx_core::events::ResourceEventBus;
//! use dtx_core::resource::{Resource, Context};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = AgentConfig::new("assistant", "claude-3-5-sonnet-20241022")
//!         .with_system_prompt("You are a helpful assistant.")
//!         .with_api_key_env("ANTHROPIC_API_KEY");
//!
//!     let event_bus = Arc::new(ResourceEventBus::new());
//!     let mut agent = AgentResource::new(config, event_bus);
//!
//!     // Start the agent
//!     agent.start(&Context::new()).await?;
//!
//!     // Send a message
//!     let response = agent.send("Hello!").await?;
//!     println!("{}", response.content);
//!
//!     // Stop the agent
//!     agent.stop(&Context::new()).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Configuration
//!
//! Agents are configured through `AgentConfig`:
//!
//! ```ignore
//! use dtx_agent::{AgentConfig, AgentRuntimeType, AgentMode, ToolConfig};
//!
//! let config = AgentConfig::new("code-assistant", "claude-3-5-sonnet-20241022")
//!     .with_runtime(AgentRuntimeType::Claude)
//!     .with_system_prompt("You are a code assistant.")
//!     .with_max_tokens(8192)
//!     .with_temperature(0.7)
//!     .with_tool(ToolConfig::Builtin {
//!         name: "read_file".to_string(),
//!         config: serde_json::json!({}),
//!     });
//! ```
//!
//! # Tools
//!
//! Built-in tools available:
//!
//! - `read_file` - Read file contents
//! - `write_file` - Write content to file
//! - `execute_command` - Run shell commands
//! - `list_files` - List directory contents
//! - `search_files` - Search for files matching a pattern
//! - `get_current_time` - Get current UTC time
//!
//! Custom tools can be defined as commands, HTTP endpoints, or custom handlers.
//!
//! # Feature Flags
//!
//! - `claude` (default) - Enable Claude/Anthropic runtime
//! - `ollama` (default) - Enable Ollama runtime
//! - `openai` - Enable OpenAI runtime
//! - `llamacpp` - Enable llama.cpp runtime (planned)
//! - `mcp` - Enable MCP integration (planned)

pub mod config;
pub mod error;
pub mod message;
pub mod resource;
pub mod runtime;
pub mod tools;

#[cfg(feature = "claude")]
pub mod claude;

#[cfg(feature = "ollama")]
pub mod ollama;

#[cfg(feature = "openai")]
pub mod openai;

// Re-exports for convenience
pub use config::{
    AgentConfig, AgentMode, AgentRuntimeType, ApiKeyConfig, ContextStrategy, McpConnection,
    McpServerConfig, MemoryConfig, ModelConfig, RateLimitConfig, ToolConfig,
};
pub use error::{AgentError, Result};
pub use message::{Content, ContentBlock, ImageSource, Message, Role, ToolCall, ToolResult};
pub use resource::AgentResource;
pub use runtime::{
    detect_runtime, AgentInfo, AgentResponse, AgentRuntime, AgentState, HealthStatus,
    RuntimeCapabilities, StopReason, StreamEvent, TokenUsage,
};
pub use tools::{BuiltinTool, ToolExecutor};

#[cfg(feature = "claude")]
pub use claude::ClaudeRuntime;

#[cfg(feature = "ollama")]
pub use ollama::OllamaRuntime;

#[cfg(feature = "openai")]
pub use openai::OpenAIRuntime;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_compiles() {
        // Verify all public types are accessible
        let _: AgentConfig = AgentConfig::default();
        let _: AgentMode = AgentMode::default();
        let _: AgentRuntimeType = AgentRuntimeType::default();
        let _: AgentState = AgentState::default();
        let _: StopReason = StopReason::default();
        let _: TokenUsage = TokenUsage::default();
    }

    #[test]
    fn config_builder() {
        let config = AgentConfig::new("test", "test-model")
            .with_runtime(AgentRuntimeType::Claude)
            .with_system_prompt("Test prompt")
            .with_max_tokens(4096)
            .with_temperature(0.7);

        assert_eq!(config.id, "test");
        assert_eq!(config.model.name, "test-model");
        assert_eq!(config.runtime, AgentRuntimeType::Claude);
        assert_eq!(config.system_prompt, Some("Test prompt".to_string()));
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.7));
    }

    #[test]
    fn message_types() {
        let user = Message::user("Hello");
        assert!(user.is_user());

        let assistant = Message::assistant("Hi!");
        assert!(assistant.is_assistant());

        let system = Message::system("You are helpful.");
        assert!(system.is_system());

        let tool = Message::tool_result("id", "result", false);
        assert!(tool.is_tool());
    }

    #[test]
    fn tool_call_new() {
        let tc = ToolCall::new("id-1", "test_tool", serde_json::json!({"arg": "value"}));
        assert_eq!(tc.id, "id-1");
        assert_eq!(tc.name, "test_tool");
    }

    #[test]
    fn tool_result_types() {
        let success = ToolResult::success("id", "Success!");
        assert!(!success.is_error);

        let error = ToolResult::error("id", "Failed!");
        assert!(error.is_error);
    }

    #[test]
    fn runtime_type_variants() {
        assert_eq!(AgentRuntimeType::Auto, AgentRuntimeType::default());

        let variants = [
            AgentRuntimeType::Auto,
            AgentRuntimeType::Claude,
            AgentRuntimeType::OpenAI,
            AgentRuntimeType::Ollama,
            AgentRuntimeType::LlamaCpp,
            AgentRuntimeType::OpenAICompatible,
            AgentRuntimeType::Mcp,
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: AgentRuntimeType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn agent_mode_variants() {
        let modes = [
            AgentMode::OneShot,
            AgentMode::Daemon,
            AgentMode::Worker {
                queue: "test".to_string(),
                concurrency: 1,
            },
            AgentMode::Cron {
                schedule: "0 * * * *".to_string(),
                task: "run".to_string(),
            },
            AgentMode::Interactive,
        ];

        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let parsed: AgentMode = serde_json::from_str(&json).unwrap();
            // Can't use assert_eq because Worker/Cron have fields
            let _ = parsed;
        }
    }

    #[test]
    fn error_types() {
        let err = AgentError::config("test error");
        assert!(err.to_string().contains("test error"));

        let err = AgentError::backend("backend failed");
        assert!(err.to_string().contains("backend failed"));

        let err = AgentError::api(401, "Unauthorized");
        assert!(err.to_string().contains("401"));

        let err = AgentError::timeout(std::time::Duration::from_secs(30));
        assert!(err.to_string().contains("30"));
    }

    #[test]
    fn token_usage() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 20,
            cache_write_tokens: 10,
        };

        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn runtime_capabilities() {
        let caps = RuntimeCapabilities {
            streaming: true,
            tools: true,
            vision: true,
            system_prompt: true,
            max_context: Some(200000),
        };

        assert!(caps.streaming);
        assert!(caps.tools);
        assert!(caps.vision);
        assert_eq!(caps.max_context, Some(200000));
    }

    #[test]
    fn health_status() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Unhealthy {
            reason: "test".to_string()
        }
        .is_healthy());
        assert!(!HealthStatus::Unknown.is_healthy());
    }
}
