//! TUI application using ResourceEventBus for decoupled communication.
//!
//! The TUI subscribes to ResourceEventBus and reacts to:
//! - Log events -> display in log panel
//! - Starting/Running/Stopped/Failed -> update service states
//!
//! This design enables the same event stream to be consumed by
//! both TUI and Web SSE simultaneously.

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute, queue,
    terminal::{
        disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use dtx_core::events::{EventFilter, LifecycleEvent, ResourceEventBus, ResourceEventSubscriber};
use dtx_core::model::Service;
use dtx_core::resource::{Context, Resource, ResourceId, ResourceState};
use dtx_core::store::ConfigStore;
use dtx_process::{ProcessResourceConfig, ResourceOrchestrator};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::ui;

/// A collected log line for display.
#[derive(Clone)]
pub struct DisplayLog {
    pub service: String,
    pub content: String,
}

/// Service info for display (derived from events).
pub struct ServiceDisplayInfo {
    pub name: String,
    pub state: DisplayState,
    pub restarts: u32,
}

/// Display-friendly state (simplified from ResourceState).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DisplayState {
    Pending,
    Starting,
    Running { pid: u32 },
    Completed { exit_code: i32 },
    Failed { error: Option<String> },
    Stopped,
}

impl From<&ResourceState> for DisplayState {
    fn from(state: &ResourceState) -> Self {
        match state {
            ResourceState::Pending => DisplayState::Pending,
            ResourceState::Starting { .. } => DisplayState::Starting,
            ResourceState::Running { pid, .. } => DisplayState::Running {
                pid: pid.unwrap_or(0),
            },
            ResourceState::Stopping { .. } => DisplayState::Running { pid: 0 },
            ResourceState::Stopped { exit_code, .. } => {
                if let Some(code) = exit_code {
                    DisplayState::Completed { exit_code: *code }
                } else {
                    DisplayState::Stopped
                }
            }
            ResourceState::Failed { error, .. } => DisplayState::Failed {
                error: Some(error.clone()),
            },
        }
    }
}

impl DisplayState {
    /// Check if the process is currently running.
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        matches!(self, DisplayState::Running { .. } | DisplayState::Starting)
    }
}

/// TUI interaction mode.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UiMode {
    #[default]
    Normal,
}

/// TUI application state.
pub struct App {
    /// Service names (ordered).
    service_names: Vec<String>,
    /// Service states (updated from events).
    service_states: HashMap<String, DisplayState>,
    /// Restart counts per service.
    restart_counts: HashMap<String, u32>,
    /// Collected logs for display.
    pub logs: VecDeque<DisplayLog>,
    /// Maximum logs to keep.
    max_logs: usize,
    /// Currently selected service index.
    pub selected: usize,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Status message.
    pub status_message: Option<String>,
    /// Config changed flag (set by ConfigChanged event).
    pub config_changed: bool,
    /// Current UI mode.
    pub mode: UiMode,
}

impl App {
    pub fn new(service_names: Vec<String>) -> Self {
        let mut service_states = HashMap::new();
        let mut restart_counts = HashMap::new();
        for name in &service_names {
            service_states.insert(name.clone(), DisplayState::Pending);
            restart_counts.insert(name.clone(), 0);
        }

        Self {
            service_names,
            service_states,
            restart_counts,
            logs: VecDeque::with_capacity(500),
            max_logs: 500,
            selected: 0,
            should_quit: false,
            status_message: None,
            config_changed: false,
            mode: UiMode::Normal,
        }
    }

    /// Get service info for display.
    pub fn service_infos(&self) -> Vec<ServiceDisplayInfo> {
        self.service_names
            .iter()
            .map(|name| ServiceDisplayInfo {
                name: name.clone(),
                state: self
                    .service_states
                    .get(name)
                    .cloned()
                    .unwrap_or(DisplayState::Pending),
                restarts: *self.restart_counts.get(name).unwrap_or(&0),
            })
            .collect()
    }

