//! Multi-domain graph types for the unified knowledge graph.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Domain classification for graph nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeDomain {
    Resource,
    Symbol,
    Memory,
    File,
    Group,
}

/// Domain-specific metadata attached to each graph node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeMetadata {
    Resource {
        kind: String,
        port: Option<u16>,
        command: String,
        package: Option<String>,
        enabled: bool,
    },
    Symbol {
        kind: String,
        file: String,
        line: usize,
    },
    Memory {
        kind: String,
        tags: Vec<String>,
    },
    File {
        path: String,
        kind: String,
        extension: Option<String>,
        size: Option<u64>,
    },
    Group {
        child_domain: NodeDomain,
        count: usize,
        representative: String,
    },
}

/// Classification of relationships between graph nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    DependsOn,
    Provides,
    Implements,
    Configures,
    References,
    Documents,
    Calls,
    Contains,
}

/// Confidence level for inferred edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeConfidence {
    Speculative,
    Probable,
    Definite,
}

/// A directed edge in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub confidence: EdgeConfidence,
}

/// Predefined views that filter the graph to specific domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphView {
    Processes,
    Code,
    Memories,
    Files,
    Knowledge,
}

/// Tracks which domains have been populated in the graph.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainStatus {
    pub resource: bool,
    pub symbol: bool,
    pub memory: bool,
    pub file: bool,
}

/// Result of impact analysis from a root node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactSet {
    pub root: String,
    pub affected: Vec<ImpactEntry>,
}

/// A single entry in an impact analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactEntry {
    pub id: String,
    pub domain: NodeDomain,
    pub label: String,
    pub distance: usize,
    pub path_confidence: EdgeConfidence,
}

/// Aggregate statistics about the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub nodes_by_domain: HashMap<NodeDomain, usize>,
    pub edges_by_kind: HashMap<EdgeKind, usize>,
    pub edges_by_confidence: HashMap<EdgeConfidence, usize>,
    pub domains: DomainStatus,
    pub max_depth: usize,
}
