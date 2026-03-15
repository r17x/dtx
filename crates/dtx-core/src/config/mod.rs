//! Configuration generation module.

pub mod generator;
pub mod loader;
pub mod process_compose;
pub mod project;
pub mod schema;

pub use loader::{load_config, load_config_from, ConfigLevel, ConfigLoader};
pub use process_compose::{
    Availability, DependencyCondition, DependencyConditionType, DependsOn, ExecProbe, HttpGetProbe,
    Probe, ProcessComposeConfig, ProcessConfig, ShutdownConfig,
};
pub use schema::{
    AiConfig, DefaultsConfig, DependencyConditionConfig, DependencyConfig, DtxConfig, GlobalConfig,
    GlobalNixConfig, HealthConfig, McpConfig, NixConfig, ProjectMetadata, ResourceConfig,
    ResourceKindConfig, RestartConfig, RestartPolicy, SchemaError, ShutdownConfigSchema, VmConfig,
    SCHEMA_VERSION,
};
