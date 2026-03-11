//! JSON-RPC protocol and MCP integration for dtx.
//!
//! This crate provides the communication protocol layer for dtx:
//!
//! - **JSON-RPC 2.0**: Core message types and error codes
//! - **Methods**: Protocol method definitions and parameter types
//! - **Handler**: Request dispatching and handler traits
//! - **Transport**: stdio, HTTP, and WebSocket transports
//! - **MCP**: Model Context Protocol integration for AI agents
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        MCP Layer                            │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
//! │  │  Resources  │  │    Tools    │  │   Handler   │         │
//! │  └─────────────┘  └─────────────┘  └─────────────┘         │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Protocol Layer                           │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
//! │  │   Methods   │  │   Handler   │  │   Dispatch  │         │
//! │  └─────────────┘  └─────────────┘  └─────────────┘         │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    JSON-RPC Core                            │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
//! │  │   Request   │  │  Response   │  │   Errors    │         │
//! │  └─────────────┘  └─────────────┘  └─────────────┘         │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Transport Layer                          │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
//! │  │    stdio    │  │    HTTP     │  │  WebSocket  │         │
//! │  └─────────────┘  └─────────────┘  └─────────────┘         │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use dtx_protocol::{Request, Response, dispatch};
//! use dtx_protocol::transport::StdioTransport;
//! use dtx_protocol::mcp::McpHandler;
//!
//! // Serve MCP over stdio
//! let handler = MyMcpHandler::new();
//! serve_stdio(|request| dispatch(&handler, request)).await?;
//! ```

pub mod handler;
pub mod jsonrpc;
pub mod mcp;
pub mod methods;
pub mod nl;
pub mod transport;

// Re-exports: JSON-RPC core
pub use jsonrpc::{error_codes, ErrorObject, Request, RequestId, Response, JSONRPC_VERSION};

// Re-exports: Methods (types and constants from methods module)
pub use methods::{
    ConfigureParams, EventFilter, LogEntry, LogsParams, ResourceListResult, ResourceParams,
    ResourceStatusResult, SubscribeParams, SubscribeResult,
};

// Re-exports: Handler
pub use handler::{dispatch, ProtocolHandler};

// Re-exports: Transport
pub use transport::{ServerTransport, Transport, TransportError};

// Re-exports: MCP
pub use mcp::{
    dtx_tools, CallToolParams, CallToolResult, ClientCapabilities, ClientInfo, InitializeParams,
    InitializeResult, ListResourcesResult, ListToolsResult, McpHandler, ReadResourceParams,
    ReadResourceResult, Resource as McpResource, ResourceContent, ServerCapabilities, ServerInfo,
    Tool, ToolContent,
};

// Re-exports: Natural Language
pub use nl::{IntentExecutor, IntentParser, ParsedIntent};
