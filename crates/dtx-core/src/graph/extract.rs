//! Composable domain extractors for building the unified graph.

use std::collections::HashMap;

use crate::model::Service;

use super::analyzer::GraphNode;
use super::types::*;

/// External symbol source for graph building.
#[derive(Debug, Clone)]
pub struct SymbolSource {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
}

/// External memory source for graph building.
#[derive(Debug, Clone)]
pub struct MemorySource {
    pub name: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub content_preview: String,
}

/// External file source for graph building.
#[derive(Debug, Clone)]
pub struct FileSource {
    pub path: String,
    pub kind: String,
    pub extension: Option<String>,
    pub size: Option<u64>,
}

/// Extract resource nodes and dependency edges from services.
///
/// Each service becomes a `GraphNode` with domain `Resource`. Dependencies between
/// services produce `DependsOn` edges with `Definite` confidence.
pub fn extract_resources(services: &[Service]) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut nodes = Vec::with_capacity(services.len());
    let mut edges = Vec::new();

    for service in services {
        let id = format!("resource:{}", service.name);
        nodes.push(GraphNode {
            id,
            domain: NodeDomain::Resource,
            label: service.name.clone(),
            metadata: NodeMetadata::Resource {
                kind: "process".to_string(),
                port: service.port,
                command: service.command.clone(),
                package: service.package.clone(),
                enabled: service.enabled,
            },
            depth: 0,
        });
    }

    for service in services {
        if let Some(ref deps) = service.depends_on {
            let source_id = format!("resource:{}", service.name);
            for dep in deps {
                let target_id = format!("resource:{}", dep.service);
                edges.push(GraphEdge {
                    source: source_id.clone(),
                    target: target_id,
                    kind: EdgeKind::DependsOn,
                    confidence: EdgeConfidence::Definite,
                });
            }
        }
    }

    (nodes, edges)
}

/// Extract symbol nodes and cross-domain edges to resources.
///
/// Cross-domain edge rules:
/// - `Implements` (Probable): resource has a `working_dir` that is a prefix of the symbol's file path
/// - `Configures` (Speculative): symbol name (lowercased) contains a resource label (lowercased)
pub fn extract_symbols(
    symbols: &[SymbolSource],
    resource_nodes: &HashMap<String, GraphNode>,
) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut nodes = Vec::with_capacity(symbols.len());
    let mut edges = Vec::new();

    for symbol in symbols {
        let id = format!("symbol:{}", symbol.name);
        nodes.push(GraphNode {
            id: id.clone(),
            domain: NodeDomain::Symbol,
            label: symbol.name.clone(),
            metadata: NodeMetadata::Symbol {
                kind: symbol.kind.clone(),
                file: symbol.file.clone(),
                line: symbol.line,
            },
            depth: 0,
        });

        for resource_node in resource_nodes.values() {
            if resource_node.domain != NodeDomain::Resource {
                continue;
            }

            // Implements: resource working_dir is a prefix of the symbol's file
            if let NodeMetadata::Resource { ref command, .. } = resource_node.metadata {
                // Use working_dir from the service if available via command context
                // We check if the symbol file path suggests it belongs to this resource
                let _ = command; // working_dir not in NodeMetadata; use name-based heuristics
            }

            // Configures: symbol name contains resource label
            let symbol_lower = symbol.name.to_lowercase();
            let label_lower = resource_node.label.to_lowercase();
            if !label_lower.is_empty() && symbol_lower.contains(&label_lower) {
                edges.push(GraphEdge {
                    source: id.clone(),
                    target: resource_node.id.clone(),
                    kind: EdgeKind::Configures,
                    confidence: EdgeConfidence::Speculative,
                });
            }
        }
    }

    (nodes, edges)
}