    /// Process a LifecycleEvent from the ResourceEventBus.
    pub fn process_event(&mut self, event: LifecycleEvent) {
        match event {
            LifecycleEvent::Starting { id, .. } => {
                let name = id.to_string();
                self.service_states.insert(name, DisplayState::Starting);
            }
            LifecycleEvent::Running { id, pid, .. } => {
                let name = id.to_string();
                self.service_states.insert(
                    name,
                    DisplayState::Running {
                        pid: pid.unwrap_or(0),
                    },
                );
            }
            LifecycleEvent::Stopping { id, .. } => {
                let name = id.to_string();
                // Keep as running with visual indication
                self.service_states
                    .insert(name, DisplayState::Running { pid: 0 });
            }
            LifecycleEvent::Stopped { id, exit_code, .. } => {
                let name = id.to_string();
                if let Some(code) = exit_code {
                    self.service_states
                        .insert(name, DisplayState::Completed { exit_code: code });
                } else {
                    self.service_states.insert(name, DisplayState::Stopped);
                }
            }
            LifecycleEvent::Failed { id, error, .. } => {
                let name = id.to_string();
                self.service_states
                    .insert(name, DisplayState::Failed { error: Some(error) });
            }
            LifecycleEvent::Restarting { id, attempt, .. } => {
                let name = id.to_string();
                self.restart_counts.insert(name.clone(), attempt);
                self.service_states.insert(name, DisplayState::Starting);
            }
            LifecycleEvent::Log { id, line, .. } => {
                self.logs.push_back(DisplayLog {
                    service: id.to_string(),
                    content: line,
                });

                while self.logs.len() > self.max_logs {
                    self.logs.pop_front();
                }
            }
            LifecycleEvent::ConfigChanged { .. } => {
                self.config_changed = true;
                self.status_message = Some("Config changed — press 'a' to reload".to_string());
            }
            LifecycleEvent::HealthCheckPassed { .. }
            | LifecycleEvent::HealthCheckFailed { .. }
            | LifecycleEvent::DependencyWaiting { .. }
            | LifecycleEvent::DependencyResolved { .. } => {}
        }
    }

    /// Poll events from subscriber (non-blocking).
    pub fn poll_events(&mut self, subscriber: &mut ResourceEventSubscriber) {
        while let Some(event) = subscriber.try_recv() {
            self.process_event(event);
        }
    }

    /// Update states from orchestrator processes.
    pub async fn sync_from_orchestrator(&mut self, orchestrator: &ResourceOrchestrator) {
        for id in orchestrator.resource_ids() {
            if let Some(resource) = orchestrator.get_resource(id) {
                let proc = resource.read().await;
                let state = DisplayState::from(proc.state());
                self.service_states.insert(id.to_string(), state);
            }
        }
    }

    /// Get currently selected service name.
    pub fn selected_service(&self) -> Option<&str> {
        self.service_names.get(self.selected).map(|s| s.as_str())
    }

    /// Handle keyboard input (returns action to take).
    pub fn handle_key(&mut self, key: KeyCode) -> Option<TuiAction> {
        match &self.mode {
            UiMode::Normal => self.handle_key_normal(key),
        }
    }

    fn handle_key_normal(&mut self, key: KeyCode) -> Option<TuiAction> {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected < self.service_names.len().saturating_sub(1) {
                    self.selected += 1;
                }
                None
            }
            KeyCode::Char('r') => {
                if let Some(name) = self.selected_service().map(|s| s.to_string()) {
                    self.status_message = Some(format!("Restarting {}...", name));
                    return Some(TuiAction::Restart(name));
                }
                None
            }
            KeyCode::Char('s') => {
                if let Some(name) = self.selected_service().map(|s| s.to_string()) {
                    self.status_message = Some(format!("Stopping {}...", name));
                    return Some(TuiAction::Stop(name));
                }
                None
            }
            KeyCode::Char('a') if self.config_changed => {
                self.config_changed = false;
                self.status_message = Some("Reloading config...".to_string());
                Some(TuiAction::Reload)
            }
            KeyCode::Char('c') => {
                self.logs.clear();
                self.status_message = Some("Logs cleared".to_string());
                None
            }
            _ => None,
        }
    }

    /// Set status message.
    pub fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
    }
}

/// Actions that can be triggered from the TUI.
pub enum TuiAction {
    Restart(String),
    Stop(String),
    Reload,
}

