//! Dependency graph validation utilities.

use super::DependencyGraph;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
        // Use topological sort - if it succeeds, there are no cycles
        if graph.topological_sort().is_some() {
            return Ok(());
        }

        // Find the cycle using DFS
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for node_name in graph.nodes.keys() {
            if !visited.contains(node_name) {
                if let Some(cycle) =
                    Self::find_cycle_dfs(node_name, graph, &mut visited, &mut rec_stack, &mut path)
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

    /// Performs DFS to find a cycle in the graph.
    fn find_cycle_dfs(
        node: &str,
        graph: &DependencyGraph,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(graph_node) = graph.nodes.get(node) {
            for dep in &graph_node.dependencies {
                if !visited.contains(dep) {
                    if let Some(cycle) = Self::find_cycle_dfs(dep, graph, visited, rec_stack, path)
                    {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(dep) {
                    // Found a cycle - extract it from the path
                    let cycle_start = path.iter().position(|n| n == dep).unwrap();
                    return Some(path[cycle_start..].to_vec());
                }
            }
        }

        rec_stack.remove(node);
        path.pop();
        None
    }

    /// Checks if adding a dependency would create a cycle.
    ///
    /// # Arguments
    ///
    /// * `graph` - The current dependency graph
    /// * `from` - The service that would depend on `to`
    /// * `to` - The service that `from` would depend on
    ///
    /// Returns `true` if adding the dependency would create a cycle.
    pub fn would_create_cycle(graph: &DependencyGraph, from: &str, to: &str) -> bool {
        // Check if 'from' is already in 'to's dependency chain
        // If so, adding 'to' as a dependency of 'from' would create a cycle
        let to_dependencies = graph.get_all_dependencies(to);
        to_dependencies.contains(&from.to_string())
    }

    /// Validates that all dependencies reference existing services.
    pub fn validate_references(graph: &DependencyGraph) -> Result<(), Vec<String>> {
        let mut invalid = Vec::new();

        for (service_name, node) in &graph.nodes {
            for dep in &node.dependencies {
                if !graph.nodes.contains_key(dep) {
                    invalid.push(format!(
                        "Service '{}' depends on non-existent service '{}'",
                        service_name, dep
                    ));
                }
            }
        }

        if invalid.is_empty() {
            Ok(())
        } else {
            Err(invalid)
        }
    }

    /// Validates that enabled services don't depend on disabled services.
    ///
    /// This function checks two conditions:
    /// 1. All dependencies must reference existing services
    /// 2. Enabled services must not depend on disabled services
    ///
    /// Disabled services are skipped from validation since they won't run.
    ///
    /// # Arguments
    ///
    /// * `services` - The list of services to validate
    ///
    /// # Returns
    ///
    /// * `Ok(())` if all enabled services have valid dependencies
    /// * `Err(Vec<String>)` containing all validation errors found
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::model::Service;
    /// use dtx_core::graph::GraphValidator;
    ///
    /// let services = vec![
    ///     Service::new("api".to_string(), "node server.js".to_string())
    ///         .with_dependency("db".to_string(), dtx_core::model::DependencyCondition::ProcessHealthy),
    ///     Service::new("db".to_string(), "postgres".to_string()),
    /// ];
    ///
    /// assert!(GraphValidator::validate_enabled_dependencies(&services).is_ok());
    /// ```
    pub fn validate_enabled_dependencies(
        services: &[crate::model::Service],
    ) -> Result<(), Vec<String>> {
        use std::collections::HashMap;

        let mut errors = Vec::new();

        // Create a map of service names to their enabled status
        let service_map: HashMap<&str, bool> = services
            .iter()
            .map(|s| (s.name.as_str(), s.enabled))
            .collect();

        // Check each enabled service
        for service in services {
            // Skip validation for disabled services (they won't run)
            if !service.enabled {
                continue;
            }

            // Check dependencies if any exist
            if let Some(ref dependencies) = service.depends_on {
                for dep in dependencies {
                    let dep_name = dep.service.as_str();

                    // Check if dependency exists
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
                        Some(&true) => {
                            // Valid dependency - enabled service depends on enabled service
                        }
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
    ///
    /// This function performs comprehensive validation:
    /// 1. Checks for circular dependencies
    /// 2. Validates that all dependencies reference existing services
    /// 3. Ensures enabled services don't depend on disabled services
    ///
    /// All errors are collected and returned together for better user experience.
    ///
    /// # Arguments
    ///
    /// * `services` - The list of services to validate
    ///
    /// # Returns
    ///
    /// * `Ok(())` if all validations pass
    /// * `Err(Vec<String>)` containing all validation errors from all checks
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::model::{Service, DependencyCondition};
    /// use dtx_core::graph::GraphValidator;
    ///
    /// let services = vec![
    ///     Service::new("api".to_string(), "node server.js".to_string())
    ///         .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
    ///     Service::new("db".to_string(), "postgres".to_string()),
    /// ];
    ///
    /// assert!(GraphValidator::validate_all(&services).is_ok());
    /// ```
    pub fn validate_all(services: &[crate::model::Service]) -> Result<(), Vec<String>> {
        let mut all_errors = Vec::new();

        // Build dependency graph for cycle and reference checks
        let graph = DependencyGraph::from_services(services);

        // Check 1: Validate no circular dependencies
        if let Err(cycle_err) = Self::validate_no_cycles(&graph) {
            all_errors.push(format!(
                "Circular dependency detected: {}. \
                 Fix: Remove one of the dependencies to break the cycle.",
                cycle_err
            ));
        }

        // Check 2: Validate all references exist
        if let Err(mut ref_errors) = Self::validate_references(&graph) {
            all_errors.append(&mut ref_errors);
        }

        // Check 3: Validate enabled dependencies
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
        assert!(err.cycle.contains(&"a".to_string()));
        assert!(err.cycle.contains(&"b".to_string()));
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
        // All services enabled, valid dependencies
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        assert!(GraphValidator::validate_enabled_dependencies(&services).is_ok());
    }

    #[test]
    fn test_validate_enabled_dependencies_depends_on_disabled() {
        // Enabled service depends on disabled service - should fail
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
        // Enabled service depends on non-existent service - should fail
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
        // Disabled service can depend on anything - should pass
        let services = vec![
            Service::new("api".to_string(), "node server.js".to_string())
                .disabled()
                .with_dependency(
                    "nonexistent".to_string(),
                    DependencyCondition::ProcessHealthy,
                ),
            Service::new("db".to_string(), "postgres".to_string()),
        ];

        // Should pass because disabled services are not validated
        assert!(GraphValidator::validate_enabled_dependencies(&services).is_ok());
    }

    #[test]
    fn test_validate_enabled_dependencies_multiple_errors() {
        // Multiple validation errors should all be collected
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

        // Check that all errors are present
        let error_str = errors.join("\n");
        assert!(error_str.contains("api") && error_str.contains("db"));
        assert!(error_str.contains("api") && error_str.contains("cache"));
        assert!(error_str.contains("worker") && error_str.contains("queue"));
    }

    #[test]
    fn test_validate_all_valid_services() {
        // All validations should pass
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
        // Should detect circular dependency
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
        // Should collect errors from all validation checks
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
        // Should have at least 2 errors:
        // 1. Reference error for 'nonexistent'
        // 2. Enabled dependency error for disabled 'db'
        assert!(errors.len() >= 2);

        let error_str = errors.join("\n");
        assert!(error_str.contains("nonexistent"));
        assert!(error_str.contains("disabled"));
    }

    #[test]
    fn test_validate_all_disabled_service_with_invalid_deps() {
        // Disabled services with invalid deps should be ignored by enabled check
        // but reference validation runs on graph which includes all services
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
        // Should fail because validate_references checks all services in graph
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("nonexistent"));
    }
}
