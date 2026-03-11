//! WebSocket transport for real-time communication.
//!
//! Uses WebSocket for full-duplex messaging.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_tungstenite::tungstenite::Message;

use super::{Transport, TransportError};
use crate::jsonrpc::{Request, RequestId, Response};

/// WebSocket client transport.
///
/// Provides full-duplex communication with request/response correlation.
pub struct WebSocketTransport {
    sender: mpsc::Sender<String>,
    pending: Arc<RwLock<HashMap<RequestId, tokio::sync::oneshot::Sender<Response>>>>,
    receiver: Arc<Mutex<mpsc::Receiver<String>>>,
}

impl WebSocketTransport {
    /// Connect to a WebSocket server.
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| TransportError::other(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();

        // Outgoing message channel
        let (tx_out, mut rx_out) = mpsc::channel::<String>(32);

        // Incoming message channel
        let (tx_in, rx_in) = mpsc::channel::<String>(32);

        // Pending requests waiting for responses
        let pending: Arc<RwLock<HashMap<RequestId, tokio::sync::oneshot::Sender<Response>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Writer task
        tokio::spawn(async move {
            while let Some(msg) = rx_out.recv().await {
                if write.send(Message::Text(msg)).await.is_err() {
                    break;
                }
            }
        });

        // Reader task
        let pending_clone = pending.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg {
                    // Try to parse as response first
                    if let Ok(response) = serde_json::from_str::<Response>(&text) {
                        if let Some(id) = &response.id {
                            if let Some(sender) = pending_clone.write().await.remove(id) {
                                let _ = sender.send(response);
                                continue;
                            }
                        }
                    }

                    // Otherwise forward to receiver (for notifications/requests)
                    if tx_in.send(text).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            sender: tx_out,
            pending,
            receiver: Arc::new(Mutex::new(rx_in)),
        })
    }

    /// Receive incoming messages (notifications or server requests).
    pub async fn recv(&self) -> Result<String, TransportError> {
        self.receiver
            .lock()
            .await
            .recv()
            .await
            .ok_or(TransportError::Closed)
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send(&self, request: Request) -> Result<Response, TransportError> {
        let id = request
            .id
            .clone()
            .ok_or_else(|| TransportError::other("Request must have an id"))?;

        // Create oneshot channel for response
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.write().await.insert(id, tx);

        // Send request
        let json = serde_json::to_string(&request)?;
        self.sender
            .send(json)
            .await
            .map_err(|_| TransportError::Closed)?;

        // Wait for response
        rx.await.map_err(|_| TransportError::Closed)
    }

    async fn notify(&self, request: Request) -> Result<(), TransportError> {
        let json = serde_json::to_string(&request)?;
        self.sender
            .send(json)
            .await
            .map_err(|_| TransportError::Closed)?;
        Ok(())
    }

    async fn close(&self) -> Result<(), TransportError> {
        // Dropping sender will close the connection
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // WebSocket tests require a running server, so they're integration tests
}
