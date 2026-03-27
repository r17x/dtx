//! Dependency graph validation utilities.

use super::types::EdgeKind;
use super::DependencyGraph;
use serde::{Deserialize, Serialize};

/// Error type for cycle detection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CycleError {
    /// The services involved in the cycle
    pub cycle: Vec<String>,
    /// Human-readable error message
    pub message: String,
}

impl std::fmt::Display for CycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Dependency cycle detected: {} -> {}",
            self.cycle.join(" -> "),
            self.cycle.first().unwrap_or(&"?".to_string())
        )
    }
}

impl std::error::Error for CycleError {}

/// Validator for dependency graphs.
pub struct GraphValidator;

impl GraphValidator {
    /// Validates that the dependency graph has no cycles.
    ///
    /// Returns `Ok(())` if the graph is acyclic, or `Err(CycleError)` if a cycle is detected.
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::model::Service;
    /// use dtx_core::graph::{DependencyGraph, GraphValidator};
    ///
    /// let services = vec![
    ///     Service::new("api".to_string(), "node server.js".to_string()),
    ///     Service::new("db".to_string(), "postgres".to_string()),
    /// ];
    ///
    /// let graph = DependencyGraph::from_services(&services);
    /// assert!(GraphValidator::validate_no_cycles(&graph).is_ok());
    /// ```
    pub fn validate_no_cycles(graph: &DependencyGraph) -> Result<(), CycleError> {
        if graph.topological_sort().is_some() {
            return Ok(());
        }

        // Find the cycle using DFS on DependsOn edges
        let mut visited = std::collections::HashSet::new();
        let mut rec_stack = std::collections::HashSet::new();
        let mut path = Vec::new();

        for node_id in graph.nodes.keys() {
            if !visited.contains(node_id.as_str()) {
                if let Some(cycle) =
                    Self::find_cycle_dfs(node_id, graph, &mut visited, &mut rec_stack, &mut path)
                {
                    return Err(CycleError {
                        message: format!(
                            "Dependency cycle detected: {} -> {}",
                            cycle.join(" -> "),
                            cycle.first().unwrap_or(&"?".to_string())
                        ),
                        cycle,
                    });
                }
            }
        }

        Ok(())
    }

    /// Performs DFS to find a cycle in the graph (follows DependsOn edges only).
    fn find_cycle_dfs(
        node_id: &str,
        graph: &DependencyGraph,
        visited: &mut std::collections::HashSet<String>,
        rec_stack: &mut std::collections::HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        visited.insert(node_id.to_string());
        rec_stack.insert(node_id.to_string());
        path.push(node_id.to_string());

        // Follow outgoing DependsOn edges
        let deps: Vec<String> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn && e.source == node_id)
            .map(|e| e.target.clone())
            .collect();

        for dep in &deps {
            if !visited.contains(dep.as_str()) {
                if let Some(cycle) = Self::find_cycle_dfs(dep, graph, visited, rec_stack, path) {
                    return Some(cycle);
                }
            } else if rec_stack.contains(dep.as_str()) {
                let cycle_start = path.iter().position(|n| n == dep).unwrap();
                return Some(path[cycle_start..].to_vec());
            }
        }

