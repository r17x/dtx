//! Start services.

use crate::context::Context;
use crate::output::Output;
use anyhow::Result;
use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::model::Service as ModelService;
use dtx_core::nix::DevEnvironment;
use dtx_core::process::{analyze_services, run_preflight_with_path};
use dtx_core::resource::LogStreamKind;
use dtx_core::{resolve_port_conflicts, FlakeGenerator, GraphValidator};
use dtx_process::{ProcessResourceConfig, ResourceOrchestrator};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Run the start command.
pub async fn run(
    ctx: &Context,
    out: &Output,
    service: Option<String>,
    foreground: bool,
) -> Result<()> {
    // Get services from config store
    let services: Vec<ModelService> = if let Some(ref name) = service {
        match ctx.store.get_resource(name) {
            Some(rc) => vec![ModelService::from_resource_config(name, rc)],
            None => {
                out.step(name).fail_untimed("not found");
                return Ok(());
            }
        }
    } else {
        ctx.store
            .list_enabled_resources()
            .map(|(name, rc)| ModelService::from_resource_config(name, rc))
            .collect()
    };

    if services.is_empty() {
        out.warning("No enabled services to start.");
        return Ok(());
    }

    // === Validate dependency graph before generation ===
    if let Err(validation_errors) = GraphValidator::validate_all(&services) {
        for err in &validation_errors {
            out.error_detail(&err.to_string(), &[], None);
        }
        out.step("graph")
            .fail_untimed(&format!("{} error(s)", validation_errors.len()));
        return Ok(());
    }

    let project_root = ctx.store.project_root().to_path_buf();
    let project_name = ctx.store.project_name().to_string();
    let dtx_dir = project_root.join(".dtx");

    // Resolve port conflicts before starting
    let (services, reassignments) = resolve_port_conflicts(&services);
    if !reassignments.is_empty() {
        for r in &reassignments {
            out.warning(&format!(
                "Port conflict resolved: {} port {} -> {} (original was in use)",
                r.service_name, r.original_port, r.new_port
            ));
        }
    }

    // === Pipeline: show all bootstrap steps upfront ===
    let mut pipe = out.pipeline(&["flake", "nix", "pre-flight", "services"]);

    // Step 0: flake
    let flake_path = project_root.join("flake.nix");
    let dtx_flake_path = dtx_dir.join("flake.nix");

    let mut deferred_warnings: Vec<String> = Vec::new();

    let effective_flake_dir = if flake_path.exists() {
        let flake_content = std::fs::read_to_string(&flake_path).unwrap_or_default();
        if flake_content.contains("managed by dtx") {
            let new_flake_content = FlakeGenerator::generate(&services, &project_name);
            std::fs::write(&flake_path, &new_flake_content)?;
            pipe.done(0, "regenerated");
            project_root.clone()
        } else {
            pipe.done_untimed(0, "using existing (user-managed)");
            project_root.clone()
        }
    } else {
        let flake_content = FlakeGenerator::generate(&services, &project_name);
        std::fs::write(&dtx_flake_path, &flake_content)?;
        pipe.done(0, "generated");
        dtx_dir.clone()
    };

    // Step 1: nix environment
    let mut nix_env: Option<HashMap<String, String>> =
        if effective_flake_dir.join("flake.nix").exists() {
            pipe.animate(1, "loading");
            match DevEnvironment::from_flake(&effective_flake_dir).await {
                Ok(env) => {
                    pipe.done(1, &format!("{} vars", env.var_count()));
                    Some(env.env_vars)
                }
                Err(e) => {
                    tracing::warn!("Failed to extract Nix environment: {}", e);
                    pipe.fail(1, &format!("{}", e));
                    deferred_warnings.push("Services will use system PATH instead.".into());
                    None
                }
            }
        } else {
            pipe.done_untimed(1, "skipped");
            None
        };

    // Step 1b: build custom flake packages not on devShell PATH
    if let Some(ref mut env) = nix_env {
        let extra = build_flake_packages(&services, &effective_flake_dir, env).await;
        if !extra.is_empty() {
            let names: Vec<&str> = extra.iter().map(|(n, _)| n.as_str()).collect();
            tracing::info!(
                "built {} flake package(s): {}",
                extra.len(),
                names.join(", ")
            );
            let extra_bin: Vec<String> = extra
                .into_iter()
                .map(|(_, p)| p.join("bin").to_string_lossy().to_string())
                .collect();
            let extra_path = extra_bin.join(":");
            if let Some(path) = env.get_mut("PATH") {
                *path = format!("{}:{}", extra_path, path);
            } else {
                env.insert("PATH".to_string(), extra_path);
            }
        }
    }

    // Step 2: pre-flight checks
    let nix_path = nix_env.as_ref().and_then(|env| env.get("PATH").cloned());
    pipe.animate(2, "checking");
    let checks = analyze_services(&services);
    let preflight_result = run_preflight_with_path(checks, nix_path.as_deref()).await;

    if !preflight_result.is_ok() {
        let total = preflight_result.passed.len() + preflight_result.failed.len();
        pipe.fail(
            2,
            &format!("{}/{} failed", preflight_result.failed.len(), total),
        );
        pipe.finish();
        for check in &preflight_result.failed {
            let mut details: Vec<(&str, &str)> = Vec::new();
            let required_by = check.required_by.join(", ");
            if !check.required_by.is_empty() {
                details.push(("required by", &required_by));
            }
            out.error_detail(&check.description, &details, check.fix_hint.as_deref());
        }
        return Ok(());
    }

    let total = preflight_result.passed.len();
    pipe.done(2, &format!("{}/{}", total, total));

    // Collect unmapped service warnings (deferred until after pipeline)
    collect_unmapped_warnings(&services, &mut deferred_warnings);

    run_with_native_backend(
        out,
        &services,
        NativeBackendParams {
            project_root,
            flake_dir: effective_flake_dir,
            nix_env,
            foreground,
            pipe,
            deferred_warnings,
        },
    )
    .await
}

