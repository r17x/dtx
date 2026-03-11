//! Port availability checking utilities.
//!
//! Validates that required ports are available before starting services,
//! providing clear error messages about conflicts.

use crate::error::{PortConflictDetail, PortConflictError};
use crate::{CoreError, Result};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use tracing::debug;

/// Service port requirement for validation.
#[derive(Debug, Clone)]
pub struct ServicePort {
    /// Name of the service.
    pub service_name: String,
    /// Port the service needs.
    pub port: u16,
}

/// Checks if a single port is available for binding.
///
/// Returns `true` if the port is available, `false` if in use.
pub fn is_port_available(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpListener::bind(addr).is_ok()
}

/// Finds an available port starting from the given port.
///
/// Returns the first available port >= `start_port`, or None if no port
/// is available up to 65535.
pub fn find_available_port(start_port: u16) -> Option<u16> {
    (start_port..=65535).find(|&p| is_port_available(p))
}

/// Finds an available port near the preferred port.
///
/// Tries the preferred port first, then searches upward.
pub fn find_available_port_near(preferred: u16) -> Option<u16> {
    if is_port_available(preferred) {
        return Some(preferred);
    }
    find_available_port(preferred + 1)
}

/// Checks multiple ports efficiently in a single pass.
///
/// Returns a list of ports that are NOT available.
pub fn check_ports_availability(ports: &[u16]) -> Vec<u16> {
    ports
        .iter()
        .filter(|&&port| !is_port_available(port))
        .copied()
        .collect()
}

/// Validates that all required ports are available.
///
/// This is a fast, synchronous check that doesn't spawn subprocesses.
/// Use `validate_ports_with_identification` if you need process info.
///
/// # Arguments
///
/// * `ports` - List of service port requirements to validate
///
/// # Returns
///
/// * `Ok(())` if all ports are available
/// * `Err(CoreError::PortConflict)` with details about conflicts
pub fn validate_ports_sync(ports: &[ServicePort]) -> Result<()> {
    if ports.is_empty() {
        return Ok(());
    }

    let mut conflicts = Vec::new();

    // Check for duplicate ports within the service list
    let mut port_map: HashMap<u16, &str> = HashMap::new();
    for sp in ports {
        if let Some(existing_service) = port_map.get(&sp.port) {
            conflicts.push(PortConflictDetail {
                port: sp.port,
                service_name: sp.service_name.clone(),
                used_by: Some(format!("service '{}'", existing_service)),
            });
        } else {
            port_map.insert(sp.port, &sp.service_name);
        }
    }

    // Check if ports are in use by external processes
    for sp in ports {
        if !is_port_available(sp.port) {
            // Avoid duplicate entries (from internal conflicts)
            if !conflicts
                .iter()
                .any(|c| c.port == sp.port && c.service_name == sp.service_name)
            {
                conflicts.push(PortConflictDetail {
                    port: sp.port,
                    service_name: sp.service_name.clone(),
                    used_by: None, // Fast mode: no process identification
                });
            }
        }
    }

    if conflicts.is_empty() {
        debug!(
            ports = ?ports.iter().map(|p| p.port).collect::<Vec<_>>(),
            "All ports validated successfully"
        );
        Ok(())
    } else {
        Err(CoreError::PortConflict(PortConflictError { conflicts }))
    }
}

/// Validates that all required ports are available (async wrapper).
///
/// This is the fast version that doesn't identify which process owns the port.
/// The error message will show which ports are blocked but not by what.
pub async fn validate_ports(ports: &[ServicePort]) -> Result<()> {
    // Use sync version - no subprocess spawning needed
    validate_ports_sync(ports)
}

/// Port reassignment result.
#[derive(Debug, Clone)]
pub struct PortReassignment {
    /// Service name.
    pub service_name: String,
    /// Original requested port.
    pub original_port: u16,
    /// New assigned port.
    pub new_port: u16,
}

