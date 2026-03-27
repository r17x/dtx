//! MCP handler trait and implementation.
//!
//! Handles MCP-specific requests like initialize, list resources, and call tools.

#[cfg(any(feature = "code", feature = "memory", feature = "graph"))]
use std::sync::Arc;

use async_trait::async_trait;

#[cfg(any(feature = "code", feature = "memory", feature = "graph"))]
mod args {
    use crate::jsonrpc::ErrorObject;

    pub fn require(v: &Option<serde_json::Value>) -> Result<&serde_json::Value, ErrorObject> {
        v.as_ref()
            .ok_or_else(|| ErrorObject::invalid_params("Missing arguments"))
    }

    pub fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, ErrorObject> {
        args.get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| ErrorObject::invalid_params(format!("Missing {key}")))
    }

    pub fn optional_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
        args.get(key).and_then(|v| v.as_str())
    }

    pub fn optional_usize(args: &serde_json::Value, key: &str) -> Option<usize> {
        args.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
    }

    pub fn require_usize(args: &serde_json::Value, key: &str) -> Result<usize, ErrorObject> {
        args.get(key)
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .ok_or_else(|| ErrorObject::invalid_params(format!("Missing {key}")))
    }

    pub fn optional_tags(args: &serde_json::Value, key: &str) -> Vec<String> {
        match args.get(key) {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            _ => vec![],
        }
    }

    pub fn optional_bool(args: &serde_json::Value, key: &str, default: bool) -> bool {
        args.get(key)
            .and_then(|v| {
                v.as_bool().or_else(|| match v.as_str() {
                    Some("true") => Some(true),
                    Some("false") => Some(false),
                    _ => None,
                })
            })
            .unwrap_or(default)
    }
}

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
    initialized: std::sync::atomic::AtomicBool,
    #[cfg(feature = "code")]
    code: Option<Arc<dtx_code::WorkspaceIndex>>,
    #[cfg(feature = "memory")]
    memory: Option<Arc<dtx_memory::MemoryStore>>,
    #[cfg(feature = "graph")]
    graph_builder: Option<Arc<dyn Fn() -> dtx_core::DependencyGraph + Send + Sync>>,
}

impl<H> DefaultMcpHandler<H> {
    /// Create a new MCP handler.
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            server_info: ServerInfo::default(),
            project_id: "default".to_string(),
            initialized: std::sync::atomic::AtomicBool::new(false),
            #[cfg(feature = "code")]
            code: None,
            #[cfg(feature = "memory")]
            memory: None,
            #[cfg(feature = "graph")]
            graph_builder: None,
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

    /// Set graph builder callback.
    #[cfg(feature = "graph")]
    pub fn with_graph_builder(
        mut self,
        builder: Arc<dyn Fn() -> dtx_core::DependencyGraph + Send + Sync>,
    ) -> Self {
        self.graph_builder = Some(builder);
        self
    }

    #[cfg(feature = "graph")]
    fn require_graph(&self) -> Result<dtx_core::DependencyGraph, ErrorObject> {
        self.graph_builder
            .as_ref()
            .map(|f| f())
            .ok_or_else(|| ErrorObject::internal_error("Graph not configured"))
    }

