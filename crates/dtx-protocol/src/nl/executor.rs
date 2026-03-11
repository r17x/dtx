//! Intent executor for natural language commands.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

use super::ParsedIntent;
use crate::handler::ProtocolHandler;
use crate::methods::ResourceParams;

/// Error executing an intent.
#[derive(Debug, Error)]
pub enum ExecuteError {
    /// Unknown operation.
    #[error("Unknown operation: {0}")]
    UnknownOperation(String),

    /// Resource not found.
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    /// Protocol error.
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Multiple errors.
    #[error("Multiple errors: {0:?}")]
    Multiple(Vec<String>),
}

/// Result of executing an intent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecuteResult {
    /// Success message.
    pub message: String,
    /// Resources affected.
    pub affected: Vec<String>,
    /// Any data returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ExecuteResult {
    /// Create a success result.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            affected: Vec::new(),
            data: None,
        }
    }

    /// Add affected resource.
    pub fn with_affected(mut self, resource: impl Into<String>) -> Self {
        self.affected.push(resource.into());
        self
    }

    /// Set data.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Executor for parsed intents.
pub struct IntentExecutor<H> {
    handler: Arc<H>,
}

impl<H> IntentExecutor<H> {
    /// Create a new executor.
    pub fn new(handler: Arc<H>) -> Self {
        Self { handler }
    }
}

impl<H: ProtocolHandler> IntentExecutor<H> {
    /// Execute a parsed intent.
    pub async fn execute(&self, intent: ParsedIntent) -> Result<ExecuteResult, ExecuteError> {
        match intent.operation.as_str() {
            "start" => self.execute_start(intent.targets).await,
            "stop" => self.execute_stop(intent.targets).await,
            "restart" => self.execute_restart(intent.targets).await,
            "status" => self.execute_status(intent.targets).await,
            "logs" => self.execute_logs(intent.targets, intent.options).await,
            _ => Err(ExecuteError::UnknownOperation(intent.operation)),
        }
    }

    /// Execute start operation.
    async fn execute_start(&self, targets: Vec<String>) -> Result<ExecuteResult, ExecuteError> {
        if targets.is_empty() {
            self.handler
                .start_all()
                .await
                .map_err(|e| ExecuteError::Protocol(e.message))?;

            Ok(ExecuteResult::success("All resources started"))
        } else {
            let mut affected = Vec::new();
            let mut errors = Vec::new();

            for target in &targets {
                let params = ResourceParams::new(target);
                match self.handler.resource_start(params).await {
                    Ok(_) => affected.push(target.clone()),
                    Err(e) => errors.push(format!("{}: {}", target, e.message)),
                }
            }

            if errors.is_empty() {
                Ok(
                    ExecuteResult::success(format!("Started: {}", affected.join(", ")))
                        .with_affected(affected.join(", ")),
                )
            } else {
                Err(ExecuteError::Multiple(errors))
            }
        }
    }

    /// Execute stop operation.
    async fn execute_stop(&self, targets: Vec<String>) -> Result<ExecuteResult, ExecuteError> {
        if targets.is_empty() {
            self.handler
                .stop_all()
                .await
                .map_err(|e| ExecuteError::Protocol(e.message))?;

            Ok(ExecuteResult::success("All resources stopped"))
        } else {
            let mut affected = Vec::new();
            let mut errors = Vec::new();

            for target in &targets {
                let params = ResourceParams::new(target);
                match self.handler.resource_stop(params).await {
                    Ok(_) => affected.push(target.clone()),
                    Err(e) => errors.push(format!("{}: {}", target, e.message)),
                }
            }

            if errors.is_empty() {
                Ok(
                    ExecuteResult::success(format!("Stopped: {}", affected.join(", ")))
                        .with_affected(affected.join(", ")),
                )
            } else {
                Err(ExecuteError::Multiple(errors))
            }
        }
    }

    /// Execute restart operation.
    async fn execute_restart(&self, targets: Vec<String>) -> Result<ExecuteResult, ExecuteError> {
        let mut affected = Vec::new();
        let mut errors = Vec::new();

        for target in &targets {
            let params = ResourceParams::new(target);
            match self.handler.resource_restart(params).await {
                Ok(_) => affected.push(target.clone()),
                Err(e) => errors.push(format!("{}: {}", target, e.message)),
            }
        }

        if errors.is_empty() {
            Ok(
                ExecuteResult::success(format!("Restarted: {}", affected.join(", ")))
                    .with_affected(affected.join(", ")),
            )
        } else {
            Err(ExecuteError::Multiple(errors))
        }
    }

    /// Execute status operation.
    async fn execute_status(&self, targets: Vec<String>) -> Result<ExecuteResult, ExecuteError> {
        if targets.is_empty() {
            let list = self
                .handler
                .resource_list()
                .await
                .map_err(|e| ExecuteError::Protocol(e.message))?;

            let summary: Vec<String> = list
                .resources
                .iter()
                .map(|r| format!("{}: {} ({})", r.id, r.state, r.kind))
                .collect();

            Ok(ExecuteResult::success(summary.join("\n"))
                .with_data(serde_json::to_value(&list).unwrap_or_default()))
        } else {
            let mut results = Vec::new();

            for target in &targets {
                let params = ResourceParams::new(target);
                match self.handler.resource_status(params).await {
                    Ok(status) => {
                        results.push(format!("{}: {} ({})", status.id, status.state, status.kind));
                    }
                    Err(e) => {
                        results.push(format!("{}: error - {}", target, e.message));
                    }
                }
            }

            Ok(ExecuteResult::success(results.join("\n")))
        }
    }

    /// Execute logs operation.
    async fn execute_logs(
        &self,
        targets: Vec<String>,
        options: std::collections::HashMap<String, String>,
    ) -> Result<ExecuteResult, ExecuteError> {
        let lines: u32 = options
            .get("lines")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);

        if targets.is_empty() {
            return Err(ExecuteError::Protocol(
                "Please specify which resource's logs to view".to_string(),
            ));
        }

        let target = &targets[0];
        let params = crate::methods::LogsParams::new(target).lines(lines);

        let logs = self
            .handler
            .resource_logs(params)
            .await
            .map_err(|e| ExecuteError::Protocol(e.message))?;

        let text: Vec<String> = logs
            .iter()
            .map(|l| format!("[{}] {} {}", l.timestamp, l.stream, l.line))
            .collect();

        Ok(ExecuteResult::success(text.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_result_builder() {
        let result = ExecuteResult::success("Done")
            .with_affected("api")
            .with_data(serde_json::json!({"count": 1}));

        assert_eq!(result.message, "Done");
        assert!(result.affected.contains(&"api".to_string()));
        assert!(result.data.is_some());
    }

    #[test]
    fn execute_error_display() {
        let err = ExecuteError::UnknownOperation("foo".to_string());
        assert!(err.to_string().contains("foo"));
    }
}
