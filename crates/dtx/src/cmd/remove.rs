//! Remove a service from the project.

use crate::context::Context;
use crate::output::Output;
use anyhow::Result;
use std::collections::HashSet;
use std::io::{self, Write};

/// Run the remove command.
pub fn run(ctx: &mut Context, out: &Output, name: String, yes: bool) -> Result<()> {
    // Check service exists
    if ctx.store.get_resource(&name).is_none() {
        out.step(&name).fail_untimed("not found");
        return Ok(());
    }

    // Confirm deletion
    if !yes {
        print!("Remove service '{}'? [y/N] ", name);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            out.step(&name).done_untimed("cancelled");
            return Ok(());
        }
    }

    // Capture package before deletion for flake sync
    let removed_package = ctx
        .store
        .get_resource(&name)
        .and_then(|r| r.nix.as_ref())
        .and_then(|n| n.packages.first().cloned());

    // Remove the resource
    ctx.store
        .remove_resource(&name)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    ctx.store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

    out.step(&name).done_untimed("removed");

    // Sync flake.nix if the removed service had a package
    if let Some(ref pkg) = removed_package {
        let remaining_packages: HashSet<String> = ctx
            .store
            .list_resources()
            .filter_map(|(_, r)| {
                r.nix
                    .as_ref()
                    .and_then(|n| n.packages.first().cloned())
            })
            .collect();
        let project_root = ctx.store.project_root();
        match dtx_core::sync_remove_package(project_root, pkg, &remaining_packages) {
            Ok(true) => out.step("flake").done_untimed(&format!("removed {}", pkg)),
            Ok(false) => {}
            Err(e) => out.step("flake").fail_untimed(&format!("{}", e)),
        }
    }

    // Notify web/TUI of config change (fire-and-forget, sync)
    dtx_core::notify_config_changed_sync();

    Ok(())
}
