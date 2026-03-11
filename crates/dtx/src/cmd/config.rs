//! Configuration management command.

use crate::output::Output;
use anyhow::Result;
use dtx_core::config::{ConfigLevel, ConfigLoader, DtxConfig};

/// Run the config command.
pub async fn run(
    out: &Output,
    global: bool,
    project: bool,
    key: Option<String>,
    value: Option<String>,
) -> Result<()> {
    let mut loader = ConfigLoader::new();

    // For project level, ensure we have a path even if .dtx/ doesn't exist yet
    let cwd = std::env::current_dir()?;
    let project_config_path = cwd.join(".dtx/config.yaml");

    // Try to discover existing project, otherwise use cwd
    if loader.discover_project_cwd().is_none() && (project || value.is_some()) {
        loader = loader.with_project_path(&project_config_path);
    }

    // Handle the different cases
    match (&key, &value, global, project) {
        // No key, no value, no level flags -> show merged config
        (None, None, false, false) => show_merged_config(out, &loader),

        // No key, no value, with level flag -> show config at that level
        (None, None, true, _) => show_level_config(out, &loader, ConfigLevel::Global),
        (None, None, _, true) => show_level_config(out, &loader, ConfigLevel::Project),

        // Key only, no level flags -> get from merged config
        (Some(k), None, false, false) => {
            let config = loader.load()?;
            let val = get_config_value(&config, k)?;
            out.raw(&format!("{}\n", val));
            Ok(())
        }

        // Key only, with level flag -> get from that level
        (Some(k), None, true, _) => show_key(out, &loader, ConfigLevel::Global, k),
        (Some(k), None, _, true) => show_key(out, &loader, ConfigLevel::Project, k),

        // Key and value -> set at specified level (default to project)
        (Some(k), Some(v), true, _) => set_key(out, &loader, ConfigLevel::Global, k, v),
        (Some(k), Some(v), _, _) => set_key(out, &loader, ConfigLevel::Project, k, v),

        // Value without key -> error
        (None, Some(_), _, _) => {
            out.step("config").fail_untimed("key required when setting a value");
            Ok(())
        }
    }
}

/// Show merged config from all levels.
fn show_merged_config(out: &Output, loader: &ConfigLoader) -> Result<()> {
    let config = loader.load()?;
    let yaml = config.to_yaml()?;

    out.raw("# Merged configuration (system + global + project)\n");
    out.raw(&format!("{}\n", yaml));

    // Show which levels are active
    let existing = loader.existing_levels();
    if !existing.is_empty() {
        out.raw("# Active config levels:\n");
        for level in existing {
            if let Some(path) = loader.path_for_level(level) {
                out.raw(&format!("#   {:?}: {}\n", level, path.display()));
            }
        }
    }

    Ok(())
}

/// Show config at a specific level.
fn show_level_config(out: &Output, loader: &ConfigLoader, level: ConfigLevel) -> Result<()> {
    match loader.load_level(level)? {
        Some(config) => {
            let yaml = config.to_yaml()?;
            out.raw(&format!("# {:?} configuration\n", level));
            if let Some(path) = loader.path_for_level(level) {
                out.raw(&format!("# Path: {}\n", path.display()));
            }
            out.raw(&format!("{}\n", yaml));
        }
        None => {
            out.raw(&format!("# No {:?} configuration found\n", level));
            if let Some(path) = loader.path_for_level(level) {
                out.raw(&format!("# Expected path: {}\n", path.display()));
            }
        }
    }
    Ok(())
}

/// Show a specific key from config.
fn show_key(out: &Output, loader: &ConfigLoader, level: ConfigLevel, key: &str) -> Result<()> {
    let config = if level == ConfigLevel::Project && !loader.exists_at_level(level) {
        // Fall back to merged config if project doesn't exist
        loader.load()?
    } else {
        loader.load_level(level)?.unwrap_or_default()
    };

    let value = get_config_value(&config, key)?;
    out.raw(&format!("{}\n", value));
    Ok(())
}

/// Set a key in config at a specific level.
fn set_key(
    out: &Output,
    loader: &ConfigLoader,
    level: ConfigLevel,
    key: &str,
    value: &str,
) -> Result<()> {
    // Load existing config at level or create new
    let mut config = loader.load_level(level)?.unwrap_or_default();

    set_config_value(&mut config, key, value)?;

    loader.save_level(&config, level)?;

    let result = if let Some(path) = loader.path_for_level(level) {
        format!("{} = {} in {}", key, value, path.display())
    } else {
        format!("{} = {}", key, value)
    };

    out.step("config").done_untimed(&result);

    Ok(())
}

/// Get a value from config by dotted key path.
fn get_config_value(config: &DtxConfig, key: &str) -> Result<String> {
    let parts: Vec<&str> = key.split('.').collect();

    match parts.as_slice() {
        ["project", "name"] => Ok(config.project.name.clone()),
        ["project", "description"] => Ok(config.project.description.clone().unwrap_or_default()),
        ["settings", "log_level"] => Ok(config.settings.log_level.clone()),
        ["settings", "health_check_interval"] => Ok(config.settings.health_check_interval.clone()),
        ["settings", "shutdown_timeout"] => Ok(config.settings.shutdown_timeout.clone()),
        ["settings", "use_uds"] => Ok(config.settings.use_uds.to_string()),
        ["settings", "auto_resolve_ports"] => Ok(config.settings.auto_resolve_ports.to_string()),
        ["defaults", "log_level"] => Ok(config
            .defaults
            .as_ref()
            .and_then(|d| d.log_level.clone())
            .unwrap_or_default()),
        ["defaults", "health_check_interval"] => Ok(config
            .defaults
            .as_ref()
            .and_then(|d| d.health_check_interval.clone())
            .unwrap_or_default()),
        ["defaults", "shutdown_timeout"] => Ok(config
            .defaults
            .as_ref()
            .and_then(|d| d.shutdown_timeout.clone())
            .unwrap_or_default()),
        _ => anyhow::bail!("Unknown config key: {}", key),
    }
}

/// Set a value in config by dotted key path.
fn set_config_value(config: &mut DtxConfig, key: &str, value: &str) -> Result<()> {
    let parts: Vec<&str> = key.split('.').collect();

    match parts.as_slice() {
        ["project", "name"] => config.project.name = value.to_string(),
        ["project", "description"] => config.project.description = Some(value.to_string()),
        ["settings", "log_level"] => config.settings.log_level = value.to_string(),
        ["settings", "health_check_interval"] => {
            config.settings.health_check_interval = value.to_string()
        }
        ["settings", "shutdown_timeout"] => config.settings.shutdown_timeout = value.to_string(),
        ["settings", "use_uds"] => config.settings.use_uds = value.parse()?,
        ["settings", "auto_resolve_ports"] => config.settings.auto_resolve_ports = value.parse()?,
        ["defaults", "log_level"] => {
            let defaults = config.defaults.get_or_insert_with(Default::default);
            defaults.log_level = Some(value.to_string());
        }
        ["defaults", "health_check_interval"] => {
            let defaults = config.defaults.get_or_insert_with(Default::default);
            defaults.health_check_interval = Some(value.to_string());
        }
        ["defaults", "shutdown_timeout"] => {
            let defaults = config.defaults.get_or_insert_with(Default::default);
            defaults.shutdown_timeout = Some(value.to_string());
        }
        _ => anyhow::bail!("Unknown or read-only config key: {}", key),
    }

    Ok(())
}
