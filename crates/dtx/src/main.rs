//! dtx - Dev Tools eXperience CLI
//!
//! A command-line tool for managing development services with process-compose and Nix.

mod cmd;
mod context;
pub mod output;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "dtx")]
#[command(version, about = "Dev Tools eXperience - Process orchestration with Nix", long_about = None)]
pub struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Arguments for the `add` subcommand.
#[derive(clap::Args)]
pub struct AddArgs {
    /// Service name
    pub name: String,

    /// Resource kind: process (default), container, vm, agent
    #[arg(short, long, value_parser = ["process", "container", "vm", "agent"])]
    pub kind: Option<String>,

    /// Command to run the service (auto-detected for known packages)
    #[arg(short, long)]
    pub command: Option<String>,

    /// Package name (auto-inferred from name/command if omitted)
    #[arg(short = 'P', long)]
    pub package: Option<String>,

    /// Port number
    #[arg(short, long)]
    pub port: Option<u16>,

    /// Working directory
    #[arg(short, long)]
    pub working_dir: Option<String>,

    /// Environment variables (KEY=VALUE format)
    #[arg(short, long, value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Services this depends on (format: "service" or "service:condition")
    /// Conditions: started (default), healthy, completed
    #[arg(long, value_delimiter = ',')]
    pub depends_on: Vec<String>,

    /// Disable the service initially
    #[arg(long)]
    pub disabled: bool,

    /// Restart policy: always, on-failure (default), no
    #[arg(long, value_parser = ["always", "on-failure", "no"])]
    pub restart: Option<String>,

    /// Health check (format: "exec:command" or "http:host:port/path")
    #[arg(long)]
    pub health_check: Option<String>,

    /// Liveness probe (format: "exec:command" or "http:host:port/path")
    #[arg(long)]
    pub liveness: Option<String>,

    /// Shutdown config (format: "command:...", "SIGTERM", or "SIGINT")
    #[arg(long)]
    pub shutdown: Option<String>,

    /// Shutdown timeout (e.g., "30s")
    #[arg(long)]
    pub shutdown_timeout: Option<String>,

    // -- Container-specific --
    /// Container image [container]
    #[arg(long)]
    pub image: Option<String>,

    /// Volume mounts [container] (format: "host:container")
    #[arg(long)]
    pub volume: Vec<String>,

    // -- VM-specific --
    /// VM backend: qemu, firecracker [vm]
    #[arg(long, value_parser = ["qemu", "firecracker"])]
    pub vm_backend: Option<String>,

    /// VM memory (e.g., "2G") [vm]
    #[arg(long)]
    pub memory: Option<String>,

    /// VM CPU count [vm]
    #[arg(long)]
    pub cpus: Option<u32>,

    /// VM disk image path [vm]
    #[arg(long)]
    pub disk: Option<String>,

    /// NixOS configuration path [vm]
    #[arg(long)]
    pub nixos: Option<String>,

    // -- Agent-specific --
    /// Agent runtime: claude, openai, ollama [agent]
    #[arg(long)]
    pub runtime: Option<String>,

    /// Agent model (e.g., "claude-sonnet-4-20250514") [agent]
    #[arg(long)]
    pub model: Option<String>,

