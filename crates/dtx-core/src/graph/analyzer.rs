//! Multi-domain dependency graph analysis.

use crate::model::Service;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use super::extract::{
    extract_files, extract_memories, extract_resources, extract_symbols, FileSource, MemorySource,
    SymbolSource,
};
use super::types::{
    DomainStatus, EdgeConfidence, EdgeKind, GraphEdge, GraphStats, GraphView, ImpactEntry,
    ImpactSet, NodeDomain, NodeMetadata,
};

/// A node in the unified knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub domain: NodeDomain,
    pub label: String,
    pub metadata: NodeMetadata,
    pub depth: usize,
}

impl GraphNode {
    /// Returns true if this node is a group (collapsed aggregate).
    pub fn is_group(&self) -> bool {
        matches!(self.metadata, NodeMetadata::Group { .. })
    }
}

/// Sources for building a multi-domain knowledge graph.
pub struct GraphSources<'a> {
    pub services: &'a [Service],
    pub symbols: Vec<SymbolSource>,
    pub memories: Vec<MemorySource>,
    pub files: Vec<FileSource>,
}

/// Multi-domain dependency graph supporting resource, symbol, memory, and file nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub nodes: HashMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub roots: Vec<String>,
    pub leaves: Vec<String>,
    pub max_depth: usize,
    pub domains: DomainStatus,
}

impl DependencyGraph {
    /// Backward-compatible constructor: builds a resource-only graph from services.
    pub fn from_services(services: &[Service]) -> Self {
        Self::build(GraphSources {
            services,
            symbols: Vec::new(),
            memories: Vec::new(),
            files: Vec::new(),
        })
    }

    /// Main entry point for graph construction from multiple sources.
    pub fn build(sources: GraphSources<'_>) -> Self {
        // 1. Extract resources
        let (resource_nodes, resource_edges) = extract_resources(sources.services);
        let resource_map: HashMap<String, GraphNode> = resource_nodes
            .iter()
            .map(|n| (n.id.clone(), n.clone()))
            .collect();

        // 2. Extract symbols
        let (symbol_nodes, symbol_edges) = extract_symbols(&sources.symbols, &resource_map);
        let symbol_map: HashMap<String, GraphNode> = symbol_nodes
            .iter()
            .map(|n| (n.id.clone(), n.clone()))
            .collect();

        // 3. Extract memories
        let (memory_nodes, memory_edges) =
            extract_memories(&sources.memories, &resource_map, &symbol_map);

        // 4. Extract files — build combined map of all existing nodes for cross-domain edges
        let mut all_nodes_map: HashMap<String, GraphNode> = resource_map.clone();
        for (k, v) in &symbol_map {
            all_nodes_map.insert(k.clone(), v.clone());
        }
        for node in &memory_nodes {
            all_nodes_map.insert(node.id.clone(), node.clone());
        }
        let (file_nodes, file_edges) = extract_files(&sources.files, &all_nodes_map);

        // Merge all nodes
        let mut nodes: HashMap<String, GraphNode> = HashMap::new();
        for node in resource_nodes
            .into_iter()
            .chain(symbol_nodes)
            .chain(memory_nodes)
            .chain(file_nodes)
        {
            nodes.insert(node.id.clone(), node);
        }

        // Merge all edges
        let mut edges = Vec::new();
        edges.extend(resource_edges);
        edges.extend(symbol_edges);
        edges.extend(memory_edges);
        edges.extend(file_edges);

        // Calculate depths via DependsOn edges
        let mut max_depth = 0;
        let node_ids: Vec<String> = nodes.keys().cloned().collect();
        for id in &node_ids {
            let depth = Self::calculate_depth(id, &nodes, &edges, &mut HashSet::new());
            if let Some(node) = nodes.get_mut(id) {
                node.depth = depth;
                max_depth = max_depth.max(depth);
            }
        }

        // roots = not a source of any DependsOn edge
        // leaves = not a target of any DependsOn edge
        let depends_on_sources: HashSet<&str> = edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
            .map(|e| e.source.as_str())
            .collect();

        let depends_on_targets: HashSet<&str> = edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
            .map(|e| e.target.as_str())
            .collect();

        let roots: Vec<String> = nodes
            .keys()
            .filter(|id| !depends_on_sources.contains(id.as_str()))
            .cloned()
            .collect();

        let leaves: Vec<String> = nodes
            .keys()
            .filter(|id| !depends_on_targets.contains(id.as_str()))
            .cloned()
            .collect();

        let domains = DomainStatus {
            resource: nodes.values().any(|n| n.domain == NodeDomain::Resource),
            symbol: nodes.values().any(|n| n.domain == NodeDomain::Symbol),
            memory: nodes.values().any(|n| n.domain == NodeDomain::Memory),
            file: nodes.values().any(|n| n.domain == NodeDomain::File),
        };

        Self {
            nodes,
            edges,
            roots,
            leaves,
            max_depth,
            domains,
        }
    }