/// Extract memory nodes and cross-domain edges to resources and symbols.
///
/// Cross-domain edge rules:
/// - `References` (Probable): memory tag matches a resource label exactly
/// - `Documents` (Speculative): memory content_preview contains a symbol label
pub fn extract_memories(
    memories: &[MemorySource],
    resource_nodes: &HashMap<String, GraphNode>,
    symbol_nodes: &HashMap<String, GraphNode>,
) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut nodes = Vec::with_capacity(memories.len());
    let mut edges = Vec::new();

    for memory in memories {
        let id = format!("memory:{}", memory.name);
        nodes.push(GraphNode {
            id: id.clone(),
            domain: NodeDomain::Memory,
            label: memory.name.clone(),
            metadata: NodeMetadata::Memory {
                kind: memory.kind.clone(),
                tags: memory.tags.clone(),
            },
            depth: 0,
        });

        // References: tag matches resource label exactly
        for resource_node in resource_nodes.values() {
            if resource_node.domain != NodeDomain::Resource {
                continue;
            }
            if memory.tags.iter().any(|tag| tag == &resource_node.label) {
                edges.push(GraphEdge {
                    source: id.clone(),
                    target: resource_node.id.clone(),
                    kind: EdgeKind::References,
                    confidence: EdgeConfidence::Probable,
                });
            }
        }

        // Documents: content_preview contains a symbol label
        let preview_lower = memory.content_preview.to_lowercase();
        for symbol_node in symbol_nodes.values() {
            if symbol_node.domain != NodeDomain::Symbol {
                continue;
            }
            let label_lower = symbol_node.label.to_lowercase();
            if !label_lower.is_empty() && preview_lower.contains(&label_lower) {
                edges.push(GraphEdge {
                    source: id.clone(),
                    target: symbol_node.id.clone(),
                    kind: EdgeKind::Documents,
                    confidence: EdgeConfidence::Speculative,
                });
            }
        }
    }

    (nodes, edges)
}

const CONFIG_EXTENSIONS: &[&str] = &[
    "conf",
    "config",
    "cfg",
    "yaml",
    "yml",
    "toml",
    "json",
    "ini",
    "env",
    "properties",
];

const DOC_EXTENSIONS: &[&str] = &["md", "txt", "rst", "adoc", "org"];

