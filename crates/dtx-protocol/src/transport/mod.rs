//! Transport layer for JSON-RPC messages.
//!
//! This module provides transport abstractions for sending and receiving
//! protocol messages over different channels:
//!
//! - **stdio**: Standard input/output for MCP compatibility
//! - **HTTP**: HTTP POST for web integration
//! - **WebSocket**: Full-duplex for real-time communication

mod stdio;

#[cfg(feature = "http")]
mod http;

#[cfg(feature = "websocket")]
mod websocket;

pub use stdio::{serve_stdio, StdioTransport};

#[cfg(feature = "http")]
pub use self::http::HttpTransport;

#[cfg(feature = "websocket")]
pub use self::websocket::WebSocketTransport;

use async_trait::async_trait;

use crate::jsonrpc::{Request, Response};

/// Transport error.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Connection closed.
    #[error("Connection closed")]
    Closed,

    /// Operation timeout.
    #[error("Timeout")]
    Timeout,

    /// Generic transport error.
    #[error("Transport error: {0}")]
    Other(String),
}

impl TransportError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

/// Client-side transport for sending requests.
///
/// Implements request/response and notification patterns.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a request and wait for response.
    async fn send(&self, request: Request) -> Result<Response, TransportError>;

    /// Send a notification (no response expected).
    async fn notify(&self, request: Request) -> Result<(), TransportError>;

    /// Close the transport connection.
    async fn close(&self) -> Result<(), TransportError>;
}

/// Server-side transport for receiving requests.
///
/// Implements the server side of the protocol.
#[async_trait]
pub trait ServerTransport: Send + Sync {
    /// Receive the next request from the client.
    async fn recv(&mut self) -> Result<Request, TransportError>;

    /// Send a response back to the client.
    async fn send(&mut self, response: Response) -> Result<(), TransportError>;

    /// Send a notification to the client.
    async fn notify(&mut self, request: Request) -> Result<(), TransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_error_display() {
        let err = TransportError::Closed;
        assert_eq!(format!("{}", err), "Connection closed");

        let err = TransportError::other("Custom error");
        assert_eq!(format!("{}", err), "Transport error: Custom error");
    }
}
