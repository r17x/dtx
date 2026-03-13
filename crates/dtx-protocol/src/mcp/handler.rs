//! MCP handler trait and implementation.
//!
//! Handles MCP-specific requests like initialize, list resources, and call tools.

#[cfg(any(feature = "code", feature = "memory"))]
use std::sync::Arc;

use async_trait::async_trait;

use crate::handler::ProtocolHandler;
use crate::jsonrpc::ErrorObject;
use crate::methods::{LogsParams, ResourceParams};

use super::resources::{
    uris, DtxUri, ListResourcesResult, ReadResourceParams, ReadResourceResult, Resource,
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
    #[cfg(feature = "code")]
    code: Option<Arc<dtx_code::WorkspaceIndex>>,
    #[cfg(feature = "memory")]
    memory: Option<Arc<dtx_memory::MemoryStore>>,
}

impl<H> DefaultMcpHandler<H> {
    /// Create a new MCP handler.
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            server_info: ServerInfo::default(),
            project_id: "default".to_string(),
            #[cfg(feature = "code")]
            code: None,
            #[cfg(feature = "memory")]
            memory: None,
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

    /// Set code intelligence backend.
    #[cfg(feature = "code")]
    pub fn with_code(mut self, index: Arc<dtx_code::WorkspaceIndex>) -> Self {
        self.code = Some(index);
        self
    }

    /// Set memory store backend.
    #[cfg(feature = "memory")]
    pub fn with_memory(mut self, store: Arc<dtx_memory::MemoryStore>) -> Self {
        self.memory = Some(store);
        self
    }

    #[cfg(feature = "code")]
    fn require_code(&self) -> Result<&Arc<dtx_code::WorkspaceIndex>, ErrorObject> {
        self.code
            .as_ref()
            .ok_or_else(|| ErrorObject::internal_error("Code intelligence not configured"))
    }

    #[cfg(feature = "memory")]
    fn require_memory(&self) -> Result<&Arc<dtx_memory::MemoryStore>, ErrorObject> {
        self.memory
            .as_ref()
            .ok_or_else(|| ErrorObject::internal_error("Memory store not configured"))
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

        #[allow(unused_mut)]
        let mut resources: Vec<Resource> = result
            .resources
            .iter()
            .map(|r| {
                Resource::new(uris::resource(&self.project_id, &r.id), r.id.clone())
                    .with_description(format!("{} ({})", r.kind, r.state))
                    .with_mime_type("application/json")
            })
            .collect();

        #[cfg(feature = "memory")]
        if let Some(mem) = &self.memory {
            resources.push(
                Resource::new(uris::memory_list(), "memories")
                    .with_description("All project memories")
                    .with_mime_type("application/json"),
            );
            if let Ok(metas) = mem.list() {
                for m in &metas {
                    resources.push(
                        Resource::new(uris::memory_item(&m.name), &m.name)
                            .with_description(
                                m.description
                                    .as_deref()
                                    .unwrap_or("Memory entry")
                                    .to_string(),
                            )
                            .with_mime_type("text/markdown"),
                    );
                }
            }
        }

        #[cfg(feature = "code")]
        if let Some(code) = &self.code {
            for file in code.list_files() {
                let path_str = file.to_string_lossy();
                resources.push(
                    Resource::new(uris::code_symbols(&path_str), format!("symbols:{path_str}"))
                        .with_description(format!("Code symbols in {path_str}"))
                        .with_mime_type("application/json"),
                );
            }
        }

        Ok(ListResourcesResult { resources })
    }

    async fn read_resource(
        &self,
        params: ReadResourceParams,
    ) -> Result<ReadResourceResult, ErrorObject> {
        let uri =
            uris::parse(&params.uri).ok_or_else(|| ErrorObject::invalid_params("Invalid URI"))?;

        match uri {
            DtxUri::Resource { resource_id, .. } => {
                let status = self
                    .inner
                    .resource_status(ResourceParams::new(&resource_id))
                    .await?;

                Ok(ReadResourceResult {
                    contents: vec![ResourceContent::json(&params.uri, &status)],
                })
            }
            DtxUri::Logs { resource_id, .. } => {
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
            DtxUri::Project { .. } => {
                let resources = self.inner.resource_list().await?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContent::json(&params.uri, &resources)],
                })
            }
            DtxUri::Config { .. } => Err(ErrorObject::resource_not_found(
                "Config resource not implemented",
            )),

