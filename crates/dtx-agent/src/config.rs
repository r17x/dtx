//! Agent configuration types.
//!
//! This module defines all configuration structures for AI agents,
//! including model settings, tools, MCP servers, and execution modes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for an AI agent resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique identifier for this agent.
    pub id: String,

    /// Human-readable name for the agent.
    #[serde(default)]
    pub name: Option<String>,

    /// Agent runtime type.
    #[serde(default)]
    pub runtime: AgentRuntimeType,

    /// Model configuration.
    pub model: ModelConfig,

    /// System prompt for the agent.
    #[serde(default)]
    pub system_prompt: Option<String>,

    /// System prompt from file.
    #[serde(default)]
    pub system_prompt_file: Option<PathBuf>,

    /// Tools available to the agent.
    #[serde(default)]
    pub tools: Vec<ToolConfig>,

    /// MCP servers to connect to.
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,

    /// Memory/context configuration.
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Execution mode.
    #[serde(default)]
    pub mode: AgentMode,

    /// Rate limiting.
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,

    /// Maximum tokens per response.
    #[serde(default)]
    pub max_tokens: Option<u32>,

    /// Temperature for sampling (0.0 to 2.0).
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Timeout for agent operations.
    #[serde(default = "default_timeout", with = "humantime_serde")]
    pub timeout: Duration,

    /// Labels for grouping.
    #[serde(default)]
    pub labels: HashMap<String, String>,

    /// Environment variables for tools.
    #[serde(default)]
    pub environment: HashMap<String, String>,
}

fn default_timeout() -> Duration {
    Duration::from_secs(300)
}

/// Agent runtime type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRuntimeType {
    /// Automatically detect best available runtime.
    #[default]
    Auto,
    /// Anthropic Claude API.
    Claude,
    /// OpenAI API.
    OpenAI,
    /// Local Ollama server.
    Ollama,
    /// Local llama.cpp server.
    LlamaCpp,
    /// Generic OpenAI-compatible API.
    OpenAICompatible,
    /// MCP-based agent.
    Mcp,
}

/// Model configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model name/identifier.
    pub name: String,

    /// API key (or environment variable reference).
    #[serde(default)]
    pub api_key: Option<ApiKeyConfig>,

    /// API base URL (for custom endpoints).
    #[serde(default)]
    pub base_url: Option<String>,

    /// Model-specific parameters.
    #[serde(default)]
    pub parameters: HashMap<String, serde_json::Value>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            name: "claude-3-5-sonnet-20241022".to_string(),
            api_key: None,
            base_url: None,
            parameters: HashMap::new(),
        }
    }
}

/// API key configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiKeyConfig {
    /// Direct API key value.
    Direct(String),
    /// Reference to environment variable.
    EnvVar { env: String },
    /// Reference to file.
    File { file: PathBuf },
}

impl ApiKeyConfig {
    /// Resolve the API key to a string value.
    pub async fn resolve(&self) -> Result<String, std::io::Error> {
        match self {
            Self::Direct(key) => Ok(key.clone()),
            Self::EnvVar { env } => std::env::var(env).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Environment variable {} not set", env),
                )
            }),
            Self::File { file } => tokio::fs::read_to_string(file)
                .await
                .map(|s| s.trim().to_string()),
        }
    }
}

/// Tool configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolConfig {
    /// Built-in tool.
    Builtin {
        name: String,
        #[serde(default)]
        config: serde_json::Value,
    },
    /// Shell command as tool.
    Command {
        name: String,
        description: String,
        command: String,
        #[serde(default)]
        args_schema: Option<serde_json::Value>,
        #[serde(default, with = "option_duration")]
        timeout: Option<Duration>,
    },
    /// HTTP endpoint as tool.
    Http {
        name: String,
        description: String,
        url: String,
        #[serde(default = "default_http_method")]
        method: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default)]
        body_schema: Option<serde_json::Value>,
    },
    /// Function from another dtx resource.
    Resource {
        name: String,
        resource: String,
        method: String,
    },
    /// MCP tool from connected server.
    Mcp { server: String, tool: String },
    /// Custom tool with handler path.
    Custom {
        name: String,
        description: String,
        handler: PathBuf,
        #[serde(default)]
        schema: Option<serde_json::Value>,
    },
}

fn default_http_method() -> String {
    "POST".to_string()
}

impl ToolConfig {
    /// Get the name of this tool.
    pub fn name(&self) -> &str {
        match self {
            Self::Builtin { name, .. }
            | Self::Command { name, .. }
            | Self::Http { name, .. }
            | Self::Resource { name, .. }
            | Self::Custom { name, .. } => name,
            Self::Mcp { tool, .. } => tool,
        }
    }

