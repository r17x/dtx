//! MCP resource types and URI scheme.
//!
//! Resources expose dtx entities to AI agents.

use serde::{Deserialize, Serialize};

/// MCP Resource definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    /// URI identifying the resource.
    pub uri: String,

    /// Human-readable name.
    pub name: String,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// MIME type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl Resource {
    /// Create a new resource.
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            name: name.into(),
            description: None,
            mime_type: None,
        }
    }

    /// Add a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set MIME type.
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

/// Resource content.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContent {
    /// Resource URI.
    pub uri: String,

    /// MIME type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Content.
    #[serde(flatten)]
    pub content: ResourceContentType,
}

/// Resource content type (text or blob).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResourceContentType {
    /// Text content.
    Text { text: String },

    /// Binary content (base64 encoded).
    Blob { blob: String },
}

impl ResourceContent {
    /// Create text content.
    pub fn text(uri: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            mime_type: Some("text/plain".to_string()),
            content: ResourceContentType::Text { text: text.into() },
        }
    }

    /// Create JSON content.
    pub fn json(uri: impl Into<String>, value: impl Serialize) -> Self {
        Self {
            uri: uri.into(),
            mime_type: Some("application/json".to_string()),
            content: ResourceContentType::Text {
                text: serde_json::to_string_pretty(&value).unwrap_or_default(),
            },
        }
    }
}

/// List resources result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListResourcesResult {
    /// Available resources.
    pub resources: Vec<Resource>,
}

/// Read resource params.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadResourceParams {
    /// Resource URI to read.
    pub uri: String,
}

/// Read resource result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// Resource contents.
    pub contents: Vec<ResourceContent>,
}

/// dtx resource URI helpers.
pub mod uris {
    use super::DxtUri;

    /// URI prefix for dtx resources.
    pub const PREFIX: &str = "dtx://";

    /// Create project URI.
    pub fn project(project_id: &str) -> String {
        format!("{}project/{}", PREFIX, project_id)
    }

    /// Create resource status URI.
    pub fn resource(project_id: &str, resource_id: &str) -> String {
        format!("{}project/{}/resource/{}", PREFIX, project_id, resource_id)
    }

    /// Create resource logs URI.
    pub fn logs(project_id: &str, resource_id: &str) -> String {
        format!(
            "{}project/{}/resource/{}/logs",
            PREFIX, project_id, resource_id
        )
    }

    /// Create resource config URI.
    pub fn config(project_id: &str, resource_id: &str) -> String {
        format!(
            "{}project/{}/resource/{}/config",
            PREFIX, project_id, resource_id
        )
    }

    /// Parse a dtx URI.
    pub fn parse(uri: &str) -> Option<DxtUri> {
        if !uri.starts_with(PREFIX) {
            return None;
        }

        let path = &uri[PREFIX.len()..];
        let parts: Vec<&str> = path.split('/').collect();

        match parts.as_slice() {
            ["project", project_id] => Some(DxtUri::Project {
                project_id: (*project_id).to_string(),
            }),
            ["project", project_id, "resource", resource_id] => Some(DxtUri::Resource {
                project_id: (*project_id).to_string(),
                resource_id: (*resource_id).to_string(),
            }),
            ["project", project_id, "resource", resource_id, "logs"] => Some(DxtUri::Logs {
                project_id: (*project_id).to_string(),
                resource_id: (*resource_id).to_string(),
            }),
            ["project", project_id, "resource", resource_id, "config"] => Some(DxtUri::Config {
                project_id: (*project_id).to_string(),
                resource_id: (*resource_id).to_string(),
            }),
            _ => None,
        }
    }
}

/// Parsed dtx URI.
#[derive(Clone, Debug, PartialEq)]
pub enum DxtUri {
    /// Project overview.
    Project { project_id: String },

    /// Resource status.
    Resource {
        project_id: String,
        resource_id: String,
    },

    /// Resource logs.
    Logs {
        project_id: String,
        resource_id: String,
    },

    /// Resource config.
    Config {
        project_id: String,
        resource_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_builder() {
        let resource = Resource::new("dtx://project/test", "test-project")
            .with_description("A test project")
            .with_mime_type("application/json");

        assert_eq!(resource.uri, "dtx://project/test");
        assert_eq!(resource.name, "test-project");
        assert!(resource.description.is_some());
    }

    #[test]
    fn uri_generation() {
        assert_eq!(uris::project("myapp"), "dtx://project/myapp");
        assert_eq!(
            uris::resource("myapp", "postgres"),
            "dtx://project/myapp/resource/postgres"
        );
        assert_eq!(
            uris::logs("myapp", "postgres"),
            "dtx://project/myapp/resource/postgres/logs"
        );
    }

    #[test]
    fn uri_parsing() {
        let uri = uris::parse("dtx://project/myapp");
        assert_eq!(
            uri,
            Some(DxtUri::Project {
                project_id: "myapp".to_string()
            })
        );

        let uri = uris::parse("dtx://project/myapp/resource/postgres");
        assert_eq!(
            uri,
            Some(DxtUri::Resource {
                project_id: "myapp".to_string(),
                resource_id: "postgres".to_string()
            })
        );

        let uri = uris::parse("https://example.com");
        assert_eq!(uri, None);
    }

    #[test]
    fn resource_content_text() {
        let content = ResourceContent::text("dtx://test", "Hello, World!");
        assert_eq!(content.mime_type, Some("text/plain".to_string()));

        if let ResourceContentType::Text { text } = content.content {
            assert_eq!(text, "Hello, World!");
        } else {
            panic!("Expected text content");
        }
    }
}
