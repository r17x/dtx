//! MCP server command for AI agent integration.

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, info};

use dtx_core::config::project::find_project_root_cwd;
use dtx_protocol::mcp::{DefaultMcpHandler, McpHandler};
use dtx_protocol::{ErrorObject, Request, Response};

/// Arguments for the MCP command.
pub struct McpArgs {
    /// Project directory.
    pub project: Option<String>,
}

impl McpArgs {
    /// Create from environment variables.
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        Self {
            project: std::env::var("DTX_PROJECT").ok(),
        }
    }
}

/// Run the MCP server over stdio.
pub async fn run(args: McpArgs) -> Result<()> {
    info!("Starting dtx MCP server");

    // Discover project root
    let project_root = args
        .project
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(find_project_root_cwd)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

    info!(root = %project_root.display(), "Using project root");

    // Create real backends
    let code_index = Arc::new(dtx_code::WorkspaceIndex::new(project_root.clone()));
    let memory_store = Arc::new(
        dtx_memory::MemoryStore::new(project_root.join(".dtx").join("memories")).unwrap_or_else(
            |e| {
                tracing::warn!("Failed to init memory store: {e}, using fallback");
                dtx_memory::MemoryStore::new(std::env::temp_dir().join("dtx-memories"))
                    .expect("fallback memory store")
            },
        ),
    );

    let handler = MockProtocolHandler::new();
    let mcp_handler = DefaultMcpHandler::new(handler)
        .with_code(code_index)
        .with_memory(memory_store);

    // Read JSON-RPC requests from stdin and write responses to stdout
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if l.is_empty() => continue,
            Ok(l) => l,
            Err(e) => {
                debug!(error = %e, "Error reading stdin");
                break;
            }
        };

        debug!(request = %line, "Received request");

        // Parse request
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let error_response = Response::error(
                    None,
                    ErrorObject::parse_error(format!("Invalid JSON: {}", e)),
                );
                let json = serde_json::to_string(&error_response)?;
                writeln!(stdout_lock, "{}", json)?;
                stdout_lock.flush()?;
                continue;
            }
        };

        // Dispatch to handler
        let response = dispatch_mcp(&mcp_handler, request).await;

        // Write response
        let json = serde_json::to_string(&response)?;
        debug!(response = %json, "Sending response");
        writeln!(stdout_lock, "{}", json)?;
        stdout_lock.flush()?;
    }

    info!("MCP server shutting down");
    Ok(())
}

/// Dispatch an MCP request to the handler.
async fn dispatch_mcp<H: McpHandler>(handler: &H, request: Request) -> Response {
    let id = request.id.clone();

    match request.method.as_str() {
        "initialize" => {
            let params: Result<dtx_protocol::InitializeParams, _> = request
                .params
                .as_ref()
                .map(|p| serde_json::from_value(p.clone()))
                .unwrap_or(Ok(dtx_protocol::InitializeParams {
                    protocol_version: "2024-11-05".to_string(),
                    capabilities: Default::default(),
                    client_info: dtx_protocol::ClientInfo {
                        name: "unknown".to_string(),
                        version: "0.0.0".to_string(),
                    },
                }));

            match params {
                Ok(params) => match handler.initialize(params).await {
                    Ok(result) => Response::success(id, serde_json::to_value(result).unwrap()),
                    Err(e) => Response::error(id, e),
                },
                Err(_) => Response::error(id, ErrorObject::invalid_params("Invalid parameters")),
            }
        }
        "resources/list" => match handler.list_resources().await {
            Ok(result) => Response::success(id, serde_json::to_value(result).unwrap()),
            Err(e) => Response::error(id, e),
        },
        "resources/read" => {
            let params: Result<dtx_protocol::ReadResourceParams, _> = request
                .params
                .as_ref()
                .map(|p| serde_json::from_value(p.clone()))
                .transpose()
                .unwrap_or(None)
                .ok_or(());

            match params {
                Ok(params) => match handler.read_resource(params).await {
                    Ok(result) => Response::success(id, serde_json::to_value(result).unwrap()),
                    Err(e) => Response::error(id, e),
                },
                Err(_) => Response::error(id, ErrorObject::invalid_params("Invalid parameters")),
            }
        }
        "tools/list" => match handler.list_tools().await {
            Ok(result) => Response::success(id, serde_json::to_value(result).unwrap()),
            Err(e) => Response::error(id, e),
        },
        "tools/call" => {
            let params: Result<dtx_protocol::CallToolParams, _> = request
                .params
                .as_ref()
                .map(|p| serde_json::from_value(p.clone()))
                .transpose()
                .unwrap_or(None)
                .ok_or(());

            match params {
                Ok(params) => match handler.call_tool(params).await {
                    Ok(result) => Response::success(id, serde_json::to_value(result).unwrap()),
                    Err(e) => Response::error(id, e),
                },
                Err(_) => Response::error(id, ErrorObject::invalid_params("Invalid parameters")),
            }
        }
        method => Response::error(id, ErrorObject::method_not_found(method)),
    }
}

