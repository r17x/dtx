//! Add a service to the project.

use super::parsers;
use crate::context::Context;
use crate::output::Output;
use anyhow::{bail, Result};
use dtx_core::config::schema::{
    DependencyConditionConfig, DependencyConfig, HealthConfig, NixConfig, ResourceConfig,
    ResourceKindConfig, RestartConfig, ShutdownConfigSchema, VmConfig,
};
use dtx_core::{sync_add_package, Environment, Port, ServiceName, ShellCommand};
use indexmap::IndexMap;

/// Arguments for the add command.
pub struct AddArgs {
    pub name: String,
    pub kind: Option<String>,
    pub command: Option<String>,
    pub package: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
    pub env_vars: Vec<String>,
    pub depends_on: Vec<String>,
    pub disabled: bool,
    pub restart: Option<String>,
    pub health_check: Option<String>,
    pub liveness: Option<String>,
    pub shutdown: Option<String>,
    pub shutdown_timeout: Option<String>,
    // Container
    pub image: Option<String>,
    pub volumes: Vec<String>,
    // VM
    pub vm_backend: Option<String>,
    pub memory: Option<String>,
    pub cpus: Option<u32>,
    pub disk: Option<String>,
    pub nixos: Option<String>,
    // Agent
    pub runtime: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
}

/// Get a default command for a known package.
fn default_command_for_package(package: &str, port: Option<u16>) -> Option<String> {
    let port_str = port
        .map(|p| p.to_string())
        .unwrap_or_else(|| "{port}".to_string());
    match package {
        "postgresql" | "postgresql_16" | "postgresql_15" | "postgresql_14" => {
            Some(format!("postgres -D $PGDATA -h 127.0.0.1 -p {}", port_str))
        }
        "redis" => Some(format!("redis-server --port {}", port_str)),
        "mysql" | "mysql80" | "mysql84" => Some(format!("mysqld --port={}", port_str)),
        "mongodb" | "mongodb-7_0" => Some(format!("mongod --port {}", port_str)),
        "nginx" => Some("nginx -g 'daemon off;'".to_string()),
        "rabbitmq-server" => Some("rabbitmq-server".to_string()),
        "memcached" => Some(format!("memcached -p {}", port_str)),
        "minio" => Some("minio server ./data".to_string()),
        _ => None,
    }
}

/// Get the conventional port for a known package.
fn conventional_port_for_package(package: &str) -> Option<u16> {
    match package {
        "postgresql" | "postgresql_16" | "postgresql_15" | "postgresql_14" => Some(5432),
        "mysql" | "mysql80" | "mysql84" => Some(3306),
        "redis" => Some(6379),
        "mongodb" | "mongodb-7_0" => Some(27017),
        "rabbitmq-server" => Some(5672),
        "memcached" => Some(11211),
        "minio" => Some(9000),
        _ => None,
    }
}

/// Parse a kind string to ResourceKindConfig.
fn parse_kind(s: &str) -> Result<ResourceKindConfig> {
    match s {
        "process" => Ok(ResourceKindConfig::Process),
        "container" => Ok(ResourceKindConfig::Container),
        "vm" => Ok(ResourceKindConfig::Vm),
        "agent" => Ok(ResourceKindConfig::Agent),
        _ => bail!("Unknown kind '{}'. Use: process, container, vm, or agent", s),
    }
}

/// Reject flags that don't belong to the given kind.
fn validate_kind_flags(kind: &ResourceKindConfig, args: &AddArgs) -> Result<()> {
    let kind_label = match kind {
        ResourceKindConfig::Process => "process",
        ResourceKindConfig::Container => "container",
        ResourceKindConfig::Vm => "vm",
        ResourceKindConfig::Agent => "agent",
    };

    // Container-only flags
    if !matches!(kind, ResourceKindConfig::Container) {
        if args.image.is_some() {
            bail!("--image is only valid for container kind (got --kind {})", kind_label);
        }
        if !args.volumes.is_empty() {
            bail!("--volume is only valid for container kind (got --kind {})", kind_label);
        }
    }

    // VM-only flags
    if !matches!(kind, ResourceKindConfig::Vm) {
        for (flag, val) in [
            ("--vm-backend", args.vm_backend.as_ref()),
            ("--memory", args.memory.as_ref()),
            ("--disk", args.disk.as_ref()),
            ("--nixos", args.nixos.as_ref()),
        ] {
            if val.is_some() {
                bail!("{} is only valid for vm kind (got --kind {})", flag, kind_label);
            }
        }
        if args.cpus.is_some() {
            bail!("--cpus is only valid for vm kind (got --kind {})", kind_label);
        }
    }

    // Agent-only flags
    if !matches!(kind, ResourceKindConfig::Agent) {
        if args.runtime.is_some() {
            bail!("--runtime is only valid for agent kind (got --kind {})", kind_label);
        }
        if args.model.is_some() {
            bail!("--model is only valid for agent kind (got --kind {})", kind_label);
        }
        if !args.tools.is_empty() {
            bail!("--tool is only valid for agent kind (got --kind {})", kind_label);
        }
    }

    // Process/container-only: package inference
    if matches!(kind, ResourceKindConfig::Vm | ResourceKindConfig::Agent) && args.package.is_some()
    {
        bail!("--package is only valid for process/container kind (got --kind {})", kind_label);
    }

    Ok(())
}

