//! Agent runtime trait and types.
//!
//! This module defines the `AgentRuntime` trait that all AI runtimes must
//! implement, along with supporting types for responses and streaming.

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::config::{AgentConfig, ToolConfig};
use crate::error::Result;
use crate::message::{Message, ToolCall, ToolResult};

/// Agent runtime information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Agent identifier.
    pub id: String,
    /// Current state.
    pub state: AgentState,
    /// Model being used.
    pub model: String,
    /// Number of turns processed.
    pub turns: u64,
    /// Tokens used (input).
    pub input_tokens: u64,
    /// Tokens used (output).
    pub output_tokens: u64,
    /// Started at.
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Last activity.
    pub last_activity: Option<chrono::DateTime<chrono::Utc>>,
}

/// Agent state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Agent is idle, ready for requests.
    #[default]
    Idle,
    /// Agent is processing a request.
    Processing,
    /// Agent is waiting for tool execution.
    WaitingForTool,
    /// Agent is paused.
    Paused,
    /// Agent has stopped.
    Stopped,
    /// Agent encountered an error.
    Error(String),
}

/// Response from the agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Response content.
    pub content: String,
    /// Tool calls requested.
    pub tool_calls: Vec<ToolCall>,
    /// Stop reason.
    pub stop_reason: StopReason,
    /// Token usage.
    pub usage: TokenUsage,
    /// Latency in milliseconds.
    pub latency_ms: u64,
}

/// Stop reason for a turn.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// End of turn (natural completion).
    #[default]
    EndTurn,
    /// Tool use requested.
    ToolUse,
    /// Maximum tokens reached.
    MaxTokens,
    /// Stop sequence encountered.
    StopSequence,
    /// Error occurred.
    Error(String),
}

/// Token usage statistics.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Cached input tokens read.
    pub cache_read_tokens: u64,
    /// Cached input tokens written.
    pub cache_write_tokens: u64,
}

impl TokenUsage {
    /// Total tokens used.
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Streaming event from agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Content block started.
    ContentStart { index: usize },
    /// Text delta.
    TextDelta { index: usize, text: String },
    /// Tool use started.
    ToolStart {
        index: usize,
        id: String,
        name: String,
    },
    /// Tool input delta (partial JSON).
    ToolInputDelta { index: usize, delta: String },
    /// Content block completed.
    ContentEnd { index: usize },
    /// Message completed.
    MessageEnd { usage: TokenUsage },
    /// Error occurred.
    Error { error: String },
}

/// Capabilities supported by a runtime.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    /// Supports streaming responses.
    pub streaming: bool,
    /// Supports tool/function calling.
    pub tools: bool,
    /// Supports vision (image input).
    pub vision: bool,
    /// Supports system prompts.
    pub system_prompt: bool,
    /// Maximum context window size.
    pub max_context: Option<u32>,
}

/// Abstract agent runtime interface.
///
/// This trait must be implemented by all AI runtimes (Claude, OpenAI, Ollama, etc.)
/// to provide a consistent interface for agent operations.
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Runtime name (e.g., "claude", "openai", "ollama").
    fn name(&self) -> &str;

    /// Check if runtime is available and configured.
    async fn is_available(&self) -> bool;

    /// Initialize the runtime with configuration.
    async fn initialize(&mut self, config: &AgentConfig) -> Result<()>;

    /// Send a message and get a response.
    ///
    /// # Arguments
    ///
    /// * `messages` - Conversation history
    /// * `tools` - Available tools for this request
    ///
    /// # Returns
    ///
    /// Agent response including content, tool calls, and usage stats.
    async fn send(&self, messages: &[Message], tools: &[ToolConfig]) -> Result<AgentResponse>;

    /// Send a message and stream the response.
    ///
    /// Returns a stream of events that can be processed incrementally.
    async fn send_streaming(
        &self,
        messages: &[Message],
        tools: &[ToolConfig],
    ) -> Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>;

    /// Execute a tool call.
    ///
    /// This method is called by the runtime when it needs to execute
    /// a tool and return the result.
    async fn execute_tool(&self, tool_call: &ToolCall) -> Result<ToolResult>;

    /// Get agent info.
    fn info(&self) -> AgentInfo;

    /// Cancel ongoing operation.
    async fn cancel(&self) -> Result<()>;

    /// Check health status.
    async fn health(&self) -> Result<HealthStatus>;

    /// Estimate token count for text.
    ///
    /// This is an approximation since exact counting requires
    /// model-specific tokenizers.
    async fn count_tokens(&self, text: &str) -> Result<u64>;

    /// Get runtime capabilities.
    fn capabilities(&self) -> RuntimeCapabilities;

    /// Shutdown the runtime.
    async fn shutdown(&mut self) -> Result<()>;
}

