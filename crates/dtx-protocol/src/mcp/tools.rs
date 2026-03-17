//! MCP tool definitions.
//!
//! Tools are operations that AI agents can invoke.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::resources::Resource;

/// MCP Tool definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Unique tool name.
    pub name: String,

    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema for input parameters.
    pub input_schema: Value,
}

impl Tool {
    /// Create a new tool.
    pub fn new(name: impl Into<String>, input_schema: Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            input_schema,
        }
    }

    /// Add a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// List tools result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// Available tools.
    pub tools: Vec<Tool>,
}

/// Call tool params.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallToolParams {
    /// Tool name.
    pub name: String,

    /// Tool arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// Call tool result.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    /// Result content.
    pub content: Vec<ToolContent>,

    /// Whether this is an error result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl CallToolResult {
    /// Create a text result.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: None,
        }
    }

    /// Create an error result.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: message.into(),
            }],
            is_error: Some(true),
        }
    }

    /// Create a JSON result.
    pub fn json(value: impl Serialize) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: serde_json::to_string_pretty(&value).unwrap_or_default(),
            }],
            is_error: None,
        }
    }
}

/// Tool content types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolContent {
    /// Text content.
    Text { text: String },

    /// Image content.
    Image { data: String, mime_type: String },

    /// Resource reference.
    Resource { resource: Resource },
}

/// Get resource management tools (original 8).
pub fn dtx_resource_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "start_resource",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Resource ID to start"
                    }
                },
                "required": ["id"]
            }),
        )
        .with_description("Start a managed resource by ID. Check status with list_resources first to see current state."),
        Tool::new(
            "stop_resource",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Resource ID to stop"
                    }
                },
                "required": ["id"]
            }),
        )
        .with_description("Stop a resource by ID"),
        Tool::new(
            "restart_resource",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Resource ID to restart"
                    }
                },
                "required": ["id"]
            }),
        )
        .with_description("Restart a resource by ID"),
        Tool::new(
            "get_status",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Resource ID"
                    }
                },
                "required": ["id"]
            }),
        )
        .with_description("Get detailed status of a resource including state, health, and uptime. Check before starting or stopping."),
        Tool::new(
            "list_resources",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        )
        .with_description("List all managed resources with current state. Use first to discover available resource IDs and their status."),
        Tool::new(
            "get_logs",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Resource ID"
                    },
                    "lines": {
                        "type": "integer",
                        "description": "Number of lines to retrieve (default: 50)",
                        "default": 50
                    }
                },
                "required": ["id"]
            }),
        )
        .with_description("Get recent log output for a resource. Default 50 lines. Use to diagnose startup failures or runtime errors."),
        Tool::new(
            "start_all",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        )
        .with_description("Start all resources"),
        Tool::new(
            "stop_all",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        )
        .with_description("Stop all resources"),
    ]
}

