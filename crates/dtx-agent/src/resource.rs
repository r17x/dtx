//! Agent resource implementation.
//!
//! This module provides the `AgentResource` type that implements the `Resource`
//! trait from dtx-core, enabling agents to be managed by the dtx orchestrator.

use async_trait::async_trait;
use std::any::Any;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::resource::{
    Context, HealthStatus, LogEntry, LogStream, LogStreamKind, Resource, ResourceError, ResourceId,
    ResourceKind, ResourceResult, ResourceState,
};

use crate::config::{AgentConfig, AgentMode};
use crate::error::AgentError;
use crate::message::Message;
use crate::runtime::{detect_runtime, AgentInfo, AgentResponse, AgentRuntime, RuntimeCapabilities};

/// Buffer capacity for agent logs.
const LOG_BUFFER_CAPACITY: usize = 1000;

/// AI agent resource implementing the Resource trait.
///
/// `AgentResource` manages an AI agent with support for:
/// - Multiple runtime backends (Claude, OpenAI, Ollama)
/// - Tool execution
/// - Conversation history management
/// - Daemon, one-shot, and worker modes
pub struct AgentResource {
    /// Configuration.
    config: AgentConfig,
    /// Resource ID.
    id: ResourceId,
    /// Current state.
    state: ResourceState,
    /// Event bus for publishing lifecycle events.
    event_bus: Arc<ResourceEventBus>,
    /// Runtime implementation.
    runtime: Option<Box<dyn AgentRuntime>>,
    /// Conversation history.
    messages: Arc<RwLock<VecDeque<Message>>>,
    /// Captured logs.
    logs: Arc<RwLock<VecDeque<LogEntry>>>,
    /// Started timestamp.
    started_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl AgentResource {
    /// Create a new agent resource with the given configuration.
    pub fn new(config: AgentConfig, event_bus: Arc<ResourceEventBus>) -> Self {
        let id = ResourceId::new(&config.id);
        Self {
            config,
            id,
            state: ResourceState::Pending,
            event_bus,
            runtime: None,
            messages: Arc::new(RwLock::new(VecDeque::with_capacity(LOG_BUFFER_CAPACITY))),
            logs: Arc::new(RwLock::new(VecDeque::with_capacity(LOG_BUFFER_CAPACITY))),
            started_at: None,
        }
    }

    /// Get the agent configuration.
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Get the underlying runtime if available.
    pub fn runtime(&self) -> Option<&dyn AgentRuntime> {
        self.runtime.as_ref().map(|r| r.as_ref())
    }

    /// Get runtime info.
    pub fn info(&self) -> Option<AgentInfo> {
        self.runtime.as_ref().map(|r| r.info())
    }

    /// Get runtime capabilities.
    pub fn capabilities(&self) -> Option<RuntimeCapabilities> {
        self.runtime.as_ref().map(|r| r.capabilities())
    }

    /// Send a message to the agent and get a response.
    ///
    /// This is the main interaction method. It handles:
    /// - Adding the message to history
    /// - Sending to the runtime
    /// - Processing tool calls if any
    /// - Adding the response to history
    pub async fn send(&self, message: impl Into<Message>) -> Result<AgentResponse, AgentError> {
        let runtime = self.runtime.as_ref().ok_or(AgentError::NotRunning)?;

        let message = message.into();

        // Add to history
        {
            let mut messages = self.messages.write().await;
            messages.push_back(message);

            // Trim history if needed
            while messages.len() > self.config.memory.max_history {
                messages.pop_front();
            }
        }

        // Get messages for request
        let messages: Vec<Message> = self.messages.read().await.iter().cloned().collect();

        // Log the interaction
        self.add_log(
            LogStreamKind::Stdout,
            format!(
                "[user] {}",
                messages.last().map(|m| m.text()).unwrap_or_default()
            ),
        )
        .await;

        // Send to runtime
        let response = runtime.send(&messages, &self.config.tools).await?;

        // Add response to history
        if !response.content.is_empty() {
            let assistant_msg = if response.tool_calls.is_empty() {
                Message::assistant(&response.content)
            } else {
                Message::assistant_with_tools(&response.content, response.tool_calls.clone())
            };

            let mut messages = self.messages.write().await;
            messages.push_back(assistant_msg);
        }

        // Log response
        self.add_log(
            LogStreamKind::Stdout,
            format!("[assistant] {}", response.content),
        )
        .await;

        // Process tool calls if any
        if !response.tool_calls.is_empty() {
            self.add_log(
                LogStreamKind::Stdout,
                format!("[tools] {} tool call(s)", response.tool_calls.len()),
            )
            .await;

            for tool_call in &response.tool_calls {
                let result = runtime.execute_tool(tool_call).await?;

                // Add tool result to history
                let tool_msg =
                    Message::tool_result(&result.tool_use_id, &result.content, result.is_error);
                self.messages.write().await.push_back(tool_msg);

                self.add_log(
                    LogStreamKind::Stdout,
                    format!(
                        "[tool:{}] {}",
                        tool_call.name,
                        if result.is_error { "error" } else { "success" }
                    ),
                )
                .await;
            }
        }

        Ok(response)
    }

    /// Get conversation history.
    pub async fn history(&self) -> Vec<Message> {
        self.messages.read().await.iter().cloned().collect()
    }

    /// Clear conversation history.
    pub async fn clear_history(&self) {
        self.messages.write().await.clear();
    }

    /// Add a log entry.
    async fn add_log(&self, stream: LogStreamKind, line: impl Into<String>) {
        let line = line.into();
        let timestamp = chrono::Utc::now();

        // Publish log event
        self.event_bus.publish(LifecycleEvent::Log {
            id: self.id.clone(),
            stream,
            line: line.clone(),
            timestamp,
        });

        // Store in buffer
        let entry = LogEntry {
            timestamp,
            stream,
            line,
        };

        let mut logs = self.logs.write().await;
        if logs.len() >= LOG_BUFFER_CAPACITY {
            logs.pop_front();
        }
        logs.push_back(entry);
    }

    fn make_error(msg: &str) -> ResourceError {
        Box::new(std::io::Error::other(msg.to_string()))
    }
}

#[async_trait]
impl Resource for AgentResource {
    fn id(&self) -> &ResourceId {
        &self.id
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::Agent
    }