/// Health status for runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Runtime is healthy and available.
    Healthy,
    /// Runtime is unhealthy.
    Unhealthy { reason: String },
    /// Health status unknown.
    Unknown,
}

impl HealthStatus {
    /// Check if healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }
}

/// Select the best available agent runtime based on configuration.
pub async fn detect_runtime(config: &AgentConfig) -> Option<Box<dyn AgentRuntime>> {
    use crate::config::AgentRuntimeType;

    match config.runtime {
        AgentRuntimeType::Auto => {
            // Try runtimes in order of preference
            #[cfg(feature = "claude")]
            {
                if let Ok(runtime) = crate::claude::ClaudeRuntime::new(config).await {
                    if runtime.is_available().await {
                        return Some(Box::new(runtime));
                    }
                }
            }

            #[cfg(feature = "openai")]
            {
                if let Ok(runtime) = crate::openai::OpenAIRuntime::new(config).await {
                    if runtime.is_available().await {
                        return Some(Box::new(runtime));
                    }
                }
            }

            #[cfg(feature = "ollama")]
            {
                if let Ok(runtime) = crate::ollama::OllamaRuntime::new(config).await {
                    if runtime.is_available().await {
                        return Some(Box::new(runtime));
                    }
                }
            }

            None
        }
        AgentRuntimeType::Claude => {
            #[cfg(feature = "claude")]
            {
                crate::claude::ClaudeRuntime::new(config)
                    .await
                    .ok()
                    .map(|r| Box::new(r) as _)
            }
            #[cfg(not(feature = "claude"))]
            None
        }
        AgentRuntimeType::OpenAI | AgentRuntimeType::OpenAICompatible => {
            #[cfg(feature = "openai")]
            {
                crate::openai::OpenAIRuntime::new(config)
                    .await
                    .ok()
                    .map(|r| Box::new(r) as _)
            }
            #[cfg(not(feature = "openai"))]
            None
        }
        AgentRuntimeType::Ollama => {
            #[cfg(feature = "ollama")]
            {
                crate::ollama::OllamaRuntime::new(config)
                    .await
                    .ok()
                    .map(|r| Box::new(r) as _)
            }
            #[cfg(not(feature = "ollama"))]
            None
        }
        AgentRuntimeType::LlamaCpp => {
            // llama.cpp runtime not yet implemented
            None
        }
        AgentRuntimeType::Mcp => {
            // MCP runtime not yet implemented
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_state_default() {
        let state = AgentState::default();
        assert_eq!(state, AgentState::Idle);
    }

    #[test]
    fn agent_state_serde() {
        let state = AgentState::Processing;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"processing\"");

        let error_state = AgentState::Error("test error".to_string());
        let json = serde_json::to_value(&error_state).unwrap();
        assert!(json.is_object());
    }

    #[test]
    fn stop_reason_default() {
        let reason = StopReason::default();
        assert_eq!(reason, StopReason::EndTurn);
    }

    #[test]
    fn token_usage_total() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 20,
            cache_write_tokens: 10,
        };
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn health_status_is_healthy() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Unhealthy {
            reason: "test".to_string()
        }
        .is_healthy());
        assert!(!HealthStatus::Unknown.is_healthy());
    }

    #[test]
    fn stream_event_serde() {
        let event = StreamEvent::TextDelta {
            index: 0,
            text: "Hello".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "text_delta");
        assert_eq!(json["text"], "Hello");
    }

    #[test]
    fn runtime_capabilities_default() {
        let caps = RuntimeCapabilities::default();
        assert!(!caps.streaming);
        assert!(!caps.tools);
        assert!(!caps.vision);
    }

    #[test]
    fn agent_response_serde() {
        let response = AgentResponse {
            content: "Hello!".to_string(),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            latency_ms: 100,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: AgentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "Hello!");
    }

    #[test]
    fn agent_info_serde() {
        let info = AgentInfo {
            id: "test".to_string(),
            state: AgentState::Idle,
            model: "test-model".to_string(),
            turns: 5,
            input_tokens: 1000,
            output_tokens: 500,
            started_at: Some(chrono::Utc::now()),
            last_activity: Some(chrono::Utc::now()),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: AgentInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test");
        assert_eq!(parsed.turns, 5);
    }
}
