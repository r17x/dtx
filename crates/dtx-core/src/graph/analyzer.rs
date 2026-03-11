//! Dependency graph analysis utilities.

use crate::model::Service;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Represents a node in the dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Service name
    pub name: String,
    /// Services this node depends on
    pub dependencies: Vec<String>,
    /// Services that depend on this node
    pub dependents: Vec<String>,
    /// Depth in the dependency tree (0 = no dependencies)
    pub depth: usize,
}

/// Dependency graph for a set of services.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    /// All nodes in the graph
    pub nodes: HashMap<String, GraphNode>,
    /// Services with no dependencies (entry points)
    pub roots: Vec<String>,
    /// Services with no dependents (leaves)
    pub leaves: Vec<String>,
    /// Maximum depth of the graph
    pub max_depth: usize,
}

impl DependencyGraph {
    /// Creates a new dependency graph from a list of services.
    ///
    /// # Arguments
    ///
    /// * `services` - The services to analyze
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::model::Service;
    /// use dtx_core::graph::DependencyGraph;
    ///
    /// let services = vec![
    ///     Service::new("api".to_string(), "node server.js".to_string()),
    ///     Service::new("db".to_string(), "postgres".to_string()),
    /// ];
    ///
    /// let graph = DependencyGraph::from_services(&services);
    /// assert_eq!(graph.nodes.len(), 2);
    /// ```
    pub fn from_services(services: &[Service]) -> Self {
        let mut nodes: HashMap<String, GraphNode> = HashMap::new();

        // First pass: create all nodes
        for service in services {
            nodes.insert(
                service.name.clone(),
                GraphNode {
                    name: service.name.clone(),
                    dependencies: Vec::new(),
                    dependents: Vec::new(),
                    depth: 0,
                },
            );
        }

        // Second pass: build dependency relationships
        for service in services {
            if let Some(ref deps) = service.depends_on {
                for dep in deps {
                    // Add to this node's dependencies
                    if let Some(node) = nodes.get_mut(&service.name) {
                        node.dependencies.push(dep.service.clone());
                    }

                    // Add to the dependency's dependents
                    if let Some(dep_node) = nodes.get_mut(&dep.service) {
                        dep_node.dependents.push(service.name.clone());
                    }
                }
            }
        }

        // Third pass: calculate depths for all nodes
        let mut max_depth = 0;
        let node_names: Vec<String> = nodes.keys().cloned().collect();
        for name in node_names {
            let depth = Self::calculate_depth(&name, &nodes, &mut HashSet::new());
            if let Some(node) = nodes.get_mut(&name) {
                node.depth = depth;
                max_depth = max_depth.max(depth);
            }
        }

        // Find roots (no dependencies) and leaves (no dependents)
        let roots: Vec<String> = nodes
            .values()
            .filter(|n| n.dependencies.is_empty())
            .map(|n| n.name.clone())
            .collect();

        let leaves: Vec<String> = nodes
            .values()
            .filter(|n| n.dependents.is_empty())
            .map(|n| n.name.clone())
            .collect();

        Self {
            nodes,
            roots,
            leaves,
            max_depth,
        }
    }

    /// Calculates the depth of a node in the dependency tree.
    fn calculate_depth(
        node_name: &str,
        nodes: &HashMap<String, GraphNode>,
        visited: &mut HashSet<String>,
    ) -> usize {
        if visited.contains(node_name) {
            // Cycle detected, return 0 to prevent infinite recursion
            return 0;
        }

        visited.insert(node_name.to_string());

        let node = match nodes.get(node_name) {
            Some(n) => n,
            None => return 0,
        };

        if node.dependencies.is_empty() {
            visited.remove(node_name);
            return 0;
        }

        let max_dep_depth = node
            .dependencies
            .iter()
            .map(|dep| Self::calculate_depth(dep, nodes, visited))
            .max()
            .unwrap_or(0);

        visited.remove(node_name);
        max_dep_depth + 1
    }

    /// Gets the topological ordering of services (dependencies before dependents).
    ///
    /// Returns `None` if the graph has cycles.
    pub fn topological_sort(&self) -> Option<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut result = Vec::new();

        // Calculate in-degrees
        for (name, node) in &self.nodes {
            in_degree.insert(name.clone(), node.dependencies.len());
        }

        // Find nodes with no dependencies
        let mut queue: Vec<String> = self.roots.clone();

        while let Some(current) = queue.pop() {
            result.push(current.clone());

            // Reduce in-degree for dependents
            if let Some(node) = self.nodes.get(&current) {
                for dependent in &node.dependents {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(dependent.clone());
                        }
                    }
                }
            }
        }

        // If not all nodes are in result, there's a cycle
        if result.len() != self.nodes.len() {
            None
        } else {
            Some(result)
        }
    }

    /// Gets all services that depend on the given service (directly or indirectly).
    pub fn get_all_dependents(&self, service: &str) -> Vec<String> {
        let mut result = HashSet::new();
        let mut to_visit = vec![service.to_string()];

        while let Some(current) = to_visit.pop() {
            if let Some(node) = self.nodes.get(&current) {
                for dependent in &node.dependents {
                    if result.insert(dependent.clone()) {
                        to_visit.push(dependent.clone());
                    }
                }
            }
        }

        result.into_iter().collect()
    }

    /// Gets all services that the given service depends on (directly or indirectly).
    pub fn get_all_dependencies(&self, service: &str) -> Vec<String> {
        let mut result = HashSet::new();
        let mut to_visit = vec![service.to_string()];

        while let Some(current) = to_visit.pop() {
            if let Some(node) = self.nodes.get(&current) {
                for dependency in &node.dependencies {
                    if result.insert(dependency.clone()) {
                        to_visit.push(dependency.clone());
                    }
                }
            }
        }

        result.into_iter().collect()
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
        assert_eq!(graph.roots[0], "api");
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
        assert_eq!(graph.roots[0], "db");
        assert_eq!(graph.leaves.len(), 1);
        assert_eq!(graph.leaves[0], "api");
        assert_eq!(graph.max_depth, 1);

        let api_node = graph.nodes.get("api").unwrap();
        assert_eq!(api_node.dependencies, vec!["db"]);
        assert_eq!(api_node.depth, 1);

        let db_node = graph.nodes.get("db").unwrap();
        assert_eq!(db_node.dependents, vec!["api"]);
        assert_eq!(db_node.depth, 0);
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
        assert_eq!(graph.nodes.get("frontend").unwrap().depth, 2);
        assert_eq!(graph.nodes.get("api").unwrap().depth, 1);
        assert_eq!(graph.nodes.get("db").unwrap().depth, 0);
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
        let db_idx = sorted.iter().position(|s| s == "db").unwrap();
        let api_idx = sorted.iter().position(|s| s == "api").unwrap();
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
}
