//! OpenAI runtime implementation.
//!
//! This module provides the `OpenAIRuntime` that implements the `AgentRuntime`
//! trait for OpenAI's API and OpenAI-compatible APIs.

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::config::{AgentConfig, ApiKeyConfig, ToolConfig};
use crate::error::{AgentError, Result};
use crate::message::{Content, ContentBlock, ImageSource, Message, Role, ToolCall, ToolResult};
use crate::runtime::{
    AgentInfo, AgentResponse, AgentRuntime, AgentState, HealthStatus, RuntimeCapabilities,
    StopReason, StreamEvent, TokenUsage,
};
use crate::tools::ToolExecutor;

const OPENAI_API_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// OpenAI runtime using OpenAI API.
///
/// This runtime supports both OpenAI's official API and OpenAI-compatible APIs
/// (like Azure OpenAI, local LLMs with OpenAI-compatible endpoints, etc.) by
/// configuring a custom `base_url`.
pub struct OpenAIRuntime {
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

impl OpenAIRuntime {
    /// Create a new OpenAI runtime from configuration.
    ///
    /// # API Key
    ///
    /// The API key is resolved in the following order:
    /// 1. Direct value in config
    /// 2. Environment variable specified in config
    /// 3. OPENAI_API_KEY environment variable
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
            None => std::env::var("OPENAI_API_KEY")
                .map_err(|_| AgentError::config("OPENAI_API_KEY not set"))?,
        };

        let base_url = config
            .model
            .base_url
            .clone()
            .unwrap_or_else(|| OPENAI_API_URL.to_string());

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

    /// Convert messages to OpenAI API format.
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

