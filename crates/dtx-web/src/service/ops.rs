//! Service operations — transport-agnostic business logic.
//!
//! All non-orchestrator business logic lives here. Handlers parse requests
//! and format responses; `ServiceOps` does the actual work.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use dtx_core::config::schema::{NixConfig, ResourceConfig};
use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::model::Service;
use dtx_core::store::ConfigStore;
use dtx_core::{
    analyze_service_packages, FlakeGenerator, MappingsConfig, NixClient, Package,
    PackageAnalysisResult, ProjectConfig, ServicePackageAnalysis,
};

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Parameter types
// ---------------------------------------------------------------------------

/// Transport-agnostic parameters for creating a service.
pub struct CreateServiceParams {
    pub name: String,
    pub command: String,
    pub package: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
}

/// Transport-agnostic parameters for updating a service.
pub struct UpdateServiceParams {
    pub name: Option<String>,
    pub command: Option<String>,
    pub package: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
    pub enabled: Option<bool>,
}

/// Editable file types within a dtx project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditableFileType {
    Config,
    Flake,
    Mappings,
}

impl EditableFileType {
    /// Parse from a string identifier.
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s {
            "config" => Ok(Self::Config),
            "flake" => Ok(Self::Flake),
            "mappings" => Ok(Self::Mappings),
            _ => Err(AppError::bad_request(format!("Unknown file type: {}", s))),
        }
    }

    fn resolve_path(&self, project_root: &std::path::Path) -> PathBuf {
        match self {
            Self::Config => project_root.join(".dtx").join("config.yaml"),
            Self::Flake => project_root.join("flake.nix"),
            Self::Mappings => project_root.join(".dtx").join("mappings.toml"),
        }
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Nix environment status.
pub struct NixStatus {
    pub has_flake: bool,
    pub has_envrc: bool,
    pub packages: Vec<String>,
}

/// Result of a Nix init operation.
pub struct NixInitResult {
    pub files: Vec<String>,
}

/// Result of reading a project file.
pub struct ReadFileResult {
    pub path: String,
    pub content: String,
    pub exists: bool,
}

/// Result of validating file content.
pub struct ValidateFileResult {
    pub valid: bool,
    pub error: Option<String>,
}

/// Project metadata.
pub struct ProjectInfo {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
}

/// Project configuration with path.
pub struct ProjectConfigInfo {
    pub config: ProjectConfig,
    pub path: String,
}

/// Package analysis info for a single service.
pub struct PackageAnalysis {
    pub service_name: String,
    pub command: String,
    pub status: String,
    pub package: Option<String>,
    pub executable: Option<String>,
}

/// Result of importing services from an external config format.
pub struct ImportResult {
    pub imported: usize,
    pub warnings: Vec<String>,
    pub service_names: Vec<String>,
    pub services: Vec<Service>,
}

// ---------------------------------------------------------------------------
// ServiceOps
// ---------------------------------------------------------------------------

/// Transport-agnostic service operations.
///
/// Encapsulates all business logic for CRUD, Nix, package mappings,
/// and file editing. Handlers delegate here after parsing requests.
pub struct ServiceOps {
    store: Arc<RwLock<ConfigStore>>,
    nix_client: Arc<NixClient>,
    event_bus: Arc<ResourceEventBus>,
}

impl ServiceOps {
    /// Create a new `ServiceOps` from shared state components.
    pub fn new(
        store: Arc<RwLock<ConfigStore>>,
        nix_client: Arc<NixClient>,
        event_bus: Arc<ResourceEventBus>,
    ) -> Self {
        Self {
            store,
            nix_client,
            event_bus,
        }
    }

    // -----------------------------------------------------------------------
    // Service CRUD
    // -----------------------------------------------------------------------

    /// List all services from the config store.
    pub async fn list_services(&self) -> Result<Vec<Service>, AppError> {
        let store = self.store.read().await;
        Ok(dtx_core::model::services_from_config(store.config()))
    }

    /// Get a single service by name.
    pub async fn get_service(&self, name: &str) -> Result<Service, AppError> {
        let store = self.store.read().await;
        let rc = store
            .get_resource(name)
            .ok_or_else(|| AppError::not_found(format!("Service '{}' not found", name)))?;
        Ok(Service::from_resource_config(name, rc))
    }

    /// Create a new service. Returns the created service and the full service list.
    pub async fn create_service(
        &self,
        params: CreateServiceParams,
    ) -> Result<(Service, Vec<Service>), AppError> {
        if let Some(ref pkg) = params.package {
            if !pkg.is_empty() {
                let valid = self
                    .nix_client
                    .validate(pkg)
                    .await
                    .map_err(AppError::from)?;
                if !valid {
                    return Err(AppError::bad_request(format!(
                        "Package '{}' not found",
                        pkg
                    )));
                }
            }
        }

        let pkg = params.package.filter(|s| !s.is_empty());

        let rc = ResourceConfig {
            command: Some(params.command),
            port: params.port,
            working_dir: params.working_dir.map(PathBuf::from),
            nix: pkg.as_ref().map(|p| NixConfig {
                packages: vec![p.clone()],
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut store = self.store.write().await;
        store
            .add_resource(&params.name, rc.clone())
            .map_err(AppError::from)?;
        store.save().map_err(AppError::from)?;

        // Sync flake.nix if service has a package
        if let Some(ref p) = pkg {
            let project_root = store.project_root().to_path_buf();
            let project_name = store.project_name().to_string();
            if let Err(e) = dtx_core::sync_add_package(&project_root, &project_name, p) {
                tracing::warn!("Failed to sync flake.nix: {}", e);
            }
        }

        let service = Service::from_resource_config(&params.name, &rc);
        let services = dtx_core::model::services_from_config(store.config());
        Ok((service, services))
    }

    /// Update an existing service. Returns the updated service.
    pub async fn update_service(
        &self,
        name: &str,
        params: UpdateServiceParams,
    ) -> Result<Service, AppError> {
        // Validate package if changing
        if let Some(ref pkg) = params.package {
            let valid = self
                .nix_client
                .validate(pkg)
                .await
                .map_err(AppError::from)?;
            if !valid {
                return Err(AppError::bad_request(format!(
                    "Package '{}' not found",
                    pkg
                )));
            }
        }

        let mut store = self.store.write().await;

        let rc = store
            .get_resource_mut(name)
            .ok_or_else(|| AppError::not_found(format!("Service '{}' not found", name)))?;

        let old_package = rc.nix.as_ref().and_then(|n| n.packages.first().cloned());

        if let Some(ref cmd) = params.command {
            rc.command = Some(cmd.clone());
        }
        if let Some(port) = params.port {
            rc.port = Some(port);
        }
        if let Some(ref wd) = params.working_dir {
            rc.working_dir = Some(PathBuf::from(wd));
        }
        if let Some(enabled) = params.enabled {
            rc.enabled = enabled;
        }
        if let Some(ref pkg) = params.package {
            rc.nix = Some(NixConfig {
                packages: vec![pkg.clone()],
                ..Default::default()
            });
        }

        let updated_rc = rc.clone();
        let final_name = if let Some(ref new_name) = params.name {
            if new_name != name {
                let rc_clone = store.remove_resource(name).map_err(AppError::from)?;
                drop(rc_clone);
                store
                    .add_resource(new_name, updated_rc.clone())
                    .map_err(AppError::from)?;
            }
            new_name.clone()
        } else {
            name.to_owned()
        };

        store.save().map_err(AppError::from)?;

        // Sync flake.nix if package changed
        let new_package = updated_rc
            .nix
            .as_ref()
            .and_then(|n| n.packages.first().cloned());
        if old_package.as_deref() != new_package.as_deref() {
            let project_root = store.project_root().to_path_buf();
            let project_name = store.project_name().to_string();

            if let Some(ref pkg) = new_package {
                if let Err(e) = dtx_core::sync_add_package(&project_root, &project_name, pkg) {
                    tracing::warn!("Failed to sync flake.nix (add): {}", e);
                }
            }

            if let Some(ref pkg) = old_package {
                let remaining_packages: HashSet<String> = store
                    .list_resources()
                    .filter_map(|(_, rc)| rc.nix.as_ref().and_then(|n| n.packages.first().cloned()))
                    .collect();
                if let Err(e) =
                    dtx_core::sync_remove_package(&project_root, pkg, &remaining_packages)
                {
                    tracing::warn!("Failed to sync flake.nix (remove): {}", e);
                }
            }
        }

        Ok(Service::from_resource_config(&final_name, &updated_rc))
    }

    /// Delete a service by name. Returns the remaining service list.
    pub async fn delete_service(&self, name: &str) -> Result<Vec<Service>, AppError> {
        let mut store = self.store.write().await;

        let removed_rc = store.remove_resource(name).map_err(AppError::from)?;
        store.save().map_err(AppError::from)?;

        // Sync flake.nix if the removed service had a package
        let removed_package = removed_rc
            .nix
            .as_ref()
            .and_then(|n| n.packages.first().cloned());
        if let Some(ref pkg) = removed_package {
            let project_root = store.project_root().to_path_buf();
            let remaining_packages: HashSet<String> = store
                .list_resources()
                .filter_map(|(_, rc)| rc.nix.as_ref().and_then(|n| n.packages.first().cloned()))
                .collect();
            if let Err(e) = dtx_core::sync_remove_package(&project_root, pkg, &remaining_packages) {
                tracing::warn!("Failed to sync flake.nix: {}", e);
            }
        }

        Ok(dtx_core::model::services_from_config(store.config()))
    }

    // -----------------------------------------------------------------------
    // Nix operations
    // -----------------------------------------------------------------------

    /// Search for Nix packages.
    pub async fn nix_search(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Package>, AppError> {
        let mut packages = self
            .nix_client
            .search(query)
            .await
            .map_err(AppError::from)?;

        if let Some(limit) = limit {
            packages.truncate(limit);
        }

        Ok(packages)
    }

    /// Validate that a Nix package exists.
    pub async fn nix_validate(&self, package: &str) -> Result<bool, AppError> {
        self.nix_client
            .validate(package)
            .await
            .map_err(AppError::from)
    }

    /// Get Nix environment status for the current project.
    pub async fn nix_status(&self) -> Result<NixStatus, AppError> {
        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();

        let has_flake = project_root.join("flake.nix").exists();
        let has_envrc = project_root.join(".envrc").exists();

        let packages: Vec<String> = store
            .list_enabled_resources()
            .filter_map(|(_, rc)| rc.nix.as_ref().and_then(|n| n.packages.first().cloned()))
            .collect();

        Ok(NixStatus {
            has_flake,
            has_envrc,
            packages,
        })
    }

    /// Initialize Nix environment (flake.nix + .envrc).
    pub async fn nix_init(&self) -> Result<NixInitResult, AppError> {
        let store = self.store.read().await;
        let services = dtx_core::model::services_from_config(store.config());
        let project_root = store.project_root().to_path_buf();
        let project_name = store.project_name().to_string();

        let flake = FlakeGenerator::generate(&services, &project_name);
        let envrc = dtx_core::EnvrcGenerator::generate_with_layout(&services);

        let flake_path = project_root.join("flake.nix");
        let envrc_path = project_root.join(".envrc");

        tokio::task::spawn_blocking(move || -> Result<NixInitResult, AppError> {
            std::fs::write(&flake_path, &flake)
                .map_err(|e| AppError::internal(format!("Failed to write flake.nix: {}", e)))?;
            std::fs::write(&envrc_path, &envrc)
                .map_err(|e| AppError::internal(format!("Failed to write .envrc: {}", e)))?;
            Ok(NixInitResult {
                files: vec!["flake.nix".to_string(), ".envrc".to_string()],
            })
        })
        .await
        .map_err(|e| AppError::internal(format!("Blocking task failed: {}", e)))?
    }

    /// Regenerate .envrc only.
    pub async fn nix_envrc(&self) -> Result<NixInitResult, AppError> {
        let store = self.store.read().await;
        let services = dtx_core::model::services_from_config(store.config());
        let project_root = store.project_root().to_path_buf();

        let envrc = dtx_core::EnvrcGenerator::generate_with_layout(&services);

        let envrc_path = project_root.join(".envrc");

        tokio::task::spawn_blocking(move || -> Result<NixInitResult, AppError> {
            std::fs::write(&envrc_path, &envrc)
                .map_err(|e| AppError::internal(format!("Failed to write .envrc: {}", e)))?;
            Ok(NixInitResult {
                files: vec![".envrc".to_string()],
            })
        })
        .await
        .map_err(|e| AppError::internal(format!("Blocking task failed: {}", e)))?
    }

    /// Generate flake.nix content from current services.
    pub async fn nix_flake(&self) -> Result<String, AppError> {
        let store = self.store.read().await;
        let services = dtx_core::model::services_from_config(store.config());
        let project_name = store.project_name().to_string();
        Ok(FlakeGenerator::generate(&services, &project_name))
    }

    // -----------------------------------------------------------------------
    // Package mapping operations
    // -----------------------------------------------------------------------

    /// Analyze packages for all services.
    pub async fn analyze_packages(&self) -> Result<Vec<PackageAnalysis>, AppError> {
        let store = self.store.read().await;
        let services = dtx_core::model::services_from_config(store.config());
        Ok(convert_package_analysis(analyze_service_packages(
            &services,
        )))
    }

    /// Add a command-to-package mapping.
    pub async fn add_mapping(
        &self,
        command: &str,
        package: &str,
    ) -> Result<Vec<PackageAnalysis>, AppError> {
        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();

        let mut config =
            ProjectConfig::load(&project_root).map_err(|e| AppError::bad_request(e.to_string()))?;

        config.add_mapping(command, package);
        config
            .save(&project_root)
            .map_err(|e| AppError::internal(e.to_string()))?;

        tracing::info!(command = %command, package = %package, "Added package mapping");

        let services = dtx_core::model::services_from_config(store.config());
        Ok(convert_package_analysis(analyze_service_packages(
            &services,
        )))
    }

    /// Remove a command-to-package mapping.
    pub async fn remove_mapping(&self, command: &str) -> Result<Vec<PackageAnalysis>, AppError> {
        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();

        let mut config =
            ProjectConfig::load(&project_root).map_err(|e| AppError::bad_request(e.to_string()))?;

        let removed = config.remove_mapping(command);
        if removed.is_some() {
            config
                .save(&project_root)
                .map_err(|e| AppError::internal(e.to_string()))?;
            tracing::info!(command = %command, "Removed package mapping");
        }

        let services = dtx_core::model::services_from_config(store.config());
        Ok(convert_package_analysis(analyze_service_packages(
            &services,
        )))
    }

    /// Mark a command as a local binary.
    pub async fn mark_local(&self, command: &str) -> Result<Vec<PackageAnalysis>, AppError> {
        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();

        let mut config = ProjectConfig::load(&project_root).unwrap_or_default();
        config.add_local(command);
        config
            .save(&project_root)
            .map_err(|e| AppError::internal(e.to_string()))?;

        let services = dtx_core::model::services_from_config(store.config());
        Ok(convert_package_analysis(analyze_service_packages(
            &services,
        )))
    }

    /// Mark a command as ignored.
    pub async fn mark_ignore(&self, command: &str) -> Result<Vec<PackageAnalysis>, AppError> {
        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();

        let mut config = ProjectConfig::load(&project_root).unwrap_or_default();
        config.add_ignore(command);
        config
            .save(&project_root)
            .map_err(|e| AppError::internal(e.to_string()))?;

        let services = dtx_core::model::services_from_config(store.config());
        Ok(convert_package_analysis(analyze_service_packages(
            &services,
        )))
    }

    // -----------------------------------------------------------------------
    // File editing
    // -----------------------------------------------------------------------

    /// Read a project file by type.
    pub async fn read_file(&self, file_type: EditableFileType) -> Result<ReadFileResult, AppError> {
        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();
        let project_name = store.project_name().to_string();
        let config = store.config().clone();
        drop(store);

        let ft = file_type;
        tokio::task::spawn_blocking(move || -> Result<ReadFileResult, AppError> {
            let file_path = ft.resolve_path(&project_root);

            let (content, exists) = if file_path.exists() {
                let content = std::fs::read_to_string(&file_path)
                    .map_err(|e| AppError::internal(format!("Failed to read file: {}", e)))?;
                (content, true)
            } else {
                let example = match ft {
                    EditableFileType::Config => ProjectConfig::example(),
                    EditableFileType::Mappings => MappingsConfig::example(),
                    EditableFileType::Flake => {
                        let services = dtx_core::model::services_from_config(&config);
                        FlakeGenerator::generate(&services, &project_name)
                    }
                };
                (example, false)
            };

            Ok(ReadFileResult {
                path: file_path.to_string_lossy().to_string(),
                content,
                exists,
            })
        })
        .await
        .map_err(|e| AppError::internal(format!("Blocking task failed: {}", e)))?
    }

    /// Save a project file after server-side validation.
    pub async fn save_file(
        &self,
        file_type: EditableFileType,
        content: &str,
    ) -> Result<String, AppError> {
        // Validate before writing
        let validation = self.validate_file(file_type, content)?;
        if !validation.valid {
            return Err(AppError::bad_request(format!(
                "Validation failed: {}",
                validation.error.unwrap_or_default()
            )));
        }

        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();
        let project_name = store.project_name().to_string();
        drop(store);

        let file_path = file_type.resolve_path(&project_root);
        let content_owned = content.to_owned();

        let saved_path = tokio::task::spawn_blocking(move || -> Result<String, AppError> {
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::internal(format!("Failed to create directory: {}", e))
                })?;
            }

            std::fs::write(&file_path, &content_owned)
                .map_err(|e| AppError::internal(format!("Failed to write file: {}", e)))?;

            Ok(file_path.to_string_lossy().to_string())
        })
        .await
        .map_err(|e| AppError::internal(format!("Blocking task failed: {}", e)))??;

        tracing::info!(
            file_type = ?file_type,
            path = %saved_path,
            "Saved file via inline editor"
        );

        // Publish config changed event
        self.event_bus.publish(LifecycleEvent::ConfigChanged {
            project_id: project_name,
            timestamp: chrono::Utc::now(),
        });

        Ok(saved_path)
    }

    /// Validate file content without saving.
    pub fn validate_file(
        &self,
        file_type: EditableFileType,
        content: &str,
    ) -> Result<ValidateFileResult, AppError> {
        let result = match file_type {
            EditableFileType::Config => ProjectConfig::parse(content).map(|_| ()),
            EditableFileType::Mappings => MappingsConfig::parse(content).map(|_| ()),
            EditableFileType::Flake => dtx_core::nix::ast::validate_flake_nix(content),
        };

        Ok(ValidateFileResult {
            valid: result.is_ok(),
            error: result.err(),
        })
    }

    // -----------------------------------------------------------------------
    // Project / config
    // -----------------------------------------------------------------------

    /// Get project metadata.
    pub async fn get_project(&self) -> Result<ProjectInfo, AppError> {
        let store = self.store.read().await;
        Ok(ProjectInfo {
            name: store.project_name().to_string(),
            description: store.project_description().map(|s| s.to_string()),
            path: store.project_root().display().to_string(),
        })
    }

    /// Get project configuration.
    pub async fn get_config(&self) -> Result<ProjectConfigInfo, AppError> {
        let store = self.store.read().await;
        let project_root = store.project_root().to_path_buf();

        let config =
            ProjectConfig::load(&project_root).map_err(|e| AppError::bad_request(e.to_string()))?;

        let config_path = ProjectConfig::config_path(&project_root);

        Ok(ProjectConfigInfo {
            config,
            path: config_path.to_string_lossy().to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // Import
    // -----------------------------------------------------------------------

    /// Import services from external configuration format.
    ///
    /// Resolves the format (auto-detect if needed), runs the appropriate
    /// importer, and adds the discovered resources to the store.
    pub async fn import_config(
        &self,
        content: &str,
        format_str: &str,
    ) -> Result<ImportResult, AppError> {
        use dtx_core::translation::import::{
            DockerComposeImporter, ImportFormat, Importer, ProcessComposeImporter, ProcfileImporter,
        };

        let format = match format_str {
            "auto" => ImportFormat::Auto,
            "process-compose" | "pc" => ImportFormat::ProcessCompose,
            "docker-compose" | "docker" | "compose" => ImportFormat::DockerCompose,
            "procfile" => ImportFormat::Procfile,
            other => {
                return Err(AppError::bad_request(format!(
                    "Unknown import format: '{}'. Supported: auto, process-compose, docker-compose, procfile",
                    other
                )));
            }
        };

        let resolved_format = if format == ImportFormat::Auto {
            ImportFormat::from_content(content).ok_or_else(|| {
                AppError::bad_request(
                    "Could not auto-detect configuration format. Please specify explicitly.",
                )
            })?
        } else {
            format
        };

        let imported = match resolved_format {
            ImportFormat::ProcessCompose => ProcessComposeImporter.import(content),
            ImportFormat::DockerCompose => DockerComposeImporter.import(content),
            ImportFormat::Procfile => ProcfileImporter.import(content),
            ImportFormat::Auto => unreachable!(),
        }
        .map_err(|e| AppError::bad_request(format!("Import failed: {}", e)))?;

        let mut store = self.store.write().await;
        let mut service_names = Vec::new();
        let mut warnings = imported.warnings.clone();

        for resource in &imported.resources {
            let mut env = indexmap::IndexMap::new();
            for (k, v) in &resource.environment {
                env.insert(k.clone(), v.clone());
            }
            let deps: Vec<dtx_core::config::schema::DependencyConfig> = resource
                .depends_on
                .iter()
                .map(|d| dtx_core::config::schema::DependencyConfig::Simple(d.clone()))
                .collect();
            let rc = ResourceConfig {
                command: resource.command.clone(),
                port: resource.port,
                working_dir: resource.working_dir.as_ref().map(PathBuf::from),
                environment: env,
                depends_on: deps,
                ..Default::default()
            };
            if let Err(e) = store.add_resource(&resource.name, rc) {
                warnings.push(format!("Skipped '{}': {}", resource.name, e));
                continue;
            }
            service_names.push(resource.name.clone());
        }

        store.save().map_err(AppError::from)?;
        let services = dtx_core::model::services_from_config(store.config());

        tracing::info!(count = service_names.len(), format = ?resolved_format, "Imported services");

        Ok(ImportResult {
            imported: service_names.len(),
            warnings,
            service_names,
            services,
        })
    }

    /// Sync flake.nix from current service configuration.
    pub async fn sync_flake(&self) -> Result<(), AppError> {
        let store = self.store.read().await;
        let services = dtx_core::model::services_from_config(store.config());
        let project_root = store.project_root().to_path_buf();
        let project_name = store.project_name().to_string();

        let flake = FlakeGenerator::generate(&services, &project_name);
        let flake_path = project_root.join("flake.nix");

        tokio::task::spawn_blocking(move || -> Result<(), AppError> {
            std::fs::write(&flake_path, &flake)
                .map_err(|e| AppError::internal(format!("Failed to write flake.nix: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::internal(format!("Blocking task failed: {}", e)))?
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert raw `ServicePackageAnalysis` into our transport-agnostic `PackageAnalysis`.
fn convert_package_analysis(analyses: Vec<ServicePackageAnalysis>) -> Vec<PackageAnalysis> {
    analyses
        .into_iter()
        .map(|a| {
            let (status, package, executable) = match a.result {
                PackageAnalysisResult::Explicit(p) => ("explicit".to_string(), Some(p), None),
                PackageAnalysisResult::AutoDetected(p) => ("auto".to_string(), Some(p), None),
                PackageAnalysisResult::LocalBinary => ("local".to_string(), None, None),
                PackageAnalysisResult::NeedsAttention(e) => {
                    ("needs_attention".to_string(), None, Some(e))
                }
            };
            PackageAnalysis {
                service_name: a.service_name,
                command: a.command,
                status,
                package,
                executable,
            }
        })
        .collect()
}