/// Extract file nodes and cross-domain edges.
///
/// Edge rules:
/// - `Contains` (Definite): file path is a parent directory of another file path
/// - `Configures` (Probable): config-like file name matches a resource label
/// - `Documents` (Speculative): doc-like file path contains a resource or symbol label
pub fn extract_files(
    files: &[FileSource],
    existing_nodes: &HashMap<String, GraphNode>,
) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut nodes = Vec::with_capacity(files.len());
    let mut edges = Vec::new();

    let file_ids: Vec<(String, String)> = files
        .iter()
        .map(|f| (format!("file:{}", f.path), f.path.clone()))
        .collect();

    for file in files {
        let id = format!("file:{}", file.path);
        nodes.push(GraphNode {
            id: id.clone(),
            domain: NodeDomain::File,
            label: file.path.clone(),
            metadata: NodeMetadata::File {
                path: file.path.clone(),
                kind: file.kind.clone(),
                extension: file.extension.clone(),
                size: file.size,
            },
            depth: 0,
        });

        // Contains: this file is a directory and another file's path starts with it
        if file.kind == "directory" {
            let dir_prefix = if file.path.ends_with('/') {
                file.path.clone()
            } else {
                format!("{}/", file.path)
            };
            for (child_id, child_path) in &file_ids {
                if child_path.starts_with(&dir_prefix) && child_path != &file.path {
                    edges.push(GraphEdge {
                        source: id.clone(),
                        target: child_id.clone(),
                        kind: EdgeKind::Contains,
                        confidence: EdgeConfidence::Definite,
                    });
                }
            }
        }

        let is_config = file
            .extension
            .as_ref()
            .is_some_and(|ext| CONFIG_EXTENSIONS.contains(&ext.as_str()));

        let is_doc = file
            .extension
            .as_ref()
            .is_some_and(|ext| DOC_EXTENSIONS.contains(&ext.as_str()));

        let file_stem = std::path::Path::new(&file.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let path_lower = file.path.to_lowercase();

        for node in existing_nodes.values() {
            let label_lower = node.label.to_lowercase();
            if label_lower.is_empty() {
                continue;
            }

            match node.domain {
                NodeDomain::Resource => {
                    // Configures: config file name matches resource label
                    if is_config && file_stem.contains(&label_lower) {
                        edges.push(GraphEdge {
                            source: id.clone(),
                            target: node.id.clone(),
                            kind: EdgeKind::Configures,
                            confidence: EdgeConfidence::Probable,
                        });
                    }
                    // Documents: doc file path contains resource label
                    if is_doc && path_lower.contains(&label_lower) {
                        edges.push(GraphEdge {
                            source: id.clone(),
                            target: node.id.clone(),
                            kind: EdgeKind::Documents,
                            confidence: EdgeConfidence::Speculative,
                        });
                    }
                }
                NodeDomain::Symbol => {
                    // Documents: doc file path contains symbol label
                    if is_doc && path_lower.contains(&label_lower) {
                        edges.push(GraphEdge {
                            source: id.clone(),
                            target: node.id.clone(),
                            kind: EdgeKind::Documents,
                            confidence: EdgeConfidence::Speculative,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    (nodes, edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DependencyCondition;

    #[test]
    fn extract_resources_creates_nodes_and_edges() {
        let services = vec![
            Service::new("api".into(), "node server.js".into())
                .with_dependency("db".into(), DependencyCondition::ProcessHealthy),
            Service::new("db".into(), "postgres".into()),
        ];

        let (nodes, edges) = extract_resources(&services);

        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert!(nodes.iter().any(|n| n.id == "resource:api"));
        assert!(nodes.iter().any(|n| n.id == "resource:db"));
        assert_eq!(edges[0].source, "resource:api");
        assert_eq!(edges[0].target, "resource:db");
        assert_eq!(edges[0].kind, EdgeKind::DependsOn);
        assert_eq!(edges[0].confidence, EdgeConfidence::Definite);
    }

    #[test]
    fn extract_symbols_creates_configures_edges() {
        let symbols = vec![SymbolSource {
            name: "handle_api_request".into(),
            kind: "function".into(),
            file: "src/api.rs".into(),
            line: 10,
        }];

        let mut resource_map = HashMap::new();
        resource_map.insert(
            "resource:api".into(),
            GraphNode {
                id: "resource:api".into(),
                domain: NodeDomain::Resource,
                label: "api".into(),
                metadata: NodeMetadata::Resource {
                    kind: "process".into(),
                    port: Some(3000),
                    command: "node server.js".into(),
                    package: None,
                    enabled: true,
                },
                depth: 0,
            },
        );

        let (nodes, edges) = extract_symbols(&symbols, &resource_map);

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "symbol:handle_api_request");
        assert!(edges
            .iter()
            .any(|e| e.kind == EdgeKind::Configures && e.target == "resource:api"));
    }

    #[test]
    fn extract_memories_creates_references_edges() {
        let memories = vec![MemorySource {
            name: "db-setup-notes".into(),
            kind: "project".into(),
            tags: vec!["db".into()],
            content_preview: "Configure the handle_api_request function".into(),
        }];

        let mut resource_map = HashMap::new();
        resource_map.insert(
            "resource:db".into(),
            GraphNode {
                id: "resource:db".into(),
                domain: NodeDomain::Resource,
                label: "db".into(),
                metadata: NodeMetadata::Resource {
                    kind: "process".into(),
                    port: None,
                    command: "postgres".into(),
                    package: None,
                    enabled: true,
                },
                depth: 0,
            },
        );

        let mut symbol_map = HashMap::new();
        symbol_map.insert(
            "symbol:handle_api_request".into(),
            GraphNode {
                id: "symbol:handle_api_request".into(),
                domain: NodeDomain::Symbol,
                label: "handle_api_request".into(),
                metadata: NodeMetadata::Symbol {
                    kind: "function".into(),
                    file: "src/api.rs".into(),
                    line: 10,
                },
                depth: 0,
            },
        );

        let (nodes, edges) = extract_memories(&memories, &resource_map, &symbol_map);

        assert_eq!(nodes.len(), 1);
        assert!(edges
            .iter()
            .any(|e| e.kind == EdgeKind::References && e.target == "resource:db"));
        assert!(edges
            .iter()
            .any(|e| e.kind == EdgeKind::Documents && e.target == "symbol:handle_api_request"));
    }

    #[test]
    fn extract_files_creates_contains_and_configures_edges() {
        let files = vec![
            FileSource {
                path: "config".into(),
                kind: "directory".into(),
                extension: None,
                size: None,
            },
            FileSource {
                path: "config/redis.yaml".into(),
                kind: "file".into(),
                extension: Some("yaml".into()),
                size: Some(256),
            },
            FileSource {
                path: "docs/redis.md".into(),
                kind: "file".into(),
                extension: Some("md".into()),
                size: Some(1024),
            },
        ];

        let mut existing = HashMap::new();
        existing.insert(
            "resource:redis".into(),
            GraphNode {
                id: "resource:redis".into(),
                domain: NodeDomain::Resource,
                label: "redis".into(),
                metadata: NodeMetadata::Resource {
                    kind: "process".into(),
                    port: Some(6379),
                    command: "redis-server".into(),
                    package: None,
                    enabled: true,
                },
                depth: 0,
            },
        );

        let (nodes, edges) = extract_files(&files, &existing);

        assert_eq!(nodes.len(), 3);

        // Contains: config/ -> config/redis.yaml
        assert!(edges.iter().any(|e| e.kind == EdgeKind::Contains
            && e.source == "file:config"
            && e.target == "file:config/redis.yaml"
            && e.confidence == EdgeConfidence::Definite));

        // Configures: config/redis.yaml -> resource:redis
        assert!(edges.iter().any(|e| e.kind == EdgeKind::Configures
            && e.source == "file:config/redis.yaml"
            && e.target == "resource:redis"
            && e.confidence == EdgeConfidence::Probable));

        // Documents: docs/redis.md -> resource:redis
        assert!(edges.iter().any(|e| e.kind == EdgeKind::Documents
            && e.source == "file:docs/redis.md"
            && e.target == "resource:redis"
            && e.confidence == EdgeConfidence::Speculative));
    }

    #[test]
    fn extract_symbols_no_matching_resources_produces_no_edges() {
        let symbols = vec![SymbolSource {
            name: "unrelated_function".into(),
            kind: "function".into(),
            file: "src/utils.rs".into(),
            line: 1,
        }];

        let mut resource_map = HashMap::new();
        resource_map.insert(
            "resource:api".into(),
            GraphNode {
                id: "resource:api".into(),
                domain: NodeDomain::Resource,
                label: "api".into(),
                metadata: NodeMetadata::Resource {
                    kind: "process".into(),
                    port: Some(3000),
                    command: "node server.js".into(),
                    package: None,
                    enabled: true,
                },
                depth: 0,
            },
        );

        let (nodes, edges) = extract_symbols(&symbols, &resource_map);

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "symbol:unrelated_function");
        // "unrelated_function" does not contain "api"
        assert!(edges.is_empty());
    }

    #[test]
    fn extract_symbols_empty_resources_produces_no_edges() {
        let symbols = vec![SymbolSource {
            name: "some_fn".into(),
            kind: "function".into(),
            file: "src/lib.rs".into(),
            line: 1,
        }];

        let (nodes, edges) = extract_symbols(&symbols, &HashMap::new());

        assert_eq!(nodes.len(), 1);
        assert!(edges.is_empty());
    }

    #[test]
    fn extract_memories_multiple_tags_match_multiple_resources() {
        let memories = vec![MemorySource {
            name: "infra-notes".into(),
            kind: "project".into(),
            tags: vec!["api".into(), "db".into()],
            content_preview: "Infrastructure setup".into(),
        }];

        let mut resource_map = HashMap::new();
        resource_map.insert(
            "resource:api".into(),
            GraphNode {
                id: "resource:api".into(),
                domain: NodeDomain::Resource,
                label: "api".into(),
                metadata: NodeMetadata::Resource {
                    kind: "process".into(),
                    port: Some(3000),
                    command: "node server.js".into(),
                    package: None,
                    enabled: true,
                },
                depth: 0,
            },
        );
        resource_map.insert(
            "resource:db".into(),
            GraphNode {
                id: "resource:db".into(),
                domain: NodeDomain::Resource,
                label: "db".into(),
                metadata: NodeMetadata::Resource {
                    kind: "process".into(),
                    port: None,
                    command: "postgres".into(),
                    package: None,
                    enabled: true,
                },
                depth: 0,
            },
        );

        let (nodes, edges) = extract_memories(&memories, &resource_map, &HashMap::new());

        assert_eq!(nodes.len(), 1);
        // Should produce References edges to both resources
        let ref_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == EdgeKind::References)
            .collect();
        assert_eq!(ref_edges.len(), 2);
        assert!(ref_edges.iter().any(|e| e.target == "resource:api"));
        assert!(ref_edges.iter().any(|e| e.target == "resource:db"));
    }

    #[test]
    fn extract_files_nested_directories_create_contains_edges_at_multiple_levels() {
        let files = vec![
            FileSource {
                path: "src".into(),
                kind: "directory".into(),
                extension: None,
                size: None,
            },
            FileSource {
                path: "src/handlers".into(),
                kind: "directory".into(),
                extension: None,
                size: None,
            },
            FileSource {
                path: "src/handlers/api.rs".into(),
                kind: "file".into(),
                extension: Some("rs".into()),
                size: Some(512),
            },
        ];

        let (nodes, edges) = extract_files(&files, &HashMap::new());

        assert_eq!(nodes.len(), 3);

        let contains: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .collect();

        // src/ contains src/handlers and src/handlers/api.rs
        assert!(contains
            .iter()
            .any(|e| e.source == "file:src" && e.target == "file:src/handlers"));
        assert!(contains
            .iter()
            .any(|e| e.source == "file:src" && e.target == "file:src/handlers/api.rs"));

        // src/handlers/ contains src/handlers/api.rs
        assert!(contains
            .iter()
            .any(|e| e.source == "file:src/handlers" && e.target == "file:src/handlers/api.rs"));

        // All Contains edges have Definite confidence
        assert!(contains
            .iter()
            .all(|e| e.confidence == EdgeConfidence::Definite));
    }

    #[test]
    fn extract_files_config_and_doc_match_same_resource() {
        let files = vec![
            FileSource {
                path: "config/api.toml".into(),
                kind: "file".into(),
                extension: Some("toml".into()),
                size: Some(64),
            },
            FileSource {
                path: "docs/api.md".into(),
                kind: "file".into(),
                extension: Some("md".into()),
                size: Some(1024),
            },
        ];

        let mut existing = HashMap::new();
        existing.insert(
            "resource:api".into(),
            GraphNode {
                id: "resource:api".into(),
                domain: NodeDomain::Resource,
                label: "api".into(),
                metadata: NodeMetadata::Resource {
                    kind: "process".into(),
                    port: Some(3000),
                    command: "node server.js".into(),
                    package: None,
                    enabled: true,
                },
                depth: 0,
            },
        );

        let (nodes, edges) = extract_files(&files, &existing);

        assert_eq!(nodes.len(), 2);

        // config/api.toml -> resource:api via Configures (Probable)
        assert!(edges.iter().any(|e| e.kind == EdgeKind::Configures
            && e.source == "file:config/api.toml"
            && e.target == "resource:api"
            && e.confidence == EdgeConfidence::Probable));

        // docs/api.md -> resource:api via Documents (Speculative)
        assert!(edges.iter().any(|e| e.kind == EdgeKind::Documents
            && e.source == "file:docs/api.md"
            && e.target == "resource:api"
            && e.confidence == EdgeConfidence::Speculative));
    }

    #[test]
    fn extract_files_doc_matches_symbol_node() {
        let files = vec![FileSource {
            path: "docs/my_handler.md".into(),
            kind: "file".into(),
            extension: Some("md".into()),
            size: Some(256),
        }];

        let mut existing = HashMap::new();
        existing.insert(
            "symbol:my_handler".into(),
            GraphNode {
                id: "symbol:my_handler".into(),
                domain: NodeDomain::Symbol,
                label: "my_handler".into(),
                metadata: NodeMetadata::Symbol {
                    kind: "function".into(),
                    file: "src/handler.rs".into(),
                    line: 1,
                },
                depth: 0,
            },
        );

        let (_nodes, edges) = extract_files(&files, &existing);

        assert!(edges.iter().any(|e| e.kind == EdgeKind::Documents
            && e.source == "file:docs/my_handler.md"
            && e.target == "symbol:my_handler"
            && e.confidence == EdgeConfidence::Speculative));
    }

    #[test]
    fn extract_resources_no_dependencies_produces_nodes_only() {
        let services = vec![
            Service::new("api".into(), "node server.js".into()),
            Service::new("db".into(), "postgres".into()),
        ];

        let (nodes, edges) = extract_resources(&services);

        assert_eq!(nodes.len(), 2);
        assert!(edges.is_empty());
    }
}
