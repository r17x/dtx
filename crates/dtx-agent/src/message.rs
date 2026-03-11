//! Message types for agent conversations.
//!
//! This module defines the message format used for communication
//! with AI agent runtimes.

use serde::{Deserialize, Serialize};

/// A message in the conversation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message sender.
    pub role: Role,
    /// Content of the message.
    pub content: Content,
    /// Optional name for the sender.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Message role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System message (instructions).
    System,
    /// User message.
    User,
    /// Assistant response.
    Assistant,
    /// Tool result.
    Tool,
}

/// Message content.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// Simple text content.
    Text(String),
    /// Multiple content blocks.
    Blocks(Vec<ContentBlock>),
}

impl Content {
    /// Get the text representation of the content.
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// Content block.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content.
    Text { text: String },
    /// Image content.
    Image { source: ImageSource },
    /// Tool use request.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool execution result.
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// Image source.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64 encoded image.
    Base64 { media_type: String, data: String },
    /// Image URL.
    Url { url: String },
}

/// Tool call request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this call.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Tool arguments.
    pub input: serde_json::Value,
}

impl ToolCall {
    /// Create a new tool call.
    pub fn new(id: impl Into<String>, name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input,
        }
    }
}

/// Tool execution result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool call ID this result corresponds to.
    pub tool_use_id: String,
    /// Result content.
    pub content: String,
    /// Whether this is an error result.
    #[serde(default)]
    pub is_error: bool,
}

impl ToolResult {
    /// Create a successful tool result.
    pub fn success(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: false,
        }
    }

    /// Create an error tool result.
    pub fn error(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: true,
        }
    }
}

impl Message {
    /// Create a user message.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Content::Text(text.into()),
            name: None,
        }
    }

    /// Create an assistant message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Content::Text(text.into()),
            name: None,
        }
    }

    /// Create a system message.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Content::Text(text.into()),
            name: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error,
            }]),
            name: None,
        }
    }

    /// Create an assistant message with tool use.
    pub fn assistant_with_tools(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        let text = text.into();
        let mut blocks = Vec::new();

        if !text.is_empty() {
            blocks.push(ContentBlock::Text { text });
        }

        for tc in tool_calls {
            blocks.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.name,
                input: tc.input,
            });
        }

        Self {
            role: Role::Assistant,
            content: Content::Blocks(blocks),
            name: None,
        }
    }

    /// Create a user message with an image.
    pub fn user_with_image(text: impl Into<String>, image: ImageSource) -> Self {
        Self {
            role: Role::User,
            content: Content::Blocks(vec![
                ContentBlock::Text { text: text.into() },
                ContentBlock::Image { source: image },
            ]),
            name: None,
        }
    }

    /// Set the sender name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Get the text content of the message.
    pub fn text(&self) -> String {
        self.content.as_text()
    }

    /// Check if this is a system message.
    pub fn is_system(&self) -> bool {
        self.role == Role::System
    }

    /// Check if this is a user message.
    pub fn is_user(&self) -> bool {
        self.role == Role::User
    }

    /// Check if this is an assistant message.
    pub fn is_assistant(&self) -> bool {
        self.role == Role::Assistant
    }

    /// Check if this is a tool result message.
    pub fn is_tool(&self) -> bool {
        self.role == Role::Tool
    }

    /// Extract tool calls from this message.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        match &self.content {
            Content::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }),
                    _ => None,
                })
                .collect(),
            Content::Text(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_user() {
        let msg = Message::user("Hello!");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "Hello!");
        assert!(msg.is_user());
    }

    #[test]
    fn message_assistant() {
        let msg = Message::assistant("Hi there!");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.text(), "Hi there!");
        assert!(msg.is_assistant());
    }

    #[test]
    fn message_system() {
        let msg = Message::system("You are a helpful assistant.");
        assert_eq!(msg.role, Role::System);
        assert!(msg.is_system());
    }

    #[test]
    fn message_tool_result() {
        let msg = Message::tool_result("call-123", "Result content", false);
        assert_eq!(msg.role, Role::Tool);
        assert!(msg.is_tool());
    }

    #[test]
    fn message_with_name() {
        let msg = Message::user("Hello").with_name("Alice");
        assert_eq!(msg.name, Some("Alice".to_string()));
    }

    #[test]
    fn message_assistant_with_tools() {
        let tool_calls = vec![ToolCall::new(
            "call-1",
            "read_file",
            serde_json::json!({"path": "/tmp/test.txt"}),
        )];
        let msg = Message::assistant_with_tools("Let me read that file.", tool_calls);

        let extracted = msg.tool_calls();
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].name, "read_file");
    }

    #[test]
    fn message_serde_roundtrip() {
        let msg = Message::user("Hello, world!");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.text(), "Hello, world!");
    }

    #[test]
    fn tool_call_new() {
        let tc = ToolCall::new("id-1", "test_tool", serde_json::json!({"arg": "value"}));
        assert_eq!(tc.id, "id-1");
        assert_eq!(tc.name, "test_tool");
    }

    #[test]
    fn tool_result_success() {
        let result = ToolResult::success("call-1", "Success!");
        assert!(!result.is_error);
        assert_eq!(result.content, "Success!");
    }

    #[test]
    fn tool_result_error() {
        let result = ToolResult::error("call-1", "Failed!");
        assert!(result.is_error);
        assert_eq!(result.content, "Failed!");
    }

    #[test]
    fn content_as_text() {
        let text = Content::Text("Hello".to_string());
        assert_eq!(text.as_text(), "Hello");

        let blocks = Content::Blocks(vec![
            ContentBlock::Text {
                text: "Line 1".to_string(),
            },
            ContentBlock::Text {
                text: "Line 2".to_string(),
            },
        ]);
        assert_eq!(blocks.as_text(), "Line 1\nLine 2");
    }

    #[test]
    fn content_block_serde() {
        let block = ContentBlock::ToolUse {
            id: "call-1".to_string(),
            name: "test".to_string(),
            input: serde_json::json!({}),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_use");
        assert_eq!(json["id"], "call-1");
    }

    #[test]
    fn image_source_base64() {
        let source = ImageSource::Base64 {
            media_type: "image/png".to_string(),
            data: "base64data".to_string(),
        };
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(json["type"], "base64");
        assert_eq!(json["media_type"], "image/png");
    }

    #[test]
    fn image_source_url() {
        let source = ImageSource::Url {
            url: "https://example.com/image.png".to_string(),
        };
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(json["type"], "url");
    }

    #[test]
    fn role_serde() {
        let role = Role::Assistant;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"assistant\"");

        let parsed: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Role::Assistant);
    }
}