impl App {
    /// Add a new service to the display (called after config reload).
    pub fn add_service(&mut self, name: String) {
        if !self.service_names.contains(&name) {
            self.service_names.push(name.clone());
            self.service_states
                .insert(name.clone(), DisplayState::Pending);
            self.restart_counts.insert(name, 0);
        }
    }

    /// Remove a service from the display.
    pub fn remove_service(&mut self, name: &str) {
        self.service_names.retain(|n| n != name);
        self.service_states.remove(name);
        self.restart_counts.remove(name);
        if self.selected >= self.service_names.len() && self.selected > 0 {
            self.selected = self.service_names.len() - 1;
        }
    }
}

/// Convert a Service to a ProcessResourceConfig.
fn service_to_config(service: &Service, project_dir: &PathBuf) -> ProcessResourceConfig {
    let mut config = ProcessResourceConfig::new(&service.name, &service.command);

    // Set working directory to project directory
    config = config.with_working_dir(project_dir);

    // Set port if defined
    if let Some(port) = service.port {
        config = config.with_port(port);
    }

    // Set environment variables
    if let Some(ref env) = service.environment {
        for (key, value) in env {
            config = config.with_env(key, value);
        }
    }

    // Set dependencies
    if let Some(ref deps) = service.depends_on {
        for dep in deps {
            config = config.depends_on(dep.service.clone());
        }
    }

    config
}

