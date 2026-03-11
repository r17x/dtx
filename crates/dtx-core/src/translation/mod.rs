//! Translation framework for resource type conversion.
//!
//! This module provides the infrastructure for converting between
//! different resource types (e.g., Process → Container).
//!
//! # Overview
//!
//! The translation system consists of:
//! - [`TranslationError`] - Error types for translation failures
//! - [`Translator`] - Trait for type conversion
//! - [`TranslatorRegistry`] - Registry for translator lookup
//! - [`TranslationContext`] - Configuration for translations
//!
//! # Example
//!
//! ```ignore
//! use dtx_core::translation::{TranslatorRegistry, TranslationContext};
//!
//! // Register translators
//! let mut registry = TranslatorRegistry::new();
//! registry.register(ProcessToContainerTranslator);
//!
//! // Translate with context
//! let ctx = TranslationContext::new()
//!     .default_value("image", "alpine:latest");
//! let container: ContainerConfig = registry.translate_with_context(&process, &ctx)?;
//! ```

mod codebase;
mod container;
mod context;
mod contextual;
mod defaults;
mod error;
pub mod import;
mod inference;
mod registry;
mod traits;

pub use codebase::{
    CodebaseInferrer, DetectedPackage, InferenceConfidence, InferenceError, ProjectInference,
    ProjectType, SuggestedService,
};
pub use container::{
    ContainerConfig, ContainerDependency, ContainerHealthCheck, ContainerRestartPolicy,
    DependencyCondition, HealthCheckTest, PortMapping, Protocol, ResourceLimits, VolumeMount,
};
pub use context::{TargetEnvironment, TranslationContext, TranslationOptions};
pub use contextual::{AsyncContextualTranslator, ContextAdapter, ContextualTranslator};
pub use defaults::new_registry;
pub use error::{TranslationError, TranslationResult};
pub use inference::{common_images, image_from_nixpkg, infer_image, Confidence, InferredImage};
pub use registry::{TranslatorInfo, TranslatorRegistry};
pub use traits::{AsyncTranslator, Translator, TranslatorMetadata};
