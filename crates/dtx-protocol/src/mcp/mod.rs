//! Model Context Protocol (MCP) integration.
//!
//! This module provides MCP-specific types and handlers for AI agent integration:
//!
//! - **Types**: Capability negotiation and initialization
//! - **Resources**: Exposing dtx resources to AI agents
//! - **Tools**: AI-callable operations
//! - **Handler**: MCP request handling

mod handler;
mod resources;
mod tools;
mod types;

pub use handler::{DefaultMcpHandler, McpHandler};
pub use resources::{
    uris, DxtUri, ListResourcesResult, ReadResourceParams, ReadResourceResult, Resource,
    ResourceContent, ResourceContentType,
};
pub use tools::{dtx_tools, CallToolParams, CallToolResult, ListToolsResult, Tool, ToolContent};
pub use types::{
    ClientCapabilities, ClientInfo, InitializeParams, InitializeResult, PromptCapabilities,
    ResourceCapabilities, RootCapabilities, SamplingCapabilities, ServerCapabilities, ServerInfo,
    ToolCapabilities,
};