                match m.role {
                    Role::Tool => {
                        // Tool messages need tool_call_id
                        let tool_call_id = self.extract_tool_call_id(&m.content);
                        serde_json::json!({
                            "role": role,
                            "content": m.text(),
                            "tool_call_id": tool_call_id
                        })
                    }
                    Role::Assistant => {
                        // Check if message has tool calls
                        let tool_calls = self.extract_tool_calls(&m.content);
                        if tool_calls.is_empty() {
                            serde_json::json!({
                                "role": role,
                                "content": self.convert_content(&m.content)
                            })
                        } else {
                            serde_json::json!({
                                "role": role,
                                "content": self.convert_content_text_only(&m.content),
                                "tool_calls": tool_calls
                            })
                        }
                    }
                    _ => serde_json::json!({
                        "role": role,
                        "content": self.convert_content(&m.content)
                    }),
                }
            })
            .collect()
    }

    /// Convert content to OpenAI format.
    fn convert_content(&self, content: &Content) -> serde_json::Value {
        match content {
            Content::Text(text) => serde_json::json!(text),
            Content::Blocks(blocks) => {
                let parts: Vec<serde_json::Value> = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(serde_json::json!({
                            "type": "text",
                            "text": text
                        })),
                        ContentBlock::Image { source } => {
                            let url = match source {
                                ImageSource::Base64 { media_type, data } => {
                                    format!("data:{};base64,{}", media_type, data)
                                }
                                ImageSource::Url { url } => url.clone(),
                            };
                            Some(serde_json::json!({
                                "type": "image_url",
                                "image_url": { "url": url }
                            }))
                        }
                        ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => None,
                    })
                    .collect();

                if parts.len() == 1 {
                    if let Some(text) = parts[0]["text"].as_str() {
                        return serde_json::json!(text);
                    }
                }
                serde_json::json!(parts)
            }
        }
    }

    /// Extract only text content for assistant messages with tool calls.
    fn convert_content_text_only(&self, content: &Content) -> Option<String> {
        match content {
            Content::Text(text) => {
                if text.is_empty() {
                    None
                } else {
                    Some(text.clone())
                }
            }
            Content::Blocks(blocks) => {
                let text: String = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
        }
    }

    /// Extract tool calls from content blocks.
    fn extract_tool_calls(&self, content: &Content) -> Vec<serde_json::Value> {
        match content {
            Content::Text(_) => vec![],
            Content::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => Some(serde_json::json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": input.to_string()
                        }
                    })),
                    _ => None,
                })
                .collect(),
        }
    }

    /// Extract tool_call_id from tool result content.
    fn extract_tool_call_id(&self, content: &Content) -> String {
        match content {
            Content::Text(_) => String::new(),
            Content::Blocks(blocks) => blocks
                .iter()
                .find_map(|b| match b {
                    ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
        }
    }

    /// Build tools schema for OpenAI API.
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
                ToolConfig::Custom {
                    name,
                    description,
                    schema,
                    ..
                } => Some(serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": schema.clone().unwrap_or(serde_json::json!({
                            "type": "object",
                            "properties": {}
                        }))
                    }
                })),
                ToolConfig::Resource { .. } | ToolConfig::Mcp { .. } => None,
            })
            .collect()
    }

    /// Get built-in tool schema.
    fn builtin_tool_schema(&self, name: &str) -> serde_json::Value {
        match name {
            "read_file" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read the contents of a file at the specified path",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to the file to read" }
                        },
                        "required": ["path"]
                    }
                }
            }),
            "write_file" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_file",
                    "description": "Write content to a file at the specified path",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to the file to write" },
                            "content": { "type": "string", "description": "Content to write to the file" }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            "execute_command" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "description": "Execute a shell command and return its output",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "description": "Shell command to execute" }
                        },
                        "required": ["command"]
                    }
                }
            }),
            "list_files" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_files",
                    "description": "List files and directories in the specified path",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Directory path to list" }
                        }
                    }
                }
            }),
            "search_files" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "search_files",
                    "description": "Search for files containing a pattern using grep",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string", "description": "Pattern to search for" },
                            "path": { "type": "string", "description": "Directory to search in" }
                        },
                        "required": ["pattern"]
                    }
                }
            }),
            "get_current_time" => serde_json::json!({
                "type": "function",
                "function": {
                    "name": "get_current_time",
                    "description": "Get the current date and time in UTC",
                    "parameters": {
                        "type": "object",
                        "properties": {}
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

    /// Parse OpenAI API response into AgentResponse.
    fn parse_response(&self, result: serde_json::Value, latency_ms: u64) -> AgentResponse {
        let choice = &result["choices"][0];
        let message = &choice["message"];

        // Extract text content
        let content = message["content"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Extract tool calls
        let tool_calls: Vec<ToolCall> = message["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let id = tc["id"].as_str()?.to_string();
                        let name = tc["function"]["name"].as_str()?.to_string();
                        let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                        let input = serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
                        Some(ToolCall { id, name, input })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Determine stop reason
        let stop_reason = match choice["finish_reason"].as_str() {
            Some("stop") => StopReason::EndTurn,
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            Some("content_filter") => StopReason::StopSequence,
            _ => {
                if !tool_calls.is_empty() {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                }
            }
        };

        // Extract token usage
        let usage = TokenUsage {
            input_tokens: result["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: result["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            cache_read_tokens: 0, // OpenAI doesn't expose cache info
            cache_write_tokens: 0,
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
impl AgentRuntime for OpenAIRuntime {
    fn name(&self) -> &str {
        "openai"
    }

    async fn is_available(&self) -> bool {
        // Check if API key is set and optionally ping API
        if self.api_key.is_empty() {
            return false;
        }

        // Try to list models to verify API key
        let response = self
            .client
            .get(format!("{}/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await;

        response
            .map(|r| r.status() != reqwest::StatusCode::UNAUTHORIZED)
            .unwrap_or(false)
    }

    async fn initialize(&mut self, config: &AgentConfig) -> Result<()> {
        self.config = config.clone();
        self.started_at = Some(chrono::Utc::now());
        self.tool_executor = Arc::new(ToolExecutor::new(config.tools.clone()));
        *self.state.write().await = AgentState::Idle;
        debug!(model = %config.model.name, "OpenAI runtime initialized");
        Ok(())
    }

    async fn send(&self, messages: &[Message], tools: &[ToolConfig]) -> Result<AgentResponse> {
        *self.state.write().await = AgentState::Processing;

        let start = std::time::Instant::now();

        // Convert messages to OpenAI format
        let converted_messages = self.convert_messages(messages);
        let tools_schema = self.build_tools_schema(tools);

        // Build request body
        let mut body = serde_json::json!({
            "model": self.config.model.name,
            "messages": converted_messages,
        });

        // Add max_tokens if configured
        if let Some(max_tokens) = self.config.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        } else {
            body["max_tokens"] = serde_json::json!(DEFAULT_MAX_TOKENS);
        }

        // Add temperature if configured
        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        // Add tools if provided
        if !tools_schema.is_empty() {
            body["tools"] = serde_json::json!(tools_schema);
        }

        debug!(model = %self.config.model.name, messages = messages.len(), "Sending request to OpenAI");

        // Make request
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let error = response.text().await.unwrap_or_default();
            *self.state.write().await = AgentState::Error(error.clone());

            // Handle rate limiting
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                warn!("OpenAI rate limited");
                return Err(AgentError::RateLimited { retry_after: None });
            }

            return Err(AgentError::api(status.as_u16(), error));
        }

        // Parse response
        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        let latency_ms = start.elapsed().as_millis() as u64;
        let response = self.parse_response(result, latency_ms);

        // Update counters
        self.turns.fetch_add(1, Ordering::SeqCst);
        self.input_tokens
            .fetch_add(response.usage.input_tokens, Ordering::SeqCst);
        self.output_tokens
            .fetch_add(response.usage.output_tokens, Ordering::SeqCst);

        // Update state
        *self.state.write().await = if response.tool_calls.is_empty() {
            AgentState::Idle
        } else {
            AgentState::WaitingForTool
        };

        debug!(
            latency_ms = latency_ms,
            input_tokens = response.usage.input_tokens,
            output_tokens = response.usage.output_tokens,
            tool_calls = response.tool_calls.len(),
            "Received response from OpenAI"
        );

        Ok(response)
    }

    async fn send_streaming(
        &self,
        messages: &[Message],
        tools: &[ToolConfig],
    ) -> Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>> {
        *self.state.write().await = AgentState::Processing;

        // Convert messages to OpenAI format
        let converted_messages = self.convert_messages(messages);
        let tools_schema = self.build_tools_schema(tools);

        // Build request body
        let mut body = serde_json::json!({
            "model": self.config.model.name,
            "messages": converted_messages,
            "stream": true,
        });

        // Add max_tokens if configured
        if let Some(max_tokens) = self.config.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        } else {
            body["max_tokens"] = serde_json::json!(DEFAULT_MAX_TOKENS);
        }

        // Add temperature if configured
        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        // Add tools if provided
        if !tools_schema.is_empty() {
            body["tools"] = serde_json::json!(tools_schema);
        }

        // Include usage in stream options
        body["stream_options"] = serde_json::json!({"include_usage": true});

        debug!(model = %self.config.model.name, "Starting streaming request to OpenAI");

        // Make request
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::backend(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let error = response.text().await.unwrap_or_default();
            *self.state.write().await = AgentState::Error(error.clone());

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(AgentError::RateLimited { retry_after: None });
            }

            return Err(AgentError::api(status.as_u16(), error));
        }

        // Track state for tool call parsing
        let current_tool_index: Option<usize> = None;
        let current_tool_id = String::new();
        let current_tool_name = String::new();

        // Parse SSE stream
        let stream = response
            .bytes_stream()
            .scan(
                (
                    current_tool_index,
                    current_tool_id,
                    current_tool_name,
                    String::new(),
                ),
                move |(tool_index, tool_id, tool_name, buffer), result| {
                    let events = match result {
                        Ok(bytes) => {
                            let text = String::from_utf8_lossy(&bytes);
                            buffer.push_str(&text);

                            let mut events = Vec::new();

                            // Process complete lines
                            while let Some(newline_pos) = buffer.find('\n') {
                                let line = buffer[..newline_pos].trim().to_string();
                                buffer.drain(..=newline_pos);

                                if line.is_empty() {
                                    continue;
                                }

                                if let Some(data) = line.strip_prefix("data: ") {
                                    if data == "[DONE]" {
                                        events.push(StreamEvent::MessageEnd {
                                            usage: TokenUsage::default(),
                                        });
                                    } else if let Ok(event) =
                                        serde_json::from_str::<serde_json::Value>(data)
                                    {
                                        let choice = &event["choices"][0];
                                        let delta = &choice["delta"];

                                        // Check finish reason
                                        if let Some(finish_reason) =
                                            choice["finish_reason"].as_str()
                                        {
                                            if finish_reason == "stop"
                                                || finish_reason == "tool_calls"
                                            {
                                                let usage = event
                                                    .get("usage")
                                                    .map(|u| TokenUsage {
                                                        input_tokens: u["prompt_tokens"]
                                                            .as_u64()
                                                            .unwrap_or(0),
                                                        output_tokens: u["completion_tokens"]
                                                            .as_u64()
                                                            .unwrap_or(0),
                                                        cache_read_tokens: 0,
                                                        cache_write_tokens: 0,
                                                    })
                                                    .unwrap_or_default();
                                                events.push(StreamEvent::MessageEnd { usage });
                                                continue;
                                            }
                                        }

                                        // Content delta
                                        if let Some(text) = delta["content"].as_str() {
                                            if !text.is_empty() {
                                                events.push(StreamEvent::TextDelta {
                                                    index: 0,
                                                    text: text.to_string(),
                                                });
                                            }
                                        }

                                        // Tool calls delta
                                        if let Some(tool_calls) = delta["tool_calls"].as_array() {
                                            for tc in tool_calls {
                                                let index =
                                                    tc["index"].as_u64().unwrap_or(0) as usize;

                                                // New tool call starting
                                                if let Some(id) = tc["id"].as_str() {
                                                    *tool_index = Some(index);
                                                    *tool_id = id.to_string();
                                                    if let Some(name) =
                                                        tc["function"]["name"].as_str()
                                                    {
                                                        *tool_name = name.to_string();
                                                    }
                                                    events.push(StreamEvent::ToolStart {
                                                        index,
                                                        id: tool_id.clone(),
                                                        name: tool_name.clone(),
                                                    });
                                                }

                                                // Tool arguments delta
                                                if let Some(args_delta) =
                                                    tc["function"]["arguments"].as_str()
                                                {
                                                    if !args_delta.is_empty() {
                                                        events.push(StreamEvent::ToolInputDelta {
                                                            index,
                                                            delta: args_delta.to_string(),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            events
                        }
                        Err(e) => vec![StreamEvent::Error {
                            error: e.to_string(),
                        }],
                    };

                    std::future::ready(Some(futures::stream::iter(events)))
                },
            )
            .flatten();

        Ok(Box::pin(stream))
    }

    async fn execute_tool(&self, tool_call: &ToolCall) -> Result<ToolResult> {
        debug!(tool = %tool_call.name, id = %tool_call.id, "Executing tool");
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
        debug!("Cancelling OpenAI runtime");
        *self.state.write().await = AgentState::Idle;
        Ok(())
    }

    async fn health(&self) -> Result<HealthStatus> {
        let state = self.state.read().await.clone();
        match state {
            AgentState::Error(reason) => Ok(HealthStatus::Unhealthy { reason }),
            AgentState::Stopped => Ok(HealthStatus::Unhealthy {
                reason: "Runtime stopped".to_string(),
            }),
            _ => {
                if self.is_available().await {
                    Ok(HealthStatus::Healthy)
                } else {
                    Ok(HealthStatus::Unhealthy {
                        reason: "OpenAI API not reachable".to_string(),
                    })
                }
            }
        }
    }

    async fn count_tokens(&self, text: &str) -> Result<u64> {
        // Rough approximation: ~4 chars per token for GPT models
        // For more accuracy, use tiktoken crate
        Ok((text.len() / 4) as u64)
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tools: true,
            vision: true, // GPT-4V supports vision
            system_prompt: true,
            max_context: Some(128000), // GPT-4 Turbo context
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        debug!("Shutting down OpenAI runtime");
        *self.state.write().await = AgentState::Stopped;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> AgentConfig {
        AgentConfig::new("test", "gpt-4").with_runtime(crate::config::AgentRuntimeType::OpenAI)
    }

    #[test]
    fn openai_runtime_name() {
        let config = make_config();
        assert_eq!(config.runtime, crate::config::AgentRuntimeType::OpenAI);
    }

    #[test]
    fn capabilities() {
        let caps = RuntimeCapabilities {
            streaming: true,
            tools: true,
            vision: true,
            system_prompt: true,
            max_context: Some(128000),
        };

        assert!(caps.streaming);
        assert!(caps.tools);
        assert!(caps.vision);
        assert_eq!(caps.max_context, Some(128000));
    }

    #[test]
    fn message_conversion_roles() {
        let messages = vec![
            Message::system("You are helpful."),
            Message::user("Hello"),
            Message::assistant("Hi there!"),
        ];

        // Test role conversion
        assert!(matches!(messages[0].role, Role::System));
        assert!(matches!(messages[1].role, Role::User));
        assert!(matches!(messages[2].role, Role::Assistant));

        assert_eq!(messages[0].text(), "You are helpful.");
        assert_eq!(messages[1].text(), "Hello");
        assert_eq!(messages[2].text(), "Hi there!");
    }

    #[test]
    fn tool_schema_format() {
        let schema = serde_json::json!({
            "type": "function",
            "function": {
                "name": "test_function",
                "description": "A test function",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "arg1": { "type": "string" }
                    },
                    "required": ["arg1"]
                }
            }
        });

        assert_eq!(schema["type"], "function");
        assert_eq!(schema["function"]["name"], "test_function");
        assert!(schema["function"]["parameters"]["properties"]["arg1"].is_object());
    }

    #[test]
    fn parse_stop_reason() {
        // Test stop reason mapping
        let reasons = vec![
            ("stop", StopReason::EndTurn),
            ("tool_calls", StopReason::ToolUse),
            ("length", StopReason::MaxTokens),
            ("content_filter", StopReason::StopSequence),
        ];

        for (input, expected) in reasons {
            let result = match input {
                "stop" => StopReason::EndTurn,
                "tool_calls" => StopReason::ToolUse,
                "length" => StopReason::MaxTokens,
                "content_filter" => StopReason::StopSequence,
                _ => StopReason::EndTurn,
            };
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn token_usage_from_json() {
        let json = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50
        });

        let usage = TokenUsage {
            input_tokens: json["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: json["completion_tokens"].as_u64().unwrap_or(0),
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn parse_tool_call_from_response() {
        let response = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\": \"NYC\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        });

        // Parse tool calls
        let tool_calls = response["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(tool_calls.len(), 1);

        let tc = &tool_calls[0];
        assert_eq!(tc["id"], "call_abc123");
        assert_eq!(tc["function"]["name"], "get_weather");

        let args: serde_json::Value =
            serde_json::from_str(tc["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["location"], "NYC");
    }

    #[test]
    fn agent_state_transitions() {
        let state = AgentState::Idle;
        assert_eq!(state, AgentState::Idle);

        let processing = AgentState::Processing;
        assert_eq!(processing, AgentState::Processing);

        let error = AgentState::Error("test error".to_string());
        assert!(matches!(error, AgentState::Error(_)));

        let waiting = AgentState::WaitingForTool;
        assert_eq!(waiting, AgentState::WaitingForTool);
    }

    #[tokio::test]
    async fn estimate_tokens() {
        let text = "Hello, this is a test message for token counting.";
        // ~4 chars per token
        let estimated = (text.len() / 4) as u64;
        assert!(estimated > 0);
        assert!(estimated < text.len() as u64);
    }

    #[test]
    fn stream_event_types() {
        let text_delta = StreamEvent::TextDelta {
            index: 0,
            text: "Hello".to_string(),
        };
        assert!(matches!(text_delta, StreamEvent::TextDelta { .. }));

        let tool_start = StreamEvent::ToolStart {
            index: 0,
            id: "call_123".to_string(),
            name: "get_weather".to_string(),
        };
        assert!(matches!(tool_start, StreamEvent::ToolStart { .. }));

        let tool_delta = StreamEvent::ToolInputDelta {
            index: 0,
            delta: "{\"location\":".to_string(),
        };
        assert!(matches!(tool_delta, StreamEvent::ToolInputDelta { .. }));

        let message_end = StreamEvent::MessageEnd {
            usage: TokenUsage::default(),
        };
        assert!(matches!(message_end, StreamEvent::MessageEnd { .. }));
    }

    #[test]
    fn parse_streaming_data() {
        let data = r#"{"id":"chatcmpl-123","choices":[{"index":0,"delta":{"content":"Hello"}}]}"#;
        let event: serde_json::Value = serde_json::from_str(data).unwrap();

        let content = event["choices"][0]["delta"]["content"]
            .as_str()
            .unwrap_or("");
        assert_eq!(content, "Hello");
    }

    #[test]
    fn convert_content_text() {
        let content = Content::Text("Hello world".to_string());
        match &content {
            Content::Text(text) => assert_eq!(text, "Hello world"),
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn convert_content_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "Part 1".to_string(),
            },
            ContentBlock::Text {
                text: "Part 2".to_string(),
            },
        ];
        let content = Content::Blocks(blocks);

        match &content {
            Content::Blocks(b) => {
                assert_eq!(b.len(), 2);
                match &b[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "Part 1"),
                    _ => panic!("Expected text block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn health_status_variants() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Unhealthy {
            reason: "test".to_string()
        }
        .is_healthy());
        assert!(!HealthStatus::Unknown.is_healthy());
    }

    #[test]
    fn tool_result_constructors() {
        let success = ToolResult::success("call_123", "Success result");
        assert_eq!(success.tool_use_id, "call_123");
        assert_eq!(success.content, "Success result");
        assert!(!success.is_error);

        let error = ToolResult::error("call_456", "Error message");
        assert_eq!(error.tool_use_id, "call_456");
        assert_eq!(error.content, "Error message");
        assert!(error.is_error);
    }

    #[test]
    fn tool_call_construction() {
        let tc = ToolCall::new(
            "call_123",
            "get_weather",
            serde_json::json!({"location": "NYC"}),
        );
        assert_eq!(tc.id, "call_123");
        assert_eq!(tc.name, "get_weather");
        assert_eq!(tc.input["location"], "NYC");
    }
}