    fn require_initialized(&self) -> Result<(), ErrorObject> {
        if !self.initialized.load(std::sync::atomic::Ordering::Acquire) {
            return Err(ErrorObject::new(
                -32002,
                "Server not initialized. Send initialize request first.",
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl<H: ProtocolHandler> McpHandler for DefaultMcpHandler<H> {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult, ErrorObject> {
        self.initialized
            .store(true, std::sync::atomic::Ordering::Release);

        Ok(InitializeResult {
            protocol_version: super::types::MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: self.server_info.clone(),
            instructions: Some("dtx provides symbol-aware code intelligence and cross-session memory. Start with list_memories to load existing context, or onboarding for new projects. Use get_symbols_overview before reading files — it shows structure with line ranges. Use replace_symbol_body for safe refactoring, find_references for impact analysis. Persist decisions with write_memory. Use reflect to synthesize memory landscape, checkpoint to save session progress.".to_string()),
        })
    }

    async fn list_resources(&self) -> Result<ListResourcesResult, ErrorObject> {
        self.require_initialized()?;
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
        self.require_initialized()?;
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
        self.require_initialized()?;
        Ok(ListToolsResult { tools: dtx_tools() })
    }

    async fn call_tool(&self, params: CallToolParams) -> Result<CallToolResult, ErrorObject> {
        self.require_initialized()?;

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

            // Code intelligence tools (sync I/O — run off async runtime)
            #[cfg(feature = "code")]
            "get_symbols_overview"
            | "find_symbol"
            | "find_references"
            | "find_referencing_symbols"
            | "search_pattern"
            | "replace_symbol_body"
            | "insert_before_symbol"
            | "insert_after_symbol"
            | "insert_at_line"
            | "replace_lines"
            | "rename_symbol"
            | "find_file"
            | "list_dir" => {
                let code = self.require_code()?.clone();
                let name = params.name.clone();
                let arguments = params.arguments.clone();
                tokio::task::spawn_blocking(move || handle_code_tool(&code, &name, &arguments))
                    .await
                    .map_err(|e| ErrorObject::internal_error(format!("Task join error: {e}")))?
            }

            // Memory tools (sync I/O — run off async runtime)
            #[cfg(feature = "memory")]
            "list_memories" | "read_memory" | "write_memory" | "edit_memory" | "delete_memory"
            | "reflect" | "checkpoint" => {
                let mem = self.require_memory()?.clone();
                let name = params.name.clone();
                let arguments = params.arguments.clone();
                tokio::task::spawn_blocking(move || handle_memory_tool(&mem, &name, &arguments))
                    .await
                    .map_err(|e| ErrorObject::internal_error(format!("Task join error: {e}")))?
            }

            // Onboarding tools (sync I/O — run off async runtime)
            #[cfg(all(feature = "code", feature = "memory"))]
            "onboarding" | "initial_instructions" => {
                let code = self.require_code()?.clone();
                let mem = self.require_memory()?.clone();
                let name = params.name.clone();
                let arguments = params.arguments.clone();
                tokio::task::spawn_blocking(move || {
                    handle_onboarding_tool(&code, &mem, &name, &arguments)
                })
                .await
                .map_err(|e| ErrorObject::internal_error(format!("Task join error: {e}")))?
            }

            // Graph query tools
            #[cfg(feature = "graph")]
            "query_graph" | "get_impact" | "graph_status" => {
                let graph = self.require_graph()?;
                let name = params.name.clone();
                let arguments = params.arguments.clone();
                tokio::task::spawn_blocking(move || handle_graph_tool(&graph, &name, &arguments))
                    .await
                    .map_err(|e| ErrorObject::internal_error(format!("Task join error: {e}")))?
            }

            _ => Err(ErrorObject::method_not_found(&params.name)),
        }
    }
}

#[cfg(any(feature = "code", feature = "memory", feature = "graph"))]
fn append_note(result: &mut CallToolResult, note: &str) {
    if let Some(super::tools::ToolContent::Text { ref mut text }) = result.content.first_mut() {
        text.push_str("\n\n");
        text.push_str(note);
    }
}

#[cfg(feature = "code")]
const REFERENCE_CAP: usize = 50;
#[cfg(feature = "code")]
const SEARCH_CAP: usize = 30;

#[cfg(feature = "code")]
fn truncated_json<T: serde::Serialize>(items: Vec<T>, cap: usize, hint: &str) -> CallToolResult {
    let total = items.len();
    if total > cap {
        let truncated: Vec<_> = items.into_iter().take(cap).collect();
        let mut result = CallToolResult::json(&truncated);
        append_note(
            &mut result,
            &format!("Showing {cap} of {cap}+ results. Narrow with {hint} param."),
        );
        result
    } else {
        CallToolResult::json(&items)
    }
}

#[cfg(feature = "code")]
fn edit_ok(message: impl Into<String>, new_hash: String) -> CallToolResult {
    CallToolResult::json(serde_json::json!({
        "message": message.into(),
        "content_hash": new_hash
    }))
}

#[cfg(feature = "code")]
fn handle_code_tool(
    code: &Arc<dtx_code::WorkspaceIndex>,
    name: &str,
    raw_args: &Option<serde_json::Value>,
) -> Result<CallToolResult, ErrorObject> {
    use std::path::Path;

    let a = args::require(raw_args)?;
    let err = |e: dtx_code::CodeError| ErrorObject::internal_error(e.to_string());

    match name {
        "get_symbols_overview" => {
            let path = args::require_str(a, "path")?;
            let depth = args::optional_usize(a, "depth");
            let overview = code.get_overview(Path::new(path), depth).map_err(err)?;
            if depth.is_none() && overview.symbols.len() > 100 {
                let shallow = code.get_overview(Path::new(path), Some(1)).map_err(err)?;
                let count = shallow.symbols.len();
                let mut result = CallToolResult::json(&shallow);
                append_note(
                    &mut result,
                    &format!(
                        "Showing depth=1 ({count} top-level symbols). Use depth=2 for nested symbols."
                    ),
                );
                return Ok(result);
            }
            Ok(CallToolResult::json(&overview))
        }
        "find_symbol" => {
            let pattern = args::require_str(a, "name_path_pattern")?;
            let path = args::optional_str(a, "path").map(Path::new);
            let depth = args::optional_usize(a, "depth");
            let include_body = args::optional_bool(a, "include_body", false);
            let matches = code
                .find_symbol(pattern, path, depth, include_body, None)
                .map_err(err)?;
            Ok(CallToolResult::json(&matches))
        }
        "find_references" => {
            let symbol_name = args::require_str(a, "symbol_name")?;
            let scope = args::optional_str(a, "scope_path").map(Path::new);
            let refs =
                dtx_code::find_references(code.root(), symbol_name, scope, Some(REFERENCE_CAP + 1))
                    .map_err(err)?;
            Ok(truncated_json(refs, REFERENCE_CAP, "scope_path"))
        }
        "find_referencing_symbols" => {
            let symbol_name = args::require_str(a, "symbol_name")?;
            let scope = args::optional_str(a, "scope_path").map(Path::new);
            let refs = dtx_code::find_referencing_symbols(
                code,
                symbol_name,
                scope,
                Some(REFERENCE_CAP + 1),
            )
            .map_err(err)?;
            Ok(truncated_json(refs, REFERENCE_CAP, "scope_path"))
        }
        "search_pattern" => {
            let pattern = args::require_str(a, "pattern")?;
            let glob = args::optional_str(a, "glob");
            let context = args::optional_usize(a, "context_lines").unwrap_or(2);
            let matches =
                dtx_code::search_pattern(code.root(), pattern, glob, context, Some(SEARCH_CAP + 1))
                    .map_err(err)?;
            Ok(truncated_json(matches, SEARCH_CAP, "glob"))
        }
        "replace_symbol_body" => {
            let path = args::require_str(a, "path")?;
            let name_path = args::require_str(a, "name_path")?;
            let new_body = args::require_str(a, "new_body")?;
            let hash = args::optional_str(a, "content_hash");
            let new_hash =
                dtx_code::replace_symbol_body(code, Path::new(path), name_path, new_body, hash)
                    .map_err(err)?;
            let mut result = edit_ok("Symbol body replaced", new_hash);
            append_note(
                &mut result,
                "Tip: Run get_symbols_overview on this file to verify the edit.",
            );
            Ok(result)
        }
        "insert_before_symbol" => {
            let path = args::require_str(a, "path")?;
            let name_path = args::require_str(a, "name_path")?;
            let content = args::require_str(a, "content")?;
            let hash = args::optional_str(a, "content_hash");
            let new_hash =
                dtx_code::insert_before_symbol(code, Path::new(path), name_path, content, hash)
                    .map_err(err)?;
            Ok(edit_ok("Content inserted before symbol", new_hash))
        }
        "insert_after_symbol" => {
            let path = args::require_str(a, "path")?;
            let name_path = args::require_str(a, "name_path")?;
            let content = args::require_str(a, "content")?;
            let hash = args::optional_str(a, "content_hash");
            let new_hash =
                dtx_code::insert_after_symbol(code, Path::new(path), name_path, content, hash)
                    .map_err(err)?;
            Ok(edit_ok("Content inserted after symbol", new_hash))
        }
        "insert_at_line" => {
            let path = args::require_str(a, "path")?;
            let line = args::require_usize(a, "line")?;
            let content = args::require_str(a, "content")?;
            let hash = args::optional_str(a, "content_hash");
            let new_hash = dtx_code::insert_at_line(code, Path::new(path), line, content, hash)
                .map_err(err)?;
            Ok(edit_ok(
                format!("Content inserted at line {line}"),
                new_hash,
            ))
        }
        "replace_lines" => {
            let path = args::require_str(a, "path")?;
            let start_line = args::require_usize(a, "start_line")?;
            let end_line = args::require_usize(a, "end_line")?;
            let new_content = args::require_str(a, "new_content")?;
            let hash = args::optional_str(a, "content_hash");
            let new_hash = dtx_code::replace_lines(
                code,
                Path::new(path),
                start_line,
                end_line,
                new_content,
                hash,
            )
            .map_err(err)?;
            Ok(edit_ok(
                format!("Lines {start_line}-{end_line} replaced"),
                new_hash,
            ))
        }
        "rename_symbol" => {
            let path = args::require_str(a, "path")?;
            let name_path = args::require_str(a, "name_path")?;
            let new_name = args::require_str(a, "new_name")?;
            let dry_run = args::optional_bool(a, "dry_run", false);
            let result =
                dtx_code::rename_symbol(code, Path::new(path), name_path, new_name, dry_run)
                    .map_err(err)?;
            Ok(CallToolResult::json(&result))
        }
        "find_file" => {
            let pattern = args::require_str(a, "pattern")?;
            let files = code.find_files(pattern).map_err(err)?;
            Ok(CallToolResult::json(&files))
        }
        "list_dir" => {
            let path = args::optional_str(a, "path")
                .map(Path::new)
                .unwrap_or_else(|| code.root());
            let recursive = args::optional_bool(a, "recursive", false);
            let entries = code.list_dir(path, recursive).map_err(err)?;
            Ok(CallToolResult::json(&entries))
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
    let mem_err = |e: dtx_memory::MemoryError| ErrorObject::internal_error(e.to_string());

    match name {
        "list_memories" => {
            let kind_filter = args
                .as_ref()
                .and_then(|a| a.get("kind"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<dtx_memory::MemoryKind>().ok());
            let name_contains = args
                .as_ref()
                .and_then(|a| args::optional_str(a, "name_contains"))
                .map(String::from);
            let content_contains = args
                .as_ref()
                .and_then(|a| args::optional_str(a, "content_contains"))
                .map(String::from);
            let tags = args
                .as_ref()
                .map(|a| args::optional_tags(a, "tags"))
                .unwrap_or_default();

            let has_search =
                name_contains.is_some() || content_contains.is_some() || !tags.is_empty();

            let metas: Vec<dtx_memory::MemoryMeta> = if has_search || kind_filter.is_some() {
                let mut filter = dtx_memory::MemoryFilter::new();
                if let Some(kind) = kind_filter {
                    filter = filter.kind(kind);
                }
                if let Some(ref n) = name_contains {
                    filter = filter.name_contains(n);
                }
                if let Some(ref c) = content_contains {
                    filter = filter.content_contains(c);
                }
                for t in &tags {
                    filter = filter.tag(t);
                }
                dtx_memory::search(store, &filter)
                    .map_err(mem_err)?
                    .into_iter()
                    .map(|m| m.meta)
                    .collect()
            } else {
                store.list().map_err(mem_err)?
            };

            let mut result = CallToolResult::json(&metas);
            if metas.is_empty() {
                append_note(
                    &mut result,
                    "No memories found. Run onboarding to create initial project context.",
                );
            }
            Ok(result)
        }
        "read_memory" => {
            let a = args::require(args)?;
            let mem_name = args::require_str(a, "name")?;
            let memory = store.read(mem_name).map_err(mem_err)?;
            Ok(CallToolResult::text(memory.to_file_content()))
        }
        "write_memory" => {
            let a = args::require(args)?;
            let mem_name = args::require_str(a, "name")?;
            let content = args::require_str(a, "content")?;
            let kind: dtx_memory::MemoryKind = args::optional_str(a, "kind")
                .and_then(|s| s.parse().ok())
                .unwrap_or(dtx_memory::MemoryKind::Project);
            let description = args::optional_str(a, "description").map(String::from);
            let tags = args::optional_tags(a, "tags");
            let no_tags = tags.is_empty();

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
            store.write(&memory).map_err(mem_err)?;
            let mut result = CallToolResult::text(format!("Memory '{mem_name}' written"));
            if no_tags {
                append_note(&mut result, "Tip: Add tags for discoverability (e.g., architecture, convention, pattern, decision).");
            }
            Ok(result)
        }
        "edit_memory" => {
            let a = args::require(args)?;
            let mem_name = args::require_str(a, "name")?;

            let mut memory = store.read(mem_name).map_err(mem_err)?;

            if let Some(content) = args::optional_str(a, "content") {
                memory.content = content.to_string();
            }
            if let Some(desc) = args::optional_str(a, "description") {
                memory.meta.description = Some(desc.to_string());
            }
            let new_tags = args::optional_tags(a, "tags");
            if !new_tags.is_empty() {
                memory.meta.tags = new_tags;
            }
            memory.meta.updated_at = chrono::Utc::now();

            store.write(&memory).map_err(mem_err)?;
            Ok(CallToolResult::text(format!("Memory '{mem_name}' updated")))
        }
        "delete_memory" => {
            let a = args::require(args)?;
            let mem_name = args::require_str(a, "name")?;
            store.delete(mem_name).map_err(mem_err)?;
            Ok(CallToolResult::text(format!("Memory '{mem_name}' deleted")))
        }
        "reflect" => {
            let metas = store.list().map_err(mem_err)?;
            if metas.is_empty() {
                return Ok(CallToolResult::text(
                    "No memories found. Run onboarding to create initial project context, then write_memory to persist discoveries.",
                ));
            }

            let focus = args
                .as_ref()
                .and_then(|a| args::optional_str(a, "focus"))
                .map(|s| s.to_lowercase());

            let filtered: Vec<&dtx_memory::MemoryMeta> = if let Some(ref focus) = focus {
                metas
                    .iter()
                    .filter(|m| {
                        m.name.to_lowercase().contains(focus)
                            || m.tags.iter().any(|t| t.to_lowercase().contains(focus))
                    })
                    .collect()
            } else {
                metas.iter().collect()
            };

            let total = filtered.len();
            let large = total > 200;

            // Kind distribution
            let kind_str = |k: dtx_memory::MemoryKind| -> &'static str {
                match k {
                    dtx_memory::MemoryKind::User => "user",
                    dtx_memory::MemoryKind::Project => "project",
                    dtx_memory::MemoryKind::Feedback => "feedback",
                    dtx_memory::MemoryKind::Reference => "reference",
                }
            };
            let mut kind_counts: std::collections::HashMap<&str, Vec<&str>> =
                std::collections::HashMap::new();
            for m in &filtered {
                kind_counts
                    .entry(kind_str(m.kind))
                    .or_default()
                    .push(&m.name);
            }

            // Tag frequency
            let mut tag_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            let mut untagged = 0usize;
            for m in &filtered {
                if m.tags.is_empty() {
                    untagged += 1;
                }
                for t in &m.tags {
                    *tag_counts.entry(t.as_str()).or_default() += 1;
                }
            }
            let mut tag_sorted: Vec<_> = tag_counts.into_iter().collect();
            tag_sorted.sort_by(|a, b| b.1.cmp(&a.1));

            // Coverage gaps
            let all_kinds = ["user", "project", "feedback", "reference"];
            let gap_kinds: Vec<&str> = all_kinds
                .iter()
                .filter(|k| !kind_counts.contains_key(*k))
                .copied()
                .collect();

            // Staleness (>7 days)
            let now = chrono::Utc::now();
            let stale: Vec<&dtx_memory::MemoryMeta> = filtered
                .iter()
                .filter(|m| (now - m.updated_at).num_days() > 7)
                .copied()
                .collect();

            let kinds_used = kind_counts.len();

            // Build output
            let mut output = format!(
                "## Memory Landscape\n\n**{total} memories** across {kinds_used} of 4 kinds\n\n"
            );

            output.push_str("### Distribution\n");
            for kind in &all_kinds {
                if let Some(names) = kind_counts.get(kind) {
                    let count = names.len();
                    if large {
                        output.push_str(&format!("- {kind} ({count})\n"));
                    } else {
                        let display: Vec<String> =
                            names.iter().take(5).map(|n| format!("\"{n}\"")).collect();
                        let suffix = if names.len() > 5 {
                            format!(", +{}", names.len() - 5)
                        } else {
                            String::new()
                        };
                        output.push_str(&format!(
                            "- {kind} ({count}): {}{suffix}\n",
                            display.join(", ")
                        ));
                    }
                }
            }
            output.push('\n');

            if !gap_kinds.is_empty() {
                output.push_str("### Coverage Gaps\n");
                for kind in &gap_kinds {
                    let hint = match *kind {
                        "user" => {
                            "consider saving role/team context with write_memory(kind: \"user\")"
                        }
                        "feedback" => {
                            "capture process preferences with write_memory(kind: \"feedback\")"
                        }
                        "project" => "save project context with write_memory(kind: \"project\")",
                        "reference" => {
                            "save external resource pointers with write_memory(kind: \"reference\")"
                        }
                        _ => "consider adding entries",
                    };
                    output.push_str(&format!("- No **{kind}** memories — {hint}\n"));
                }
                output.push('\n');
            }

            if !stale.is_empty() {
                output.push_str("### Staleness\n");
                for m in stale.iter().take(10) {
                    let days = (now - m.updated_at).num_days();
                    output.push_str(&format!(
                        "- \"{}\" last updated {days} days ago — may need refresh\n",
                        m.name
                    ));
                }
                if stale.len() > 10 {
                    output.push_str(&format!("- +{} more stale entries\n", stale.len() - 10));
                }
                output.push('\n');
            }

            if !tag_sorted.is_empty() || untagged > 0 {
                output.push_str("### Tags\n");
                let tag_display: Vec<String> = tag_sorted
                    .iter()
                    .take(15)
                    .map(|(t, c)| format!("{t}({c})"))
                    .collect();
                if untagged > 0 {
                    output.push_str(&format!(
                        "{}, untagged({untagged})\n\n",
                        tag_display.join(", ")
                    ));
                } else {
                    output.push_str(&format!("{}\n\n", tag_display.join(", ")));
                }
            }

            // Suggested actions
            output.push_str("### Suggested Actions\n");
            for kind in &gap_kinds {
                output.push_str(&format!(
                    "- write_memory(kind: \"{kind}\") — fill coverage gap\n"
                ));
            }
            for m in stale.iter().take(3) {
                output.push_str(&format!(
                    "- read_memory(\"{}\") — review for freshness\n",
                    m.name
                ));
            }
            if untagged > 0 {
                output.push_str(
                    "- edit_memory with tags — improve discoverability of untagged entries\n",
                );
            }
            if output.ends_with("### Suggested Actions\n") {
                output.push_str(
                    "- All looking good! Consider checkpoint to save session progress.\n",
                );
            }

            if output.len() > 4000 {
                output.truncate(3950);
                output.push_str("\n\n[truncated]");
            }

            Ok(CallToolResult::text(output))
        }
        "checkpoint" => {
            let a = args::require(args)?;
            let summary = args::require_str(a, "summary")?;
            let decisions = args::optional_str(a, "decisions").unwrap_or("None recorded");
            let open_questions = args::optional_str(a, "open_questions").unwrap_or("None");
            let user_tags = args::optional_tags(a, "tags");

            let now = chrono::Utc::now();
            let cp_name = format!("checkpoint-{}", now.format("%Y%m%d-%H%M%S"));

            let mut auto_tags = vec!["checkpoint".to_string(), "session".to_string()];
            for t in &user_tags {
                if !auto_tags.contains(t) {
                    auto_tags.push(t.clone());
                }
            }

            let desc_preview: String = summary.chars().take(80).collect();
            let content = format!("## Summary\n{summary}\n\n## Decisions\n{decisions}\n\n## Open Questions\n{open_questions}");

            let memory = dtx_memory::Memory {
                meta: dtx_memory::MemoryMeta {
                    name: cp_name.clone(),
                    kind: dtx_memory::MemoryKind::Project,
                    description: Some(format!("Session checkpoint: {desc_preview}")),
                    created_at: now,
                    updated_at: now,
                    tags: auto_tags,
                },
                content,
            };
            store.write(&memory).map_err(mem_err)?;
            Ok(CallToolResult::text(format!("Checkpoint saved: {cp_name}")))
        }
        _ => Err(ErrorObject::method_not_found(name)),
    }
}

#[cfg(all(feature = "code", feature = "memory"))]
fn handle_onboarding_tool(
    code: &Arc<dtx_code::WorkspaceIndex>,
    memory: &Arc<dtx_memory::MemoryStore>,
    name: &str,
    args: &Option<serde_json::Value>,
) -> Result<CallToolResult, ErrorObject> {
    match name {
        "onboarding" => {
            let force = args
                .as_ref()
                .map(|a| args::optional_bool(a, "force", false))
                .unwrap_or(false);
            let save = args
                .as_ref()
                .map(|a| args::optional_bool(a, "save_to_memory", true))
                .unwrap_or(true);

            // Return cached onboarding if recent and not forced
            if !force {
                let metas = memory.list().unwrap_or_default();
                if let Some(m) = metas.iter().find(|m| m.name == "onboarding") {
                    let age = chrono::Utc::now() - m.updated_at;
                    if age.num_hours() < 24 {
                        if let Ok(mem) = memory.read("onboarding") {
                            return Ok(CallToolResult::text(format!(
                                "{}\n\n_Cached from {}. Use force:true to re-run._",
                                mem.content,
                                m.updated_at.format("%Y-%m-%d %H:%M UTC")
                            )));
                        }
                    }
                }
            }

            // Discover project structure
            let files = code.list_files();
            let file_count = files.len();

            // Count languages by extension
            let mut lang_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for f in &files {
                if let Some(ext) = f.extension().and_then(|e| e.to_str()) {
                    *lang_counts.entry(ext.to_string()).or_default() += 1;
                }
            }
            let mut languages: Vec<_> = lang_counts.into_iter().collect();
            languages.sort_by(|a, b| b.1.cmp(&a.1));

            // Directory tree (depth 2, dirs only)
            let tree = match code.list_dir_with_depth(code.root(), true, Some(2)) {
                Ok(entries) => {
                    let mut tree_lines = Vec::new();
                    for e in &entries {
                        let depth = e.name.matches('/').count();
                        if depth <= 2 && matches!(e.entry_type, dtx_code::EntryKind::Dir) {
                            let indent = "  ".repeat(depth);
                            let display_name = e.name.rsplit('/').next().unwrap_or(&e.name);
                            tree_lines.push(format!("{indent}{display_name}/"));
                        }
                    }
                    tree_lines.join("\n")
                }
                Err(_) => String::from("(unable to list directories)"),
            };

            // Detect build systems and frameworks
            let well_known = [
                "Cargo.toml",
                "package.json",
                "flake.nix",
                "Makefile",
                "CMakeLists.txt",
                "go.mod",
                "pyproject.toml",
                "build.gradle",
                "pom.xml",
                "Gemfile",
                "mix.exs",
                "stack.yaml",
            ];
            let key_files: Vec<String> = well_known
                .iter()
                .filter(|f| {
                    code.resolve_path(std::path::Path::new(f))
                        .is_ok_and(|p| p.exists())
                })
                .map(|f| f.to_string())
                .collect();

            // Detect workspace members
            let mut workspace_members = Vec::new();
            if let Ok(cargo_path) = code.resolve_path(std::path::Path::new("Cargo.toml")) {
                if let Ok(content) = std::fs::read_to_string(&cargo_path) {
                    // Parse members from [workspace] section
                    let mut in_members = false;
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with("[") && trimmed != "[workspace]" {
                            in_members = false;
                        }
                        if trimmed.starts_with("members") && trimmed.contains('[') {
                            in_members = true;
                        }
                        if in_members {
                            // Extract quoted strings like "crates/*" or "crates/dtx-core"
                            for part in trimmed.split('"') {
                                let candidate = part.trim().trim_matches(',');
                                if !candidate.is_empty()
                                    && (candidate.contains('/') || candidate.contains('*'))
                                {
                                    workspace_members.push(candidate.to_string());
                                }
                            }
                        }
                        if in_members && trimmed.contains(']') && !trimmed.contains('[') {
                            in_members = false;
                        }
                    }
                }
            }
            if let Ok(pkg_path) = code.resolve_path(std::path::Path::new("package.json")) {
                if let Ok(content) = std::fs::read_to_string(&pkg_path) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(workspaces) =
                            parsed.get("workspaces").and_then(|w| w.as_array())
                        {
                            for ws in workspaces {
                                if let Some(s) = ws.as_str() {
                                    workspace_members.push(s.to_string());
                                }
                            }
                        }
                    }
                }
            }

            // Detect entry points with symbol summaries
            let entry_candidates = [
                "src/main.rs",
                "src/lib.rs",
                "src/index.ts",
                "src/index.js",
                "main.py",
                "main.go",
                "app.py",
                "index.js",
                "index.ts",
            ];
            let mut entry_sections = Vec::new();
            for candidate in &entry_candidates {
                let p = std::path::Path::new(candidate);
                if let Ok(overview) = code.get_overview(p, Some(0)) {
                    let names: Vec<String> = overview
                        .symbols
                        .iter()
                        .take(10)
                        .map(|s| format!("{} {}", s.kind, s.name))
                        .collect();
                    let symbol_summary = if names.is_empty() {
                        "(no top-level symbols)".to_string()
                    } else {
                        names.join(", ")
                    };
                    entry_sections.push(format!("- {candidate}: {symbol_summary}"));
                }
            }

            // Build project name from root dir
            let project_name = code
                .root()
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("project"))
                .to_string_lossy();

            // Format language stats
            let lang_stats: Vec<String> = languages
                .iter()
                .take(10)
                .map(|(ext, count)| format!("{ext} ({count})"))
                .collect();

            // Build structured output
            let mut output = format!("# Project: {project_name}\n\n");

            output.push_str("## Structure (depth=2)\n```\n");
            let tree_capped: Vec<&str> = tree.lines().take(60).collect();
            let tree_total = tree.lines().count();
            output.push_str(&tree_capped.join("\n"));
            if tree_total > 60 {
                output.push_str("\n... (truncated)");
            }
            output.push_str("\n```\n\n");

            output.push_str("## Stack\n");
            output.push_str(&format!("- Languages: {}\n", lang_stats.join(", ")));
            output.push_str(&format!("- Files: {file_count}\n"));
            if !key_files.is_empty() {
                output.push_str(&format!("- Build: {}\n", key_files.join(", ")));
            }
            output.push('\n');

            if !workspace_members.is_empty() {
                output.push_str("## Workspace Members\n");
                for m in &workspace_members {
                    output.push_str(&format!("- {m}\n"));
                }
                output.push('\n');
            }

            if !entry_sections.is_empty() {
                output.push_str("## Entry Points\n");
                for section in &entry_sections {
                    output.push_str(&format!("{section}\n"));
                }
                output.push('\n');
            }

            // Cap total output at 4000 chars
            if output.len() > 4000 {
                output.truncate(3950);
                output.push_str("\n\n... (truncated)");
            }

            // Save to memory (before consuming output)
            if save {
                let now = chrono::Utc::now();
                memory
                    .write(&dtx_memory::Memory {
                        meta: dtx_memory::MemoryMeta {
                            name: "onboarding".to_string(),
                            kind: dtx_memory::MemoryKind::Project,
                            description: Some("Project onboarding summary".to_string()),
                            created_at: now,
                            updated_at: now,
                            tags: vec!["onboarding".to_string()],
                        },
                        content: output.clone(),
                    })
                    .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            }

            Ok(CallToolResult::text(output))
        }
        "initial_instructions" => {
            let instructions = r#"# dtx MCP Tool Guide

## Session Start Checklist
1. `list_memories` — load existing project context
2. `read_memory` on relevant entries — understand prior decisions
3. `onboarding` (if no memories exist) — generate project structure snapshot
4. `reflect` — synthesize memory landscape, identify gaps

## Session End Checklist
1. `checkpoint(summary, decisions, open_questions)` — save session progress
2. `write_memory` for any new conventions, patterns, or architecture decisions discovered

## When to Use dtx vs Native Tools

**Prefer dtx for:**
- Understanding file structure → `get_symbols_overview` (shows functions, structs, classes with line ranges without reading full files)
- Reading specific definitions → `find_symbol` with `include_body:true` (reads one function without loading the whole file)
- Finding all usages → `find_references` / `find_referencing_symbols` (word-boundary matching, containing symbol context)
- Editing code → `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol` (name-based, survives line shifts)
- Cross-session context → `write_memory` / `read_memory` (persists knowledge between conversations)

**Native tools are fine for:**
- Reading a known file path in full
- Simple directory listing of a known path

## Recommended Workflow

1. Run `onboarding` to discover project structure (cached for 24h)
2. Use `get_symbols_overview` to understand file structure before editing
3. Use `find_symbol` with `include_body:true` to read specific definitions
4. Use `find_references` / `find_referencing_symbols` for impact analysis
5. Use `replace_symbol_body` for safe refactoring (name-based, not line-based)
6. Save architecture decisions and conventions to memory with `write_memory`
7. Use `reflect` to synthesize findings and identify knowledge gaps
8. Use `checkpoint` before ending to save session progress for future sessions

## Tool Categories

### Memory (7 tools)
- `list_memories` — search by kind, name, content, or tags
- `read_memory` / `write_memory` / `edit_memory` / `delete_memory` — CRUD
- `reflect` — synthesize memory landscape with distribution, gaps, and staleness
- `checkpoint` — save structured session progress (auto-named, auto-tagged)

### Code Intelligence (13 tools)
- `get_symbols_overview` — Symbol tree with line ranges
- `find_symbol` — Find by name path, optionally include source body
- `find_references` / `find_referencing_symbols` — Cross-workspace reference search (capped at 50)
- `search_pattern` — Regex search with glob filtering (capped at 30)
- `replace_symbol_body` — Replace definition by name (safe refactoring)
- `rename_symbol` — Cross-file rename with dry_run preview
- `insert_before_symbol` / `insert_after_symbol` — Name-based insertion
- `insert_at_line` / `replace_lines` — Line-based editing with content hash locking

### Resource Management (8 tools)
- `start_resource` / `stop_resource` / `restart_resource` / `get_status` / `list_resources` / `get_logs` / `start_all` / `stop_all`
"#;
            Ok(CallToolResult::text(instructions))
        }
        _ => Err(ErrorObject::method_not_found(name)),
    }
}

#[cfg(feature = "graph")]
fn handle_graph_tool(
    graph: &dtx_core::DependencyGraph,
    name: &str,
    raw_args: &Option<serde_json::Value>,
) -> Result<CallToolResult, ErrorObject> {
    match name {
        "query_graph" => {
            let mut g = if let Some(a) = raw_args.as_ref() {
                if let Some(view_str) = args::optional_str(a, "view") {
                    let view = parse_graph_view(view_str)?;
                    graph.filter_by_view(view)
                } else {
                    graph.clone()
                }
            } else {
                graph.clone()
            };

            if let Some(a) = raw_args.as_ref() {
                // Filter edges by kind
                let edge_kind = args::optional_str(a, "edge_kind")
                    .map(parse_edge_kind)
                    .transpose()?;

                // Filter edges by min confidence
                let min_confidence = args::optional_str(a, "min_confidence")
                    .map(parse_edge_confidence)
                    .transpose()?;

                if edge_kind.is_some() || min_confidence.is_some() {
                    let filtered: Vec<dtx_core::GraphEdge> = g
                        .edges_filtered(edge_kind, min_confidence)
                        .into_iter()
                        .cloned()
                        .collect();
                    g.edges = filtered;
                }

                // Filter nodes by domain
                if let Some(domain_str) = args::optional_str(a, "domain") {
                    let domain = parse_node_domain(domain_str)?;
                    g.nodes.retain(|_, n| n.domain == domain);
                }

                // Filter nodes by label pattern
                if let Some(pattern) = args::optional_str(a, "pattern") {
                    let lower = pattern.to_lowercase();
                    g.nodes
                        .retain(|_, n| n.label.to_lowercase().contains(&lower));
                }

                // Apply limit
                if let Some(limit) = args::optional_usize(a, "limit") {
                    if g.nodes.len() > limit {
                        let keys: Vec<String> = g.nodes.keys().skip(limit).cloned().collect();
                        for key in keys {
                            g.nodes.remove(&key);
                        }
                    }
                }

                // Remove edges referencing removed nodes
                let node_ids: std::collections::HashSet<&String> = g.nodes.keys().collect();
                g.edges
                    .retain(|e| node_ids.contains(&e.source) && node_ids.contains(&e.target));
            }

            let total_nodes = g.nodes.len();
            let total_edges = g.edges.len();
            let mut result = CallToolResult::json(&g);
            append_note(
                &mut result,
                &format!("{total_nodes} nodes, {total_edges} edges"),
            );
            Ok(result)
        }
        "get_impact" => {
            let a = args::require(raw_args)?;
            let node_id = args::require_str(a, "node_id")?;
            let min_confidence = args::optional_str(a, "min_confidence")
                .map(parse_edge_confidence)
                .transpose()?
                .unwrap_or(dtx_core::EdgeConfidence::Speculative);

            let impact = graph.impact(node_id, min_confidence);
            Ok(CallToolResult::json(&impact))
        }
        "graph_status" => {
            let stats = graph.stats();
            Ok(CallToolResult::json(&stats))
        }
        _ => Err(ErrorObject::method_not_found(name)),
    }
}

#[cfg(feature = "graph")]
fn parse_graph_view(s: &str) -> Result<dtx_core::GraphView, ErrorObject> {
    match s {
        "processes" => Ok(dtx_core::GraphView::Processes),
        "code" => Ok(dtx_core::GraphView::Code),
        "memories" => Ok(dtx_core::GraphView::Memories),
        "files" => Ok(dtx_core::GraphView::Files),
        "knowledge" => Ok(dtx_core::GraphView::Knowledge),
        _ => Err(ErrorObject::invalid_params(format!(
            "Invalid view: {s}. Expected: processes, code, memories, files, knowledge"
        ))),
    }
}

#[cfg(feature = "graph")]
fn parse_edge_kind(s: &str) -> Result<dtx_core::EdgeKind, ErrorObject> {
    match s {
        "depends_on" => Ok(dtx_core::EdgeKind::DependsOn),
        "provides" => Ok(dtx_core::EdgeKind::Provides),
        "implements" => Ok(dtx_core::EdgeKind::Implements),
        "configures" => Ok(dtx_core::EdgeKind::Configures),
        "references" => Ok(dtx_core::EdgeKind::References),
        "documents" => Ok(dtx_core::EdgeKind::Documents),
        "calls" => Ok(dtx_core::EdgeKind::Calls),
        "contains" => Ok(dtx_core::EdgeKind::Contains),
        _ => Err(ErrorObject::invalid_params(format!(
            "Invalid edge kind: {s}. Expected: depends_on, provides, implements, configures, references, documents, calls, contains"
        ))),
    }
}

#[cfg(feature = "graph")]
fn parse_edge_confidence(s: &str) -> Result<dtx_core::EdgeConfidence, ErrorObject> {
    match s {
        "speculative" => Ok(dtx_core::EdgeConfidence::Speculative),
        "probable" => Ok(dtx_core::EdgeConfidence::Probable),
        "definite" => Ok(dtx_core::EdgeConfidence::Definite),
        _ => Err(ErrorObject::invalid_params(format!(
            "Invalid confidence: {s}. Expected: speculative, probable, definite"
        ))),
    }
}

#[cfg(feature = "graph")]
fn parse_node_domain(s: &str) -> Result<dtx_core::NodeDomain, ErrorObject> {
    match s {
        "resource" => Ok(dtx_core::NodeDomain::Resource),
        "symbol" => Ok(dtx_core::NodeDomain::Symbol),
        "memory" => Ok(dtx_core::NodeDomain::Memory),
        "file" => Ok(dtx_core::NodeDomain::File),
        _ => Err(ErrorObject::invalid_params(format!(
            "Invalid domain: {s}. Expected: resource, symbol, memory, file"
        ))),
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

    fn init_params() -> InitializeParams {
        InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: Default::default(),
            client_info: super::super::types::ClientInfo {
                name: "test".to_string(),
                version: "1.0".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn mcp_list_resources() {
        let handler = DefaultMcpHandler::new(MockProtocolHandler::new());
        handler.initialize(init_params()).await.unwrap();
        let result = handler.list_resources().await.unwrap();
        assert_eq!(result.resources.len(), 1);
        assert!(result.resources[0].uri.contains("postgres"));
    }

    #[tokio::test]
    async fn mcp_list_tools() {
        let handler = DefaultMcpHandler::new(MockProtocolHandler::new());
        handler.initialize(init_params()).await.unwrap();
        let result = handler.list_tools().await.unwrap();
        assert!(!result.tools.is_empty());
    }

    #[tokio::test]
    async fn mcp_call_tool() {
        let mock = MockProtocolHandler::new();
        let handler = DefaultMcpHandler::new(mock);
        handler.initialize(init_params()).await.unwrap();

        let result = handler
            .call_tool(CallToolParams {
                name: "start_resource".to_string(),
                arguments: Some(serde_json::json!({"id": "postgres"})),
            })
            .await
            .unwrap();

        assert!(result.is_error.is_none());
    }

    #[tokio::test]
    async fn mcp_requires_initialization() {
        let handler = DefaultMcpHandler::new(MockProtocolHandler::new());
        let err = handler.list_tools().await.unwrap_err();
        assert_eq!(err.code, -32002);
    }
}
