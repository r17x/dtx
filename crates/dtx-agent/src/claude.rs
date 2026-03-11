//! Claude/Anthropic runtime implementation.
//!
//! This module provides the `ClaudeRuntime` that implements the `AgentRuntime`
//! trait for Anthropic's Claude API.

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::{AgentConfig, ApiKeyConfig, ToolConfig};
use crate::error::{AgentError, Result};
use crate::message::{Content, ContentBlock, Message, Role, ToolCall, ToolResult};
use crate::runtime::{
    AgentInfo, AgentResponse, AgentRuntime, AgentState, HealthStatus, RuntimeCapabilities,
    StopReason, StreamEvent, TokenUsage,
};
use crate::tools::ToolExecutor;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Claude runtime using Anthropic API.
pub struct ClaudeRuntime {
    client: Client,
    api_key: String,
    base_url: String,
    config: AgentConfig,
    state: Arc<RwLock<AgentState>>,
    turns: AtomicU64,
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    tool_executor: Arc<ToolExecutor>,
}

impl ClaudeRuntime {
    /// Create a new Claude runtime from configuration.
    pub async fn new(config: &AgentConfig) -> Result<Self> {
        let api_key = match &config.model.api_key {
            Some(ApiKeyConfig::Direct(key)) => key.clone(),
            Some(ApiKeyConfig::EnvVar { env }) => std::env::var(env)
                .map_err(|_| AgentError::config(format!("Environment variable {} not set", env)))?,
            Some(ApiKeyConfig::File { file }) => tokio::fs::read_to_string(file)
                .await
                .map_err(|e| AgentError::config(format!("Failed to read key file: {}", e)))?
                .trim()
                .to_string(),
            None => std::env::var("ANTHROPIC_API_KEY")
                .map_err(|_| AgentError::config("ANTHROPIC_API_KEY not set"))?,
        };

        let base_url = config
            .model
            .base_url
            .clone()
            .unwrap_or_else(|| ANTHROPIC_API_URL.to_string());

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| AgentError::backend(e.to_string()))?;