    fn state(&self) -> &ResourceState {
        &self.state
    }

    async fn start(&mut self, _ctx: &Context) -> ResourceResult<()> {
        // Check current state
        if self.state.is_running() {
            return Ok(());
        }

        tracing::info!(id = %self.id, model = %self.config.model.name, "Starting agent");

        // Transition to Starting
        self.state = ResourceState::Starting {
            started_at: chrono::Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Starting {
            id: self.id.clone(),
            kind: ResourceKind::Agent,
            timestamp: chrono::Utc::now(),
        });

        // Initialize runtime
        match detect_runtime(&self.config).await {
            Some(mut runtime) => {
                if let Err(e) = runtime.initialize(&self.config).await {
                    let error = format!("Failed to initialize runtime: {}", e);
                    self.state = ResourceState::Failed {
                        error: error.clone(),
                        exit_code: None,
                        started_at: None,
                        failed_at: chrono::Utc::now(),
                    };
                    self.event_bus.publish(LifecycleEvent::Failed {
                        id: self.id.clone(),
                        kind: ResourceKind::Agent,
                        error,
                        exit_code: None,
                        timestamp: chrono::Utc::now(),
                    });
                    return Err(Self::make_error("Runtime initialization failed"));
                }
                self.runtime = Some(runtime);
            }
            None => {
                let error = format!("No runtime available for {:?}", self.config.runtime);
                self.state = ResourceState::Failed {
                    error: error.clone(),
                    exit_code: None,
                    started_at: None,
                    failed_at: chrono::Utc::now(),
                };
                self.event_bus.publish(LifecycleEvent::Failed {
                    id: self.id.clone(),
                    kind: ResourceKind::Agent,
                    error,
                    exit_code: None,
                    timestamp: chrono::Utc::now(),
                });
                return Err(Self::make_error("No runtime available"));
            }
        }

        // Add system prompt to history
        if let Some(prompt) = &self.config.system_prompt {
            self.messages
                .write()
                .await
                .push_back(Message::system(prompt));
        }

        // Transition to Running
        self.started_at = Some(chrono::Utc::now());
        self.state = ResourceState::Running {
            pid: None,
            started_at: self.started_at.unwrap(),
        };
        self.event_bus.publish(LifecycleEvent::Running {
            id: self.id.clone(),
            kind: ResourceKind::Agent,
            pid: None,
            timestamp: chrono::Utc::now(),
        });

        // Handle different modes
        match &self.config.mode {
            AgentMode::OneShot => {
                // One-shot mode: agent is ready for a single request
                tracing::info!(id = %self.id, "Agent ready in one-shot mode");
            }
            AgentMode::Daemon => {
                // Daemon mode: agent stays running until stopped
                tracing::info!(id = %self.id, "Agent running in daemon mode");
            }
            AgentMode::Worker { queue, concurrency } => {
                // Worker mode: TODO - implement queue processing
                tracing::info!(
                    id = %self.id,
                    queue = %queue,
                    concurrency = %concurrency,
                    "Agent running in worker mode (queue processing not yet implemented)"
                );
            }
            AgentMode::Cron { schedule, task } => {
                // Cron mode: TODO - implement scheduled execution
                tracing::info!(
                    id = %self.id,
                    schedule = %schedule,
                    task = %task,
                    "Agent running in cron mode (scheduling not yet implemented)"
                );
            }
            AgentMode::Interactive => {
                // Interactive mode: agent is ready for REPL-style interaction
                tracing::info!(id = %self.id, "Agent ready in interactive mode");
            }
        }

        Ok(())
    }