    /// Get the description of this tool.
    pub fn description(&self) -> Option<&str> {
        match self {
            Self::Command { description, .. }
            | Self::Http { description, .. }
            | Self::Custom { description, .. } => Some(description),
            _ => None,
        }
    }
}

/// MCP server connection configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name.
    pub name: String,

    /// Connection method.
    pub connection: McpConnection,

    /// Tools to expose from this server (empty = all).
    #[serde(default)]
    pub tools: Vec<String>,

    /// Resources to expose from this server (empty = all).
    #[serde(default)]
    pub resources: Vec<String>,
}

/// MCP connection type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpConnection {
    /// Command to spawn (stdio transport).
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// HTTP/WebSocket endpoint.
    Http {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
    /// Unix socket.
    Unix { path: PathBuf },
}

/// Memory/context configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum conversation history (message pairs).
    #[serde(default = "default_max_history")]
    pub max_history: usize,

    /// Persistent memory path.
    #[serde(default)]
    pub persist_path: Option<PathBuf>,

    /// Context window strategy.
    #[serde(default)]
    pub strategy: ContextStrategy,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_history: default_max_history(),
            persist_path: None,
            strategy: ContextStrategy::default(),
        }
    }
}

fn default_max_history() -> usize {
    100
}

/// Context window strategy.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    /// First-in, first-out truncation.
    #[default]
    Fifo,
    /// Summarize older messages.
    Summarize,
    /// Sliding window with overlap.
    Sliding {
        window: usize,
        #[serde(default)]
        overlap: usize,
    },
}

/// Agent execution mode.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    /// One-shot: process single request and stop.
    OneShot,
    /// Daemon: continuously running, responding to requests.
    #[default]
    Daemon,
    /// Worker: process queue of tasks.
    Worker {
        queue: String,
        #[serde(default = "default_concurrency")]
        concurrency: usize,
    },
    /// Cron: run on schedule.
    Cron {
        schedule: String,
        #[serde(default)]
        task: String,
    },
    /// Interactive: REPL-style interaction.
    Interactive,
}

fn default_concurrency() -> usize {
    1
}

/// Rate limiting configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Requests per minute.
    pub requests_per_minute: u32,

    /// Tokens per minute.
    #[serde(default)]
    pub tokens_per_minute: Option<u32>,

    /// Maximum concurrent requests.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

fn default_max_concurrent() -> usize {
    5
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: None,
            runtime: AgentRuntimeType::default(),
            model: ModelConfig::default(),
            system_prompt: None,
            system_prompt_file: None,
            tools: Vec::new(),
            mcp_servers: Vec::new(),
            memory: MemoryConfig::default(),
            mode: AgentMode::default(),
            rate_limit: None,
            max_tokens: None,
            temperature: None,
            timeout: default_timeout(),
            labels: HashMap::new(),
            environment: HashMap::new(),
        }
    }
}

impl AgentConfig {
    /// Create a new agent configuration with the given ID and model.
    pub fn new(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            model: ModelConfig {
                name: model.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Set the runtime type.
    pub fn with_runtime(mut self, runtime: AgentRuntimeType) -> Self {
        self.runtime = runtime;
        self
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Add a tool.
    pub fn with_tool(mut self, tool: ToolConfig) -> Self {
        self.tools.push(tool);
        self
    }

    /// Set the API key from environment variable.
    pub fn with_api_key_env(mut self, env_var: impl Into<String>) -> Self {
        self.model.api_key = Some(ApiKeyConfig::EnvVar {
            env: env_var.into(),
        });
        self
    }

    /// Set max tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }
}

/// Serde helper for optional duration using humantime format.
mod option_duration {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => serializer.serialize_str(&format!("{}s", d.as_secs())),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                // Simple parsing: expect "Ns" format
                let secs = s
                    .trim_end_matches('s')
                    .parse::<u64>()
                    .map_err(serde::de::Error::custom)?;
                Ok(Some(Duration::from_secs(secs)))
            }
            None => Ok(None),
        }
    }
}

