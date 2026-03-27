//! Core engine for dtx.
//!
//! This crate provides the core functionality for dtx:
//!
//! - Nix package search and validation
//! - Dependency graph analysis and validation
//! - Domain types with Parse Don't Validate pattern
//! - Event system for process lifecycle events
//! - Resource abstraction for universal orchestration
//! - Middleware system for composable operation processing
//! - Preflight checks and port utilities
//!
//! ## Process Orchestration
//!
//! Process orchestration is provided by the `dtx-process` crate:
//!
//! ```ignore
//! use dtx_process::{ProcessResource, ResourceOrchestrator, ProcessResourceConfig};
//! use dtx_core::events::ResourceEventBus;
//! ```

pub mod config;
pub mod domain;
pub mod error;
pub mod events;
pub mod export;
pub mod graph;
pub mod middleware;
pub mod model;
pub mod nix;
pub mod process;
pub mod resource;
pub mod store;
pub mod translation;

// Re-exports
pub use config::generator::YamlGenerator;
pub use config::process_compose::{
    Availability, DependencyCondition, DependencyConditionType, DependsOn, ExecProbe, HttpGetProbe,
    Probe, ProcessComposeConfig, ProcessConfig, ShutdownConfig,
};
pub use config::project::{
    discover_project, discover_project_from, find_project_root, find_project_root_cwd,
    global_dtx_dir, ConfigError, DiscoveredProject, MappingsSection, ProjectConfig, ProjectMeta,
    RuntimeSection, ServicesSection, CONFIG_FILE, DTX_DIR, GLOBAL_DTX_DIR,
};
pub use domain::{
    EnvVar, Environment, ParseEnvironmentError, ParsePortError, ParseServiceNameError,
    ParseShellCommandError, Port, ServiceName, ShellCommand, MIN_NON_PRIVILEGED_PORT,
};
pub use error::{CoreError, NixError, PortConflictDetail, PortConflictError, Result};
pub use events::{
    event_socket_path, find_running_instance, notify_config_changed, notify_config_changed_sync,
    read_web_port, register_instance, start_event_listener,
    DependencyCondition as LifecycleDependencyCondition, EventFilter, InstanceEntry,
    InstanceGuard, LifecycleEvent, PortGuard, ResourceEventBus, ResourceEventSubscriber,
    SocketGuard,
};
pub use graph::{
    CycleError, DependencyGraph, DomainStatus, EdgeConfidence, EdgeKind, FileSource, GraphEdge,
    GraphNode, GraphSources, GraphStats, GraphValidator, GraphView, ImpactEntry, ImpactSet,
    MemorySource, NodeDomain, NodeMetadata, SymbolSource,
};
pub use nix::{
    analyze_service_packages, ast::FlakeAst, dev_env_cache, extract_executable, find_flake_path,
    get_services_needing_attention, infer_package, infer_package_detailed,
    infer_package_with_config, infer_packages_for_services, init_project_config, init_user_config,
    is_local_binary, sync_add_package, sync_remove_package, CliBackend, DevEnvCache,
    DevEnvironment, EnvrcGenerator, FlakeGenerator, FlakeLock, MappingsConfig, NixBackend,
    NixClient, NixShell, Package, PackageAnalysisResult, PackageInference, PackageInfo,
    PackageMappings, SearchResult, SearchTier, ServicePackageAnalysis,
};
pub use process::port::{
    check_ports_availability, extract_service_ports, find_available_port, find_available_port_near,
    is_port_available, resolve_port_conflicts, validate_ports, validate_ports_sync,
    validate_service_ports, PortReassignment, ServicePort,
};
pub use process::preflight::{
    analyze_services, run_preflight, CheckType, PreflightCheck, PreflightResult,
};

pub use resource::{
    ConfigError as ResourceConfigError, Context as ResourceContext, Error as ResourceError2,
    ErrorConfigError as ResourceConfigError2, HealthStatus, IoResultExt, LogEntry, LogStream,
    LogStreamKind, Resource, ResourceConfig, ResourceError, ResourceExt, ResourceId, ResourceKind,
    ResourceResult, ResourceState, Result as ResourceResult2, ResultExt,
};

pub use middleware::{
    FnHandler, Handler, Middleware, MiddlewareChain, MiddlewareStack, MiddlewareStackBuilder, Next,
    NoopHandler, Operation, PassthroughMiddleware, Response,
};

pub use store::{ConfigStore, ProjectRegistry, StoreError};

pub use export::{
    DockerComposeExporter, ExportError, ExportFormat, ExportResult, ExportableProject,
    ExportableService, Exporter, KubernetesExporter,
};

pub use model::{
    enabled_services_from_config, services_from_config, Dependency as ModelDependency,
    DependencyCondition as ModelDependencyCondition, HealthCheck as ModelHealthCheck,
    HealthCheckType as ModelHealthCheckType, HttpHealthCheck as ModelHttpHealthCheck,
    Service as ModelService,
};

pub use translation::{
    common_images, image_from_nixpkg, infer_image, new_registry, AsyncContextualTranslator,
    AsyncTranslator, Confidence, ContainerConfig, ContainerDependency, ContainerHealthCheck,
    ContainerRestartPolicy, ContextAdapter, ContextualTranslator,
    DependencyCondition as ContainerDependencyCondition, HealthCheckTest, InferredImage,
    PortMapping, Protocol, ResourceLimits, TargetEnvironment, TranslationContext, TranslationError,
    TranslationOptions, TranslationResult, Translator, TranslatorInfo, TranslatorMetadata,
    TranslatorRegistry, VolumeMount,
};