// Mock protocol handler for testing
use async_trait::async_trait;
use dtx_protocol::handler::ProtocolHandler;
use dtx_protocol::methods::{
    LogEntry, LogsParams, ResourceListResult, ResourceParams, ResourceStatusResult,
    SubscribeParams, SubscribeResult,
};
use serde_json::Value;

struct MockProtocolHandler;

impl MockProtocolHandler {
    fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProtocolHandler for MockProtocolHandler {
    async fn resource_start(&self, params: ResourceParams) -> Result<Value, ErrorObject> {
        info!(id = %params.id, "Starting resource");
        Ok(serde_json::json!({"status": "started"}))
    }

    async fn resource_stop(&self, params: ResourceParams) -> Result<Value, ErrorObject> {
        info!(id = %params.id, "Stopping resource");
        Ok(serde_json::json!({"status": "stopped"}))
    }

    async fn resource_restart(&self, params: ResourceParams) -> Result<Value, ErrorObject> {
        info!(id = %params.id, "Restarting resource");
        Ok(serde_json::json!({"status": "restarted"}))
    }

    async fn resource_kill(&self, params: ResourceParams) -> Result<Value, ErrorObject> {
        info!(id = %params.id, "Killing resource");
        Ok(serde_json::json!({"status": "killed"}))
    }

    async fn resource_status(
        &self,
        params: ResourceParams,
    ) -> Result<ResourceStatusResult, ErrorObject> {
        Ok(ResourceStatusResult {
            id: params.id,
            kind: "process".to_string(),
            state: "running".to_string(),
            pid: Some(1234),
            healthy: Some(true),
            started_at: None,
            stopped_at: None,
            exit_code: None,
        })
    }

    async fn resource_list(&self) -> Result<ResourceListResult, ErrorObject> {
        Ok(ResourceListResult {
            resources: vec![ResourceStatusResult {
                id: "api".to_string(),
                kind: "process".to_string(),
                state: "running".to_string(),
                pid: Some(1234),
                healthy: Some(true),
                started_at: None,
                stopped_at: None,
                exit_code: None,
            }],
        })
    }

    async fn resource_logs(&self, _params: LogsParams) -> Result<Vec<LogEntry>, ErrorObject> {
        Ok(vec![LogEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            stream: "stdout".to_string(),
            line: "Sample log line".to_string(),
            level: None,
        }])
    }

    async fn start_all(&self) -> Result<Value, ErrorObject> {
        info!("Starting all resources");
        Ok(serde_json::json!({"count": 1}))
    }

    async fn stop_all(&self) -> Result<Value, ErrorObject> {
        info!("Stopping all resources");
        Ok(serde_json::json!({"count": 1}))
    }

    async fn events_subscribe(
        &self,
        _params: SubscribeParams,
    ) -> Result<SubscribeResult, ErrorObject> {
        Ok(SubscribeResult {
            subscription_id: "sub-1".to_string(),
        })
    }

    async fn events_unsubscribe(&self, _subscription_id: String) -> Result<Value, ErrorObject> {
        Ok(serde_json::json!({}))
    }
}
