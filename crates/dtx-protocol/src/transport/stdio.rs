//! stdio transport for MCP compatibility.
//!
//! Uses newline-delimited JSON over stdin/stdout.

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Stdin, Stdout};

use super::{ServerTransport, TransportError};
use crate::jsonrpc::{Request, Response};

/// stdio transport for MCP servers.
///
/// Reads JSON-RPC requests from stdin and writes responses to stdout.
/// Each message is a single line of JSON.
pub struct StdioTransport {
    reader: BufReader<Stdin>,
    writer: BufWriter<Stdout>,
}

impl StdioTransport {
    /// Create a new stdio transport.
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(tokio::io::stdin()),
            writer: BufWriter::new(tokio::io::stdout()),
        }
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServerTransport for StdioTransport {
    async fn recv(&mut self) -> Result<Request, TransportError> {
        let mut line = String::new();
        let bytes_read = self.reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            return Err(TransportError::Closed);
        }

        let request: Request = serde_json::from_str(line.trim())?;
        Ok(request)
    }

    async fn send(&mut self, response: Response) -> Result<(), TransportError> {
        let json = serde_json::to_string(&response)?;
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    async fn notify(&mut self, request: Request) -> Result<(), TransportError> {
        let json = serde_json::to_string(&request)?;
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }
}

/// Serve requests over stdio using the provided handler.
///
/// This is the main entry point for MCP server mode.
///
/// # Example
///
/// ```ignore
/// use dtx_protocol::transport::serve_stdio;
/// use dtx_protocol::{dispatch, Request, Response};
///
/// serve_stdio(|request| async {
///     dispatch(&my_handler, request).await
/// }).await?;
/// ```
pub async fn serve_stdio<H, F>(handler: H) -> Result<(), TransportError>
where
    H: Fn(Request) -> F + Send + Sync,
    F: std::future::Future<Output = Response> + Send,
{
    let mut transport = StdioTransport::new();

    loop {
        match transport.recv().await {
            Ok(request) => {
                let is_notification = request.is_notification();
                let response = handler(request).await;

                if !is_notification {
                    transport.send(response).await?;
                }
            }
            Err(TransportError::Closed) => {
                // Clean shutdown
                break;
            }
            Err(e) => {
                // Log error to stderr (not stdout to avoid protocol corruption)
                eprintln!("Transport error: {}", e);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_transport_default() {
        // Just verify it compiles - actual I/O testing requires integration tests
        let _transport = StdioTransport::default();
    }
}
