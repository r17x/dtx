//! Edit an existing service.

use super::parsers;
use crate::context::Context;
use crate::output::Output;
use anyhow::{bail, Result};
use dtx_core::config::schema::{
    DependencyConditionConfig, DependencyConfig, RestartConfig,
};
use dtx_core::{Environment, Port, ShellCommand};
use indexmap::IndexMap;

/// Arguments for the edit command.
pub struct EditArgs {
    pub name: String,
    pub command: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
    pub add_env: Vec<String>,
    pub remove_env: Vec<String>,
    pub add_dep: Vec<String>,
    pub remove_dep: Vec<String>,
    pub restart: Option<String>,
    pub health_check: Option<String>,
    pub clear_health_check: bool,
    pub enable: bool,
    pub disable: bool,
}

/// Run the edit command.
pub fn run(ctx: &mut Context, out: &Output, args: EditArgs) -> Result<()> {
    let EditArgs {
        name,
        command,
        port,
        working_dir,
        add_env,
        remove_env,
        add_dep,
        remove_dep,
        restart,
        health_check,
        clear_health_check,
        enable,
        disable,
    } = args;

    // Validate conflicting flags
    if enable && disable {
        out.step(&name).fail_untimed("cannot use both --enable and --disable");
        return Ok(());
    }

    // Get mutable resource
    let resource = match ctx.store.get_resource_mut(&name) {
        Some(r) => r,
        None => {
            out.step(&name).fail_untimed("not found");
            return Ok(());
        }
    };

    // Track what changed for output
    let command_changed = command.is_some();
    let port_changed = port.is_some();
    let working_dir_changed = working_dir.is_some();
    let health_check_changed = health_check.is_some() || clear_health_check;

    // Update command
    if let Some(cmd) = command {
        let _: ShellCommand = cmd
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid command: {}", e))?;
        resource.command = Some(cmd);
    }

    // Update port
    if let Some(p) = port {
        let _: Port = p
            .try_into()
            .map_err(|e| anyhow::anyhow!("Invalid port: {}", e))?;
        resource.port = Some(p);
    }

    // Update working dir
    if let Some(wd) = working_dir {
        resource.working_dir = Some(std::path::PathBuf::from(wd));
    }

    // Handle environment variable updates
    for key in &remove_env {
        resource.environment.shift_remove(key);
    }
    if !add_env.is_empty() {
        let new_env = Environment::from_strings(&add_env)
            .map_err(|e| anyhow::anyhow!("Invalid environment variable: {}", e))?;
        for (k, v) in new_env.into_map() {
            resource.environment.insert(k, v);
        }
    }

    // Handle dependency updates
    for dep_name in &remove_dep {
        resource.depends_on.retain(|d| d.name() != *dep_name);
    }
    for dep_str in &add_dep {
        let new_dep = parse_dependency_config(dep_str)?;
        let new_name = new_dep.name().to_string();
        resource.depends_on.retain(|d| d.name() != new_name);
        resource.depends_on.push(new_dep);
    }

    // Handle health check
    if clear_health_check {
        resource.health = None;
    } else if let Some(hc_str) = health_check {
        let hc = parsers::parse_health_check(&hc_str)?;
        resource.health = Some(super::add::health_check_to_config(&hc));
    }

    // Handle enabled state
    if enable {
        resource.enabled = true;
    } else if disable {
        resource.enabled = false;
    }

    // Handle restart policy
    let restart_policy = restart
        .as_deref()
        .map(|s| parsers::parse_restart_policy(s))
        .transpose()?;
    if let Some(policy) = restart_policy.clone() {
        resource.restart = Some(RestartConfig::Simple(policy));
    }

    // Save
    ctx.store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Build group showing what changed
    let mut grp = out.group(&name);

    if command_changed {
        if let Some(ref cmd) = ctx.store.get_resource(&name).and_then(|r| r.command.as_ref()) {
            grp.child_done("command", cmd);
        }
    }
    if port_changed {
        if let Some(p) = ctx.store.get_resource(&name).and_then(|r| r.port) {
            grp.child_done("port", &format!("{}", p));
        }
    }
    if working_dir_changed {
        if let Some(ref wd) = ctx.store.get_resource(&name).and_then(|r| r.working_dir.as_ref()) {
            grp.child_done("working directory", &format!("{}", wd.display()));
        }
    }
    if !add_env.is_empty() || !remove_env.is_empty() {
        grp.child_done("environment", "updated");
    }
    if !add_dep.is_empty() || !remove_dep.is_empty() {
        if let Some(r) = ctx.store.get_resource(&name) {
            if r.depends_on.is_empty() {
                grp.child_done("dependencies", "none");
            } else {
                let dep_names: Vec<&str> = r.depends_on.iter().map(|d| d.name()).collect();
                grp.child_done("dependencies", &dep_names.join(", "));
            }
        }
    }
    if health_check_changed || clear_health_check {
        if let Some(r) = ctx.store.get_resource(&name) {
            if r.health.is_some() {
                grp.child_done("health check", "configured");
            } else {
                grp.child_done("health check", "cleared");
            }
        }
    }
    if enable {
        grp.child_done("status", "enabled");
    } else if disable {
        grp.child_done("status", "disabled");
    }
    if let Some(ref policy) = restart_policy {
        grp.child_done("restart", &format!("{:?}", policy));
    }

    grp.done_with_summary("updated");

    // Notify web/TUI of config change (fire-and-forget, sync)
    dtx_core::notify_config_changed_sync();

    Ok(())
}

/// Parse a dependency string to DependencyConfig.
fn parse_dependency_config(s: &str) -> Result<DependencyConfig> {
    use dtx_core::ServiceName;

    if let Some((service, condition_str)) = s.split_once(':') {
        service
            .parse::<ServiceName>()
            .map_err(|e| anyhow::anyhow!("Invalid dependency service name '{}': {}", service, e))?;

        let condition = match condition_str.to_lowercase().as_str() {
            "started" => DependencyConditionConfig::Started,
            "healthy" => DependencyConditionConfig::Healthy,
            "completed" => DependencyConditionConfig::Completed,
            _ => bail!(
                "Invalid dependency condition '{}'. Use: started, healthy, or completed",
                condition_str
            ),
        };

        let mut map = IndexMap::new();
        map.insert(service.to_string(), condition);
        Ok(DependencyConfig::WithCondition(map))
    } else {
        s.parse::<ServiceName>()
            .map_err(|e| anyhow::anyhow!("Invalid dependency service name '{}': {}", s, e))?;

        Ok(DependencyConfig::Simple(s.to_string()))
    }
}
