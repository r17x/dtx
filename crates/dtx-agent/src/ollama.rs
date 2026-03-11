//! Ollama runtime implementation.
//!
//! This module provides the `OllamaRuntime` that implements the `AgentRuntime`
//! trait for local Ollama servers.

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::{AgentConfig, ToolConfig};
use crate::error::{AgentError, Result};
use crate::message::{Content, ContentBlock, Message, Role, ToolCall, ToolResult};
use crate::runtime::{
    AgentInfo, AgentResponse, AgentRuntime, AgentState, HealthStatus, RuntimeCapabilities,
    StopReason, StreamEvent, TokenUsage,
};
use crate::tools::ToolExecutor;

const OLLAMA_DEFAULT_URL: &str = "http://localhost:11434";

/// Ollama runtime for local models.
pub struct OllamaRuntime {
    client: Client,
    base_url: String,
    config: AgentConfig,
    state: Arc<RwLock<AgentState>>,
    turns: AtomicU64,
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    tool_executor: Arc<ToolExecutor>,
}

impl OllamaRuntime {
    /// Create a new Ollama runtime from configuration.
    pub async fn new(config: &AgentConfig) -> Result<Self> {
        let base_url = config
            .model
            .base_url
            .clone()
            .unwrap_or_else(|| OLLAMA_DEFAULT_URL.to_string());

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| AgentError::backend(e.to_string()))?;

        Ok(Self {
            client,
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

    /// Convert messages to Ollama API format.
    fn convert_messages(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                let content = match &m.content {
                    Content::Text(text) => text.clone(),
                    Content::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.clone()),
                            ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                serde_json::json!({
                    "role": role,
                    "content": content
                })
            })
            .collect()
    }

    /// Check if a model is available locally.
    async fn model_available(&self, model: &str) -> bool {
        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    json["models"]
                        .as_array()
                        .map(|arr| arr.iter().any(|m| m["name"].as_str() == Some(model)))
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Pull a model if not available.
    async fn pull_model(&self, model: &str) -> Result<()> {
        tracing::info!(model = %model, "Pulling Ollama model");

        let response = self
            .client
            .post(format!("{}/api/pull", self.base_url))
            .json(&serde_json::json!({ "name": model }))
            .send()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(AgentError::backend(format!(
                "Failed to pull model: {}",
                error
            )));
        }

        // Stream the pull progress
        let mut stream = response.bytes_stream();
        while let Some(result) = stream.next().await {
            if let Ok(bytes) = result {
                let text = String::from_utf8_lossy(&bytes);
                for line in text.lines() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(status) = json["status"].as_str() {
                            tracing::debug!(status = %status, "Pull progress");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Build tools schema for Ollama (if supported by model).
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
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": args_schema.clone().unwrap_or(serde_json::json!({
                            "type": "object",
                            "properties": {}
                        }))
                    }
                })),
                ToolConfig::Http {
                    name,
                    description,
                    body_schema,
                    ..
                } => Some(serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": body_schema.clone().unwrap_or(serde_json::json!({
                            "type": "object",
                            "properties": {}
                        }))
                    }
                })),
                _ => None,
            })
            .collect()
    }

    fn builtin_tool_schema(&self, name: &str) -> serde_json::Value {
        match name {
            "read_file" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read the contents of a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to the file" }
                        },
                        "required": ["path"]
                    }
                }
            }),
            "write_file" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_file",
                    "description": "Write content to a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to the file" },
                            "content": { "type": "string", "description": "Content to write" }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            "execute_command" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "description": "Execute a shell command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "description": "Command to execute" }
                        },
                        "required": ["command"]
                    }
                }
            }),
            _ => serde_json::json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": format!("Built-in tool: {}", name),
                    "parameters": { "type": "object", "properties": {} }
                }
            }),
        }
    }
}

#[async_trait]
impl AgentRuntime for OllamaRuntime {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn initialize(&mut self, config: &AgentConfig) -> Result<()> {
        self.config = config.clone();
        self.started_at = Some(chrono::Utc::now());
        self.tool_executor = Arc::new(ToolExecutor::new(config.tools.clone()));
        *self.state.write().await = AgentState::Idle;

        // Check if model is available, pull if not
        if !self.model_available(&config.model.name).await {
            self.pull_model(&config.model.name).await?;
        }

        Ok(())
    }

