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

/// Get all dtx tools.
pub fn dtx_tools() -> Vec<Tool> {
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
