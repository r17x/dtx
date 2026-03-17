//! Shared types used across dtx-web handlers.

use serde::Serialize;

/// Package analysis info for display in mappings views.
///
/// Used by both full-page HTML and HTMX partial handlers.
#[derive(Clone, Debug)]
pub struct PackageAnalysis {
    pub service_name: String,
    pub command: String,
    pub status: String,
    pub status_class: String,
    pub package: Option<String>,
    pub executable: Option<String>,
    /// Key to use for mapping operations (executable or command).
    pub mapping_key: String,
}

/// Status update event data sent via SSE.
#[derive(Clone, Debug, Serialize)]
pub struct StatusUpdate {
    pub running: bool,
    pub services: Vec<ServiceStatus>,
    pub timestamp: String,
}

/// Service status information within a status update.
#[derive(Clone, Debug, Serialize)]
pub struct ServiceStatus {
    pub name: String,
    pub status: String,
    pub is_running: bool,
    pub pid: u32,
    pub restarts: u32,
}

/// Log entry sent via SSE for service log streams.
#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub service: String,
    pub message: String,
    pub timestamp: String,
    pub level: String,
}

/// Response for process control operations.
#[derive(Clone, Debug, Serialize)]
pub struct ProcessResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_reassignments: Option<Vec<PortReassignmentInfo>>,
}

/// Information about a port reassignment.
#[derive(Clone, Debug, Serialize)]
pub struct PortReassignmentInfo {
    pub service: String,
    pub original_port: u16,
    pub assigned_port: u16,
}
