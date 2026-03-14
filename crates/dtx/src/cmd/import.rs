//! Import configuration from external formats.

use crate::context::Context;
use crate::output::Output;
use anyhow::{bail, Result};
use dtx_core::config::schema::{
    DependencyConditionConfig, DependencyConfig, HealthConfig, NixConfig, ResourceConfig,
};
use dtx_core::translation::import::{
    DockerComposeImporter, ImportFormat, ImportedConfig, ImportedResource, Importer,
    ProcessComposeImporter, ProcfileImporter,
};
use dtx_core::{sync_add_package, Environment, Port, ServiceName, ShellCommand};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for the import command.
pub struct ImportArgs {
    pub file: PathBuf,
    pub format: Option<String>,
    pub no_nix: bool,
    pub dry_run: bool,
}

/// Parse format string to ImportFormat.
fn parse_format(format: &str) -> Result<ImportFormat> {
    match format.to_lowercase().as_str() {
        "process-compose" | "pc" => Ok(ImportFormat::ProcessCompose),
        "docker-compose" | "docker" | "dc" => Ok(ImportFormat::DockerCompose),
        "procfile" | "heroku" => Ok(ImportFormat::Procfile),
        "auto" => Ok(ImportFormat::Auto),
        _ => bail!(
            "Unknown format '{}'. Valid formats: process-compose, docker-compose, procfile, auto",
            format
        ),
    }
}

/// Detect format from file path and content.
fn detect_format(path: &Path, content: &str) -> Result<ImportFormat> {
    if let Some(format) = ImportFormat::from_path(path) {
        return Ok(format);
    }

    if let Some(format) = ImportFormat::from_content(content) {
        return Ok(format);
    }

    bail!(
        "Unable to detect configuration format for '{}'. Use --format to specify.",
        path.display()
    )
}

/// Get the appropriate importer for a format.
fn get_importer(format: ImportFormat) -> Box<dyn Importer> {
    match format {
        ImportFormat::ProcessCompose => Box::new(ProcessComposeImporter::new()),
        ImportFormat::DockerCompose => Box::new(DockerComposeImporter::new()),
        ImportFormat::Procfile => Box::new(ProcfileImporter::new()),
        ImportFormat::Auto => {
            unreachable!("Auto format should be resolved before calling get_importer")
        }
    }
}

/// Display a summary of imported resources.
fn display_import_summary(out: &Output, config: &ImportedConfig) {
    if let Some(ref name) = config.project_name {
        out.raw(&format!("Project: {}\n", name));
    }

    out.blank();
    out.raw(&format!(
        "Imported {} resource(s):\n",
        config.resources.len()
    ));
    out.blank();

    for resource in &config.resources {
        out.raw(&format!("  [{}]\n", resource.name));

        if let Some(ref cmd) = resource.command {
            out.raw(&format!("    command: {}\n", cmd));
        }

        if let Some(ref image) = resource.image {
            out.raw(&format!("    image: {}\n", image));
        }

        if let Some(port) = resource.port {
            out.raw(&format!("    port: {}\n", port));
        }

        if let Some(ref dir) = resource.working_dir {
            out.raw(&format!("    working_dir: {}\n", dir));
        }

        if !resource.environment.is_empty() {
            out.raw("    environment:\n");
            for (key, value) in &resource.environment {
                out.raw(&format!("      {}={}\n", key, value));
            }
        }

        if !resource.depends_on.is_empty() {
            out.raw(&format!(
                "    depends_on: {}\n",
                resource.depends_on.join(", ")
            ));
        }

        if let Some(ref hc) = resource.health_check {
            out.raw(&format!("    health_check: {}\n", hc));
        }

        if let Some(ref restart) = resource.restart {
            out.raw(&format!("    restart: {}\n", restart));
        }

        out.blank();
    }

    if !config.warnings.is_empty() {
        for warning in &config.warnings {
            out.warning(&warning.to_string());
        }
        out.blank();
    }
}

/// Infer Nix package from resource.
fn infer_nix_package(resource: &ImportedResource) -> Option<String> {
    use dtx_core::nix::{extract_executable, PackageMappings};

    let mappings = PackageMappings::load();

    if let Some(pkg) = mappings.get_package(&resource.name) {
        return Some(pkg.clone());
    }

    if let Some(ref cmd) = resource.command {
        if let Some(executable) = extract_executable(cmd) {
            if let Some(pkg) = mappings.get_package(&executable) {
                return Some(pkg.clone());
            }
        }
    }

    if let Some(ref image) = resource.image {
        let base = image.split(':').next().unwrap_or(image);
        let base = base.split('/').next_back().unwrap_or(base);
        if let Some(pkg) = mappings.get_package(base) {
            return Some(pkg.clone());
        }
    }

    None
}

