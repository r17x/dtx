//! Default translator configuration.
//!
//! This module provides utilities for creating pre-configured translator registries.
//!
//! # Architecture Note
//!
//! The base `TranslatorRegistry::new()` provides an empty registry. Specific translators
//! like `ProcessToContainerTranslator` are registered by the crates that define them
//! (e.g., `dtx-process`) to avoid circular dependencies.
//!
//! For a fully-configured registry with all default translators, use:
//! ```ignore
//! use dtx_process::default_registry;
//! let registry = default_registry();
//! ```

use super::TranslatorRegistry;

/// Create a new empty translator registry.
///
/// This is the base registry without any translators registered.
/// Use crate-specific `default_registry()` functions to get
/// pre-configured registries.
///
/// # Example
///
/// ```
/// use dtx_core::translation::new_registry;
///
/// let registry = new_registry();
/// assert!(registry.is_empty());
/// ```
pub fn new_registry() -> TranslatorRegistry {
    TranslatorRegistry::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let registry = new_registry();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}