/// Get code intelligence tools (7).
#[cfg(feature = "code")]
pub fn dtx_code_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "get_symbols_overview",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to get symbols from"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Max depth of symbol tree (default: unlimited)"
                    }
                },
                "required": ["path"]
            }),
        )
        .with_description("Get a file's symbol tree (functions, structs, impls, classes) with line ranges. Shows structure without reading the entire file. Use as first step before editing."),
        Tool::new(
            "find_symbol",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name_path_pattern": {
                        "type": "string",
                        "description": "Symbol name path pattern (substring match, e.g. 'MyStruct/my_method')"
                    },
                    "path": {
                        "type": "string",
                        "description": "Restrict search to this file or directory"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Max depth of children to include"
                    },
                    "include_body": {
                        "type": "boolean",
                        "description": "Include source text of matched symbols",
                        "default": false
                    }
                },
                "required": ["name_path_pattern"]
            }),
        )
        .with_description("Find symbols by hierarchical name path (e.g. 'MyStruct/method'). Set include_body:true to read specific definitions without loading whole files."),
        Tool::new(
            "find_references",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "symbol_name": {
                        "type": "string",
                        "description": "Symbol name to find references for"
                    },
                    "scope_path": {
                        "type": "string",
                        "description": "Restrict search to this directory"
                    }
                },
                "required": ["symbol_name"]
            }),
        )
        .with_description("Find all references to a symbol across the workspace with file:line locations and context. Scope to a directory with scope_path. Capped at 50."),
        Tool::new(
            "search_pattern",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for"
                    },
                    "glob": {
                        "type": "string",
                        "description": "File glob filter (e.g. '*.rs')"
                    },
                    "context_lines": {
                        "type": "integer",
                        "description": "Lines of context around matches (default: 2)",
                        "default": 2
                    }
                },
                "required": ["pattern"]
            }),
        )
        .with_description("Regex search across workspace files with context lines. Capped at 30 results. Always use glob param to narrow scope (e.g. '*.rs')."),
        Tool::new(
            "replace_symbol_body",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path containing the symbol"
                    },
                    "name_path": {
                        "type": "string",
                        "description": "Symbol name path (e.g. 'MyStruct/my_method')"
                    },
                    "new_body": {
                        "type": "string",
                        "description": "New source code to replace the symbol body"
                    },
                    "content_hash": {
                        "type": "string",
                        "description": "SHA256 hash of file content for optimistic locking. If provided, edit fails when file has changed."
                    }
                },
                "required": ["path", "name_path", "new_body"]
            }),
        )
        .with_description("Replace a symbol's entire definition by name path. Survives line-number shifts — safer than line-based edits. Use find_symbol first to get current body."),
        Tool::new(
            "insert_before_symbol",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path containing the symbol"
                    },
                    "name_path": {
                        "type": "string",
                        "description": "Symbol name path to insert before"
                    },
                    "content": {
                        "type": "string",
                        "description": "Source code to insert before the symbol"
                    },
                    "content_hash": {
                        "type": "string",
                        "description": "SHA256 hash of file content for optimistic locking. If provided, edit fails when file has changed."
                    }
                },
                "required": ["path", "name_path", "content"]
            }),
        )
        .with_description("Insert code before a named symbol. Doesn't require line numbers — use when adding code adjacent to a known symbol."),
        Tool::new(
            "insert_after_symbol",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path containing the symbol"
                    },
                    "name_path": {
                        "type": "string",
                        "description": "Symbol name path to insert after"
                    },
                    "content": {
                        "type": "string",
                        "description": "Source code to insert after the symbol"
                    },
                    "content_hash": {
                        "type": "string",
                        "description": "SHA256 hash of file content for optimistic locking. If provided, edit fails when file has changed."
                    }
                },
                "required": ["path", "name_path", "content"]
            }),
        )
        .with_description("Insert code after a named symbol. Doesn't require line numbers — use when adding code adjacent to a known symbol."),
        Tool::new(
            "insert_at_line",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path"
                    },
                    "line": {
                        "type": "integer",
                        "description": "Line number to insert at (1-indexed)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to insert"
                    },
                    "content_hash": {
                        "type": "string",
                        "description": "SHA256 hash of file content for optimistic locking. If provided, edit fails when file has changed."
                    }
                },
                "required": ["path", "line", "content"]
            }),
        )
        .with_description("Insert content at a specific line number. Uses SHA256 content hash for optimistic locking. Use when symbol-based editing doesn't apply."),
        Tool::new(
            "replace_lines",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Start line (1-indexed, inclusive)"
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "End line (1-indexed, inclusive)"
                    },
                    "new_content": {
                        "type": "string",
                        "description": "Replacement content"
                    },
                    "content_hash": {
                        "type": "string",
                        "description": "SHA256 hash of file content for optimistic locking. If provided, edit fails when file has changed."
                    }
                },
                "required": ["path", "start_line", "end_line", "new_content"]
            }),
        )
        .with_description("Replace a range of lines with new content. Uses SHA256 content hash for optimistic locking. Use when symbol-based editing doesn't apply."),
        Tool::new(
            "rename_symbol",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path containing the symbol definition"
                    },
                    "name_path": {
                        "type": "string",
                        "description": "Symbol name path (e.g. 'MyStruct/my_method')"
                    },
                    "new_name": {
                        "type": "string",
                        "description": "New name for the symbol"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "Preview changes without writing files (default: false)",
                        "default": false
                    }
                },
                "required": ["path", "name_path", "new_name"]
            }),
        )
        .with_description("Rename a symbol across all files with word-boundary matching. Handles definition + all references. Use dry_run:true to preview changes."),
        Tool::new(
            "find_referencing_symbols",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "symbol_name": {
                        "type": "string",
                        "description": "Symbol name to find references for"
                    },
                    "scope_path": {
                        "type": "string",
                        "description": "Restrict search to this directory"
                    }
                },
                "required": ["symbol_name"]
            }),
        )
        .with_description("Like find_references but identifies which function/class contains each reference. Use for impact analysis before refactoring. Capped at 50."),
        Tool::new(
            "find_file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern (e.g. '*.rs', '**/test_*')"
                    }
                },
                "required": ["pattern"]
            }),
        )
        .with_description("Find files by glob pattern. Workspace-aware with gitignore support. Alternative to host's native file search."),
        Tool::new(
            "list_dir",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path (default: workspace root)"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "List recursively (default: false)",
                        "default": false
                    }
                }
            }),
        )
        .with_description("List directory contents with optional recursion. Respects gitignore. Alternative to host's native directory listing."),
    ]
}