            DtxUri::MemoryList => {
                #[cfg(feature = "memory")]
                {
                    let mem = self.require_memory()?;
                    let metas = mem
                        .list()
                        .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContent::json(&params.uri, &metas)],
                    })
                }
                #[cfg(not(feature = "memory"))]
                Err(ErrorObject::internal_error("Memory feature not enabled"))
            }
            DtxUri::MemoryItem { name } => {
                #[cfg(feature = "memory")]
                {
                    let mem = self.require_memory()?;
                    let memory = mem
                        .read(&name)
                        .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContent {
                            uri: params.uri,
                            mime_type: Some("text/markdown".to_string()),
                            content: super::resources::ResourceContentType::Text {
                                text: memory.to_file_content(),
                            },
                        }],
                    })
                }
                #[cfg(not(feature = "memory"))]
                {
                    let _ = name;
                    Err(ErrorObject::internal_error("Memory feature not enabled"))
                }
            }
            DtxUri::CodeSymbols { path } => {
                #[cfg(feature = "code")]
                {
                    let code = self.require_code()?;
                    let overview = code
                        .get_overview(std::path::Path::new(&path), None)
                        .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContent::json(&params.uri, &overview)],
                    })
                }
                #[cfg(not(feature = "code"))]
                {
                    let _ = path;
                    Err(ErrorObject::internal_error("Code feature not enabled"))
                }
            }
            DtxUri::CodeSymbol { path, name_path } => {
                #[cfg(feature = "code")]
                {
                    let code = self.require_code()?;
                    let matches = code
                        .find_symbol(
                            &name_path,
                            Some(std::path::Path::new(&path)),
                            None,
                            true,
                            None,
                        )
                        .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContent::json(&params.uri, &matches)],
                    })
                }
                #[cfg(not(feature = "code"))]
                {
                    let _ = (path, name_path);
                    Err(ErrorObject::internal_error("Code feature not enabled"))
                }
            }
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

            // Code intelligence tools
            #[cfg(feature = "code")]
            "get_symbols_overview"
            | "find_symbol"
            | "find_references"
            | "search_pattern"
            | "replace_symbol_body"
            | "insert_before_symbol"
            | "insert_after_symbol" => {
                let code = self.require_code()?;
                handle_code_tool(code, &params.name, &params.arguments)
            }

            // Memory tools
            #[cfg(feature = "memory")]
            "list_memories" | "read_memory" | "write_memory" | "edit_memory" | "delete_memory" => {
                let mem = self.require_memory()?;
                handle_memory_tool(mem, &params.name, &params.arguments)
            }

            _ => Err(ErrorObject::method_not_found(&params.name)),
        }
    }
}

