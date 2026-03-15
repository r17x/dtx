//! MCP handler trait and implementation.
//!
//! Handles MCP-specific requests like initialize, list resources, and call tools.

#[cfg(any(feature = "code", feature = "memory"))]
use std::sync::Arc;

use async_trait::async_trait;

#[cfg(any(feature = "code", feature = "memory"))]
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
            "list_memories" | "read_memory" | "write_memory" | "edit_memory" | "delete_memory" => {
                let mem = self.require_memory()?.clone();
                let name = params.name.clone();
                let arguments = params.arguments.clone();
                tokio::task::spawn_blocking(move || handle_memory_tool(&mem, &name, &arguments))
                    .await
                    .map_err(|e| ErrorObject::internal_error(format!("Task join error: {e}")))?
            }

            // Onboarding tools (sync I/O — run off async runtime)
            #[cfg(all(feature = "code", feature = "memory"))]
            "onboarding" | "check_onboarding_performed" | "initial_instructions" => {
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

            _ => Err(ErrorObject::method_not_found(&params.name)),
        }
    }
}

#[cfg(feature = "code")]
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
            &format!("Showing {cap} of {total} results. Narrow with {hint} param."),
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
            let refs = dtx_code::find_references(code.root(), symbol_name, scope).map_err(err)?;
            Ok(truncated_json(refs, REFERENCE_CAP, "scope_path"))
        }
        "find_referencing_symbols" => {
            let symbol_name = args::require_str(a, "symbol_name")?;
            let scope = args::optional_str(a, "scope_path").map(Path::new);
            let refs = dtx_code::find_referencing_symbols(code, symbol_name, scope).map_err(err)?;
            Ok(truncated_json(refs, REFERENCE_CAP, "scope_path"))
        }
        "search_pattern" => {
            let pattern = args::require_str(a, "pattern")?;
            let glob = args::optional_str(a, "glob");
            let context = args::optional_usize(a, "context_lines").unwrap_or(2);
            let matches =
                dtx_code::search_pattern(code.root(), pattern, glob, context).map_err(err)?;
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
            Ok(edit_ok("Symbol body replaced", new_hash))
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
            let a = args::require(args)?;
            let mem_name = args::require_str(a, "name")?;
            let memory = store
                .read(mem_name)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
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
            let tags: Vec<String> = a
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
            let a = args::require(args)?;
            let mem_name = args::require_str(a, "name")?;

            let mut memory = store
                .read(mem_name)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;

            if let Some(content) = args::optional_str(a, "content") {
                memory.content = content.to_string();
            }
            if let Some(desc) = args::optional_str(a, "description") {
                memory.meta.description = Some(desc.to_string());
            }
            if let Some(tags) = a.get("tags").and_then(|v| v.as_array()) {
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
            let a = args::require(args)?;
            let mem_name = args::require_str(a, "name")?;
            store
                .delete(mem_name)
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            Ok(CallToolResult::text(format!("Memory '{mem_name}' deleted")))
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
            let save = args
                .as_ref()
                .map(|a| args::optional_bool(a, "save_to_memory", true))
                .unwrap_or(true);

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

            // Detect entry points
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
            let entry_points: Vec<String> = entry_candidates
                .iter()
                .filter(|f| {
                    code.resolve_path(std::path::Path::new(f))
                        .is_ok_and(|p| p.exists())
                })
                .map(|f| f.to_string())
                .collect();

            // Build summary
            let summary = serde_json::json!({
                "file_count": file_count,
                "languages": languages.iter().map(|(ext, count)| {
                    serde_json::json!({"extension": ext, "count": count})
                }).collect::<Vec<_>>(),
                "key_files": key_files,
                "entry_points": entry_points,
            });

            // Optionally save to memory
            if save {
                let now = chrono::Utc::now();
                let mem = dtx_memory::Memory {
                    meta: dtx_memory::MemoryMeta {
                        name: "onboarding".to_string(),
                        kind: dtx_memory::MemoryKind::Project,
                        description: Some("Project onboarding summary".to_string()),
                        created_at: now,
                        updated_at: now,
                        tags: vec!["onboarding".to_string()],
                    },
                    content: serde_json::to_string_pretty(&summary).unwrap_or_default(),
                };
                memory
                    .write(&mem)
                    .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            }

            Ok(CallToolResult::json(&summary))
        }
        "check_onboarding_performed" => {
            let metas = memory
                .list()
                .map_err(|e| ErrorObject::internal_error(e.to_string()))?;
            let onboarding = metas
                .iter()
                .find(|m| m.name == "onboarding" || m.tags.contains(&"onboarding".to_string()));
            match onboarding {
                Some(m) => Ok(CallToolResult::json(serde_json::json!({
                    "performed": true,
                    "memory_name": m.name,
                }))),
                None => Ok(CallToolResult::json(serde_json::json!({
                    "performed": false,
                }))),
            }
        }
        "initial_instructions" => {
            let instructions = r#"# dtx MCP Tool Guide

## Tool Categories

### Resource Management (8 tools)
- `start_resource`, `stop_resource`, `restart_resource` — Lifecycle control by ID
- `get_status` — Get resource status
- `list_resources` — List all managed resources
- `get_logs` — Get resource logs (default: 50 lines)
- `start_all`, `stop_all` — Batch operations

### Code Intelligence (13 tools)
- `get_symbols_overview` — Get symbol tree for a file (use `depth` to limit)
- `find_symbol` — Search by name_path pattern (substring match), optionally include body
- `find_references` / `find_referencing_symbols` — Find all references with containing symbol context
- `search_pattern` — Regex search across files with glob filtering
- `rename_symbol` — Cross-file rename with all references updated
- `replace_symbol_body` — Replace entire symbol definition
- `insert_before_symbol` / `insert_after_symbol` — Insert code adjacent to symbols
- `insert_at_line` — Insert content at a specific line number
- `replace_lines` — Replace a range of lines

### Navigation (2 tools)
- `find_file` — Glob pattern file search (e.g., "**/*.rs", "**/test_*")
- `list_dir` — List directory contents (optionally recursive)

### Memory (5 tools)
- `list_memories` — List all memories (optional kind filter)
- `read_memory` — Read memory by name
- `write_memory` — Create/update memory with kind, description, tags
- `edit_memory` — Edit memory metadata or content
- `delete_memory` — Delete a memory

### Onboarding (3 tools)
- `onboarding` — Auto-discover project structure and save to memory
- `check_onboarding_performed` — Check if onboarding has been done
- `initial_instructions` — This help text

## Recommended Workflow

1. Run `onboarding` first to discover project structure
2. Use `find_symbol` and `get_symbols_overview` to explore code (prefer over reading full files)
3. Use `find_references` / `find_referencing_symbols` to understand dependencies
4. Use symbol-level editing tools for precise modifications
5. Save important context to memory for future sessions
"#;
            Ok(CallToolResult::text(instructions))
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
