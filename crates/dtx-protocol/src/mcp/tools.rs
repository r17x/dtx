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
        .with_description("Start a resource by ID"),
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
        .with_description("Get status of a resource"),
        Tool::new(
            "list_resources",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        )
        .with_description("List all resources"),
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
        .with_description("Get recent logs for a resource"),
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
        .with_description("Get symbol overview for a file"),
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
        .with_description("Find symbols by name path pattern"),
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
        .with_description("Find all references to a symbol"),
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
        .with_description("Search for a regex pattern across files"),
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
                    }
                },
                "required": ["path", "name_path", "new_body"]
            }),
        )
        .with_description("Replace a symbol's body with new source code"),
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
                    }
                },
                "required": ["path", "name_path", "content"]
            }),
        )
        .with_description("Insert code before a symbol"),
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
                    }
                },
                "required": ["path", "name_path", "content"]
            }),
        )
        .with_description("Insert code after a symbol"),
    ]
}

/// Get memory management tools (5).
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
                    }
                }
            }),
        )
        .with_description("List all memories"),
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
        .with_description("Read a memory by name"),
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
        .with_description("Create or update a memory"),
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
        .with_description("Edit an existing memory's metadata or content"),
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
        .with_description("Delete a memory"),
    ]
}

/// Get all dtx tools (resource + code + memory).
#[allow(unused_mut)]
pub fn dtx_tools() -> Vec<Tool> {
    let mut tools = dtx_resource_tools();

    #[cfg(feature = "code")]
    tools.extend(dtx_code_tools());

    #[cfg(feature = "memory")]
    tools.extend(dtx_memory_tools());

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
