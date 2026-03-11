//! Operation and Response types for middleware processing.

use serde::{Deserialize, Serialize};

use crate::resource::ResourceId;

/// Operations that can be performed on resources.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    // Lifecycle operations
    /// Start a resource.
    Start { id: ResourceId },
    /// Stop a resource.
    Stop { id: ResourceId },
    /// Restart a resource.
    Restart { id: ResourceId },
    /// Force kill a resource.
    Kill { id: ResourceId },

    // Query operations
    /// Get resource status.
    Status { id: ResourceId },
    /// Check resource health.
    Health { id: ResourceId },
    /// Get resource logs.
    Logs { id: ResourceId, follow: bool },

    // Batch operations
    /// Start all resources.
    StartAll,
    /// Stop all resources.
    StopAll,

    // Configuration
    /// Configure a resource.
    Configure {
        id: ResourceId,
        config: serde_json::Value,
    },

    // Custom operation
    /// Custom operation defined by plugins.
    Custom {
        name: String,
        params: serde_json::Value,
    },
}

impl Operation {
    /// Get the resource ID if this operation targets a specific resource.
    pub fn resource_id(&self) -> Option<&ResourceId> {
        match self {
            Self::Start { id }
            | Self::Stop { id }
            | Self::Restart { id }
            | Self::Kill { id }
            | Self::Status { id }
            | Self::Health { id }
            | Self::Logs { id, .. }
            | Self::Configure { id, .. } => Some(id),
            Self::StartAll | Self::StopAll | Self::Custom { .. } => None,
        }
    }

    /// Get the operation name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Start { .. } => "start",
            Self::Stop { .. } => "stop",
            Self::Restart { .. } => "restart",
            Self::Kill { .. } => "kill",
            Self::Status { .. } => "status",
            Self::Health { .. } => "health",
            Self::Logs { .. } => "logs",
            Self::StartAll => "start_all",
            Self::StopAll => "stop_all",
            Self::Configure { .. } => "configure",
            Self::Custom { .. } => "custom",
        }
    }

    /// Check if this is a mutating operation.
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::Start { .. }
                | Self::Stop { .. }
                | Self::Restart { .. }
                | Self::Kill { .. }
                | Self::StartAll
                | Self::StopAll
                | Self::Configure { .. }
        )
    }

    /// Check if this is a query operation.
    pub fn is_query(&self) -> bool {
        !self.is_mutating()
    }
}

/// Response from an operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Operation completed successfully.
    Ok,

    /// Operation completed with data.
    Data { data: serde_json::Value },

    /// Resource status response.
    Status {
        id: ResourceId,
        state: String,
        pid: Option<u32>,
        healthy: Option<bool>,
    },

    /// Stream of events (for logs).
    Stream { stream_id: String },

    /// Error response.
    Error { message: String },
}

impl Response {
    /// Create a success response.
    pub fn ok() -> Self {
        Self::Ok
    }

    /// Create a data response.
    pub fn data(data: impl Serialize) -> Self {
        Self::Data {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        }
    }

    /// Create an error response.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    /// Create a status response.
    pub fn status(
        id: ResourceId,
        state: impl Into<String>,
        pid: Option<u32>,
        healthy: Option<bool>,
    ) -> Self {
        Self::Status {
            id,
            state: state.into(),
            pid,
            healthy,
        }
    }

    /// Create a stream response.
    pub fn stream(stream_id: impl Into<String>) -> Self {
        Self::Stream {
            stream_id: stream_id.into(),
        }
    }

    /// Check if this is an error response.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Check if this is a success response.
    pub fn is_ok(&self) -> bool {
        !self.is_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_resource_id() {
        let op = Operation::Start {
            id: ResourceId::new("api"),
        };
        assert_eq!(op.resource_id().map(|id| id.as_str()), Some("api"));

        let op = Operation::StartAll;
        assert!(op.resource_id().is_none());
    }

    #[test]
    fn operation_name() {
        assert_eq!(
            Operation::Start {
                id: ResourceId::new("x")
            }
            .name(),
            "start"
        );
        assert_eq!(Operation::StartAll.name(), "start_all");
        assert_eq!(
            Operation::Custom {
                name: "foo".into(),
                params: serde_json::Value::Null
            }
            .name(),
            "custom"
        );
    }

    #[test]
    fn operation_is_mutating() {
        assert!(Operation::Start {
            id: ResourceId::new("x")
        }
        .is_mutating());
        assert!(Operation::StartAll.is_mutating());
        assert!(!Operation::Status {
            id: ResourceId::new("x")
        }
        .is_mutating());
        assert!(!Operation::Health {
            id: ResourceId::new("x")
        }
        .is_mutating());
    }

    #[test]
    fn operation_serde() {
        let op = Operation::Start {
            id: ResourceId::new("api"),
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("start"));

        let parsed: Operation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name(), "start");
    }

    #[test]
    fn response_ok() {
        let resp = Response::ok();
        assert!(resp.is_ok());
        assert!(!resp.is_error());
    }

    #[test]
    fn response_error() {
        let resp = Response::error("something failed");
        assert!(resp.is_error());
        assert!(!resp.is_ok());
    }

    #[test]
    fn response_data() {
        let resp = Response::data(vec![1, 2, 3]);
        assert!(resp.is_ok());
        if let Response::Data { data } = resp {
            assert!(data.is_array());
        } else {
            panic!("Expected Data response");
        }
    }

    #[test]
    fn response_serde() {
        let resp = Response::status(ResourceId::new("api"), "running", Some(1234), Some(true));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("running"));
        assert!(json.contains("1234"));
    }
}
