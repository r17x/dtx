//! MCP capability and initialization types.
//!
//! These types implement the MCP initialization handshake protocol.

use serde::{Deserialize, Serialize};

/// MCP protocol version.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Server capabilities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Resource capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceCapabilities>,

    /// Tool capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolCapabilities>,

    /// Prompt capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptCapabilities>,
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            resources: Some(ResourceCapabilities {
                subscribe: true,
                list_changed: true,
            }),
            tools: Some(ToolCapabilities { list_changed: true }),
            prompts: None,
        }
    }
}

/// Resource capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCapabilities {
    /// Supports resource subscription.
    #[serde(default)]
    pub subscribe: bool,

    /// Notifies when resource list changes.
    #[serde(default)]
    pub list_changed: bool,
}

/// Tool capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapabilities {
    /// Notifies when tool list changes.
    #[serde(default)]
    pub list_changed: bool,
}

/// Prompt capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCapabilities {
    /// Notifies when prompt list changes.
    #[serde(default)]
    pub list_changed: bool,
}

/// Server info.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Server name.
    pub name: String,

    /// Server version.
    pub version: String,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            name: "dtx".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Client capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Root capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootCapabilities>,

    /// Sampling capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapabilities>,
}

/// Root capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootCapabilities {
    /// Notifies when roots change.
    #[serde(default)]
    pub list_changed: bool,
}

/// Sampling capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SamplingCapabilities {}

/// Client info.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Client name.
    pub name: String,

    /// Client version.
    pub version: String,
}

/// Initialize request parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Protocol version.
    pub protocol_version: String,

    /// Client capabilities.
    pub capabilities: ClientCapabilities,

    /// Client info.
    pub client_info: ClientInfo,
}

/// Initialize response result.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Protocol version.
    pub protocol_version: String,

    /// Server capabilities.
    pub capabilities: ServerCapabilities,

    /// Server info.
    pub server_info: ServerInfo,

    /// Instructions for the AI agent on how to use this server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl Default for InitializeResult {
    fn default() -> Self {
        Self {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: ServerInfo::default(),
            instructions: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_server_capabilities() {
        let caps = ServerCapabilities::default();
        assert!(caps.resources.is_some());
        assert!(caps.tools.is_some());
        assert!(caps.prompts.is_none());
    }

    #[test]
    fn initialize_result_serialization() {
        let result = InitializeResult::default();
        let json = serde_json::to_string(&result).unwrap();

        assert!(json.contains("protocolVersion"));
        assert!(json.contains("capabilities"));
        assert!(json.contains("serverInfo"));
        // instructions is None by default, so should be skipped
        assert!(!json.contains("instructions"));

        // With instructions set
        let result_with_instructions = InitializeResult {
            instructions: Some("test instructions".to_string()),
            ..Default::default()
        };
        let json2 = serde_json::to_string(&result_with_instructions).unwrap();
        assert!(json2.contains("instructions"));
        assert!(json2.contains("test instructions"));
    }

    #[test]
    fn initialize_params_deserialization() {
        let json = r#"{
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }"#;

        let params: InitializeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.protocol_version, "2024-11-05");
        assert_eq!(params.client_info.name, "test-client");
    }
}