    /// Agent tools [agent]
    #[arg(long)]
    pub tool: Vec<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new project
    Init {
        /// Project name (optional with --detect)
        name: Option<String>,

        /// Project path (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,

        /// Project description
        #[arg(short, long)]
        description: Option<String>,

        /// Detect project type and infer Nix packages from codebase
        #[arg(long)]
        detect: bool,

        /// Auto-accept inferred packages without prompting
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Add a service to the project
    Add(Box<AddArgs>),

    /// Edit an existing service
    Edit {
        /// Service name
        name: String,

        /// Command to run the service
        #[arg(short, long)]
        command: Option<String>,

        /// Port number
        #[arg(short, long)]
        port: Option<u16>,

        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,

        /// Add environment variable (KEY=VALUE format)
        #[arg(long, value_name = "KEY=VALUE")]
        add_env: Vec<String>,

        /// Remove environment variable by key
        #[arg(long, value_name = "KEY")]
        remove_env: Vec<String>,

        /// Add dependency (format: "service" or "service:condition")
        #[arg(long)]
        add_dep: Vec<String>,

        /// Remove dependency by service name
        #[arg(long)]
        remove_dep: Vec<String>,

        /// Restart policy: always, on-failure, no
        #[arg(long, value_parser = ["always", "on-failure", "no"])]
        restart: Option<String>,

        /// Health check (format: "exec:command" or "http:host:port/path")
        #[arg(long)]
        health_check: Option<String>,

        /// Clear health check
        #[arg(long)]
        clear_health_check: bool,

        /// Enable the service
        #[arg(long)]
        enable: bool,

        /// Disable the service
        #[arg(long)]
        disable: bool,
    },

    /// List projects or services
    List {
        /// List services in current project instead of projects
        #[arg(short, long)]
        services: bool,
    },

    /// Start services
    Start {
        /// Specific service to start (defaults to all enabled)
        service: Option<String>,

        /// Run in foreground (no TUI, logs to stdout)
        #[arg(short, long)]
        foreground: bool,
    },

    /// Stop services
    Stop {
        /// Specific service to stop (defaults to all)
        service: Option<String>,
    },

    /// View service logs
    Logs {
        /// Specific service to view logs for
        service: Option<String>,

        /// Show logs for all services
        #[arg(short, long)]
        all: bool,

        /// Follow log output (stream new logs)
        #[arg(short, long)]
        follow: bool,
    },

    /// Show service status
    Status {
        /// Specific service (defaults to all)
        service: Option<String>,
    },

    /// Remove a service
    Remove {
        /// Service name
        name: String,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Export configuration to various formats
    Export {
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Export format: docker-compose, kubernetes, process-compose, dtx
        #[arg(short, long, default_value = "process-compose")]
        format: String,

        /// Kubernetes namespace (only for kubernetes format)
        #[arg(long)]
        namespace: Option<String>,

        /// Default container image for services without explicit images
        #[arg(long)]
        default_image: Option<String>,

        /// Filter to specific services (comma-separated)
        #[arg(long)]
        services: Option<String>,
    },

    /// Import configuration from external formats (process-compose, docker-compose, Procfile)
    Import {
        /// File to import (process-compose.yaml, docker-compose.yml, or Procfile)
        file: std::path::PathBuf,

        /// Force format: process-compose, docker-compose, procfile, auto
        #[arg(short, long)]
        format: Option<String>,

        /// Skip Nix package inference
        #[arg(long)]
        no_nix: bool,

        /// Dry run (show what would be imported)
        #[arg(long)]
        dry_run: bool,
    },

    /// Search for Nix packages
    Search {
        /// Search query
        query: String,

        /// Maximum number of results to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Start the web UI
    Web {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,

        /// Open browser after starting
        #[arg(short, long)]
        open: bool,
    },

    /// Run as MCP server for AI agent integration
    Mcp {
        /// Project directory (defaults to current directory)
        #[arg(short, long, env = "DTX_PROJECT")]
        project: Option<String>,
    },

    /// Generate shell completions
    Completions {
        /// Shell type
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// View or set configuration
    Config {
        /// Use global config (~/.config/dtx/config.yaml)
        #[arg(long)]
        global: bool,

        /// Use project config (.dtx/config.yaml)
        #[arg(long)]
        project: bool,

        /// Configuration key (e.g., settings.log_level)
        key: Option<String>,

        /// Value to set (omit to get current value)
        value: Option<String>,
    },

    /// Nix environment management
    Nix {
        #[command(subcommand)]
        command: NixCommands,
    },
}

#[derive(Subcommand)]
pub enum NixCommands {
    /// Generate flake.nix and .envrc for the project
    Init,

    /// Regenerate .envrc only
    Envrc,

    /// Run command in nix shell (or enter interactive shell)
    Shell {
        /// Command to run (interactive shell if omitted)
        command: Option<String>,
    },

    /// List Nix packages from services
    Packages,
}

#[tokio::main]
async fn main() {
    let out = output::Output::new();
    if let Err(err) = run(&out).await {
        // Format chain: "message" or "message — caused by: ..."
        let msg = if err.chain().count() > 1 {
            let causes: Vec<String> = err.chain().skip(1).map(|c| format!("{}", c)).collect();
            format!("{} — {}", err, causes.join(", "))
        } else {
            format!("{}", err)
        };
        out.step("error").fail_untimed(&msg);
        std::process::exit(1);
    }
}

async fn run(out: &output::Output) -> Result<()> {
    let cli = Cli::parse();

    // Check if we're running TUI mode (start without -f)
    // In that case, skip tracing initialization to avoid corrupting the TUI
    let skip_tracing = matches!(
        &cli.command,
        Commands::Start {
            foreground: false,
            ..
        }
    );

    // Setup logging (skip for TUI mode)
    if !skip_tracing {
        let filter = if cli.verbose {
            EnvFilter::new("debug")
        } else {
            EnvFilter::new("warn")
        };

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .without_time()
            .init();
    }

    // Handle commands that don't need a project config
    match cli.command {
        Commands::Init {
            name,
            path,
            description,
            detect,
            yes,
        } => return cmd::init::run(out, name, path, description, detect, yes).await,

        Commands::Completions { shell } => {
            cmd::completions::run(shell);
            return Ok(());
        }

        Commands::Search { query, limit } => {
            return cmd::search::run(out, query, limit).await;
        }

        Commands::Config {
            global,
            project,
            key,
            value,
        } => {
            return cmd::config::run(out, global, project, key, value).await;
        }

        Commands::Web { port, open } => {
            return cmd::web::run(out, port, open).await;
        }

        Commands::Mcp { project } => {
            return cmd::mcp::run(cmd::mcp::McpArgs { project }).await;
        }

        _ => {}
    }

    // Create context with config store for other commands
    let mut ctx = context::Context::new()?;

    // Dispatch commands that need config store
    match cli.command {
        Commands::Init { .. }
        | Commands::Completions { .. }
        | Commands::Search { .. }
        | Commands::Web { .. }
        | Commands::Mcp { .. }
        | Commands::Config { .. } => unreachable!(),

        Commands::Import {
            file,
            format,
            no_nix,
            dry_run,
        } => cmd::import::run(
            &mut ctx,
            out,
            cmd::import::ImportArgs {
                file,
                format,
                no_nix,
                dry_run,
            },
        ),

        Commands::Add(args) => cmd::add::run(
            &mut ctx,
            out,
            cmd::add::AddArgs {
                name: args.name,
                kind: args.kind,
                command: args.command,
                package: args.package,
                port: args.port,
                working_dir: args.working_dir,
                env_vars: args.env,
                depends_on: args.depends_on,
                disabled: args.disabled,
                restart: args.restart,
                health_check: args.health_check,
                liveness: args.liveness,
                shutdown: args.shutdown,
                shutdown_timeout: args.shutdown_timeout,
                image: args.image,
                volumes: args.volume,
                vm_backend: args.vm_backend,
                memory: args.memory,
                cpus: args.cpus,
                disk: args.disk,
                nixos: args.nixos,
                runtime: args.runtime,
                model: args.model,
                tools: args.tool,
            },
        ),

        Commands::Edit {
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
        } => cmd::edit::run(
            &mut ctx,
            out,
            cmd::edit::EditArgs {
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
            },
        ),

        Commands::List { services } => cmd::list::run(&ctx, out, services),

        Commands::Start {
            service,
            foreground,
        } => cmd::start::run(&ctx, out, service, foreground).await,

        Commands::Stop { service } => cmd::stop::run(&ctx, out, service).await,

        Commands::Logs {
            service,
            all,
            follow,
        } => cmd::logs::run(&ctx, out, service, all, follow).await,

        Commands::Status { service } => cmd::status::run(&ctx, out, service).await,

        Commands::Remove { name, yes } => cmd::remove::run(&mut ctx, out, name, yes),

        Commands::Export {
            output,
            format,
            namespace,
            default_image,
            services,
        } => cmd::export::run(
            &ctx,
            out,
            output,
            &format,
            namespace,
            default_image,
            services,
        ),

        Commands::Nix { command } => match command {
            NixCommands::Init => cmd::nix::init(&ctx, out).await,
            NixCommands::Envrc => cmd::nix::envrc(&ctx, out).await,
            NixCommands::Shell { command } => cmd::nix::shell(&ctx, out, command).await,
            NixCommands::Packages => cmd::nix::packages(&ctx, out),
        },
    }
}
