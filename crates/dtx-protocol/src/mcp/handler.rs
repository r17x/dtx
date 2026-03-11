//! MCP handler trait and implementation.
//!
//! Handles MCP-specific requests like initialize, list resources, and call tools.

use async_trait::async_trait;

use crate::handler::ProtocolHandler;
use crate::jsonrpc::ErrorObject;
use crate::methods::{LogsParams, ResourceParams};

use super::resources::{
    uris, DxtUri, ListResourcesResult, ReadResourceParams, ReadResourceResult, Resource,
    ResourceContent,
};
use super::tools::{dtx_tools, CallToolParams, CallToolResult, ListToolsResult};
use super::types::{InitializeParams, InitializeResult, ServerCapabilities, ServerInfo};

/// MCP-specific handler trait.
///
/// Extends the base protocol handler with MCP-specific operations.
#[async_trait]
pub trait McpHandler: Send + Sync {
    /// Handle initialize request.
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult, ErrorObject>;

    /// List available resources.
    async fn list_resources(&self) -> Result<ListResourcesResult, ErrorObject>;

    /// Read a resource by URI.
    async fn read_resource(
        &self,
        params: ReadResourceParams,
    ) -> Result<ReadResourceResult, ErrorObject>;

    /// List available tools.
    async fn list_tools(&self) -> Result<ListToolsResult, ErrorObject>;

    /// Call a tool.
    async fn call_tool(&self, params: CallToolParams) -> Result<CallToolResult, ErrorObject>;
}

/// Default MCP handler wrapping a ProtocolHandler.
pub struct DefaultMcpHandler<H> {
    inner: H,
    server_info: ServerInfo,
    project_id: String,
}

impl<H> DefaultMcpHandler<H> {
    /// Create a new MCP handler.
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            server_info: ServerInfo::default(),
            project_id: "default".to_string(),
        }
    }

    /// Set the project ID.
    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = project_id.into();
        self
    }

    /// Set custom server info.
    pub fn with_server_info(mut self, info: ServerInfo) -> Self {
        self.server_info = info;
        self
    }
}

#[async_trait]
impl<H: ProtocolHandler> McpHandler for DefaultMcpHandler<H> {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult, ErrorObject> {
        Ok(InitializeResult {
            protocol_version: super::types::MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: self.server_info.clone(),
        })
    }

    async fn list_resources(&self) -> Result<ListResourcesResult, ErrorObject> {
        let result = self.inner.resource_list().await?;

        let resources = result
            .resources
            .iter()
            .map(|r| {
                Resource::new(uris::resource(&self.project_id, &r.id), r.id.clone())
                    .with_description(format!("{} ({})", r.kind, r.state))
                    .with_mime_type("application/json")
            })
            .collect();

        Ok(ListResourcesResult { resources })
    }