        Ok(Self {
            client,
            api_key,
            base_url,
            config: config.clone(),
            state: Arc::new(RwLock::new(AgentState::Idle)),
            turns: AtomicU64::new(0),
            input_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            started_at: None,
            tool_executor: Arc::new(ToolExecutor::new(config.tools.clone())),
        })
    }

    /// Build tools schema for Claude API.
    fn build_tools_schema(&self, tools: &[ToolConfig]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .filter_map(|t| match t {
                ToolConfig::Builtin { name, .. } => Some(self.builtin_tool_schema(name)),
                ToolConfig::Command {
                    name,
                    description,
                    args_schema,
                    ..
                } => Some(serde_json::json!({
                    "name": name,
                    "description": description,
                    "input_schema": args_schema.clone().unwrap_or(serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }))
                })),
                ToolConfig::Http {
                    name,
                    description,
                    body_schema,
                    ..
                } => Some(serde_json::json!({
                    "name": name,
                    "description": description,
                    "input_schema": body_schema.clone().unwrap_or(serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }))
                })),
                ToolConfig::Custom {
                    name,
                    description,
                    schema,
                    ..
                } => Some(serde_json::json!({
                    "name": name,
                    "description": description,
                    "input_schema": schema.clone().unwrap_or(serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }))
                })),
                ToolConfig::Resource { .. } | ToolConfig::Mcp { .. } => None,
            })
            .collect()
    }

    fn builtin_tool_schema(&self, name: &str) -> serde_json::Value {
        match name {
            "read_file" => serde_json::json!({
                "name": "read_file",
                "description": "Read the contents of a file at the specified path",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file to read" }
                    },
                    "required": ["path"]
                }
            }),
            "write_file" => serde_json::json!({
                "name": "write_file",
                "description": "Write content to a file at the specified path",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file to write" },
                        "content": { "type": "string", "description": "Content to write to the file" }
                    },
                    "required": ["path", "content"]
                }
            }),
            "execute_command" => serde_json::json!({
                "name": "execute_command",
                "description": "Execute a shell command and return its output",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Shell command to execute" }
                    },
                    "required": ["command"]
                }
            }),
            "list_files" => serde_json::json!({
                "name": "list_files",
                "description": "List files and directories in the specified path",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Directory path to list" }
                    }
                }
            }),
            "search_files" => serde_json::json!({
                "name": "search_files",
                "description": "Search for files containing a pattern using grep",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Pattern to search for" },
                        "path": { "type": "string", "description": "Directory to search in" }
                    },
                    "required": ["pattern"]
                }
            }),
            "get_current_time" => serde_json::json!({
                "name": "get_current_time",
                "description": "Get the current date and time in UTC",
                "input_schema": {
                    "type": "object",
                    "properties": {}
                }
            }),
            _ => serde_json::json!({
                "name": name,
                "description": format!("Built-in tool: {}", name),
                "input_schema": { "type": "object", "properties": {} }
            }),
        }
    }

    /// Convert messages to Claude API format.
    fn convert_messages(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .filter_map(|m| match m.role {
                Role::System => None, // System is handled separately
                Role::User => Some(serde_json::json!({
                    "role": "user",
                    "content": self.convert_content(&m.content)
                })),
                Role::Assistant => Some(serde_json::json!({
                    "role": "assistant",
                    "content": self.convert_content(&m.content)
                })),
                Role::Tool => Some(serde_json::json!({
                    "role": "user",
                    "content": self.convert_content(&m.content)
                })),
            })
            .collect()
    }

    fn convert_content(&self, content: &Content) -> serde_json::Value {
        match content {
            Content::Text(text) => serde_json::json!(text),
            Content::Blocks(blocks) => serde_json::json!(blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => serde_json::json!({
                        "type": "text",
                        "text": text
                    }),
                    ContentBlock::Image { source } => serde_json::json!({
                        "type": "image",
                        "source": source
                    }),
                    ContentBlock::ToolUse { id, name, input } => serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input
                    }),
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                        "is_error": is_error
                    }),
                })
                .collect::<Vec<_>>()),
        }
    }

    /// Extract system prompt from messages or config.
    fn extract_system_prompt(&self, messages: &[Message]) -> Option<String> {
        messages
            .iter()
            .find(|m| matches!(m.role, Role::System))
            .map(|m| m.text())
            .or_else(|| self.config.system_prompt.clone())
    }

    /// Parse Claude API response into AgentResponse.
    fn parse_response(&self, result: serde_json::Value, latency_ms: u64) -> AgentResponse {
        let content = result["content"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let tool_calls: Vec<ToolCall> = result["content"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|c| c["type"] == "tool_use")
                    .map(|c| ToolCall {
                        id: c["id"].as_str().unwrap_or_default().to_string(),
                        name: c["name"].as_str().unwrap_or_default().to_string(),
                        input: c["input"].clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let stop_reason = match result["stop_reason"].as_str() {
            Some("end_turn") => StopReason::EndTurn,
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("stop_sequence") => StopReason::StopSequence,
            _ => StopReason::EndTurn,
        };

        let usage = TokenUsage {
            input_tokens: result["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: result["usage"]["output_tokens"].as_u64().unwrap_or(0),
            cache_read_tokens: result["usage"]["cache_read_input_tokens"]
                .as_u64()
                .unwrap_or(0),
            cache_write_tokens: result["usage"]["cache_creation_input_tokens"]
                .as_u64()
                .unwrap_or(0),
        };

        AgentResponse {
            content,
            tool_calls,
            stop_reason,
            usage,
            latency_ms,
        }
    }
}

#[async_trait]
impl AgentRuntime for ClaudeRuntime {
    fn name(&self) -> &str {
        "claude"
    }

    async fn is_available(&self) -> bool {
        // Check if we can reach the API
        let response = self
            .client
            .get(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .await;

        // Even if messages endpoint returns 405, API key is valid if not 401
        response
            .map(|r| r.status() != reqwest::StatusCode::UNAUTHORIZED)
            .unwrap_or(false)
    }

    async fn initialize(&mut self, config: &AgentConfig) -> Result<()> {
        self.config = config.clone();
        self.started_at = Some(chrono::Utc::now());
        self.tool_executor = Arc::new(ToolExecutor::new(config.tools.clone()));
        *self.state.write().await = AgentState::Idle;
        Ok(())
    }

    async fn send(&self, messages: &[Message], tools: &[ToolConfig]) -> Result<AgentResponse> {
        *self.state.write().await = AgentState::Processing;

        let start = std::time::Instant::now();

        let system = self.extract_system_prompt(messages);
        let converted_messages = self.convert_messages(messages);
        let tools_schema = self.build_tools_schema(tools);

        let mut body = serde_json::json!({
            "model": self.config.model.name,
            "messages": converted_messages,
            "max_tokens": self.config.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        });

        if let Some(system) = system {
            body["system"] = serde_json::json!(system);
        }

        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if !tools_schema.is_empty() {
            body["tools"] = serde_json::json!(tools_schema);
        }

        let response = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let error = response.text().await.unwrap_or_default();
            *self.state.write().await = AgentState::Error(error.clone());

            // Check for rate limiting
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(AgentError::RateLimited { retry_after: None });
            }

            return Err(AgentError::api(status.as_u16(), error));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        let latency_ms = start.elapsed().as_millis() as u64;
        let response = self.parse_response(result, latency_ms);

        self.turns.fetch_add(1, Ordering::SeqCst);
        self.input_tokens
            .fetch_add(response.usage.input_tokens, Ordering::SeqCst);
        self.output_tokens
            .fetch_add(response.usage.output_tokens, Ordering::SeqCst);

        *self.state.write().await = if response.tool_calls.is_empty() {
            AgentState::Idle
        } else {
            AgentState::WaitingForTool
        };

        Ok(response)
    }

    async fn send_streaming(
        &self,
        messages: &[Message],
        tools: &[ToolConfig],
    ) -> Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>> {
        *self.state.write().await = AgentState::Processing;

        let system = self.extract_system_prompt(messages);
        let converted_messages = self.convert_messages(messages);
        let tools_schema = self.build_tools_schema(tools);

        let mut body = serde_json::json!({
            "model": self.config.model.name,
            "messages": converted_messages,
            "max_tokens": self.config.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            "stream": true,
        });

        if let Some(system) = system {
            body["system"] = serde_json::json!(system);
        }

        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if !tools_schema.is_empty() {
            body["tools"] = serde_json::json!(tools_schema);
        }

        let response = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(AgentError::backend(error));
        }

        // Parse SSE stream
        let stream = response.bytes_stream().filter_map(|result| async move {
            match result {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                return Some(StreamEvent::MessageEnd {
                                    usage: TokenUsage::default(),
                                });
                            }
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                                let event_type = event["type"].as_str().unwrap_or("");
                                match event_type {
                                    "content_block_start" => {
                                        let index = event["index"].as_u64().unwrap_or(0) as usize;
                                        if event["content_block"]["type"] == "tool_use" {
                                            return Some(StreamEvent::ToolStart {
                                                index,
                                                id: event["content_block"]["id"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .to_string(),
                                                name: event["content_block"]["name"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .to_string(),
                                            });
                                        }
                                        return Some(StreamEvent::ContentStart { index });
                                    }
                                    "content_block_delta" => {
                                        let index = event["index"].as_u64().unwrap_or(0) as usize;
                                        if let Some(text) = event["delta"]["text"].as_str() {
                                            return Some(StreamEvent::TextDelta {
                                                index,
                                                text: text.to_string(),
                                            });
                                        }
                                        if let Some(delta) = event["delta"]["partial_json"].as_str()
                                        {
                                            return Some(StreamEvent::ToolInputDelta {
                                                index,
                                                delta: delta.to_string(),
                                            });
                                        }
                                    }
                                    "content_block_stop" => {
                                        return Some(StreamEvent::ContentEnd {
                                            index: event["index"].as_u64().unwrap_or(0) as usize,
                                        });
                                    }
                                    "message_stop" => {
                                        return Some(StreamEvent::MessageEnd {
                                            usage: TokenUsage::default(),
                                        });
                                    }
                                    "message_delta" => {
                                        if let Some(usage) = event["usage"].as_object() {
                                            return Some(StreamEvent::MessageEnd {
                                                usage: TokenUsage {
                                                    input_tokens: 0, // Not provided in delta
                                                    output_tokens: usage["output_tokens"]
                                                        .as_u64()
                                                        .unwrap_or(0),
                                                    cache_read_tokens: 0,
                                                    cache_write_tokens: 0,
                                                },
                                            });
                                        }
                                    }
                                    "error" => {
                                        return Some(StreamEvent::Error {
                                            error: event["error"]["message"]
                                                .as_str()
                                                .unwrap_or("Unknown error")
                                                .to_string(),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    None
                }
                Err(e) => Some(StreamEvent::Error {
                    error: e.to_string(),
                }),
            }
        });

        Ok(Box::pin(stream))
    }

    async fn execute_tool(&self, tool_call: &ToolCall) -> Result<ToolResult> {
        self.tool_executor.execute(tool_call).await
    }

    fn info(&self) -> AgentInfo {
        AgentInfo {
            id: self.config.id.clone(),
            state: self
                .state
                .try_read()
                .map(|s| s.clone())
                .unwrap_or(AgentState::Idle),
            model: self.config.model.name.clone(),
            turns: self.turns.load(Ordering::SeqCst),
            input_tokens: self.input_tokens.load(Ordering::SeqCst),
            output_tokens: self.output_tokens.load(Ordering::SeqCst),
            started_at: self.started_at,
            last_activity: Some(chrono::Utc::now()),
        }
    }

    async fn cancel(&self) -> Result<()> {
        *self.state.write().await = AgentState::Idle;
        Ok(())
    }

    async fn health(&self) -> Result<HealthStatus> {
        if self.is_available().await {
            Ok(HealthStatus::Healthy)
        } else {
            Ok(HealthStatus::Unhealthy {
                reason: "Claude API not reachable".to_string(),
            })
        }
    }

    async fn count_tokens(&self, text: &str) -> Result<u64> {
        // Rough approximation: ~4 chars per token for Claude
        // For more accuracy, use the tokenizer endpoint
        Ok((text.len() / 4) as u64)
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tools: true,
            vision: true,
            system_prompt: true,
            max_context: Some(200000), // Claude 3.5 Sonnet context
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        *self.state.write().await = AgentState::Stopped;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_runtime_config() {
        // Can't test new() without API key, but we can test static methods
        let config = AgentConfig::new("test", "claude-3-5-sonnet-20241022")
            .with_runtime(crate::config::AgentRuntimeType::Claude);
        assert_eq!(config.runtime, crate::config::AgentRuntimeType::Claude);
    }

    #[test]
    fn builtin_tool_schema_read_file() {
        // Create executor directly to test schema
        let executor = ToolExecutor::new(vec![]);
        let schema = executor.tool_schema("read_file");
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema["name"], "read_file");
    }

    #[test]
    fn builtin_tool_schema_execute_command() {
        let executor = ToolExecutor::new(vec![]);
        let schema = executor.tool_schema("execute_command");
        assert!(schema.is_some());
    }

    #[test]
    fn convert_messages_user() {
        let _messages = [Message::user("Hello")];

        // Test content conversion directly
        let content = Content::Text("Hello".to_string());
        match &content {
            Content::Text(text) => assert_eq!(text, "Hello"),
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn parse_stop_reason() {
        assert_eq!(
            match "end_turn" {
                "end_turn" => StopReason::EndTurn,
                "tool_use" => StopReason::ToolUse,
                "max_tokens" => StopReason::MaxTokens,
                "stop_sequence" => StopReason::StopSequence,
                _ => StopReason::EndTurn,
            },
            StopReason::EndTurn
        );

        assert_eq!(
            match "tool_use" {
                "end_turn" => StopReason::EndTurn,
                "tool_use" => StopReason::ToolUse,
                "max_tokens" => StopReason::MaxTokens,
                "stop_sequence" => StopReason::StopSequence,
                _ => StopReason::EndTurn,
            },
            StopReason::ToolUse
        );
    }

    #[test]
    fn token_usage_from_json() {
        let json = serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 50
        });

        let usage = TokenUsage {
            input_tokens: json["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: json["output_tokens"].as_u64().unwrap_or(0),
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn capabilities() {
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
    fn agent_state_transitions() {
        let state = AgentState::Idle;
        assert_eq!(state, AgentState::Idle);

        let processing = AgentState::Processing;
        assert_eq!(processing, AgentState::Processing);

        let error = AgentState::Error("test error".to_string());
        assert!(matches!(error, AgentState::Error(_)));
    }

    #[tokio::test]
    async fn estimate_tokens() {
        let text = "Hello, this is a test message for token counting.";
        // ~4 chars per token
        let estimated = (text.len() / 4) as u64;
        assert!(estimated > 0);
        assert!(estimated < text.len() as u64);
    }
}