/// Parameters for the native backend runner.
struct NativeBackendParams {
    project_root: PathBuf,
    flake_dir: PathBuf,
    nix_env: Option<HashMap<String, String>>,
    foreground: bool,
    pipe: crate::output::Pipeline,
    deferred_warnings: Vec<String>,
}

/// Run services using the Orchestrator (unified process management).
async fn run_with_native_backend(
    out: &Output,
    services: &[ModelService],
    params: NativeBackendParams,
) -> Result<()> {
    if params.foreground {
        run_foreground(out, services, params).await
    } else {
        let NativeBackendParams {
            project_root,
            flake_dir,
            nix_env,
            mut pipe,
            deferred_warnings,
            ..
        } = params;
        pipe.done_untimed(3, &format!("{} ready", services.len()));
        pipe.finish();
        let result = crate::tui::run_tui(
            out,
            services.to_vec(),
            project_root,
            Some(flake_dir),
            nix_env,
        )
        .await;
        for w in &deferred_warnings {
            out.warning(w);
        }
        result
    }
}

/// Run ResourceOrchestrator in foreground mode (no TUI, logs to stdout).
async fn run_foreground(
    out: &Output,
    services: &[ModelService],
    params: NativeBackendParams,
) -> Result<()> {
    let NativeBackendParams {
        project_root,
        nix_env,
        mut pipe,
        deferred_warnings,
        ..
    } = params;
    let event_bus = Arc::new(ResourceEventBus::new());
    let mut subscriber = event_bus.subscribe();

    let mut orchestrator = ResourceOrchestrator::new(event_bus);

    for svc in services {
        let mut config = service_to_process_config(svc, &project_root);
        if let Some(ref env) = nix_env {
            let user_env = std::mem::take(&mut config.environment);
            config.environment = env.clone();
            config.environment.extend(user_env);
        }
        orchestrator.add_resource(config);
    }

    pipe.animate(3, "starting");

    let result = orchestrator
        .start_all()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    let started = result.started.len();
    let failed_count = result.failed.len();
    if failed_count > 0 {
        pipe.fail(
            3,
            &format!("{}/{} failed", failed_count, started + failed_count),
        );
    } else {
        pipe.done(3, &format!("{} started", started));
    }
    pipe.finish();

    // Print deferred warnings now that pipeline is done
    for w in &deferred_warnings {
        out.warning(w);
    }

    // Print service details below the pipeline
    for id in &result.started {
        out.step_child(id.as_str()).done_untimed("started");
    }
    for (id, err) in &result.failed {
        out.step_child(id.as_str()).fail_untimed(&err.to_string());
    }

    out.separator("logs (ctrl+c to stop)");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                break;
            }
            event = subscriber.recv() => {
                if let Some(event) = event {
                    print_lifecycle_event(out, &event);
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                orchestrator.poll().await;
            }
        }
    }

    out.blank();
    let mut stop_step = out.step("stop");
    stop_step.animate("shutting down");
    orchestrator
        .stop_all()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    stop_step.done("all services stopped");

    Ok(())
}