    async fn stop(&mut self, _ctx: &Context) -> ResourceResult<()> {
        if !self.state.is_running() {
            return Ok(());
        }

        tracing::info!(id = %self.id, "Stopping agent");

        // Transition to Stopping
        let started_at = self.started_at.unwrap_or_else(chrono::Utc::now);
        self.state = ResourceState::Stopping {
            started_at,
            stopping_at: chrono::Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Stopping {
            id: self.id.clone(),
            kind: ResourceKind::Agent,
            timestamp: chrono::Utc::now(),
        });

        // Shutdown runtime
        if let Some(mut runtime) = self.runtime.take() {
            if let Err(e) = runtime.shutdown().await {
                tracing::warn!(id = %self.id, error = %e, "Error shutting down runtime");
            }
        }

        // Transition to Stopped
        self.state = ResourceState::Stopped {
            exit_code: Some(0),
            started_at,
            stopped_at: chrono::Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Stopped {
            id: self.id.clone(),
            kind: ResourceKind::Agent,
            exit_code: Some(0),
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    async fn kill(&mut self, ctx: &Context) -> ResourceResult<()> {
        // For agents, kill is the same as stop
        self.stop(ctx).await
    }

    async fn restart(&mut self, ctx: &Context) -> ResourceResult<()> {
        // Clear history on restart
        self.clear_history().await;
        self.stop(ctx).await?;
        self.start(ctx).await
    }

    async fn health(&self) -> HealthStatus {
        if !self.state.is_running() {
            return HealthStatus::Unhealthy {
                reason: format!("Agent not running (state: {})", self.state),
            };
        }

        match &self.runtime {
            Some(runtime) => match runtime.health().await {
                Ok(crate::runtime::HealthStatus::Healthy) => HealthStatus::Healthy,
                Ok(crate::runtime::HealthStatus::Unhealthy { reason }) => {
                    HealthStatus::Unhealthy { reason }
                }
                Ok(crate::runtime::HealthStatus::Unknown) => HealthStatus::Unknown,
                Err(e) => HealthStatus::Unhealthy {
                    reason: e.to_string(),
                },
            },
            None => HealthStatus::Unhealthy {
                reason: "No runtime initialized".to_string(),
            },
        }
    }

    fn logs(&self) -> Option<Box<dyn LogStream>> {
        Some(Box::new(AgentLogStream {
            logs: Arc::clone(&self.logs),
            position: 0,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Log stream for agent logs.
struct AgentLogStream {
    logs: Arc<RwLock<VecDeque<LogEntry>>>,
    position: usize,
}

impl LogStream for AgentLogStream {
    fn try_recv(&mut self) -> Option<LogEntry> {
        // Note: This is a non-async method, so we use try_read
        if let Ok(logs) = self.logs.try_read() {
            if self.position < logs.len() {
                let entry = logs[self.position].clone();
                self.position += 1;
                return Some(entry);
            }
        }
        None
    }

    fn is_open(&self) -> bool {
        self.logs
            .try_read()
            .map(|logs| self.position < logs.len())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(id: &str) -> AgentConfig {
        AgentConfig::new(id, "test-model").with_system_prompt("You are a helpful assistant.")
    }

    fn make_resource(config: AgentConfig) -> AgentResource {
        let event_bus = Arc::new(ResourceEventBus::new());
        AgentResource::new(config, event_bus)
    }

    #[test]
    fn agent_resource_new() {
        let config = make_config("test-agent");
        let resource = make_resource(config);

        assert_eq!(resource.id().as_str(), "test-agent");
        assert_eq!(resource.kind(), ResourceKind::Agent);
        assert_eq!(resource.state(), &ResourceState::Pending);
        assert!(resource.runtime().is_none());
    }

    #[test]
    fn agent_resource_config() {
        let config = make_config("test-agent");
        let resource = make_resource(config);

        assert_eq!(resource.config().id, "test-agent");
        assert_eq!(resource.config().model.name, "test-model");
        assert_eq!(
            resource.config().system_prompt,
            Some("You are a helpful assistant.".to_string())
        );
    }

    #[test]
    fn agent_resource_state_initial() {
        let config = make_config("test");
        let resource = make_resource(config);

        assert!(!resource.state().is_running());
        assert!(resource.state().is_pending());
    }

    #[tokio::test]
    async fn agent_resource_clear_history() {
        let config = make_config("test");
        let resource = make_resource(config);

        // Add some messages
        {
            let mut messages = resource.messages.write().await;
            messages.push_back(Message::user("Hello"));
            messages.push_back(Message::assistant("Hi!"));
        }

        assert_eq!(resource.history().await.len(), 2);

        resource.clear_history().await;
        assert_eq!(resource.history().await.len(), 0);
    }

    #[tokio::test]
    async fn agent_resource_health_not_running() {
        let config = make_config("test");
        let resource = make_resource(config);

        let health = resource.health().await;
        match health {
            HealthStatus::Unhealthy { reason } => {
                assert!(reason.contains("not running"));
            }
            _ => panic!("Expected unhealthy status"),
        }
    }

    #[test]
    fn agent_resource_downcast() {
        let config = make_config("test");
        let resource = make_resource(config);

        let any_ref = resource.as_any();
        assert!(any_ref.downcast_ref::<AgentResource>().is_some());
    }

    #[test]
    fn agent_resource_logs() {
        let config = make_config("test");
        let resource = make_resource(config);

        let logs = resource.logs();
        assert!(logs.is_some());
    }

    #[tokio::test]
    async fn agent_log_stream() {
        let logs = Arc::new(RwLock::new(VecDeque::new()));

        {
            let mut write = logs.write().await;
            write.push_back(LogEntry {
                timestamp: chrono::Utc::now(),
                stream: LogStreamKind::Stdout,
                line: "Test log 1".to_string(),
            });
            write.push_back(LogEntry {
                timestamp: chrono::Utc::now(),
                stream: LogStreamKind::Stdout,
                line: "Test log 2".to_string(),
            });
        }

        let mut stream = AgentLogStream { logs, position: 0 };

        let entry1 = stream.try_recv().unwrap();
        assert_eq!(entry1.line, "Test log 1");

        let entry2 = stream.try_recv().unwrap();
        assert_eq!(entry2.line, "Test log 2");

        assert!(stream.try_recv().is_none());
        assert!(!stream.is_open());
    }

    #[test]
    fn agent_mode_default() {
        let config = AgentConfig::default();
        assert!(matches!(config.mode, AgentMode::Daemon));
    }

    #[tokio::test]
    async fn agent_resource_add_log() {
        let config = make_config("test");
        let resource = make_resource(config);

        resource
            .add_log(LogStreamKind::Stdout, "Test message")
            .await;

        let logs = resource.logs.read().await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].line, "Test message");
    }
}