/// Serde helper for duration using humantime format.
mod humantime_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}s", value.as_secs()))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Simple parsing: expect "Ns" format
        let secs = s
            .trim_end_matches('s')
            .parse::<u64>()
            .map_err(serde::de::Error::custom)?;
        Ok(Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = AgentConfig::default();
        assert_eq!(config.id, "default");
        assert_eq!(config.model.name, "claude-3-5-sonnet-20241022");
        assert_eq!(config.runtime, AgentRuntimeType::Auto);
    }

    #[test]
    fn config_new() {
        let config = AgentConfig::new("my-agent", "gpt-4");
        assert_eq!(config.id, "my-agent");
        assert_eq!(config.model.name, "gpt-4");
    }

    #[test]
    fn config_builder() {
        let config = AgentConfig::new("assistant", "claude-3-opus")
            .with_runtime(AgentRuntimeType::Claude)
            .with_system_prompt("You are a helpful assistant.")
            .with_max_tokens(4096)
            .with_temperature(0.7)
            .with_api_key_env("ANTHROPIC_API_KEY");

        assert_eq!(config.runtime, AgentRuntimeType::Claude);
        assert_eq!(
            config.system_prompt,
            Some("You are a helpful assistant.".to_string())
        );
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.7));
        assert!(matches!(
            config.model.api_key,
            Some(ApiKeyConfig::EnvVar { env }) if env == "ANTHROPIC_API_KEY"
        ));
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = AgentConfig::new("test", "claude-3-5-sonnet-20241022")
            .with_runtime(AgentRuntimeType::Claude)
            .with_system_prompt("Test prompt");

        let json = serde_json::to_string(&config).unwrap();
        let parsed: AgentConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, "test");
        assert_eq!(parsed.model.name, "claude-3-5-sonnet-20241022");
        assert_eq!(parsed.runtime, AgentRuntimeType::Claude);
        assert_eq!(parsed.system_prompt, Some("Test prompt".to_string()));
    }

    #[test]
    fn tool_config_builtin() {
        let tool = ToolConfig::Builtin {
            name: "read_file".to_string(),
            config: serde_json::json!({}),
        };
        assert_eq!(tool.name(), "read_file");
    }

    #[test]
    fn tool_config_command() {
        let tool = ToolConfig::Command {
            name: "run_tests".to_string(),
            description: "Run the test suite".to_string(),
            command: "cargo test".to_string(),
            args_schema: None,
            timeout: Some(Duration::from_secs(300)),
        };
        assert_eq!(tool.name(), "run_tests");
        assert_eq!(tool.description(), Some("Run the test suite"));
    }

    #[test]
    fn tool_config_http() {
        let tool = ToolConfig::Http {
            name: "webhook".to_string(),
            description: "Send webhook".to_string(),
            url: "https://example.com/webhook".to_string(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            body_schema: None,
        };
        assert_eq!(tool.name(), "webhook");
    }

    #[test]
    fn mcp_connection_stdio() {
        let conn = McpConnection::Stdio {
            command: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@anthropic/mcp-server-filesystem".to_string(),
            ],
            env: HashMap::new(),
        };

        let json = serde_json::to_value(&conn).unwrap();
        assert_eq!(json["type"], "stdio");
        assert_eq!(json["command"], "npx");
    }

    #[test]
    fn agent_mode_variants() {
        let daemon = AgentMode::Daemon;
        let json = serde_json::to_value(&daemon).unwrap();
        assert_eq!(json, serde_json::json!("daemon"));

        let worker = AgentMode::Worker {
            queue: "tasks".to_string(),
            concurrency: 3,
        };
        let json = serde_json::to_value(&worker).unwrap();
        // Tagged enum produces {"worker": {...}}
        assert!(json.is_object());
        assert!(json.to_string().contains("tasks"));
    }

    #[test]
    fn runtime_type_serde() {
        let rt = AgentRuntimeType::Claude;
        let json = serde_json::to_string(&rt).unwrap();
        assert_eq!(json, "\"claude\"");

        let parsed: AgentRuntimeType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, AgentRuntimeType::Claude);
    }

    #[test]
    fn memory_config_default() {
        let config = MemoryConfig::default();
        assert_eq!(config.max_history, default_max_history());
        assert!(config.persist_path.is_none());
        assert!(matches!(config.strategy, ContextStrategy::Fifo));
    }

    #[test]
    fn context_strategy_sliding() {
        let strategy = ContextStrategy::Sliding {
            window: 50,
            overlap: 10,
        };
        let json = serde_json::to_value(&strategy).unwrap();
        // Tagged enum produces {"sliding": {...}} structure
        assert!(json.is_object());
        assert!(json.to_string().contains("50"));
        assert!(json.to_string().contains("10"));
    }

    #[tokio::test]
    async fn api_key_resolve_env() {
        std::env::set_var("TEST_API_KEY_12345", "secret-key");
        let config = ApiKeyConfig::EnvVar {
            env: "TEST_API_KEY_12345".to_string(),
        };
        let key = config.resolve().await.unwrap();
        assert_eq!(key, "secret-key");
        std::env::remove_var("TEST_API_KEY_12345");
    }

    #[tokio::test]
    async fn api_key_resolve_direct() {
        let config = ApiKeyConfig::Direct("my-api-key".to_string());
        let key = config.resolve().await.unwrap();
        assert_eq!(key, "my-api-key");
    }
}