/// Print a lifecycle event using the Output API.
fn print_lifecycle_event(out: &Output, event: &LifecycleEvent) {
    match event {
        LifecycleEvent::Log {
            id, stream, line, ..
        } => {
            let is_stderr = matches!(stream, LogStreamKind::Stderr);
            out.log(id.as_str(), line, is_stderr);
        }
        LifecycleEvent::Starting { .. }
        | LifecycleEvent::Running { .. }
        | LifecycleEvent::Stopped { .. }
        | LifecycleEvent::Failed { .. } => {}
        _ => {}
    }
}

/// Convert a ModelService to ProcessResourceConfig.
fn service_to_process_config(service: &ModelService, project_root: &Path) -> ProcessResourceConfig {
    let mut config = ProcessResourceConfig::new(&service.name, &service.command);

    if let Some(ref wd) = service.working_dir {
        config = config.with_working_dir(wd.clone());
    } else {
        config = config.with_working_dir(project_root.to_path_buf());
    }

    if let Some(ref env) = service.environment {
        config = config.with_environment(env.clone());
    }

    if let Some(port) = service.port {
        config = config.with_port(port);
    }

    if let Some(ref deps) = service.depends_on {
        for dep in deps {
            config = config.depends_on(dep.service.clone());
        }
    }

    config
}

/// Build flake packages whose binaries aren't on the current PATH.
/// Returns (package_name, output_path) for each successfully built package.
async fn build_flake_packages(
    services: &[ModelService],
    flake_dir: &Path,
    env: &HashMap<String, String>,
) -> Vec<(String, PathBuf)> {
    use dtx_core::nix::extract_executable;

    let path_env = env.get("PATH").map(|s| s.as_str()).unwrap_or("");

    // Collect packages whose command binary isn't on PATH
    let mut to_build: Vec<String> = Vec::new();
    for svc in services {
        let Some(ref pkg) = svc.package else {
            continue;
        };
        let Some(target) = extract_executable(&svc.command) else {
            continue;
        };
        // Skip absolute/relative paths — only bare basenames need building
        if target.contains('/') {
            continue;
        }
        // Check if already on PATH
        let found = dtx_core::nix::find_on_path(&target, path_env);
        if !found && !to_build.contains(pkg) {
            to_build.push(pkg.clone());
        }
    }

    // Build all packages concurrently
    let mut set = tokio::task::JoinSet::new();
    let flake_dir = flake_dir.to_path_buf();
    for pkg in to_build {
        let dir = flake_dir.clone();
        set.spawn(async move {
            let result = build_single_flake_package(&dir, &pkg).await;
            (pkg, result)
        });
    }

    let mut results = Vec::new();
    while let Some(Ok((pkg, outcome))) = set.join_next().await {
        match outcome {
            Ok(path) => results.push((pkg, path)),
            Err(e) => tracing::debug!("nix build .#{} skipped: {}", pkg, e),
        }
    }
    results
}

/// Run `nix build .#<package> --print-out-paths --no-link` and return the output path.
async fn build_single_flake_package(flake_dir: &Path, package: &str) -> Result<PathBuf> {
    let output = tokio::process::Command::new("nix")
        .args([
            "build",
            &format!(".#{}", package),
            "--print-out-paths",
            "--no-link",
        ])
        .current_dir(flake_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", stderr.trim());
    }

    let path = String::from_utf8(output.stdout)?.trim().to_string();
    Ok(PathBuf::from(path))
}

/// Collects warnings about services that have no package and can't be inferred.
fn collect_unmapped_warnings(services: &[ModelService], warnings: &mut Vec<String>) {
    let unmapped: Vec<_> = services
        .iter()
        .filter(|s| s.enabled)
        .filter(|s| s.package.is_none() && dtx_core::infer_package(&s.command).is_none())
        .collect();

    if !unmapped.is_empty() {
        let names: Vec<_> = unmapped
            .iter()
            .map(|svc| format!("{} (command: {})", svc.name, svc.command))
            .collect();
        warnings.push(format!(
            "No mapped package for: {}. Use 'dtx add <service> --package <pkg>' to map.",
            names.join(", ")
        ));
    }
}
