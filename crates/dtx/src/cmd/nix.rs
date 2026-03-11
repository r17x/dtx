//! Nix environment management commands.

use crate::context::Context;
use crate::output::Output;
use anyhow::{Context as _, Result};
use dtx_core::model::Service as ModelService;
use dtx_core::{EnvrcGenerator, FlakeGenerator, NixShell};

/// Initialize Nix environment (generate flake.nix and .envrc).
pub async fn init(ctx: &Context, out: &Output) -> Result<()> {
    let services: Vec<ModelService> = ctx
        .store
        .list_resources()
        .map(|(name, rc)| ModelService::from_resource_config(name, rc))
        .collect();

    let project_name = ctx.store.project_name();
    let project_root = ctx.store.project_root();

    // Generate flake.nix
    let flake = FlakeGenerator::generate(&services, project_name);
    let flake_path = project_root.join("flake.nix");

    if flake_path.exists() {
        out.warning("flake.nix already exists");
        print!("Overwrite? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            out.step("nix init").done_untimed("cancelled");
            return Ok(());
        }
    }

    std::fs::write(&flake_path, &flake)
        .with_context(|| format!("Failed to write flake.nix to {}", flake_path.display()))?;
    out.step("flake").done_untimed("generated");

    // Generate .envrc
    let envrc = EnvrcGenerator::generate_with_layout(&services);
    let envrc_path = project_root.join(".envrc");

    std::fs::write(&envrc_path, &envrc)
        .with_context(|| format!("Failed to write .envrc to {}", envrc_path.display()))?;
    out.step("envrc").done_untimed("generated");

    out.blank();
    out.raw("Next steps:\n");
    out.raw(&format!("  cd {}\n", project_root.display()));
    out.raw("  direnv allow    # Enable environment (requires direnv)\n");
    out.raw("  nix develop     # Or manually enter nix shell\n");
    out.raw("  dtx start       # Start services\n");

    Ok(())
}

/// Regenerate .envrc only.
pub async fn envrc(ctx: &Context, out: &Output) -> Result<()> {
    let services: Vec<ModelService> = ctx
        .store
        .list_resources()
        .map(|(name, rc)| ModelService::from_resource_config(name, rc))
        .collect();

    let project_root = ctx.store.project_root();

    let envrc = EnvrcGenerator::generate_with_layout(&services);
    let envrc_path = project_root.join(".envrc");

    std::fs::write(&envrc_path, &envrc)
        .with_context(|| format!("Failed to write .envrc to {}", envrc_path.display()))?;

    out.step("envrc").done_untimed("generated");
    out.blank();
    out.raw("Run 'direnv allow' to enable the environment\n");

    Ok(())
}

/// Run a command in the Nix shell, or enter interactive shell.
pub async fn shell(ctx: &Context, out: &Output, command: Option<String>) -> Result<()> {
    let project_root = ctx.store.project_root();
    let project_name = ctx.store.project_name();

    let nix_shell = NixShell::new(project_root);

    if !nix_shell.has_flake() {
        out.warning("No flake.nix found, generating...");
        init(ctx, out).await?;
        out.blank();
    }

    match command {
        Some(cmd) => {
            out.step("shell").done_untimed(&cmd);
            let output = nix_shell.run(&cmd).await?;

            print!("{}", String::from_utf8_lossy(&output.stdout));
            eprint!("{}", String::from_utf8_lossy(&output.stderr));

            if !output.status.success() {
                std::process::exit(output.status.code().unwrap_or(1));
            }
        }
        None => {
            out.step("shell").done_untimed(&format!("interactive ({})", project_name));
            out.raw("(Use 'exit' to leave the shell)\n");
            out.blank();

            let status = std::process::Command::new("nix")
                .args(["develop"])
                .current_dir(project_root)
                .status()?;

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
    }

    Ok(())
}

/// List Nix packages used by services.
pub fn packages(ctx: &Context, out: &Output) -> Result<()> {
    let mut packages: Vec<(&str, String)> = ctx
        .store
        .list_resources()
        .filter(|(_, r)| r.enabled)
        .filter_map(|(name, r)| {
            r.nix
                .as_ref()
                .and_then(|n| n.packages.first().cloned())
                .map(|pkg| (name, pkg))
        })
        .collect();

    if packages.is_empty() {
        out.warning("No Nix packages configured");
        out.blank();
        out.raw("Add packages with: dtx add SERVICE --package PACKAGE\n");
        return Ok(());
    }

    packages.sort_by(|(_, a), (_, b)| a.cmp(b));

    out.step("packages")
        .done_untimed(&format!("{} packages", packages.len()));
    out.blank();

    for (service, package) in &packages {
        out.raw(&format!("  {} -> {}\n", package, service));
    }

    out.blank();
    out.raw("Additional packages:\n");
    out.raw("  process-compose -> (always included)\n");
    out.blank();
    out.raw("Generate with: dtx nix init\n");

    Ok(())
}