/// Get memory management tools (5 + 2 meta-cognitive).
#[cfg(feature = "memory")]
pub fn dtx_memory_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "list_memories",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "description": "Filter by kind: user, project, feedback, reference"
                    },
                    "name_contains": {
                        "type": "string",
                        "description": "Filter by name substring"
                    },
                    "content_contains": {
                        "type": "string",
                        "description": "Filter by content substring"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by tags (match any)"
                    }
                }
            }),
        )
        .with_description("List memories with optional filters. Filter by kind, name substring, content substring, or tags. Use at session start to load existing project context."),
        Tool::new(
            "read_memory",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Memory name (kebab-case)"
                    }
                },
                "required": ["name"]
            }),
        )
        .with_description("Read a memory's full content by name. Use after list_memories to retrieve specific context. Memories persist across sessions."),
        Tool::new(
            "write_memory",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Memory name (kebab-case)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Memory content text"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Memory kind: user, project, feedback, reference",
                        "default": "project"
                    },
                    "description": {
                        "type": "string",
                        "description": "One-line description of the memory"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tags for categorization"
                    }
                },
                "required": ["name", "content"]
            }),
        )
        .with_description("Persist knowledge across sessions. Use for: architecture decisions, conventions, project context that shouldn't be re-discovered each time."),
        Tool::new(
            "edit_memory",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Memory name to edit"
                    },
                    "content": {
                        "type": "string",
                        "description": "New content (if provided, replaces existing)"
                    },
                    "description": {
                        "type": "string",
                        "description": "New description"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "New tags (replaces existing)"
                    }
                },
                "required": ["name"]
            }),
        )
        .with_description("Update a memory's content, description, or tags. Prefer over delete and rewrite for incremental updates."),
        Tool::new(
            "delete_memory",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Memory name to delete"
                    }
                },
                "required": ["name"]
            }),
        )
        .with_description("Delete a memory by name."),
        Tool::new(
            "reflect",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "focus": {
                        "type": "string",
                        "description": "Optional focus area to narrow analysis"
                    }
                }
            }),
        )
        .with_description("Synthesize project memory landscape. Shows distribution by kind, tag frequency, coverage gaps, staleness, and suggested actions. Use after research phases to identify what's known and what's missing."),
        Tool::new(
            "checkpoint",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "What was accomplished this session"
                    },
                    "decisions": {
                        "type": "string",
                        "description": "Key decisions made and their rationale"
                    },
                    "open_questions": {
                        "type": "string",
                        "description": "Unresolved items for next session"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Additional tags beyond auto-tags"
                    }
                },
                "required": ["summary"]
            }),
        )
        .with_description("Save structured session checkpoint to memory. Auto-names with timestamp, auto-tags with 'checkpoint' and 'session'. Creates data that reflect can analyze for temporal patterns."),
    ]
}

/// Get onboarding tools (2) — requires both code and memory features.
#[cfg(all(feature = "code", feature = "memory"))]
pub fn dtx_onboarding_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "onboarding",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "force": {
                        "type": "boolean",
                        "description": "Force re-run even if cached results exist (default: false)",
                        "default": false
                    },
                    "save_to_memory": {
                        "type": "boolean",
                        "description": "Save onboarding results to memory store (default: true)",
                        "default": true
                    }
                }
            }),
        )
        .with_description("Analyze project structure: directory tree, languages, frameworks, workspace layout, entry points. Returns cached result if recent (use force:true to re-run). Saves to memory."),
        Tool::new(
            "initial_instructions",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        )
        .with_description("Get dtx MCP tool guide with recommended workflow and when to use dtx vs native tools."),
    ]
}

/// Get all dtx tools (resource + code + memory + onboarding).
#[allow(unused_mut)]
pub fn dtx_tools() -> Vec<Tool> {
    let mut tools = dtx_resource_tools();

    #[cfg(feature = "code")]
    tools.extend(dtx_code_tools());

    #[cfg(feature = "memory")]
    tools.extend(dtx_memory_tools());

    #[cfg(all(feature = "code", feature = "memory"))]
    tools.extend(dtx_onboarding_tools());

    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtx_tools_defined() {
        let tools = dtx_tools();
        assert!(!tools.is_empty());

        let tool_names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"start_resource"));
        assert!(tool_names.contains(&"stop_resource"));
        assert!(tool_names.contains(&"list_resources"));
    }

    #[test]
    fn tool_result_text() {
        let result = CallToolResult::text("Resource started");
        assert!(result.is_error.is_none());
        assert_eq!(result.content.len(), 1);
    }

    #[test]
    fn tool_result_error() {
        let result = CallToolResult::error("Failed to start");
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn call_tool_params() {
        let params = CallToolParams {
            name: "start_resource".to_string(),
            arguments: Some(serde_json::json!({"id": "postgres"})),
        };

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("start_resource"));
        assert!(json.contains("postgres"));
    }
}