/// Run the add command.
pub fn run(ctx: &mut Context, out: &Output, args: AddArgs) -> Result<()> {
    // === Parse kind (defaults to process) ===
    let kind = args
        .kind
        .as_deref()
        .map(parse_kind)
        .transpose()?
        .unwrap_or(ResourceKindConfig::Process);

    // Validate kind-specific flags early
    validate_kind_flags(&kind, &args)?;

    let AddArgs {
        name,
        command,
        package,
        port,
        working_dir,
        env_vars,
        depends_on,
        disabled,
        restart,
        health_check,
        liveness,
        shutdown,
        shutdown_timeout,
        image,
        volumes,
        vm_backend,
        memory,
        cpus,
        disk,
        nixos,
        runtime,
        model,
        tools,
        ..
    } = args;

    // === Parse Don't Validate: validate at CLI input boundary ===

    // Validate service name (DNS-compatible, 2-63 chars, lowercase)
    let validated_name: ServiceName = name
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid service name '{}': {}", name, e))?;

    // Parse dependencies early (validates service names and conditions)
    let parsed_deps: Vec<DependencyConfig> = depends_on
        .iter()
        .map(|s| parse_dependency_config(s))
        .collect::<Result<Vec<_>>>()?;

    // Parse environment variables using domain type
    let environment = if env_vars.is_empty() {
        IndexMap::new()
    } else {
        let env = Environment::from_strings(&env_vars)
            .map_err(|e| anyhow::anyhow!("Invalid environment variable: {}", e))?;
        env.into_map()
            .into_iter()
            .collect::<IndexMap<String, String>>()
    };

    // Parse health check
    let health = match health_check {
        Some(s) => {
            let hc = parsers::parse_health_check(&s)?;
            Some(health_check_to_config(&hc))
        }
        None => None,
    };

    // Parse liveness probe
    let liveness_config = match liveness {
        Some(s) => {
            let hc = parsers::parse_health_check(&s)?;
            Some(health_check_to_config(&hc))
        }
        None => None,
    };

    // Parse restart policy
    let restart_config = restart
        .as_deref()
        .map(|s| parsers::parse_restart_policy(s))
        .transpose()?
        .map(RestartConfig::Simple);

    // Parse shutdown config
    let shutdown_config = parse_shutdown(shutdown.as_deref(), shutdown_timeout.as_deref())?;

    // === Kind-specific resource building ===
    let (resolved_command, resolved_package, resolved_port, nix) = match kind {
        ResourceKindConfig::Process | ResourceKindConfig::Container => {
            build_process_context(command, package, port, &name)?
        }
        ResourceKindConfig::Vm | ResourceKindConfig::Agent => {
            // No package inference for VM/Agent
            (command, None, port, None)
        }
    };

    // Validate command for process kind (required)
    if kind == ResourceKindConfig::Process {
        let cmd = resolved_command
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Process resource requires a command"))?;
        let _validated: ShellCommand = cmd
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid command: {}", e))?;
    }

    // Validate container kind (requires image or command)
    if kind == ResourceKindConfig::Container && image.is_none() && resolved_command.is_none() {
        bail!("Container resource requires --image or --command");
    }

    // Validate agent kind (requires runtime)
    if kind == ResourceKindConfig::Agent && runtime.is_none() {
        bail!("Agent resource requires --runtime");
    }

    // Validate port (non-privileged, >= 1024)
    let validated_port = resolved_port
        .map(Port::try_from)
        .transpose()
        .map_err(|e| anyhow::anyhow!("Invalid port: {}", e))?;

    // Build VM config
    let vm_config = if kind == ResourceKindConfig::Vm {
        Some(VmConfig {
            backend: vm_backend,
            memory,
            cpus,
            disk,
            nixos,
        })
    } else {
        None
    };

    // Build ResourceConfig
    let kind_clone = kind.clone();
    let rc = ResourceConfig {
        kind,
        command: resolved_command.clone(),
        port: validated_port.map(u16::from),
        working_dir: working_dir.map(std::path::PathBuf::from),
        environment,
        depends_on: parsed_deps,
        health,
        liveness: liveness_config,
        restart: restart_config.clone(),
        shutdown: shutdown_config,
        nix,
        image,
        volumes,
        vm: vm_config,
        runtime,
        model,
        tools,
        enabled: !disabled,
    };

    // Add resource to store
    ctx.store
        .add_resource(validated_name.as_ref(), rc)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    ctx.store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Build summary group
    let kind_label = match &kind_clone {
        ResourceKindConfig::Process => "process",
        ResourceKindConfig::Container => "container",
        ResourceKindConfig::Vm => "vm",
        ResourceKindConfig::Agent => "agent",
    };
    let mut grp = out.group(&format!("{}", validated_name));
    if kind_label != "process" {
        grp.child_done("kind", kind_label);
    }
    if let Some(ref pkg) = resolved_package {
        grp.child_done("package", pkg);
    }
    if let Some(ref cmd) = resolved_command {
        grp.child_done("command", cmd);
    }
    if let Some(p) = validated_port {
        grp.child_done("port", &format!("{}", u16::from(p)));
    }
    if disabled {
        grp.child_done("status", "disabled");
    }

    // Sync flake.nix if service has a package
    if let Some(ref pkg) = resolved_package {
        let project_root = ctx.store.project_root();
        let project_name = ctx.store.project_name();
        match sync_add_package(project_root, project_name, pkg) {
            Ok(true) => {
                grp.child_done("flake", &format!("updated with {}", pkg));
            }
            Ok(false) => {}
            Err(e) => grp.child_fail("flake", &format!("{}", e)),
        }
    }

    if let Some(ref policy) = restart_config {
        grp.child_done("restart", &format!("{:?}", policy.policy()));
    }

    grp.done_with_summary("added");

    // Notify web/TUI of config change (fire-and-forget, sync)
    dtx_core::notify_config_changed_sync();

    Ok(())
}

