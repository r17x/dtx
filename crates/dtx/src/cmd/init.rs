//! Initialize a new dtx project.

use crate::output::Output;
use anyhow::{Context as _, Result};
use dtx_core::config::schema::ResourceConfig;
use dtx_core::store::ConfigStore;
use dtx_core::translation::CodebaseInferrer;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

/// Run the init command.
pub async fn run(
    out: &Output,
    name: Option<String>,
    path: Option<String>,
    description: Option<String>,
    detect: bool,
    yes: bool,
) -> Result<()> {
    // Determine project path
    let project_path = if let Some(p) = &path {
        let p = PathBuf::from(p);
        if p.is_absolute() {
            p
        } else {
            env::current_dir()?.join(p)
        }
    } else {
        env::current_dir()?
    };

    // Require a name unless --detect is used
    if name.is_none() && !detect {
        out.step("init")
            .fail_untimed("project name required (or use --detect)");
        return Ok(());
    }

    // Run codebase detection if requested
    let inference = if detect {
        let inferrer = CodebaseInferrer::new();
        let path_to_scan = if project_path.exists() {
            &project_path
        } else {
            &env::current_dir()?
        };

        match inferrer.infer(path_to_scan) {
            Ok(result) => {
                if !result.is_empty() {
                    out.step("detect")
                        .done_untimed(&format!("{}", result.project_type));

                    if !result.detected_packages.is_empty() {
                        let mut grp = out.group("packages");
                        for pkg in &result.detected_packages {
                            grp.child_done(
                                &pkg.name,
                                &format!(
                                    "{} ({:?}, from {})",
                                    pkg.nixpkg, pkg.confidence, pkg.source
                                ),
                            );
                        }
                        grp.done();
                    }

                    if !result.suggested_services.is_empty() {
                        let mut grp = out.group("services");
                        for svc in &result.suggested_services {
                            let port_str = svc
                                .port
                                .map(|p| format!(" (port {})", p))
                                .unwrap_or_default();
                            grp.child_done(
                                &svc.name,
                                &format!("`{}`{} — {}", svc.command, port_str, svc.description),
                            );
                        }
                        grp.done();
                    }

                    if !yes {
                        print!("Continue with these settings? [Y/n] ");
                        io::stdout().flush()?;
                        let mut input = String::new();
                        io::stdin().read_line(&mut input)?;
                        let input = input.trim().to_lowercase();
                        if input == "n" || input == "no" {
                            out.step("init").done_untimed("cancelled");
                            return Ok(());
                        }
                    }
                }
                Some(result)
            }
            Err(e) => {
                tracing::warn!("Codebase detection failed: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Determine project name
    let project_name = if let Some(n) = name {
        n
    } else if detect {
        project_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "project".to_string())
    } else {
        out.step("init").fail_untimed("project name required");
        return Ok(());
    };

    let mut step = out.step("project");
    step.pending("initializing");

    // Create project directory
    std::fs::create_dir_all(&project_path)
        .with_context(|| format!("Failed to create directory: {}", project_path.display()))?;

    // Canonicalize to absolute path (after directory exists)
    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("Failed to resolve path: {}", project_path.display()))?;

    // Initialize ConfigStore (creates .dtx/config.yaml)
    let mut store = ConfigStore::init(project_path.clone(), &project_name)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if let Some(ref desc) = description {
        store.set_project_description(Some(desc.clone()));
    }

    // Add inferred services if detection was used
    if let Some(ref inf) = inference {
        let mut grp = out.group("add");
        for svc in &inf.suggested_services {
            let nixpkg = inf
                .high_confidence_packages()
                .first()
                .map(|p| p.nixpkg.clone());

            let nix = nixpkg.map(|pkg| dtx_core::config::schema::NixConfig {
                packages: vec![pkg],
                ..Default::default()
            });

            let rc = ResourceConfig {
                command: Some(svc.command.clone()),
                port: svc.port,
                nix,
                ..Default::default()
            };

            store
                .add_resource(&svc.name, rc)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            grp.child_done(&svc.name, "added");
        }
        if !inf.suggested_services.is_empty() {
            grp.done();
        }
    }

    store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

    step.done(&format!("{} at {}", project_name, project_path.display()));

    if let Some(ref inf) = inference {
        if !inf.detected_packages.is_empty() {
            out.blank();
            out.raw("Recommended Nix packages:\n");
            for pkg in inf.high_confidence_packages() {
                out.raw(&format!("  {}\n", pkg.nixpkg));
            }
            out.blank();
            out.raw("Run `dtx nix init` to generate flake.nix with these packages.\n");
        }
    }

    out.blank();
    out.raw("Next steps:\n");
    out.raw(&format!("  cd {}\n", project_path.display()));
    if inference.is_none()
        || inference
            .as_ref()
            .map(|i| i.suggested_services.is_empty())
            .unwrap_or(true)
    {
        out.raw("  dtx add <service> --command <cmd>\n");
    }
    out.raw("  dtx start\n");

    Ok(())
}