    fn calculate_depth(
        node_id: &str,
        nodes: &HashMap<String, GraphNode>,
        edges: &[GraphEdge],
        visited: &mut HashSet<String>,
    ) -> usize {
        if visited.contains(node_id) {
            return 0;
        }
        if !nodes.contains_key(node_id) {
            return 0;
        }

        visited.insert(node_id.to_string());

        // Find DependsOn edges where this node is the source
        let dep_targets: Vec<&str> = edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn && e.source == node_id)
            .map(|e| e.target.as_str())
            .collect();

        if dep_targets.is_empty() {
            visited.remove(node_id);
            return 0;
        }

        let max_dep_depth = dep_targets
            .iter()
            .map(|target| Self::calculate_depth(target, nodes, edges, visited))
            .max()
            .unwrap_or(0);

        visited.remove(node_id);
        max_dep_depth + 1
    }

    /// Topological sort using Kahn's algorithm on DependsOn edges only.
    ///
    /// Returns `None` if the graph contains cycles.
    pub fn topological_sort(&self) -> Option<Vec<String>> {
        // Only consider resource-domain nodes and DependsOn edges
        let resource_ids: HashSet<&str> = self
            .nodes
            .values()
            .filter(|n| n.domain == NodeDomain::Resource)
            .map(|n| n.id.as_str())
            .collect();

        let depends_on_edges: Vec<&GraphEdge> = self
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
            .filter(|e| {
                resource_ids.contains(e.source.as_str()) && resource_ids.contains(e.target.as_str())
            })
            .collect();

        // Calculate in-degrees (number of DependsOn edges pointing to each node)
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for id in &resource_ids {
            in_degree.insert(id, 0);
        }
        for edge in &depends_on_edges {
            // source DependsOn target means source has a dependency on target
            // In topological sort terms, target must come before source
            // So in_degree of source increases
            *in_degree.entry(edge.source.as_str()).or_insert(0) += 1;
        }

        // Start with nodes that have no dependencies (in_degree == 0)
        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut result = Vec::new();

        while let Some(current) = queue.pop() {
            result.push(current.to_string());

            // For each edge where current is the target (i.e., some node depends on current),
            // reduce that node's in-degree
            for edge in &depends_on_edges {
                if edge.target == current {
                    if let Some(degree) = in_degree.get_mut(edge.source.as_str()) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(edge.source.as_str());
                        }
                    }
                }
            }
        }

        if result.len() != resource_ids.len() {
            None
        } else {
            Some(result)
        }
    }

    /// BFS impact analysis from a node, following outgoing edges with confidence >= min_confidence.
    pub fn impact(&self, node_id: &str, min_confidence: EdgeConfidence) -> ImpactSet {
        let mut affected = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        visited.insert(node_id.to_string());
        queue.push_back((node_id.to_string(), 0usize, min_confidence));

        while let Some((current, distance, path_conf)) = queue.pop_front() {
            for edge in self.outgoing(&current) {
                if edge.confidence >= min_confidence && !visited.contains(&edge.target) {
                    visited.insert(edge.target.clone());
                    let edge_conf = std::cmp::min(path_conf, edge.confidence);
                    let next_distance = distance + 1;

                    if let Some(target_node) = self.nodes.get(&edge.target) {
                        affected.push(ImpactEntry {
                            id: edge.target.clone(),
                            domain: target_node.domain,
                            label: target_node.label.clone(),
                            distance: next_distance,
                            path_confidence: edge_conf,
                        });
                    }

                    queue.push_back((edge.target.clone(), next_distance, edge_conf));
                }
            }
        }

        ImpactSet {
            root: node_id.to_string(),
            affected,
        }
    }

    /// All nodes transitively upstream (follow edges where target == node_id).
    pub fn upstream(&self, node_id: &str) -> Vec<String> {
        let mut result = HashSet::new();
        let mut to_visit = vec![node_id.to_string()];

        while let Some(current) = to_visit.pop() {
            for edge in &self.edges {
                if edge.target == current && result.insert(edge.source.clone()) {
                    to_visit.push(edge.source.clone());
                }
            }
        }

        result.into_iter().collect()
    }

    /// All nodes transitively downstream (follow edges where source == node_id).
    pub fn downstream(&self, node_id: &str) -> Vec<String> {
        let mut result = HashSet::new();
        let mut to_visit = vec![node_id.to_string()];

        while let Some(current) = to_visit.pop() {
            for edge in &self.edges {
                if edge.source == current && result.insert(edge.target.clone()) {
                    to_visit.push(edge.target.clone());
                }
            }
        }

        result.into_iter().collect()
    }

    /// All nodes in a given domain.
    pub fn nodes_by_domain(&self, domain: NodeDomain) -> Vec<&GraphNode> {
        self.nodes.values().filter(|n| n.domain == domain).collect()
    }

    /// Edges filtered by kind and/or minimum confidence.
    pub fn edges_filtered(
        &self,
        kind: Option<EdgeKind>,
        min_confidence: Option<EdgeConfidence>,
    ) -> Vec<&GraphEdge> {
        self.edges
            .iter()
            .filter(|e| kind.map_or(true, |k| e.kind == k))
            .filter(|e| min_confidence.map_or(true, |c| e.confidence >= c))
            .collect()
    }

    /// Edges originating from the given node.
    pub fn outgoing(&self, node_id: &str) -> Vec<&GraphEdge> {
        self.edges.iter().filter(|e| e.source == node_id).collect()
    }

    /// Edges targeting the given node.
    pub fn incoming(&self, node_id: &str) -> Vec<&GraphEdge> {
        self.edges.iter().filter(|e| e.target == node_id).collect()
    }

    /// All nodes connected to the given node in either direction.
    pub fn neighbors(&self, node_id: &str) -> Vec<&GraphNode> {
        let mut ids = HashSet::new();
        for edge in &self.edges {
            if edge.source == node_id {
                ids.insert(&edge.target);
            }
            if edge.target == node_id {
                ids.insert(&edge.source);
            }
        }
        ids.into_iter()
            .filter_map(|id| self.nodes.get(id))
            .collect()
    }

    /// Returns a subgraph containing only nodes from the domains matching the view.
    pub fn filter_by_view(&self, view: GraphView) -> DependencyGraph {
        let domain_filter: Option<NodeDomain> = match view {
            GraphView::Processes => Some(NodeDomain::Resource),
            GraphView::Code => Some(NodeDomain::Symbol),
            GraphView::Memories => Some(NodeDomain::Memory),
            GraphView::Files => Some(NodeDomain::File),
            GraphView::Knowledge => None,
        };

        if domain_filter.is_none() {
            return self.clone();
        }
        let domain = domain_filter.unwrap();

        let filtered_nodes: HashMap<String, GraphNode> = self
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.domain == domain
                    || matches!(&n.metadata, NodeMetadata::Group { child_domain, .. } if *child_domain == domain)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let node_ids: HashSet<&str> = filtered_nodes.keys().map(|k| k.as_str()).collect();

        let filtered_edges: Vec<GraphEdge> = self
            .edges
            .iter()
            .filter(|e| {
                node_ids.contains(e.source.as_str()) && node_ids.contains(e.target.as_str())
            })
            .cloned()
            .collect();

        let depends_on_sources: HashSet<&str> = filtered_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
            .map(|e| e.source.as_str())
            .collect();

        let depends_on_targets: HashSet<&str> = filtered_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
            .map(|e| e.target.as_str())
            .collect();

        let roots = filtered_nodes
            .keys()
            .filter(|id| !depends_on_sources.contains(id.as_str()))
            .cloned()
            .collect();

        let leaves = filtered_nodes
            .keys()
            .filter(|id| !depends_on_targets.contains(id.as_str()))
            .cloned()
            .collect();

        let max_depth = filtered_nodes.values().map(|n| n.depth).max().unwrap_or(0);

        DependencyGraph {
            nodes: filtered_nodes,
            edges: filtered_edges,
            roots,
            leaves,
            max_depth,
            domains: self.domains.clone(),
        }
    }

    /// Aggregate statistics about the graph.
    pub fn stats(&self) -> GraphStats {
        let mut nodes_by_domain: HashMap<NodeDomain, usize> = HashMap::new();
        for node in self.nodes.values() {
            *nodes_by_domain.entry(node.domain).or_insert(0) += 1;
        }

        let mut edges_by_kind: HashMap<EdgeKind, usize> = HashMap::new();
        let mut edges_by_confidence: HashMap<EdgeConfidence, usize> = HashMap::new();
        for edge in &self.edges {
            *edges_by_kind.entry(edge.kind).or_insert(0) += 1;
            *edges_by_confidence.entry(edge.confidence).or_insert(0) += 1;
        }

        GraphStats {
            total_nodes: self.nodes.len(),
            total_edges: self.edges.len(),
            nodes_by_domain,
            edges_by_kind,
            edges_by_confidence,
            domains: self.domains.clone(),
            max_depth: self.max_depth,
        }
    }

    // === Backward-compatible convenience methods ===

    /// Gets all nodes that transitively depend on the given node (downstream via DependsOn edges).
    ///
    /// This method works with both raw service names and prefixed node IDs.
    pub fn get_all_dependents(&self, service: &str) -> Vec<String> {
        let node_id = self.resolve_resource_id(service);
        let mut result = HashSet::new();
        let mut to_visit = vec![node_id];

        while let Some(current) = to_visit.pop() {
            // Find edges where current is the target (nodes that depend ON current)
            for edge in &self.edges {
                if edge.kind == EdgeKind::DependsOn && edge.target == current {
                    let source_label = self
                        .nodes
                        .get(&edge.source)
                        .map(|n| n.label.clone())
                        .unwrap_or_else(|| edge.source.clone());
                    if result.insert(source_label.clone()) {
                        to_visit.push(edge.source.clone());
                    }
                }
            }
        }

        result.into_iter().collect()
    }

    /// Gets all nodes that the given node transitively depends on (upstream via DependsOn edges).
    ///
    /// This method works with both raw service names and prefixed node IDs.
    pub fn get_all_dependencies(&self, service: &str) -> Vec<String> {
        let node_id = self.resolve_resource_id(service);
        let mut result = HashSet::new();
        let mut to_visit = vec![node_id];

        while let Some(current) = to_visit.pop() {
            // Find edges where current is the source (current depends ON target)
            for edge in &self.edges {
                if edge.kind == EdgeKind::DependsOn && edge.source == current {
                    let target_label = self
                        .nodes
                        .get(&edge.target)
                        .map(|n| n.label.clone())
                        .unwrap_or_else(|| edge.target.clone());
                    if result.insert(target_label.clone()) {
                        to_visit.push(edge.target.clone());
                    }
                }
            }
        }

        result.into_iter().collect()
    }

    /// Resolves a service name to its resource node ID.
    /// If the input already has a "resource:" prefix, returns it as-is.
    fn resolve_resource_id(&self, service: &str) -> String {
        if service.starts_with("resource:") {
            service.to_string()
        } else {
            format!("resource:{}", service)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DependencyCondition;

    #[test]
    fn test_empty_graph() {
        let services = vec![];
        let graph = DependencyGraph::from_services(&services);
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.roots.len(), 0);
        assert_eq!(graph.max_depth, 0);
    }

    #[test]
    fn test_single_service() {
        let services = vec![Service::new(
            "api".to_string(),
            "node server.js".to_string(),
        )];
        let graph = DependencyGraph::from_services(&services);

        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.roots.len(), 1);
        assert!(graph.roots[0].contains("api"));
        assert_eq!(graph.max_depth, 0);
    }

    #[test]
    fn test_simple_dependency() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);

        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.roots.len(), 1);
        assert!(graph.roots[0].contains("db"));
        assert_eq!(graph.leaves.len(), 1);
        assert!(graph.leaves[0].contains("api"));
        assert_eq!(graph.max_depth, 1);

        let api_node = graph.nodes.get("resource:api").unwrap();
        assert_eq!(api_node.label, "api");
        assert_eq!(api_node.depth, 1);

        let db_node = graph.nodes.get("resource:db").unwrap();
        assert_eq!(db_node.label, "db");
        assert_eq!(db_node.depth, 0);

        // Check edges
        let api_deps: Vec<&GraphEdge> = graph.outgoing("resource:api");
        assert_eq!(api_deps.len(), 1);
        assert_eq!(api_deps[0].target, "resource:db");
        assert_eq!(api_deps[0].kind, EdgeKind::DependsOn);

        let db_incoming: Vec<&GraphEdge> = graph.incoming("resource:db");
        assert_eq!(db_incoming.len(), 1);
        assert_eq!(db_incoming[0].source, "resource:api");
    }

    #[test]
    fn test_multi_level_dependencies() {
        let services = vec![
            Service::new("frontend".to_string(), "npm start".to_string())
                .with_dependency("api".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);

        assert_eq!(graph.max_depth, 2);
        assert_eq!(graph.nodes.get("resource:frontend").unwrap().depth, 2);
        assert_eq!(graph.nodes.get("resource:api").unwrap().depth, 1);
        assert_eq!(graph.nodes.get("resource:db").unwrap().depth, 0);
    }

    #[test]
    fn test_topological_sort() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        let sorted = graph.topological_sort().unwrap();

        // db should come before api
        let db_idx = sorted.iter().position(|s| s.contains("db")).unwrap();
        let api_idx = sorted.iter().position(|s| s.contains("api")).unwrap();
        assert!(db_idx < api_idx);
    }

    #[test]
    fn test_get_all_dependents() {
        let services = vec![
            Service::new("frontend".to_string(), "npm start".to_string())
                .with_dependency("api".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        let db_dependents = graph.get_all_dependents("db");

        assert_eq!(db_dependents.len(), 2);
        assert!(db_dependents.contains(&"api".to_string()));
        assert!(db_dependents.contains(&"frontend".to_string()));
    }

    #[test]
    fn test_get_all_dependencies() {
        let services = vec![
            Service::new("frontend".to_string(), "npm start".to_string())
                .with_dependency("api".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        let frontend_deps = graph.get_all_dependencies("frontend");

        assert_eq!(frontend_deps.len(), 2);
        assert!(frontend_deps.contains(&"api".to_string()));
        assert!(frontend_deps.contains(&"db".to_string()));
    }

    #[test]
    fn test_impact_analysis() {
        let services = vec![
            Service::new("frontend".to_string(), "npm start".to_string())
                .with_dependency("api".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        // DependsOn edges go source->target (api depends_on db means edge api->db)
        // Impact of db: who points TO db? That's api. Who points to api? That's frontend.
        // But impact follows outgoing edges from db — db has no outgoing edges.
        // Impact from api: api->db is outgoing, so db is affected.
        let impact = graph.impact("resource:api", EdgeConfidence::Definite);
        assert_eq!(impact.root, "resource:api");
        assert_eq!(impact.affected.len(), 1);
        assert_eq!(impact.affected[0].id, "resource:db");
    }

    #[test]
    fn test_stats() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        let stats = graph.stats();

        assert_eq!(stats.total_nodes, 2);
        assert_eq!(stats.total_edges, 1);
        assert_eq!(
            *stats.nodes_by_domain.get(&NodeDomain::Resource).unwrap(),
            2
        );
        assert_eq!(*stats.edges_by_kind.get(&EdgeKind::DependsOn).unwrap(), 1);
    }

    #[test]
    fn test_filter_by_view() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string()),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);

        let processes = graph.filter_by_view(GraphView::Processes);
        assert_eq!(processes.nodes.len(), 2);

        let code = graph.filter_by_view(GraphView::Code);
        assert_eq!(code.nodes.len(), 0);

        let all = graph.filter_by_view(GraphView::Knowledge);
        assert_eq!(all.nodes.len(), 2);
    }

    #[test]
    fn test_neighbors() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
            Service::new("cache".to_string(), "redis".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        let api_neighbors = graph.neighbors("resource:api");
        assert_eq!(api_neighbors.len(), 1);
        assert_eq!(api_neighbors[0].label, "db");

        let db_neighbors = graph.neighbors("resource:db");
        assert_eq!(db_neighbors.len(), 1);
        assert_eq!(db_neighbors[0].label, "api");
    }

    // === Multi-domain build tests ===

    fn sample_multi_domain_sources() -> (
        Vec<Service>,
        Vec<SymbolSource>,
        Vec<MemorySource>,
        Vec<FileSource>,
    ) {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let symbols = vec![
            SymbolSource {
                name: "handle_api_request".into(),
                kind: "function".into(),
                file: "src/api.rs".into(),
                line: 10,
            },
            SymbolSource {
                name: "db_connect".into(),
                kind: "function".into(),
                file: "src/db.rs".into(),
                line: 5,
            },
        ];

        let memories = vec![MemorySource {
            name: "api-deployment-notes".into(),
            kind: "project".into(),
            tags: vec!["api".into()],
            content_preview: "Use handle_api_request for routing".into(),
        }];

        let files = vec![
            FileSource {
                path: "config".into(),
                kind: "directory".into(),
                extension: None,
                size: None,
            },
            FileSource {
                path: "config/db.yaml".into(),
                kind: "file".into(),
                extension: Some("yaml".into()),
                size: Some(128),
            },
        ];

        (services, symbols, memories, files)
    }

    #[test]
    fn build_multi_domain_graph_has_all_domains() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        assert!(graph.domains.resource);
        assert!(graph.domains.symbol);
        assert!(graph.domains.memory);
        assert!(graph.domains.file);

        // 2 resources + 2 symbols + 1 memory + 2 files = 7
        assert_eq!(graph.nodes.len(), 7);
        assert_eq!(graph.nodes_by_domain(NodeDomain::Resource).len(), 2);
        assert_eq!(graph.nodes_by_domain(NodeDomain::Symbol).len(), 2);
        assert_eq!(graph.nodes_by_domain(NodeDomain::Memory).len(), 1);
        assert_eq!(graph.nodes_by_domain(NodeDomain::File).len(), 2);
    }

    #[test]
    fn build_multi_domain_graph_creates_cross_domain_edges() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        // DependsOn: api -> db
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::DependsOn
            && e.source == "resource:api"
            && e.target == "resource:db"));

        // Configures: symbol "handle_api_request" contains "api"
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Configures
            && e.source == "symbol:handle_api_request"
            && e.target == "resource:api"));

        // Configures: symbol "db_connect" contains "db"
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Configures
            && e.source == "symbol:db_connect"
            && e.target == "resource:db"));

        // References: memory tag "api" matches resource "api"
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::References
            && e.source == "memory:api-deployment-notes"
            && e.target == "resource:api"));

        // Documents: memory content mentions "handle_api_request"
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Documents
            && e.source == "memory:api-deployment-notes"
            && e.target == "symbol:handle_api_request"));

        // Configures: config/db.yaml matches resource "db"
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Configures
            && e.source == "file:config/db.yaml"
            && e.target == "resource:db"));

        // Contains: config/ -> config/db.yaml
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Contains
            && e.source == "file:config"
            && e.target == "file:config/db.yaml"));
    }

    #[test]
    fn topological_sort_with_cycle_returns_none() {
        let services = vec![
            Service::new("a".to_string(), "cmd-a".to_string())
                .with_dependency("b".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("b".to_string(), "cmd-b".to_string())
                .with_dependency("a".to_string(), DependencyCondition::ProcessHealthy),
        ];

        let graph = DependencyGraph::from_services(&services);
        assert!(graph.topological_sort().is_none());
    }

    #[test]
    fn topological_sort_single_node() {
        let services = vec![Service::new("solo".to_string(), "echo solo".to_string())];
        let graph = DependencyGraph::from_services(&services);
        let sorted = graph.topological_sort().unwrap();

        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0], "resource:solo");
    }

    #[test]
    fn topological_sort_disconnected_nodes() {
        let services = vec![
            Service::new("alpha".to_string(), "cmd-a".to_string()),
            Service::new("beta".to_string(), "cmd-b".to_string()),
            Service::new("gamma".to_string(), "cmd-g".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        let sorted = graph.topological_sort().unwrap();

        assert_eq!(sorted.len(), 3);
        let ids: HashSet<String> = sorted.into_iter().collect();
        assert!(ids.contains("resource:alpha"));
        assert!(ids.contains("resource:beta"));
        assert!(ids.contains("resource:gamma"));
    }

    #[test]
    fn topological_sort_ignores_non_resource_nodes() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];
        let symbols = vec![SymbolSource {
            name: "handle_api_request".into(),
            kind: "function".into(),
            file: "src/api.rs".into(),
            line: 10,
        }];

        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories: Vec::new(),
            files: Vec::new(),
        });

        let sorted = graph.topological_sort().unwrap();
        // Only resource nodes appear in topological sort
        assert_eq!(sorted.len(), 2);
        assert!(sorted.iter().all(|id| id.starts_with("resource:")));
    }

    #[test]
    fn impact_cross_domain_traversal() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        // Impact from resource:api — outgoing edges from api include DependsOn(db)
        // Also, memory and symbol nodes point TO api, so they are NOT downstream
        let impact = graph.impact("resource:api", EdgeConfidence::Definite);
        assert_eq!(impact.root, "resource:api");
        // api -> db via DependsOn (Definite)
        assert!(impact.affected.iter().any(|e| e.id == "resource:db"));

        // Impact from symbol:handle_api_request — has Configures edge to resource:api (Speculative)
        // With Speculative min_confidence, we should traverse it
        let impact = graph.impact("symbol:handle_api_request", EdgeConfidence::Speculative);
        // symbol -> resource:api -> resource:db
        assert!(impact.affected.iter().any(|e| e.id == "resource:api"));
        assert!(impact.affected.iter().any(|e| e.id == "resource:db"));

        // The api entry should be at distance 1, db at distance 2
        let api_entry = impact
            .affected
            .iter()
            .find(|e| e.id == "resource:api")
            .unwrap();
        assert_eq!(api_entry.distance, 1);
        assert_eq!(api_entry.domain, NodeDomain::Resource);

        let db_entry = impact
            .affected
            .iter()
            .find(|e| e.id == "resource:db")
            .unwrap();
        assert_eq!(db_entry.distance, 2);
    }

    #[test]
    fn impact_with_confidence_filtering() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        // From symbol:handle_api_request with Definite confidence — Configures is Speculative,
        // so should NOT be traversed
        let impact = graph.impact("symbol:handle_api_request", EdgeConfidence::Definite);
        assert!(impact.affected.is_empty());

        // Same with Probable — Configures is Speculative, still blocked
        let impact = graph.impact("symbol:handle_api_request", EdgeConfidence::Probable);
        assert!(impact.affected.is_empty());
    }

    #[test]
    fn upstream_transitive() {
        let services = vec![
            Service::new("frontend".to_string(), "npm start".to_string())
                .with_dependency("api".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);

        // Upstream of db: edges pointing TO db are api->db and frontend->api
        // So upstream should find api (directly) and frontend (transitively via api pointing to db
        // means api is upstream; then frontend points to api so frontend is upstream)
        let upstream = graph.upstream("resource:db");
        assert_eq!(upstream.len(), 2);
        assert!(upstream.contains(&"resource:api".to_string()));
        assert!(upstream.contains(&"resource:frontend".to_string()));

        // Upstream of frontend: nothing points to frontend
        let upstream = graph.upstream("resource:frontend");
        assert!(upstream.is_empty());
    }

    #[test]
    fn downstream_transitive() {
        let services = vec![
            Service::new("frontend".to_string(), "npm start".to_string())
                .with_dependency("api".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);

        // Downstream of frontend: frontend->api, api->db
        let downstream = graph.downstream("resource:frontend");
        assert_eq!(downstream.len(), 2);
        assert!(downstream.contains(&"resource:api".to_string()));
        assert!(downstream.contains(&"resource:db".to_string()));

        // Downstream of db: nothing
        let downstream = graph.downstream("resource:db");
        assert!(downstream.is_empty());
    }

    #[test]
    fn filter_by_view_returns_correct_domain_subset() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        let processes = graph.filter_by_view(GraphView::Processes);
        assert!(processes
            .nodes
            .values()
            .all(|n| n.domain == NodeDomain::Resource));
        assert_eq!(processes.nodes.len(), 2);

        let code = graph.filter_by_view(GraphView::Code);
        assert!(code.nodes.values().all(|n| n.domain == NodeDomain::Symbol));
        assert_eq!(code.nodes.len(), 2);

        let mems = graph.filter_by_view(GraphView::Memories);
        assert!(mems.nodes.values().all(|n| n.domain == NodeDomain::Memory));
        assert_eq!(mems.nodes.len(), 1);

        let file_view = graph.filter_by_view(GraphView::Files);
        assert!(file_view
            .nodes
            .values()
            .all(|n| n.domain == NodeDomain::File));
        assert_eq!(file_view.nodes.len(), 2);

        // Knowledge returns everything
        let knowledge = graph.filter_by_view(GraphView::Knowledge);
        assert_eq!(knowledge.nodes.len(), graph.nodes.len());
        assert_eq!(knowledge.edges.len(), graph.edges.len());
    }

    #[test]
    fn filter_by_view_preserves_intra_domain_edges_only() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        let processes = graph.filter_by_view(GraphView::Processes);
        // Only DependsOn edge between resources should remain
        assert!(processes
            .edges
            .iter()
            .all(|e| { e.source.starts_with("resource:") && e.target.starts_with("resource:") }));
        // api->db
        assert_eq!(processes.edges.len(), 1);
    }

    #[test]
    fn edges_filtered_by_kind() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        let depends_on = graph.edges_filtered(Some(EdgeKind::DependsOn), None);
        assert!(depends_on.iter().all(|e| e.kind == EdgeKind::DependsOn));
        assert_eq!(depends_on.len(), 1);

        let configures = graph.edges_filtered(Some(EdgeKind::Configures), None);
        assert!(configures.iter().all(|e| e.kind == EdgeKind::Configures));

        let definite_only = graph.edges_filtered(None, Some(EdgeConfidence::Definite));
        assert!(definite_only
            .iter()
            .all(|e| e.confidence >= EdgeConfidence::Definite));
    }

    #[test]
    fn outgoing_incoming_consistency() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        // Every outgoing edge from node X targeting Y should appear as incoming for Y
        for (node_id, _) in &graph.nodes {
            for outgoing_edge in graph.outgoing(node_id) {
                let target_incoming = graph.incoming(&outgoing_edge.target);
                assert!(
                    target_incoming
                        .iter()
                        .any(|e| e.source == *node_id && e.target == outgoing_edge.target),
                    "Outgoing edge from {} to {} not found in incoming edges of target",
                    node_id,
                    outgoing_edge.target
                );
            }
        }
    }

    #[test]
    fn neighbors_bidirectional() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        // resource:api has outgoing DependsOn to db, and incoming Configures from symbol and
        // References from memory, Configures from file
        let api_neighbors = graph.neighbors("resource:api");
        let neighbor_ids: HashSet<&str> = api_neighbors.iter().map(|n| n.id.as_str()).collect();

        // db is a neighbor via DependsOn
        assert!(neighbor_ids.contains("resource:db"));
        // handle_api_request is a neighbor via Configures
        assert!(neighbor_ids.contains("symbol:handle_api_request"));
    }

    #[test]
    fn stats_match_actual_graph() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        let stats = graph.stats();

        assert_eq!(stats.total_nodes, graph.nodes.len());
        assert_eq!(stats.total_edges, graph.edges.len());
        assert_eq!(stats.max_depth, graph.max_depth);
        assert_eq!(stats.domains.resource, graph.domains.resource);
        assert_eq!(stats.domains.symbol, graph.domains.symbol);
        assert_eq!(stats.domains.memory, graph.domains.memory);
        assert_eq!(stats.domains.file, graph.domains.file);

        // Verify per-domain node counts sum to total
        let domain_sum: usize = stats.nodes_by_domain.values().sum();
        assert_eq!(domain_sum, stats.total_nodes);

        // Verify per-kind edge counts sum to total
        let kind_sum: usize = stats.edges_by_kind.values().sum();
        assert_eq!(kind_sum, stats.total_edges);

        // Verify per-confidence edge counts sum to total
        let conf_sum: usize = stats.edges_by_confidence.values().sum();
        assert_eq!(conf_sum, stats.total_edges);
    }

    #[test]
    fn impact_on_nonexistent_node_returns_empty() {
        let services = vec![Service::new(
            "api".to_string(),
            "node server.js".to_string(),
        )];
        let graph = DependencyGraph::from_services(&services);
        let impact = graph.impact("resource:nonexistent", EdgeConfidence::Speculative);
        assert!(impact.affected.is_empty());
    }

    #[test]
    fn upstream_downstream_of_nonexistent_node() {
        let services = vec![Service::new(
            "api".to_string(),
            "node server.js".to_string(),
        )];
        let graph = DependencyGraph::from_services(&services);
        assert!(graph.upstream("resource:nonexistent").is_empty());
        assert!(graph.downstream("resource:nonexistent").is_empty());
    }

    #[test]
    fn multi_domain_integration_full_traversal() {
        let (services, symbols, memories, files) = sample_multi_domain_sources();
        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files,
        });

        // Topological sort should work (no cycles in resource domain)
        let sorted = graph.topological_sort().unwrap();
        let db_idx = sorted.iter().position(|s| s == "resource:db").unwrap();
        let api_idx = sorted.iter().position(|s| s == "resource:api").unwrap();
        assert!(db_idx < api_idx, "db must start before api");

        // Impact from memory node through symbol to resource
        let impact = graph.impact("memory:api-deployment-notes", EdgeConfidence::Speculative);
        // memory -> resource:api (References, Probable)
        // memory -> symbol:handle_api_request (Documents, Speculative)
        // resource:api -> resource:db (DependsOn, Definite)
        // symbol:handle_api_request -> resource:api (Configures, Speculative)
        assert!(impact.affected.iter().any(|e| e.id == "resource:api"));
        assert!(impact
            .affected
            .iter()
            .any(|e| e.id == "symbol:handle_api_request"));
        assert!(impact.affected.iter().any(|e| e.id == "resource:db"));

        // Verify cross-domain propagation: memory -> resource -> resource
        let db_entry = impact
            .affected
            .iter()
            .find(|e| e.id == "resource:db")
            .unwrap();
        assert!(db_entry.distance >= 2);
    }
}