/// Run the TUI with the ResourceOrchestrator.
pub async fn run_tui(
    out: &crate::output::Output,
    model_services: Vec<Service>,
    project_dir: PathBuf,
    _flake_dir: Option<PathBuf>,
    nix_env: Option<HashMap<String, String>>,
) -> Result<()> {
    // Filter to enabled services only
    let enabled_services: Vec<Service> = model_services.into_iter().filter(|s| s.enabled).collect();

    if enabled_services.is_empty() {
        return Err(anyhow::anyhow!("No enabled services to start"));
    }

    let enabled_count = enabled_services.len();
    out.step("prepare")
        .done_untimed(&format!("{} service(s)", enabled_count));

    // Create shared ResourceEventBus
    let event_bus = Arc::new(ResourceEventBus::new());

    // Start Unix socket listener so CLI events (dtx add/remove/edit) reach the TUI
    let _socket_guard = match dtx_core::events::start_event_listener(event_bus.clone()).await {
        Ok(guard) => Some(guard),
        Err(e) => {
            tracing::debug!("No event socket listener: {}", e);
            None
        }
    };

    // Create ResourceOrchestrator
    let mut orchestrator = ResourceOrchestrator::new(event_bus.clone());

    // Add services to orchestrator
    let service_names: Vec<String> = enabled_services.iter().map(|s| s.name.clone()).collect();
    for service in &enabled_services {
        let mut config = service_to_config(service, &project_dir);
        if let Some(ref env) = nix_env {
            let user_env = std::mem::take(&mut config.environment);
            config.environment = env.clone();
            config.environment.extend(user_env);
        }
        orchestrator.add_resource(config);
    }

    out.step("tui").done_untimed("starting");

    // Setup terminal FIRST (before any process output)
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    execute!(
        stdout,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )?;
    stdout.flush()?;

    let term_backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(term_backend)?;

    // Subscribe to ResourceEventBus BEFORE starting services
    // Use filter that includes logs for TUI display
    let mut subscriber = event_bus.subscribe_filtered(EventFilter::all());

    // Start all services via Orchestrator (dependency-ordered)
    let start_result = orchestrator
        .start_all()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    // Create app
    let mut app = App::new(service_names);

    if !start_result.failed.is_empty() {
        let failed_names: Vec<_> = start_result
            .failed
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        app.status_message = Some(format!(
            "Started {}, failed: {}",
            start_result.started.len(),
            failed_names.join(", ")
        ));
    }

    // Main loop
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        // Poll events from ResourceEventBus
        app.poll_events(&mut subscriber);

        // Poll all processes for output (they publish to EventBus)
        orchestrator.poll().await;

        // Sync state from orchestrator
        app.sync_from_orchestrator(&orchestrator).await;

        // Get service infos for rendering
        let service_infos = app.service_infos();

        // Draw UI with synchronized output to prevent tearing
        queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
        terminal.draw(|f| ui::draw_with_infos(f, &app, &service_infos))?;
        execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;

        // Handle events — drain all queued keys to avoid stale intermediate redraws
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        let mut pending_actions: Vec<TuiAction> = Vec::new();

        if event::poll(timeout)? {
            loop {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Press {
                        if let Some(action) = app.handle_key(key.code) {
                            pending_actions.push(action);
                        }
                    }
                }
                // Drain remaining queued events without waiting
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }

        for action in pending_actions {
            match action {
                TuiAction::Restart(name) => {
                    let id = ResourceId::new(&name);
                    if let Some(resource) = orchestrator.get_resource(&id) {
                        let mut resource = resource.write().await;
                        let ctx = Context::new();
                        match resource.stop(&ctx).await {
                            Ok(()) => {
                                if let Err(e) = resource.start(&ctx).await {
                                    app.set_status(format!("Failed to restart: {}", e));
                                } else {
                                    app.set_status(format!("Restarted {}", name));
                                }
                            }
                            Err(e) => app.set_status(format!("Failed: {}", e)),
                        }
                    } else {
                        app.set_status(format!("Resource {} not found", name));
                    }
                }
                TuiAction::Stop(name) => {
                    let id = ResourceId::new(&name);
                    if let Some(resource) = orchestrator.get_resource(&id) {
                        let mut resource = resource.write().await;
                        let ctx = Context::new();
                        match resource.stop(&ctx).await {
                            Ok(()) => app.set_status(format!("Stopped {}", name)),
                            Err(e) => app.set_status(format!("Failed: {}", e)),
                        }
                    } else {
                        app.set_status(format!("Resource {} not found", name));
                    }
                }
                TuiAction::Reload => match ConfigStore::discover_and_load() {
                    Ok(store) => {
                        let mut added = Vec::new();
                        let mut removed = Vec::new();

                        let current_names: std::collections::HashSet<_> =
                            app.service_infos().iter().map(|s| s.name.clone()).collect();
                        let config_names: std::collections::HashSet<_> = store
                            .list_enabled_resources()
                            .map(|(n, _)| n.to_string())
                            .collect();

                        for (name, rc) in store.list_enabled_resources() {
                            if !current_names.contains(name) {
                                let svc = Service::from_resource_config(name, rc);
                                let mut config = service_to_config(&svc, &project_dir);
                                if let Some(ref env) = nix_env {
                                    let user_env = std::mem::take(&mut config.environment);
                                    config.environment = env.clone();
                                    config.environment.extend(user_env);
                                }
                                orchestrator.add_resource(config);
                                app.add_service(name.to_string());
                                added.push(name.to_string());
                            }
                        }

                        for name in &current_names {
                            if !config_names.contains(name.as_str()) {
                                app.remove_service(name);
                                removed.push(name.clone());
                            }
                        }

                        for name in &added {
                            let id = ResourceId::new(name);
                            if let Some(resource) = orchestrator.get_resource(&id) {
                                let mut resource = resource.write().await;
                                let ctx = Context::new();
                                if let Err(e) = resource.start(&ctx).await {
                                    app.set_status(format!("Failed to start {}: {}", name, e));
                                }
                            }
                        }

                        let mut msg = String::new();
                        if !added.is_empty() {
                            msg.push_str(&format!("Added: {}", added.join(", ")));
                        }
                        if !removed.is_empty() {
                            if !msg.is_empty() {
                                msg.push_str(" | ");
                            }
                            msg.push_str(&format!("Removed: {}", removed.join(", ")));
                        }
                        if msg.is_empty() {
                            msg = "Config reloaded (no changes)".to_string();
                        }
                        app.set_status(msg);
                    }
                    Err(e) => {
                        app.set_status(format!("Reload failed: {}", e));
                    }
                },
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Stop all services via Orchestrator
    let mut stop_step = out.step("stop");
    stop_step.animate("shutting down");
    orchestrator
        .stop_all()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    stop_step.done("all services stopped");

    Ok(())
}