#[cfg(feature = "code")]
fn handle_code_tool(
    code: &Arc<dtx_code::WorkspaceIndex>,
    name: &str,
    args: &Option<serde_json::Value>,
) -> Result<CallToolResult, ErrorObject> {
    let args = args
        .as_ref()
        .ok_or_else(|| ErrorObject::invalid_params("Missing arguments"))?;

    match name {
        "get_symbols_overview" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing path"))?;
            let depth = args
                .get("depth")
                .and_then(|v| v.as_u64())
                .map(|d| d as usize);
            let overview = code
                .get_overview(std::path::Path::new(path), depth)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::json(&overview))
        }
        "find_symbol" => {
            let pattern = args
                .get("name_path_pattern")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name_path_pattern"))?;
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(std::path::Path::new);
            let depth = args
                .get("depth")
                .and_then(|v| v.as_u64())
                .map(|d| d as usize);
            let include_body = args
                .get("include_body")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let matches = code
                .find_symbol(pattern, path, depth, include_body, None)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::json(&matches))
        }
        "find_references" => {
            let symbol_name = args
                .get("symbol_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing symbol_name"))?;
            let scope = args
                .get("scope_path")
                .and_then(|v| v.as_str())
                .map(std::path::Path::new);
            let refs = dtx_code::find_references(code.root(), symbol_name, scope)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::json(&refs))
        }
        "search_pattern" => {
            let pattern = args
                .get("pattern")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing pattern"))?;
            let glob = args.get("glob").and_then(|v| v.as_str());
            let context = args
                .get("context_lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(2) as usize;
            let matches = dtx_code::search_pattern(code.root(), pattern, glob, context)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::json(&matches))
        }
        "replace_symbol_body" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing path"))?;
            let name_path = args
                .get("name_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name_path"))?;
            let new_body = args
                .get("new_body")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing new_body"))?;
            dtx_code::replace_symbol_body(code, std::path::Path::new(path), name_path, new_body)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text("Symbol body replaced"))
        }
        "insert_before_symbol" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing path"))?;
            let name_path = args
                .get("name_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name_path"))?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing content"))?;
            dtx_code::insert_before_symbol(code, std::path::Path::new(path), name_path, content)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text("Content inserted before symbol"))
        }
        "insert_after_symbol" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing path"))?;
            let name_path = args
                .get("name_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name_path"))?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing content"))?;
            dtx_code::insert_after_symbol(code, std::path::Path::new(path), name_path, content)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text("Content inserted after symbol"))
        }
        _ => Err(ErrorObject::method_not_found(name)),
    }
}

#[cfg(feature = "memory")]
fn handle_memory_tool(
    store: &Arc<dtx_memory::MemoryStore>,
    name: &str,
    args: &Option<serde_json::Value>,
) -> Result<CallToolResult, ErrorObject> {
    match name {
        "list_memories" => {
            let kind_filter = args
                .as_ref()
                .and_then(|a| a.get("kind"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<dtx_memory::MemoryKind>().ok());

            let mut metas = store
                .list()
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;

            if let Some(kind) = kind_filter {
                metas.retain(|m| m.kind == kind);
            }
            Ok(CallToolResult::json(&metas))
        }
        "read_memory" => {
            let args = args
                .as_ref()
                .ok_or_else(|| ErrorObject::invalid_params("Missing arguments"))?;
            let mem_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name"))?;
            let memory = store
                .read(mem_name)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text(memory.to_file_content()))
        }
        "write_memory" => {
            let args = args
                .as_ref()
                .ok_or_else(|| ErrorObject::invalid_params("Missing arguments"))?;
            let mem_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name"))?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing content"))?;
            let kind: dtx_memory::MemoryKind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(dtx_memory::MemoryKind::Project);
            let description = args
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from);
            let tags: Vec<String> = args
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let now = chrono::Utc::now();
            let memory = dtx_memory::Memory {
                meta: dtx_memory::MemoryMeta {
                    name: mem_name.to_string(),
                    kind,
                    description,
                    created_at: now,
                    updated_at: now,
                    tags,
                },
                content: content.to_string(),
            };
            store
                .write(&memory)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text(format!("Memory '{mem_name}' written")))
        }
        "edit_memory" => {
            let args = args
                .as_ref()
                .ok_or_else(|| ErrorObject::invalid_params("Missing arguments"))?;
            let mem_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name"))?;

            let mut memory = store
                .read(mem_name)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;

            if let Some(content) = args.get("content").and_then(|v| v.as_str()) {
                memory.content = content.to_string();
            }
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                memory.meta.description = Some(desc.to_string());
            }
            if let Some(tags) = args.get("tags").and_then(|v| v.as_array()) {
                memory.meta.tags = tags
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
            memory.meta.updated_at = chrono::Utc::now();

            store
                .write(&memory)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text(format!("Memory '{mem_name}' updated")))
        }
        "delete_memory" => {
            let args = args
                .as_ref()
                .ok_or_else(|| ErrorObject::invalid_params("Missing arguments"))?;
            let mem_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorObject::invalid_params("Missing name"))?;
            store
                .delete(mem_name)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text(format!("Memory '{mem_name}' deleted")))
        }
        _ => Err(ErrorObject::method_not_found(name)),
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