/// Create a ResourceConfig from an imported resource.
fn resource_config_from_imported(
    resource: &ImportedResource,
    nix_package: Option<String>,
) -> Result<ResourceConfig> {
    // Validate service name
    let _: ServiceName = resource
        .name
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid service name '{}': {}", resource.name, e))?;

    // Validate dependency names
    for dep_name in &resource.depends_on {
        dep_name
            .parse::<ServiceName>()
            .map_err(|e| anyhow::anyhow!("Invalid dependency name '{}': {}", dep_name, e))?;
    }

    // Get command
    let command = match &resource.command {
        Some(cmd) => cmd.clone(),
        None => {
            if let Some(ref image) = resource.image {
                format!("# TODO: Replace with actual command for {}", image)
            } else {
                format!("# TODO: Add command for {}", resource.name)
            }
        }
    };

    // Validate command
    let _: ShellCommand = command
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid command: {}", e))?;

    // Validate port
    let port = resource
        .port
        .map(Port::try_from)
        .transpose()
        .map_err(|e| anyhow::anyhow!("Invalid port: {}", e))?
        .map(u16::from);

    // Parse environment variables
    let environment: IndexMap<String, String> = if resource.environment.is_empty() {
        IndexMap::new()
    } else {
        let env_strings: Vec<String> = resource
            .environment
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        let env = Environment::from_strings(&env_strings)
            .map_err(|e| anyhow::anyhow!("Invalid environment variable: {}", e))?;
        env.into_map().into_iter().collect()
    };

    // Parse dependencies
    let depends_on: Vec<DependencyConfig> = resource
        .depends_on
        .iter()
        .map(|name| {
            let mut map = IndexMap::new();
            map.insert(name.clone(), DependencyConditionConfig::Healthy);
            DependencyConfig::WithCondition(map)
        })
        .collect();

    // Parse health check
    let health = resource.health_check.as_ref().map(|hc| HealthConfig {
        exec: Some(hc.clone()),
        ..Default::default()
    });

    // Build nix config: merge inferred package with custom exported packages
    let mut all_packages = resource.nix_packages.clone();
    if let Some(pkg) = nix_package {
        if !all_packages.contains(&pkg) {
            all_packages.push(pkg);
        }
    }
    let nix = if all_packages.is_empty() {
        None
    } else {
        Some(NixConfig {
            packages: all_packages,
            ..Default::default()
        })
    };

    Ok(ResourceConfig {
        command: Some(command),
        port,
        working_dir: resource.working_dir.as_ref().map(PathBuf::from),
        environment,
        depends_on,
        health,
        nix,
        ..Default::default()
    })
}

/// Normalize all service names and dependency references in an imported config.
/// Shows rename warnings for any names that changed.
fn normalize_imported_config(config: &mut ImportedConfig, out: &Output) {
    // Build old→new name mapping
    let renames: HashMap<String, String> = config
        .resources
        .iter()
        .filter_map(|r| {
            let normalized = ServiceName::normalize(&r.name);
            if normalized != r.name {
                Some((r.name.clone(), normalized))
            } else {
                None
            }
        })
        .collect();

    // Show rename warnings
    for (old, new) in &renames {
        out.step("rename")
            .done_untimed(&format!("'{}' → '{}' (normalized)", old, new));
    }

    // Apply normalization to resource names and depends_on references
    for resource in &mut config.resources {
        if let Some(new_name) = renames.get(&resource.name) {
            resource.name = new_name.clone();
        }
        for dep in &mut resource.depends_on {
            if let Some(new_name) = renames.get(dep.as_str()) {
                *dep = new_name.clone();
            }
        }
    }
}

