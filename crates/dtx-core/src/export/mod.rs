//! Export system for resource configurations.
//!
//! This module provides infrastructure for exporting dtx projects
//! to various configuration formats (docker-compose, Kubernetes, etc.).
//!
//! # Overview
//!
//! - [`ExportFormat`] - Supported export formats
//! - [`Exporter`] - Trait for format-specific exporters
//! - [`ExportableProject`] - Project data for export
//! - [`ExportableService`] - Service data for export
//!
//! # Example
//!
//! ```ignore
//! use dtx_core::export::{ExportFormat, ExportableProject, DockerComposeExporter};
//!
//! let project = ExportableProject::new("my-app")
//!     .with_service(api_service)
//!     .with_service(db_service);
//!
//! let exporter = DockerComposeExporter::new();
//! let yaml = exporter.export(&project)?;
//! ```

mod docker_compose;
mod error;
mod kubernetes;
mod process_compose;
mod types;

pub use docker_compose::{
    ComposeCommand, ComposeDependency, ComposeDependsOn, ComposeDeploy, ComposeFile,
    ComposeHealthCheck, ComposeNetwork, ComposeResources, ComposeService, ComposeVolume,
    DockerComposeExporter,
};
pub use error::{ExportError, ExportResult};
pub use kubernetes::{
    K8sContainer, K8sContainerPort, K8sDeployment, K8sDeploymentSpec, K8sEnvVar, K8sMetadata,
    K8sPodSpec, K8sPodTemplateSpec, K8sProbe, K8sResource, K8sResourceLimits, K8sResources,
    K8sSelector, K8sService, K8sServicePort, K8sServiceSpec, KubernetesExporter,
};
pub use process_compose::{
    ProcessComposeAvailability, ProcessComposeDependency, ProcessComposeExec,
    ProcessComposeExporter, ProcessComposeFile, ProcessComposeHttpGet, ProcessComposeProbe,
    ProcessComposeProcess, ProcessComposeShutdown, PROCESS_COMPOSE_VERSION,
};
pub use types::{ExportFormat, ExportableProject, ExportableService, Exporter};