        rec_stack.remove(node_id);
        path.pop();
        None
    }

    /// Checks if adding a dependency would create a cycle.
    pub fn would_create_cycle(graph: &DependencyGraph, from: &str, to: &str) -> bool {
        let from_id = if from.starts_with("resource:") {
            from.to_string()
        } else {
            format!("resource:{}", from)
        };
        let to_id = if to.starts_with("resource:") {
            to.to_string()
        } else {
            format!("resource:{}", to)
        };
        let to_dependencies = graph.get_all_dependencies(&to_id);
        // get_all_dependencies returns labels, so check against the label of from
        let from_label = graph
            .nodes
            .get(&from_id)
            .map(|n| n.label.clone())
            .unwrap_or_else(|| from.to_string());
        to_dependencies.contains(&from_label)
    }

    /// Validates that all DependsOn edge targets exist as nodes in the graph.
    pub fn validate_references(graph: &DependencyGraph) -> Result<(), Vec<String>> {
        let mut invalid = Vec::new();

        for edge in &graph.edges {
            if edge.kind == EdgeKind::DependsOn && !graph.nodes.contains_key(&edge.target) {
                let source_label = graph
                    .nodes
                    .get(&edge.source)
                    .map(|n| n.label.as_str())
                    .unwrap_or(&edge.source);
                invalid.push(format!(
                    "Service '{}' depends on non-existent service '{}'",
                    source_label,
                    edge.target
                        .strip_prefix("resource:")
                        .unwrap_or(&edge.target)
                ));
            }
        }

        if invalid.is_empty() {
            Ok(())
        } else {
            Err(invalid)
        }
    }

    /// Validates that enabled services don't depend on disabled services.
    pub fn validate_enabled_dependencies(
        services: &[crate::model::Service],
    ) -> Result<(), Vec<String>> {
        use std::collections::HashMap;

        let mut errors = Vec::new();

        let service_map: HashMap<&str, bool> = services
            .iter()
            .map(|s| (s.name.as_str(), s.enabled))
            .collect();

        for service in services {
            if !service.enabled {
                continue;
            }

            if let Some(ref dependencies) = service.depends_on {
                for dep in dependencies {
                    let dep_name = dep.service.as_str();

                    match service_map.get(dep_name) {
                        None => {
                            errors.push(format!(
                                "Service '{}' depends on non-existent service '{}'. \
                                 Fix: Remove the dependency or add the '{}' service.",
                                service.name, dep_name, dep_name
                            ));
                        }
                        Some(&false) => {
                            errors.push(format!(
                                "Enabled service '{}' depends on disabled service '{}'. \
                                 Fix: Either disable '{}' or enable '{}'.",
                                service.name, dep_name, service.name, dep_name
                            ));
                        }
                        Some(&true) => {}
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validates all aspects of the service dependency graph.
    pub fn validate_all(services: &[crate::model::Service]) -> Result<(), Vec<String>> {
        let mut all_errors = Vec::new();

        let graph = DependencyGraph::from_services(services);

        if let Err(cycle_err) = Self::validate_no_cycles(&graph) {
            all_errors.push(format!(
                "Circular dependency detected: {}. \
                 Fix: Remove one of the dependencies to break the cycle.",
                cycle_err
            ));
        }

        if let Err(mut ref_errors) = Self::validate_references(&graph) {
            all_errors.append(&mut ref_errors);
        }

        if let Err(mut enabled_errors) = Self::validate_enabled_dependencies(services) {
            all_errors.append(&mut enabled_errors);
        }

        if all_errors.is_empty() {
            Ok(())
        } else {
            Err(all_errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DependencyCondition, Service};

    #[test]
    fn test_no_cycles_simple() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        assert!(GraphValidator::validate_no_cycles(&graph).is_ok());
    }

    #[test]
    fn test_detect_simple_cycle() {
        let services = vec![
            Service::new("a".to_string(), "service-a".to_string())
                .with_dependency("b".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("b".to_string(), "service-b".to_string())
                .with_dependency("a".to_string(), DependencyCondition::ProcessHealthy),
        ];

        let graph = DependencyGraph::from_services(&services);
        let result = GraphValidator::validate_no_cycles(&graph);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.cycle.len(), 2);
        assert!(err.cycle.iter().any(|s| s.contains("a")));
        assert!(err.cycle.iter().any(|s| s.contains("b")));
    }

    #[test]
    fn test_detect_multi_level_cycle() {
        let services = vec![
            Service::new("a".to_string(), "service-a".to_string())
                .with_dependency("b".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("b".to_string(), "service-b".to_string())
                .with_dependency("c".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("c".to_string(), "service-c".to_string())
                .with_dependency("a".to_string(), DependencyCondition::ProcessHealthy),
        ];

        let graph = DependencyGraph::from_services(&services);
        let result = GraphValidator::validate_no_cycles(&graph);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.cycle.len(), 3);
    }

    #[test]
    fn test_would_create_cycle() {
        let services = vec![
            Service::new("a".to_string(), "service-a".to_string())
                .with_dependency("b".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("b".to_string(), "service-b".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);

        // Adding b -> a would create a cycle
        assert!(GraphValidator::would_create_cycle(&graph, "b", "a"));

        // Adding a -> b is already there, not a new cycle
        assert!(!GraphValidator::would_create_cycle(&graph, "a", "b"));

        // Adding a -> c wouldn't create a cycle (c doesn't exist yet)
        assert!(!GraphValidator::would_create_cycle(&graph, "a", "c"));
    }

    #[test]
    fn test_validate_references() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string()).with_dependency(
                "nonexistent".to_string(),
                DependencyCondition::ProcessHealthy,
            ),
        ];

        let graph = DependencyGraph::from_services(&services);
        let result = GraphValidator::validate_references(&graph);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("nonexistent"));
    }

    #[test]
    fn test_validate_references_ok() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        assert!(GraphValidator::validate_references(&graph).is_ok());
    }

    #[test]
    fn test_validate_enabled_dependencies_valid() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        assert!(GraphValidator::validate_enabled_dependencies(&services).is_ok());
    }

    #[test]
    fn test_validate_enabled_dependencies_depends_on_disabled() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()).disabled(),
        ];

        let result = GraphValidator::validate_enabled_dependencies(&services);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("api"));
        assert!(errors[0].contains("disabled service 'db'"));
        assert!(errors[0].contains("Fix:"));
    }

    #[test]
    fn test_validate_enabled_dependencies_nonexistent() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string()).with_dependency(
                "nonexistent".to_string(),
                DependencyCondition::ProcessHealthy,
            ),
        ];

        let result = GraphValidator::validate_enabled_dependencies(&services);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("api"));
        assert!(errors[0].contains("non-existent service 'nonexistent'"));
        assert!(errors[0].contains("Fix:"));
    }

    #[test]
    fn test_validate_enabled_dependencies_disabled_service_ignored() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .disabled()
                .with_dependency(
                    "nonexistent".to_string(),
                    DependencyCondition::ProcessHealthy,
                ),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        assert!(GraphValidator::validate_enabled_dependencies(&services).is_ok());
    }

    #[test]
    fn test_validate_enabled_dependencies_multiple_errors() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy)
                .with_dependency("cache".to_string(), DependencyCondition::ProcessStarted),
            Service::new("db".to_string(), "postgres".to_string()).disabled(),
            Service::new("worker".to_string(), "python worker.py".to_string())
                .with_dependency("queue".to_string(), DependencyCondition::ProcessHealthy),
        ];

        let result = GraphValidator::validate_enabled_dependencies(&services);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 3);

        let error_str = errors.join("\n");
        assert!(error_str.contains("api") && error_str.contains("db"));
        assert!(error_str.contains("api") && error_str.contains("cache"));
        assert!(error_str.contains("worker") && error_str.contains("queue"));
    }

    #[test]
    fn test_validate_all_valid_services() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
            Service::new("cache".to_string(), "redis-server".to_string()),
        ];

        assert!(GraphValidator::validate_all(&services).is_ok());
    }

    #[test]
    fn test_validate_all_cycle_error() {
        let services = vec![
            Service::new("a".to_string(), "service-a".to_string())
                .with_dependency("b".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("b".to_string(), "service-b".to_string())
                .with_dependency("a".to_string(), DependencyCondition::ProcessHealthy),
        ];

        let result = GraphValidator::validate_all(&services);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("Circular dependency"));
        assert!(errors[0].contains("Fix:"));
    }

    #[test]
    fn test_validate_all_multiple_error_types() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy)
                .with_dependency(
                    "nonexistent".to_string(),
                    DependencyCondition::ProcessStarted,
                ),
            Service::new("db".to_string(), "postgres".to_string()).disabled(),
        ];

        let result = GraphValidator::validate_all(&services);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(errors.len() >= 2);

        let error_str = errors.join("\n");
        assert!(error_str.contains("nonexistent"));
        assert!(error_str.contains("disabled"));
    }

    #[test]
    fn test_validate_all_disabled_service_with_invalid_deps() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string()),
            Service::new("disabled-service".to_string(), "echo test".to_string())
                .disabled()
                .with_dependency(
                    "nonexistent".to_string(),
                    DependencyCondition::ProcessHealthy,
                ),
        ];

        let result = GraphValidator::validate_all(&services);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("nonexistent"));
    }

    #[test]
    fn test_cycle_detection_in_multi_domain_graph() {
        // Cycles are only checked via DependsOn edges (resource domain).
        // Cross-domain edges (Configures, References, etc.) should not trigger cycle detection.
        use super::super::analyzer::GraphSources;
        use super::super::extract::{MemorySource, SymbolSource};

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

        let memories = vec![MemorySource {
            name: "db-notes".into(),
            kind: "project".into(),
            tags: vec!["db".into()],
            content_preview: "handle_api_request details".into(),
        }];

        let graph = DependencyGraph::build(GraphSources {
            services: &services,
            symbols,
            memories,
            files: Vec::new(),
        });

        // No cycles in resource domain — should pass
        assert!(GraphValidator::validate_no_cycles(&graph).is_ok());
    }

    #[test]
    fn test_validate_references_valid_multi_domain_graph() {
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);
        // All DependsOn edges point to existing nodes
        assert!(GraphValidator::validate_references(&graph).is_ok());
    }

    #[test]
    fn test_validate_references_detects_dangling_edge() {
        // Manually verify that a graph built with a missing dependency is caught
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string()).with_dependency(
                "missing-db".to_string(),
                DependencyCondition::ProcessHealthy,
            ),
        ];

        let graph = DependencyGraph::from_services(&services);
        let result = GraphValidator::validate_references(&graph);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing-db"));
    }

    #[test]
    fn test_validate_no_cycles_empty_graph() {
        let services: Vec<Service> = vec![];
        let graph = DependencyGraph::from_services(&services);
        assert!(GraphValidator::validate_no_cycles(&graph).is_ok());
    }

    #[test]
    fn test_validate_no_cycles_single_node() {
        let services = vec![Service::new("solo".to_string(), "echo solo".to_string())];
        let graph = DependencyGraph::from_services(&services);
        assert!(GraphValidator::validate_no_cycles(&graph).is_ok());
    }

    #[test]
    fn test_validate_all_acyclic_complex_graph() {
        let services = vec![
            Service::new("frontend".to_string(), "npm start".to_string())
                .with_dependency("api".to_string(), DependencyCondition::ProcessHealthy)
                .with_dependency("cdn".to_string(), DependencyCondition::ProcessStarted),
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy)
                .with_dependency("cache".to_string(), DependencyCondition::ProcessStarted),
            Service::new("db".to_string(), "postgres".to_string()),
            Service::new("cache".to_string(), "redis-server".to_string()),
            Service::new("cdn".to_string(), "nginx".to_string()),
        ];

        assert!(GraphValidator::validate_all(&services).is_ok());
    }

    #[test]
    fn test_would_create_cycle_transitive() {
        // a -> b -> c; adding c -> a would create cycle
        let services = vec![
            Service::new("a".to_string(), "cmd-a".to_string())
                .with_dependency("b".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("b".to_string(), "cmd-b".to_string())
                .with_dependency("c".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("c".to_string(), "cmd-c".to_string()),
        ];

        let graph = DependencyGraph::from_services(&services);

        assert!(GraphValidator::would_create_cycle(&graph, "c", "a"));
        // c -> b would also create a cycle since b already depends on c
        assert!(GraphValidator::would_create_cycle(&graph, "c", "b"));
        // a -> c would NOT create a cycle (c has no deps that lead back to a)
        assert!(!GraphValidator::would_create_cycle(&graph, "a", "c"));
    }

    #[test]
    fn test_cycle_error_display() {
        let err = CycleError {
            cycle: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            message: "test".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("a -> b -> c -> a"));
    }
}