/// Build process/container context: resolve command, package, port, and nix config.
fn build_process_context(
    command: Option<String>,
    package: Option<String>,
    port: Option<u16>,
    name: &str,
) -> Result<(Option<String>, Option<String>, Option<u16>, Option<NixConfig>)> {
    // Package inference
    let resolved_package = match package {
        Some(pkg) => Some(pkg),
        None => {
            use dtx_core::nix::{extract_executable, PackageMappings};
            let mappings = PackageMappings::load();
            if let Some(pkg) = mappings.get_package(name) {
                Some(pkg.clone())
            } else if let Some(ref cmd) = command {
                if let Some(executable) = extract_executable(cmd) {
                    mappings.get_package(&executable).cloned()
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    // Port inference
    let resolved_port = match port {
        Some(p) => Some(p),
        None => resolved_package
            .as_ref()
            .and_then(|pkg| conventional_port_for_package(pkg)),
    };

    // Command resolution
    let resolved_command = match command {
        Some(cmd) => Some(cmd),
        None => {
            if let Some(ref pkg) = resolved_package {
                Some(
                    default_command_for_package(pkg, resolved_port)
                        .unwrap_or_else(|| name.to_string()),
                )
            } else {
                None
            }
        }
    };

    // Build nix config if package is set
    let nix = resolved_package.as_ref().map(|pkg| NixConfig {
        packages: vec![pkg.clone()],
        ..Default::default()
    });

    Ok((resolved_command, resolved_package, resolved_port, nix))
}

/// Parse shutdown CLI value into ShutdownConfigSchema.
fn parse_shutdown(
    shutdown: Option<&str>,
    timeout: Option<&str>,
) -> Result<Option<ShutdownConfigSchema>> {
    if shutdown.is_none() && timeout.is_none() {
        return Ok(None);
    }

    let (command, signal) = match shutdown {
        Some(s) if s.starts_with("command:") => (Some(s[8..].to_string()), None),
        Some(s) if s.starts_with("SIG") => (None, Some(s.to_string())),
        Some(s) => {
            bail!(
                "Invalid shutdown format '{}'. Use 'command:...' or a signal name (SIGTERM, SIGINT)",
                s
            );
        }
        None => (None, None),
    };

    Ok(Some(ShutdownConfigSchema {
        command,
        signal,
        timeout: timeout.map(|t| t.to_string()),
    }))
}

/// Parse a dependency string to DependencyConfig.
fn parse_dependency_config(s: &str) -> Result<DependencyConfig> {
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

/// Convert a model HealthCheck to a ResourceConfig HealthConfig.
pub fn health_check_to_config(hc: &dtx_core::model::HealthCheck) -> HealthConfig {
    use dtx_core::model::HealthCheckType;
    match hc.check_type {
        HealthCheckType::Exec => HealthConfig {
            exec: hc.command.clone(),
            http: None,
            tcp: None,
            interval: format!("{}s", hc.period_seconds),
            timeout: "10s".to_string(),
            retries: 3,
            initial_delay: if hc.initial_delay_seconds > 0 {
                Some(format!("{}s", hc.initial_delay_seconds))
            } else {
                None
            },
        },
        HealthCheckType::HttpGet => {
            let http_path = hc
                .http_get
                .as_ref()
                .map(|h| format!("{}:{}{}", h.host, h.port, h.path));
            HealthConfig {
                exec: None,
                http: http_path,
                tcp: None,
                interval: format!("{}s", hc.period_seconds),
                timeout: "10s".to_string(),
                retries: 3,
                initial_delay: if hc.initial_delay_seconds > 0 {
                    Some(format!("{}s", hc.initial_delay_seconds))
                } else {
                    None
                },
            }
        }
    }
}
