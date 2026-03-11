//! JSON-RPC 2.0 message types.
//!
//! This module implements the core JSON-RPC 2.0 specification:
//! - Request objects with method, params, and optional id
//! - Response objects with result or error
//! - Error objects with code, message, and optional data
//! - Notification support (requests without id)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC version string.
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC Request.
///
/// A request object with method name, optional parameters, and optional id.
/// If `id` is `None`, this is a notification (no response expected).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// Method name to invoke.
    pub method: String,

    /// Optional method parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,

    /// Optional request id. If None, this is a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
}

impl Request {
    /// Create a new request with a random numeric id.
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
            id: Some(RequestId::Number(rand::random::<u32>() as i64)),
        }
    }

    /// Create a new request with a specific id.
    pub fn with_id(method: impl Into<String>, params: Option<Value>, id: RequestId) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
            id: Some(id),
        }
    }

    /// Create a notification (no response expected).
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
            id: None,
        }
    }

    /// Check if this request is a notification.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// JSON-RPC Response.
///
/// A response object with either a result or an error, and the request id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// Result value on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Error object on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,

    /// Request id (null for notifications).
    pub id: Option<RequestId>,
}

impl Response {
    /// Create a success response.
    pub fn success(id: Option<RequestId>, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response.
    pub fn error(id: Option<RequestId>, error: ErrorObject) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }

    /// Check if this response is an error.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Check if this response is successful.
    pub fn is_success(&self) -> bool {
        self.result.is_some()
    }
}

/// Request ID (can be string or number per JSON-RPC spec).
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    /// Numeric id.
    Number(i64),
    /// String id.
    String(String),
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for RequestId {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{}", n),
            Self::String(s) => write!(f, "{}", s),
        }
    }
}

/// JSON-RPC Error object.
///
/// Contains an error code, message, and optional additional data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorObject {
    /// Error code (see `error_codes` module).
    pub code: i32,

    /// Human-readable error message.
    pub message: String,

    /// Optional additional error data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorObject {
    /// Create a new error object.
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Add data to the error object.
    pub fn with_data(mut self, data: impl Serialize) -> Self {
        self.data = serde_json::to_value(data).ok();
        self
    }

    // Standard error constructors

    /// Parse error (-32700).
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(error_codes::PARSE_ERROR, message)
    }

    /// Invalid request (-32600).
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(error_codes::INVALID_REQUEST, message)
    }

    /// Method not found (-32601).
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {}", method),
        )
    }

    /// Invalid params (-32602).
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(error_codes::INVALID_PARAMS, message)
    }

    /// Internal error (-32603).
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(error_codes::INTERNAL_ERROR, message)
    }

    // dtx custom error constructors

    /// Resource not found (-32000).
    pub fn resource_not_found(resource_id: &str) -> Self {
        Self::new(
            error_codes::RESOURCE_NOT_FOUND,
            format!("Resource not found: {}", resource_id),
        )
    }

    /// Resource already exists (-32001).
    pub fn resource_exists(resource_id: &str) -> Self {
        Self::new(
            error_codes::RESOURCE_EXISTS,
            format!("Resource already exists: {}", resource_id),
        )
    }

    /// Invalid state for operation (-32002).
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(error_codes::INVALID_STATE, message)
    }

    /// Operation failed (-32003).
    pub fn operation_failed(message: impl Into<String>) -> Self {
        Self::new(error_codes::OPERATION_FAILED, message)
    }

    /// Timeout (-32004).
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(error_codes::TIMEOUT, message)
    }

    /// Cancelled (-32005).
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(error_codes::CANCELLED, message)
    }
}

impl std::fmt::Display for ErrorObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for ErrorObject {}

/// Standard JSON-RPC and dtx custom error codes.
pub mod error_codes {
    // Standard JSON-RPC error codes

    /// Parse error: Invalid JSON.
    pub const PARSE_ERROR: i32 = -32700;

    /// Invalid request: Not a valid JSON-RPC request.
    pub const INVALID_REQUEST: i32 = -32600;

    /// Method not found.
    pub const METHOD_NOT_FOUND: i32 = -32601;

    /// Invalid params.
    pub const INVALID_PARAMS: i32 = -32602;

    /// Internal error.
    pub const INTERNAL_ERROR: i32 = -32603;

    // Server error range: -32000 to -32099

    /// Resource not found.
    pub const RESOURCE_NOT_FOUND: i32 = -32000;

    /// Resource already exists.
    pub const RESOURCE_EXISTS: i32 = -32001;

    /// Invalid state for operation.
    pub const INVALID_STATE: i32 = -32002;

    /// Operation failed.
    pub const OPERATION_FAILED: i32 = -32003;

    /// Timeout.
    pub const TIMEOUT: i32 = -32004;

    /// Operation cancelled.
    pub const CANCELLED: i32 = -32005;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization() {
        let request = Request::with_id(
            "resource/start",
            Some(serde_json::json!({"id": "postgres"})),
            RequestId::Number(1),
        );

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"resource/start\""));
        assert!(json.contains("\"id\":1"));

        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.method, "resource/start");
        assert_eq!(parsed.id, Some(RequestId::Number(1)));
    }

    #[test]
    fn notification_has_no_id() {
        let notification = Request::notification("events/notify", None);
        assert!(notification.is_notification());

        let json = serde_json::to_string(&notification).unwrap();
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn response_success() {
        let response = Response::success(
            Some(RequestId::Number(1)),
            serde_json::json!({"status": "running"}),
        );

        assert!(response.is_success());
        assert!(!response.is_error());

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn response_error() {
        let response = Response::error(
            Some(RequestId::Number(1)),
            ErrorObject::resource_not_found("postgres"),
        );

        assert!(response.is_error());
        assert!(!response.is_success());

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32000"));
    }

    #[test]
    fn request_id_types() {
        let numeric: RequestId = 42.into();
        let string: RequestId = "test-id".into();

        assert_eq!(numeric, RequestId::Number(42));
        assert_eq!(string, RequestId::String("test-id".to_string()));
    }

    #[test]
    fn error_with_data() {
        let error = ErrorObject::invalid_params("Missing required field")
            .with_data(serde_json::json!({"field": "id"}));

        assert!(error.data.is_some());
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"data\""));
        assert!(json.contains("\"field\""));
    }
}
