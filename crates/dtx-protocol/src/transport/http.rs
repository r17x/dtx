//! HTTP transport for web integration.
//!
//! Uses HTTP POST for request/response pattern.

use async_trait::async_trait;
use reqwest::Client;

use super::{Transport, TransportError};
use crate::jsonrpc::{Request, Response};

/// HTTP client transport.
///
/// Sends JSON-RPC requests over HTTP POST.
pub struct HttpTransport {
    client: Client,
    url: String,
}

impl HttpTransport {
    /// Create a new HTTP transport.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            url: url.into(),
        }
    }

    /// Create with a custom reqwest client.
    pub fn with_client(client: Client, url: impl Into<String>) -> Self {
        Self {
            client,
            url: url.into(),
        }
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send(&self, request: Request) -> Result<Response, TransportError> {
        let response = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| TransportError::other(e.to_string()))?;

        if !response.status().is_success() {
            return Err(TransportError::other(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let json_response: Response = response
            .json()
            .await
            .map_err(|e| TransportError::other(e.to_string()))?;

        Ok(json_response)
    }

    async fn notify(&self, request: Request) -> Result<(), TransportError> {
        self.client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| TransportError::other(e.to_string()))?;

        Ok(())
    }

    async fn close(&self) -> Result<(), TransportError> {
        // HTTP is stateless, nothing to close
        Ok(())
    }
}

/// Axum handler integration.
#[cfg(feature = "axum")]
pub mod axum_handler {
    use axum::{response::IntoResponse, Json};

    use crate::jsonrpc::{Request, Response};

    /// Create a JSON-RPC handler for Axum.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use axum::{Router, routing::post};
    /// use dtx_protocol::transport::http::axum_handler::jsonrpc_handler;
    ///
    /// let app = Router::new()
    ///     .route("/rpc", post(|Json(req)| async move {
    ///         jsonrpc_handler(my_handler, req).await
    ///     }));
    /// ```
    pub async fn jsonrpc_handler<H, F>(handler: H, request: Request) -> impl IntoResponse
    where
        H: Fn(Request) -> F + Send + Sync,
        F: std::future::Future<Output = Response> + Send,
    {
        let response = handler(request).await;
        Json(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_transport_new() {
        let transport = HttpTransport::new("http://localhost:3000/rpc");
        assert_eq!(transport.url, "http://localhost:3000/rpc");
    }
}