    async fn read_resource(
        &self,
        params: ReadResourceParams,
    ) -> Result<ReadResourceResult, ErrorObject> {
        let uri =
            uris::parse(&params.uri).ok_or_else(|| ErrorObject::invalid_params("Invalid URI"))?;

        match uri {
            DxtUri::Resource { resource_id, .. } => {
                let status = self
                    .inner
                    .resource_status(ResourceParams::new(&resource_id))
                    .await?;

                Ok(ReadResourceResult {
                    contents: vec![ResourceContent::json(&params.uri, &status)],
                })
            }
            DxtUri::Logs { resource_id, .. } => {
                let logs = self
                    .inner
                    .resource_logs(LogsParams::new(&resource_id).lines(100))
                    .await?;

                let text = logs
                    .iter()
                    .map(|l| format!("[{}] {} {}", l.timestamp, l.stream, l.line))
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(ReadResourceResult {
                    contents: vec![ResourceContent::text(&params.uri, text)],
                })
            }
            DxtUri::Project { .. } => {
                let resources = self.inner.resource_list().await?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContent::json(&params.uri, &resources)],
                })
            }
            DxtUri::Config { .. } => Err(ErrorObject::resource_not_found(
                "Config resource not implemented",
            )),
        }
    }

    async fn list_tools(&self) -> Result<ListToolsResult, ErrorObject> {
        Ok(ListToolsResult { tools: dtx_tools() })
    }

    async fn call_tool(&self, params: CallToolParams) -> Result<CallToolResult, ErrorObject> {
        let get_id = |args: &Option<serde_json::Value>| -> Result<String, ErrorObject> {
            args.as_ref()
                .and_then(|a| a.get("id"))
                .and_then(|v| v.as_str())
                .map(String::from)
                .ok_or_else(|| ErrorObject::invalid_params("Missing id parameter"))
        };

        match params.name.as_str() {
            "start_resource" => {
                let id = get_id(&params.arguments)?;
                self.inner.resource_start(ResourceParams::new(&id)).await?;
                Ok(CallToolResult::text(format!("Resource '{}' started", id)))
            }
            "stop_resource" => {
                let id = get_id(&params.arguments)?;
                self.inner.resource_stop(ResourceParams::new(&id)).await?;
                Ok(CallToolResult::text(format!("Resource '{}' stopped", id)))
            }
            "restart_resource" => {
                let id = get_id(&params.arguments)?;
                self.inner
                    .resource_restart(ResourceParams::new(&id))
                    .await?;
                Ok(CallToolResult::text(format!("Resource '{}' restarted", id)))
            }
            "get_status" => {
                let id = get_id(&params.arguments)?;
                let status = self.inner.resource_status(ResourceParams::new(&id)).await?;
                Ok(CallToolResult::json(&status))
            }
            "list_resources" => {
                let result = self.inner.resource_list().await?;
                Ok(CallToolResult::json(&result))
            }
            "get_logs" => {
                let id = get_id(&params.arguments)?;
                let lines = params
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("lines"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50) as u32;

                let logs = self
                    .inner
                    .resource_logs(LogsParams::new(&id).lines(lines))
                    .await?;

                let text = logs
                    .iter()
                    .map(|l| format!("[{}] {} {}", l.timestamp, l.stream, l.line))
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(CallToolResult::text(text))
            }
            "start_all" => {
                self.inner.start_all().await?;
                Ok(CallToolResult::text("All resources started"))
            }
            "stop_all" => {
                self.inner.stop_all().await?;
                Ok(CallToolResult::text("All resources stopped"))
            }
            _ => Err(ErrorObject::method_not_found(&params.name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::{
        LogEntry, ResourceListResult, ResourceStatusResult, SubscribeParams, SubscribeResult,
    };
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockProtocolHandler {
        call_count: AtomicUsize,
    }

    impl MockProtocolHandler {
        fn new() -> Self {
            Self {
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ProtocolHandler for MockProtocolHandler {
        async fn resource_start(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({}))
        }

        async fn resource_stop(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({}))
        }

        async fn resource_restart(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({}))
        }

        async fn resource_kill(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({}))
        }

        async fn resource_status(
            &self,
            params: ResourceParams,
        ) -> Result<ResourceStatusResult, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
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
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(ResourceListResult {
                resources: vec![ResourceStatusResult {
                    id: "postgres".to_string(),
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
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(vec![LogEntry {
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                stream: "stdout".to_string(),
                line: "Test log line".to_string(),
                level: None,
            }])
        }

        async fn start_all(&self) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"count": 1}))
        }

        async fn stop_all(&self) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
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

    #[tokio::test]
    async fn mcp_initialize() {
        let handler = DefaultMcpHandler::new(MockProtocolHandler::new());
        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: Default::default(),
            client_info: super::super::types::ClientInfo {
                name: "test".to_string(),
                version: "1.0".to_string(),
            },
        };

        let result = handler.initialize(params).await.unwrap();
        assert_eq!(result.server_info.name, "dtx");
    }

    #[tokio::test]
    async fn mcp_list_resources() {
        let handler = DefaultMcpHandler::new(MockProtocolHandler::new());
        let result = handler.list_resources().await.unwrap();
        assert_eq!(result.resources.len(), 1);
        assert!(result.resources[0].uri.contains("postgres"));
    }

    #[tokio::test]
    async fn mcp_list_tools() {
        let handler = DefaultMcpHandler::new(MockProtocolHandler::new());
        let result = handler.list_tools().await.unwrap();
        assert!(!result.tools.is_empty());
    }

    #[tokio::test]
    async fn mcp_call_tool() {
        let mock = MockProtocolHandler::new();
        let handler = DefaultMcpHandler::new(mock);

        let result = handler
            .call_tool(CallToolParams {
                name: "start_resource".to_string(),
                arguments: Some(serde_json::json!({"id": "postgres"})),
            })
            .await
            .unwrap();

        assert!(result.is_error.is_none());
    }
}