/// Run the import command.
pub async fn run(ctx: &mut Context, out: &Output, args: ImportArgs) -> Result<()> {
    let ImportArgs {
        file,
        format,
        no_nix,
        dry_run,
    } = args;

    if !file.exists() {
        out.step("import")
            .fail_untimed(&format!("file not found: {}", file.display()));
        return Ok(());
    }

    let content = fs::read_to_string(&file)?;

    let resolved_format = match format {
        Some(f) => {
            let fmt = parse_format(&f)?;
            if fmt == ImportFormat::Auto {
                detect_format(&file, &content)?
            } else {
                fmt
            }
        }
        None => detect_format(&file, &content)?,
    };

    let importer = get_importer(resolved_format);
    let mut config = importer.import(&content)?;

    // Normalize all names in-place (underscores→hyphens, uppercase→lowercase)
    // so display, store ops, and dependency references all use canonical form.
    normalize_imported_config(&mut config, out);

    // === Pipeline: show all steps upfront ===
    let has_flake = !no_nix && ctx.store.project_root().join("flake.nix").exists();
    let labels: Vec<&str> = if has_flake {
        vec!["format", "nix", "import"]
    } else {
        vec!["format", "import"]
    };
    let mut pipe = out.pipeline(&labels);

    // Step 0: format
    pipe.done_untimed(
        0,
        &format!("{:?} (from {})", resolved_format, file.display()),
    );

    // Step 1: nix (only if flake exists)
    if has_flake {
        pipe.animate(1, "loading devShell");

        // Sanitize nix store paths
        if let Some(path) = get_devshell_path(ctx.store.project_root()).await {
            let count = dtx_core::translation::import::sanitize_nix_commands(&mut config, &path);
            let mut nix_notes = Vec::new();
            if count > 0 {
                nix_notes.push(format!("{} path(s) sanitized", count));
            }

            // Export custom nix scripts as packages
            let project_root = ctx.store.project_root();
            let (export_count, names) =
                dtx_core::translation::import::export_custom_scripts(&mut config, project_root);
            if export_count > 0 {
                nix_notes.push(format!(
                    "{} script(s) exported: {}",
                    export_count,
                    names.join(", ")
                ));
            }

            if nix_notes.is_empty() {
                pipe.done(1, "no store paths");
            } else {
                pipe.done(1, &nix_notes.join(", "));
            }
        } else {
            pipe.done(1, "no devShell");
        }
    }

    let import_step = if has_flake { 2 } else { 1 };

    if dry_run {
        pipe.done_untimed(import_step, "dry run");
        pipe.finish();
        display_import_summary(out, &config);
        return Ok(());
    }

    pipe.animate(import_step, "creating");

    let mut created_count = 0;
    let mut skipped_count = 0;
    let mut details: Vec<(String, String, bool)> = Vec::new(); // (name, note, failed)

    for resource in &config.resources {
        // Check if service already exists
        if ctx.store.get_resource(&resource.name).is_some() {
            skipped_count += 1;
            continue;
        }

        // Infer Nix package
        let nix_package = if no_nix {
            None
        } else {
            infer_nix_package(resource)
        };

        match resource_config_from_imported(resource, nix_package.clone()) {
            Ok(rc) => {
                ctx.store
                    .add_resource(&resource.name, rc)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                // Sync flake.nix if service has a package
                let flake_note = if let Some(ref pkg) = nix_package {
                    let project_root = ctx.store.project_root();
                    let project_name = ctx.store.project_name();
                    match sync_add_package(project_root, project_name, pkg) {
                        Ok(true) => format!("added (flake.nix updated with '{}')", pkg),
                        Ok(false) => format!("added (package: {})", pkg),
                        Err(e) => {
                            tracing::debug!("Failed to sync flake.nix: {}", e);
                            format!("added (package: {}, flake sync skipped)", pkg)
                        }
                    }
                } else {
                    "added".to_string()
                };

                details.push((resource.name.clone(), flake_note, false));
                created_count += 1;
            }
            Err(e) => {
                details.push((resource.name.clone(), format!("{}", e), true));
            }
        }
    }

    // Save all at once
    ctx.store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

    pipe.done(
        import_step,
        &format!("{} created, {} skipped", created_count, skipped_count),
    );
    pipe.finish();

    // Show per-resource details below the pipeline
    for (name, note, failed) in &details {
        if *failed {
            out.step_child(name).fail_untimed(note);
        } else {
            out.step_child(name).done_untimed(note);
        }
    }

    // Notify web/TUI of config change (fire-and-forget, sync)
    dtx_core::notify_config_changed_sync();

    Ok(())
}

async fn get_devshell_path(project_root: &std::path::Path) -> Option<String> {
    use dtx_core::nix::DevEnvironment;

    if !project_root.join("flake.nix").exists() {
        return None;
    }

    match DevEnvironment::from_flake_auto(project_root).await {
        Ok(env) => env.path().map(|p| p.to_string()),
        Err(e) => {
            tracing::debug!(
                "devShell eval failed, skipping nix path sanitization: {}",
                e
            );
            None
        }
    }
}