    async fn send(&self, messages: &[Message], tools: &[ToolConfig]) -> Result<AgentResponse> {
        *self.state.write().await = AgentState::Processing;

        let start = std::time::Instant::now();
        let converted_messages = self.convert_messages(messages);
        let tools_schema = self.build_tools_schema(tools);

        let mut body = serde_json::json!({
            "model": self.config.model.name,
            "messages": converted_messages,
            "stream": false,
            "options": {
                "temperature": self.config.temperature.unwrap_or(0.7),
                "num_predict": self.config.max_tokens.unwrap_or(4096) as i64,
            }
        });

        // Only include tools if the model supports them
        if !tools_schema.is_empty() {
            body["tools"] = serde_json::json!(tools_schema);
        }

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            *self.state.write().await = AgentState::Error(error.clone());
            return Err(AgentError::backend(error));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        let content = result["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        // Parse tool calls from response (Ollama tool support)
        let tool_calls: Vec<ToolCall> = result["message"]["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let id = uuid::Uuid::new_v4().to_string();
                        let name = tc["function"]["name"].as_str()?;
                        let args = tc["function"]["arguments"].clone();
                        Some(ToolCall {
                            id,
                            name: name.to_string(),
                            input: args,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let stop_reason = if !tool_calls.is_empty() {
            StopReason::ToolUse
        } else if result["done_reason"].as_str() == Some("length") {
            StopReason::MaxTokens
        } else {
            StopReason::EndTurn
        };

        let usage = TokenUsage {
            input_tokens: result["prompt_eval_count"].as_u64().unwrap_or(0),
            output_tokens: result["eval_count"].as_u64().unwrap_or(0),
            ..Default::default()
        };

        self.turns.fetch_add(1, Ordering::SeqCst);
        self.input_tokens
            .fetch_add(usage.input_tokens, Ordering::SeqCst);
        self.output_tokens
            .fetch_add(usage.output_tokens, Ordering::SeqCst);

        *self.state.write().await = if tool_calls.is_empty() {
            AgentState::Idle
        } else {
            AgentState::WaitingForTool
        };

        Ok(AgentResponse {
            content,
            tool_calls,
            stop_reason,
            usage,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn send_streaming(
        &self,
        messages: &[Message],
        _tools: &[ToolConfig],
    ) -> Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>> {
        *self.state.write().await = AgentState::Processing;

        let converted_messages = self.convert_messages(messages);

        let body = serde_json::json!({
            "model": self.config.model.name,
            "messages": converted_messages,
            "stream": true,
            "options": {
                "temperature": self.config.temperature.unwrap_or(0.7),
                "num_predict": self.config.max_tokens.unwrap_or(4096) as i64,
            }
        });

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(AgentError::backend(error));
        }

        let stream = response.bytes_stream().filter_map(|result| async move {
            match result {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines() {
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                            if let Some(content) = event["message"]["content"].as_str() {
                                if !content.is_empty() {
                                    return Some(StreamEvent::TextDelta {
                                        index: 0,
                                        text: content.to_string(),
                                    });
                                }
                            }
                            if event["done"].as_bool() == Some(true) {
                                return Some(StreamEvent::MessageEnd {
                                    usage: TokenUsage {
                                        input_tokens: event["prompt_eval_count"]
                                            .as_u64()
                                            .unwrap_or(0),
                                        output_tokens: event["eval_count"].as_u64().unwrap_or(0),
                                        ..Default::default()
                                    },
                                });
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
                reason: "Ollama server not reachable".to_string(),
            })
        }
    }

    async fn count_tokens(&self, text: &str) -> Result<u64> {
        // Rough approximation: ~4 chars per token
        Ok((text.len() / 4) as u64)
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tools: true,  // Ollama 0.1.15+ supports tools for some models
            vision: true, // LLaVA and similar models support vision
            system_prompt: true,
            max_context: Some(8192), // Varies by model
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

    fn make_config() -> AgentConfig {
        AgentConfig::new("test", "llama3.2").with_runtime(crate::config::AgentRuntimeType::Ollama)
    }

    #[test]
    fn ollama_runtime_name() {
        let config = make_config();
        assert_eq!(config.runtime, crate::config::AgentRuntimeType::Ollama);
    }

    #[test]
    fn convert_messages() {
        let messages = [
            Message::system("You are helpful."),
            Message::user("Hello"),
            Message::assistant("Hi there!"),
        ];

        // Test message content extraction
        assert_eq!(messages[0].text(), "You are helpful.");
        assert_eq!(messages[1].text(), "Hello");
        assert_eq!(messages[2].text(), "Hi there!");
    }

    #[test]
    fn convert_message_roles() {
        let system = Message::system("test");
        let user = Message::user("test");
        let assistant = Message::assistant("test");

        assert_eq!(system.role, Role::System);
        assert_eq!(user.role, Role::User);
        assert_eq!(assistant.role, Role::Assistant);
    }

    #[test]
    fn capabilities() {
        let caps = RuntimeCapabilities {
            streaming: true,
            tools: true,
            vision: true,
            system_prompt: true,
            max_context: Some(8192),
        };

        assert!(caps.streaming);
        assert!(caps.tools);
        assert_eq!(caps.max_context, Some(8192));
    }

    #[test]
    fn token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total(), 0);
    }

    #[test]
    fn builtin_tool_schema() {
        let schema = serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read the contents of a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file" }
                    },
                    "required": ["path"]
                }
            }
        });

        assert_eq!(schema["function"]["name"], "read_file");
        assert!(schema["function"]["parameters"]["properties"]["path"].is_object());
    }

    #[test]
    fn agent_state_serde() {
        let idle = AgentState::Idle;
        let json = serde_json::to_string(&idle).unwrap();
        assert_eq!(json, "\"idle\"");

        let processing = AgentState::Processing;
        let json = serde_json::to_string(&processing).unwrap();
        assert_eq!(json, "\"processing\"");
    }

    #[test]
    fn stop_reason_from_done_reason() {
        let reasons = ["length", "stop", ""];
        let expected = [
            StopReason::MaxTokens,
            StopReason::EndTurn,
            StopReason::EndTurn,
        ];

        for (reason, expected) in reasons.iter().zip(expected.iter()) {
            let result = if *reason == "length" {
                StopReason::MaxTokens
            } else {
                StopReason::EndTurn
            };
            assert_eq!(&result, expected);
        }
    }

    #[test]
    fn agent_info_creation() {
        let info = AgentInfo {
            id: "test".to_string(),
            state: AgentState::Idle,
            model: "llama3.2".to_string(),
            turns: 5,
            input_tokens: 1000,
            output_tokens: 500,
            started_at: Some(chrono::Utc::now()),
            last_activity: Some(chrono::Utc::now()),
        };

        assert_eq!(info.id, "test");
        assert_eq!(info.model, "llama3.2");
        assert_eq!(info.turns, 5);
    }

    #[tokio::test]
    async fn estimate_tokens() {
        let text = "Hello, this is a test message for token counting.";
        let estimated = (text.len() / 4) as u64;
        assert!(estimated > 0);
    }
}
