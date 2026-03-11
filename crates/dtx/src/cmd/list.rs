//! List projects or services.

use crate::context::Context;
use crate::output::{Cell, Output};
use anyhow::Result;

/// Run the list command.
pub fn run(ctx: &Context, out: &Output, services: bool) -> Result<()> {
    if services {
        list_services(ctx, out)
    } else {
        list_project(ctx, out)
    }
}

fn list_project(ctx: &Context, out: &Output) -> Result<()> {
    let table = out
        .table()
        .headers(vec!["NAME", "PATH"])
        .row(vec![
            Cell::new(ctx.store.project_name()),
            Cell::new(ctx.store.project_root().to_string_lossy()),
        ]);

    out.print_table(table);

    Ok(())
}

fn list_services(ctx: &Context, out: &Output) -> Result<()> {
    let resources: Vec<_> = ctx.store.list_resources().collect();

    if resources.is_empty() {
        out.step(ctx.store.project_name()).done_untimed("no services");
        return Ok(());
    }

    out.step("project")
        .done_untimed(&format!("{} (services)", ctx.store.project_name()));

    let mut table = out
        .table()
        .headers(vec!["SERVICE", "COMMAND", "PORT", "STATUS"]);

    for (name, rc) in resources {
        let status = if rc.enabled { "enabled" } else { "disabled" };
        let port = rc
            .port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let command = rc.command.as_deref().unwrap_or("-").to_string();

        table = table.row(vec![
            Cell::new(name),
            Cell::new(command),
            Cell::new(port),
            Cell::new(status),
        ]);
    }

    out.print_table(table);

    Ok(())
}
