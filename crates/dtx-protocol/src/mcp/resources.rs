//! MCP resource types and URI scheme.
//!
//! Resources expose dtx entities to AI agents.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
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

/// Percent-encoding set for path segments in dtx URIs.
const PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

fn encode_path(path: &str) -> String {
    utf8_percent_encode(path, PATH_ENCODE_SET).to_string()
}

fn decode_path(encoded: &str) -> String {
    percent_decode_str(encoded).decode_utf8_lossy().into_owned()
}

/// dtx resource URI helpers.
pub mod uris {
    use super::{decode_path, encode_path, DtxUri};

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

    /// Create memory list URI.
    pub fn memory_list() -> String {
        format!("{}memory", PREFIX)
    }

    /// Create memory item URI.
    pub fn memory_item(name: &str) -> String {
        format!("{}memory/{}", PREFIX, name)
    }

    /// Create code symbols URI for a file.
    pub fn code_symbols(path: &str) -> String {
        format!("{}code/{}/symbols", PREFIX, encode_path(path))
    }

    /// Create code symbol URI for a specific symbol in a file.
    pub fn code_symbol(path: &str, name_path: &str) -> String {
        format!(
            "{}code/{}/symbols/{}",
            PREFIX,
            encode_path(path),
            encode_path(name_path)
        )
    }

    /// Parse a dtx URI.
    pub fn parse(uri: &str) -> Option<DtxUri> {
        if !uri.starts_with(PREFIX) {
            return None;
        }

        let path = &uri[PREFIX.len()..];
        let parts: Vec<&str> = path.split('/').collect();

        match parts.as_slice() {
            ["project", project_id] => Some(DtxUri::Project {
                project_id: (*project_id).to_string(),
            }),
            ["project", project_id, "resource", resource_id] => Some(DtxUri::Resource {
                project_id: (*project_id).to_string(),
                resource_id: (*resource_id).to_string(),
            }),
            ["project", project_id, "resource", resource_id, "logs"] => Some(DtxUri::Logs {
                project_id: (*project_id).to_string(),
                resource_id: (*resource_id).to_string(),
            }),
            ["project", project_id, "resource", resource_id, "config"] => Some(DtxUri::Config {
                project_id: (*project_id).to_string(),
                resource_id: (*resource_id).to_string(),
            }),
            ["memory"] => Some(DtxUri::MemoryList),
            ["memory", name] => Some(DtxUri::MemoryItem {
                name: (*name).to_string(),
            }),
            ["code", encoded_path, "symbols"] => Some(DtxUri::CodeSymbols {
                path: decode_path(encoded_path),
            }),
            ["code", encoded_path, "symbols", encoded_name_path] => Some(DtxUri::CodeSymbol {
                path: decode_path(encoded_path),
                name_path: decode_path(encoded_name_path),
            }),
            _ => None,
        }
    }
}

/// Parsed dtx URI.
#[derive(Clone, Debug, PartialEq)]
pub enum DtxUri {
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

    /// Memory list.
    MemoryList,

    /// Single memory item.
    MemoryItem { name: String },

    /// Code symbols overview for a file.
    CodeSymbols { path: String },

    /// Single code symbol in a file.
    CodeSymbol { path: String, name_path: String },
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
            Some(DtxUri::Project {
                project_id: "myapp".to_string()
            })
        );

        let uri = uris::parse("dtx://project/myapp/resource/postgres");
        assert_eq!(
            uri,
            Some(DtxUri::Resource {
                project_id: "myapp".to_string(),
                resource_id: "postgres".to_string()
            })
        );

        let uri = uris::parse("https://example.com");
        assert_eq!(uri, None);
    }

    #[test]
    fn memory_uri_roundtrip() {
        let list_uri = uris::memory_list();
        assert_eq!(uris::parse(&list_uri), Some(DtxUri::MemoryList));

        let item_uri = uris::memory_item("my-notes");
        assert_eq!(
            uris::parse(&item_uri),
            Some(DtxUri::MemoryItem {
                name: "my-notes".to_string()
            })
        );
    }

    #[test]
    fn code_uri_roundtrip() {
        let symbols_uri = uris::code_symbols("src/main.rs");
        assert_eq!(
            uris::parse(&symbols_uri),
            Some(DtxUri::CodeSymbols {
                path: "src/main.rs".to_string()
            })
        );

        let symbol_uri = uris::code_symbol("src/main.rs", "MyStruct/new");
        assert_eq!(
            uris::parse(&symbol_uri),
            Some(DtxUri::CodeSymbol {
                path: "src/main.rs".to_string(),
                name_path: "MyStruct/new".to_string()
            })
        );
    }

    #[test]
    fn code_uri_with_special_chars() {
        let uri = uris::code_symbols("path with spaces/file.rs");
        assert_eq!(
            uris::parse(&uri),
            Some(DtxUri::CodeSymbols {
                path: "path with spaces/file.rs".to_string()
            })
        );
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