/// Resolves port conflicts by reassigning to available ports.
///
/// Returns the list of services with updated ports and any reassignments made.
/// Services without ports are passed through unchanged.
pub fn resolve_port_conflicts(
    services: &[crate::model::Service],
) -> (Vec<crate::model::Service>, Vec<PortReassignment>) {
    let mut result = Vec::with_capacity(services.len());
    let mut reassignments = Vec::new();
    let mut used_ports: std::collections::HashSet<u16> = std::collections::HashSet::new();

    for service in services {
        if !service.enabled {
            result.push(service.clone());
            continue;
        }

        let mut new_service = service.clone();

        if let Some(port) = service.port {
            // Check if port is available (not used by another service and not in use externally)
            let port_available = !used_ports.contains(&port) && is_port_available(port);

            if port_available {
                used_ports.insert(port);
            } else {
                // Find next available port
                let mut candidate = port.saturating_add(1);
                while used_ports.contains(&candidate) || !is_port_available(candidate) {
                    if candidate == u16::MAX {
                        // No more ports available - use original and let it fail
                        candidate = port;
                        break;
                    }
                    candidate = candidate.saturating_add(1);
                }

                if candidate != port {
                    reassignments.push(PortReassignment {
                        service_name: service.name.clone(),
                        original_port: port,
                        new_port: candidate,
                    });
                    new_service.port = Some(candidate);
                    used_ports.insert(candidate);
                }
            }
        }

        result.push(new_service);
    }

    (result, reassignments)
}

/// Extracts ports from a list of services.
///
/// Filters to only enabled services with defined ports.
pub fn extract_service_ports(services: &[crate::model::Service]) -> Vec<ServicePort> {
    services
        .iter()
        .filter(|s| s.enabled)
        .filter_map(|s| {
            s.port.map(|port| ServicePort {
                service_name: s.name.clone(),
                port,
            })
        })
        .collect()
}

/// Validates ports for a list of services.
///
/// Convenience function that extracts ports and validates them.
pub async fn validate_service_ports(services: &[crate::model::Service]) -> Result<()> {
    let ports = extract_service_ports(services);
    validate_ports(&ports).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_port_available_ephemeral() {
        // Ephemeral port should be available
        // Use port 0 to let OS assign, but we test a high port instead
        let high_port = 49152 + (std::process::id() as u16 % 1000);
        // This might fail if something is using it, so just check it doesn't panic
        let _ = is_port_available(high_port);
    }

    #[test]
    fn test_extract_service_ports() {
        use crate::model::Service;

        let services = vec![
            Service::new("api".to_string(), "node app.js".to_string()).with_port(3000),
            Service::new("db".to_string(), "postgres".to_string()).with_port(5432),
            Service::new("worker".to_string(), "node worker.js".to_string()), // No port
            Service::new("disabled".to_string(), "echo disabled".to_string())
                .with_port(8080)
                .disabled(),
        ];

        let ports = extract_service_ports(&services);
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].service_name, "api");
        assert_eq!(ports[0].port, 3000);
        assert_eq!(ports[1].service_name, "db");
        assert_eq!(ports[1].port, 5432);
    }

    #[tokio::test]
    async fn test_validate_ports_empty() {
        let result = validate_ports(&[]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_ports_duplicate_internal() {
        let ports = vec![
            ServicePort {
                service_name: "api".to_string(),
                port: 3000,
            },
            ServicePort {
                service_name: "web".to_string(),
                port: 3000,
            },
        ];

        let result = validate_ports(&ports).await;
        assert!(result.is_err());

        if let Err(CoreError::PortConflict(err)) = result {
            assert!(!err.conflicts.is_empty());
            let conflict = &err.conflicts[0];
            assert_eq!(conflict.port, 3000);
            assert!(conflict.used_by.as_ref().unwrap().contains("api"));
        } else {
            panic!("Expected PortConflict error");
        }
    }

    #[test]
    fn test_resolve_port_conflicts_internal_duplicate() {
        use crate::model::Service;

        // Find a port we know is free for testing
        let base_port = find_available_port(50000).expect("Should find a free port");

        // Two services wanting the same port
        let services = vec![
            Service::new("api".to_string(), "node app.js".to_string()).with_port(base_port),
            Service::new("web".to_string(), "nginx".to_string()).with_port(base_port),
        ];

        let (resolved, reassignments) = resolve_port_conflicts(&services);

        assert_eq!(resolved.len(), 2);
        // First service keeps its port
        assert_eq!(resolved[0].port, Some(base_port));
        // Second service should get a different port (next available)
        assert!(resolved[1].port.is_some());
        assert_ne!(resolved[1].port, Some(base_port));
        // Should have one reassignment
        assert_eq!(reassignments.len(), 1);
        assert_eq!(reassignments[0].service_name, "web");
        assert_eq!(reassignments[0].original_port, base_port);
    }

    #[test]
    fn test_resolve_port_conflicts_disabled_service() {
        use crate::model::Service;

        let base_port = find_available_port(51000).expect("Should find a free port");

        // Disabled service should be passed through unchanged
        let services = vec![Service::new("api".to_string(), "node app.js".to_string())
            .with_port(base_port)
            .disabled()];

        let (resolved, reassignments) = resolve_port_conflicts(&services);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].port, Some(base_port));
        assert!(reassignments.is_empty());
    }
}
