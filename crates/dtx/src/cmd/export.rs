//! Export configuration to various formats.

use crate::context::Context;
use crate::output::Output;
use anyhow::{bail, Result};
use dtx_core::model::Service as ModelService;
use dtx_core::{
    DockerComposeExporter, ExportFormat, ExportableProject, ExportableService, Exporter,
    KubernetesExporter, TranslationContext, YamlGenerator,
};
use dtx_process::{default_registry, ProcessResourceConfig};
use std::fs;
use std::io::{self, Write};

/// Export options.
pub struct ExportOptions {
    pub output: Option<String>,
    #[allow(dead_code)]
    pub format: ExportFormat,
    pub namespace: Option<String>,
    pub default_image: Option<String>,
    pub services_filter: Option<Vec<String>>,
}

/// Run the export command.
pub fn run(
    ctx: &Context,
    out: &Output,
    output: Option<String>,
    format: &str,
    namespace: Option<String>,
    default_image: Option<String>,
    services: Option<String>,
) -> Result<()> {
    let format: ExportFormat = format.parse().map_err(|_| {
        anyhow::anyhow!(
            "Invalid format '{}'. Valid formats: docker-compose, kubernetes, process-compose, dtx",
            format
        )
    })?;

    let services_filter = services.map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

    let opts = ExportOptions {
        output,
        format,
        namespace,
        default_image,
        services_filter,
    };

    match format {
        ExportFormat::ProcessCompose | ExportFormat::Dtx => export_process_compose(ctx, out, &opts),
        ExportFormat::DockerCompose => export_docker_compose(ctx, out, &opts),
        ExportFormat::Kubernetes => export_kubernetes(ctx, out, &opts),
    }
}

/// Export to process-compose format (original behavior).
fn export_process_compose(ctx: &Context, out: &Output, opts: &ExportOptions) -> Result<()> {
    let services = get_model_services(ctx, &opts.services_filter)?;

    let generator = YamlGenerator::new();
    let yaml = generator.generate(services)?;

    write_output(out, &yaml, &opts.output)
}

/// Export to docker-compose format.
fn export_docker_compose(ctx: &Context, out: &Output, opts: &ExportOptions) -> Result<()> {
    let project = get_exportable_project(ctx, opts)?;

    let exporter = DockerComposeExporter::new();
    let yaml = exporter.export(&project)?;

    write_output(out, &yaml, &opts.output)
}

/// Export to kubernetes format.
fn export_kubernetes(ctx: &Context, out: &Output, opts: &ExportOptions) -> Result<()> {
    let project = get_exportable_project(ctx, opts)?;

    let mut exporter = KubernetesExporter::new();
    if let Some(ref ns) = opts.namespace {
        exporter = exporter.with_namespace(ns);
    }

    let yaml = exporter.export(&project)?;

    write_output(out, &yaml, &opts.output)
}

/// Get model services from config store, with optional filter.
fn get_model_services(ctx: &Context, filter: &Option<Vec<String>>) -> Result<Vec<ModelService>> {
    let services: Vec<ModelService> = ctx
        .store
        .list_resources()
        .filter(|(name, _)| match filter {
            Some(names) => names.iter().any(|n| n == *name),
            None => true,
        })
        .map(|(name, rc)| ModelService::from_resource_config(name, rc))
        .collect();

    if services.is_empty() {
        bail!("no services to export");
    }

    Ok(services)
}

/// Get exportable project from config store.
fn get_exportable_project(ctx: &Context, opts: &ExportOptions) -> Result<ExportableProject> {
    let services = get_model_services(ctx, &opts.services_filter)?;

    // Build translation context
    let mut translation_ctx = TranslationContext::new();
    if let Some(ref image) = opts.default_image {
        translation_ctx = translation_ctx.default_value("image", image.clone());
    }

    let registry = default_registry();

    let mut exportable_services = Vec::new();

    for svc in &services {
        let mut process = ProcessResourceConfig::new(&svc.name, &svc.command);

        if let Some(port) = svc.port {
            process = process.with_port(port);
        }

        if let Some(ref wd) = svc.working_dir {
            process = process.with_working_dir(wd);
        }

        if let Some(ref env) = svc.environment {
            for (key, value) in env {
                process = process.with_env(key, value);
            }
        }

        match registry.translate_with_context(&process, &translation_ctx) {
            Ok(container) => {
                let mut export_svc = ExportableService::from_container(container);
                export_svc.enabled = svc.enabled;
                exportable_services.push(export_svc);
            }
            Err(e) => {
                if opts.default_image.is_some() {
                    bail!(
                        "Failed to translate service '{}': {}. Consider using --default-image",
                        svc.name,
                        e
                    );
                } else {
                    bail!(
                        "Failed to translate service '{}': {}. Use --default-image to specify a fallback image.",
                        svc.name, e
                    );
                }
            }
        }
    }

    let mut exportable_project = ExportableProject::new(ctx.store.project_name());
    if let Some(desc) = ctx.store.project_description() {
        exportable_project = exportable_project.with_description(desc);
    }
    exportable_project = exportable_project.with_services(exportable_services);

    Ok(exportable_project)
}

/// Write output to file or stdout.
fn write_output(out: &Output, content: &str, output: &Option<String>) -> Result<()> {
    if let Some(ref path) = output {
        fs::write(path, content)?;
        out.step("export").done_untimed(path);
    } else {
        io::stdout().write_all(content.as_bytes())?;
    }
    Ok(())
}
