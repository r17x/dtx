//! Integration test to verify graph validation API is accessible from external crates.

use dtx_core::model::{DependencyCondition, Service};
use dtx_core::GraphValidator;

#[test]
fn test_validate_enabled_dependencies_api() {
    let services = vec![
        Service::new("api".to_string(), "node server.js".to_string())
            .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
        Service::new("db".to_string(), "postgres".to_string()),
    ];

    // Verify the function is accessible and works
    let result = GraphValidator::validate_enabled_dependencies(&services);
    assert!(result.is_ok());
}

#[test]
fn test_validate_all_api() {
    let services = vec![
        Service::new("api".to_string(), "node server.js".to_string())
            .with_dependency("db".to_string(), DependencyCondition::ProcessHealthy),
        Service::new("db".to_string(), "postgres".to_string()),
        Service::new("cache".to_string(), "redis-server".to_string()),
    ];

    // Verify the function is accessible and works
    let result = GraphValidator::validate_all(&services);
    assert!(result.is_ok());
}

#[test]
fn test_validate_all_collects_multiple_errors() {
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
    // Should have at least 2 errors
    assert!(errors.len() >= 2);

    // Verify error messages are actionable
    let error_str = errors.join("\n");
    assert!(error_str.contains("Fix:") || error_str.contains("nonexistent"));
}
