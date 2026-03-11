//! Search for Nix packages.

use crate::output::{Cell, Output};
use anyhow::Result;
use dtx_core::NixClient;
use std::env;

/// Run the search command.
pub async fn run(out: &Output, query: String, limit: usize) -> Result<()> {
    let mut step = out.step("search");
    step.animate(&format!("searching '{}'", query));

    // Use current directory as project path for flake-aware search
    let client = NixClient::new().with_project_path(env::current_dir()?);
    let mut packages = client.search(&query).await?;

    if packages.is_empty() {
        step.fail_untimed(&format!("no results for '{}'", query));
        out.raw("Try a different search term or check the package name.\n");
        return Ok(());
    }

    // Sort by relevance (exact pname match first, then contains, then alphabetical)
    let query_lower = query.to_lowercase();
    packages.sort_by(|a, b| {
        let a_pname = a.pname.to_lowercase();
        let b_pname = b.pname.to_lowercase();

        let a_exact = a_pname == query_lower;
        let b_exact = b_pname == query_lower;

        if a_exact != b_exact {
            return if a_exact {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }

        let a_starts = a_pname.starts_with(&query_lower);
        let b_starts = b_pname.starts_with(&query_lower);

        if a_starts != b_starts {
            return if a_starts {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }

        a_pname.cmp(&b_pname)
    });

    let total = packages.len();
    let display_packages: Vec<_> = packages.iter().take(limit).collect();

    step.done(&format!("{} packages found", total));

    let mut table = out
        .table()
        .headers(vec!["PACKAGE", "VERSION", "DESCRIPTION"]);

    for pkg in &display_packages {
        table = table.row(vec![
            Cell::new(&pkg.pname),
            Cell::new(&pkg.version),
            Cell::new(&pkg.description),
        ]);
    }

    out.print_table(table);

    if total > limit {
        out.blank();
        out.raw(&format!(
            "Showing {} of {} results. Use --limit to see more.\n",
            limit, total
        ));
    }

    out.blank();
    out.raw("To add a service: dtx add <name> --command <cmd>\n");

    Ok(())
}
