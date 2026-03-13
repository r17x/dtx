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
use std::collections::HashMap;
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
    pub is_stderr: bool,
}

/// Service info for display (derived from events).
pub struct ServiceDisplayInfo {
    pub name: String,
    pub state: DisplayState,
    pub restarts: u32,
    pub health: DisplayHealth,
    pub port: Option<u16>,
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

/// Health status for display (derived from health check events).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum DisplayHealth {
    #[default]
    Unknown,
    Healthy,
    Unhealthy {
        reason: String,
    },
}

/// Log scroll state for PgUp/PgDn navigation.
pub struct LogScroll {
    pub offset_from_bottom: usize,
    pub following: bool,
}

impl LogScroll {
    pub fn new() -> Self {
        Self {
            offset_from_bottom: 0,
            following: true,
        }
    }

    pub fn scroll_up(&mut self, lines: usize, total: usize) {
        self.following = false;
        self.offset_from_bottom = (self.offset_from_bottom + lines).min(total.saturating_sub(1));
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.offset_from_bottom = self.offset_from_bottom.saturating_sub(lines);
        if self.offset_from_bottom == 0 {
            self.following = true;
        }
    }

    pub fn jump_to_bottom(&mut self) {
        self.offset_from_bottom = 0;
        self.following = true;
    }
}

/// TUI interaction mode.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UiMode {
    #[default]
    Normal,
    Search {
        query: String,
        cursor: usize,
    },
    Filter {
        query: String,
        cursor: usize,
    },
    Detail,
    Confirm {
        action: ConfirmAction,
        message: String,
    },
    Help,
    Wizard(Box<super::wizard::WizardState>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    Delete(String),
}

/// Search state for log search results.
pub struct SearchState {
    pub query: String,
    pub matches: Vec<usize>,
    pub current_match: usize,
}

#[derive(Clone, Debug)]
pub struct ServiceDetail {
    pub name: String,
    pub state: DisplayState,
    pub health: DisplayHealth,
    pub port: Option<u16>,
    pub uptime: Option<Duration>,
    pub restart_count: u32,
    pub command: Option<String>,
    pub dependencies: Vec<String>,
}

/// TUI application state.
pub struct App {
    /// Service names (ordered).
    service_names: Vec<String>,
    /// Service states (updated from events).
    service_states: HashMap<String, DisplayState>,
    /// Restart counts per service.
    restart_counts: HashMap<String, u32>,
    /// File-backed log store with in-memory recent buffer.
    pub log_store: super::logs::LogStore,
    /// Currently selected service index.
    pub selected: usize,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Status message.
    pub status_message: Option<String>,
    /// Config changed flag (set by ConfigChanged event).
    pub config_changed: bool,
    /// Health states per service (updated from health check events).
    health_states: HashMap<String, DisplayHealth>,
    /// Port numbers per service.
    service_ports: HashMap<String, u16>,
    /// Current UI mode.
    pub mode: UiMode,
    /// Log scroll state.
    pub log_scroll: LogScroll,
    /// Search state for log search.
    pub search_state: Option<SearchState>,
    /// Active log filter (persistent text filter).
    pub active_filter: Option<String>,
    /// Service detail view data.
    pub detail: Option<ServiceDetail>,
    /// Track when services started (for uptime).
    started_at: HashMap<String, Instant>,
}

impl App {
    pub fn new(service_names: Vec<String>, log_dir: Option<PathBuf>) -> Self {
        let mut service_states = HashMap::new();
        let mut restart_counts = HashMap::new();
        for name in &service_names {
            service_states.insert(name.clone(), DisplayState::Pending);
            restart_counts.insert(name.clone(), 0);
        }
        let health_states = service_names
            .iter()
            .map(|n| (n.clone(), DisplayHealth::Unknown))
            .collect();

        Self {
            service_names,
            service_states,
            restart_counts,
            log_store: match log_dir {
                Some(dir) => super::logs::LogStore::new(dir, 2000),
                None => super::logs::LogStore::memory_only(2000),
            },
            selected: 0,
            should_quit: false,
            status_message: None,
            config_changed: false,
            health_states,
            service_ports: HashMap::new(),
            mode: UiMode::Normal,
            log_scroll: LogScroll::new(),
            search_state: None,
            active_filter: None,
            detail: None,
            started_at: HashMap::new(),
        }
    }

    /// Set port for a service.
    pub fn set_port(&mut self, name: &str, port: u16) {
        self.service_ports.insert(name.to_string(), port);
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
                health: self.health(name).clone(),
                port: self.service_ports.get(name).copied(),
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
                self.started_at
                    .entry(name.clone())
                    .or_insert_with(Instant::now);
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
                        .insert(name.clone(), DisplayState::Completed { exit_code: code });
                } else {
                    self.service_states
                        .insert(name.clone(), DisplayState::Stopped);
                }
                self.started_at.remove(&name);
            }
            LifecycleEvent::Failed { id, error, .. } => {
                let name = id.to_string();
                self.service_states
                    .insert(name.clone(), DisplayState::Failed { error: Some(error) });
                self.started_at.remove(&name);
            }
            LifecycleEvent::Restarting { id, attempt, .. } => {
                let name = id.to_string();
                self.restart_counts.insert(name.clone(), attempt);
                self.service_states.insert(name, DisplayState::Starting);
            }
            LifecycleEvent::Log {
                id, stream, line, ..
            } => {
                self.log_store.append(DisplayLog {
                    service: id.to_string(),
                    content: line,
                    is_stderr: matches!(stream, dtx_core::resource::LogStreamKind::Stderr),
                });

                if self.log_scroll.following {
                    self.log_scroll.offset_from_bottom = 0;
                }
            }
            LifecycleEvent::ConfigChanged { .. } => {
                self.config_changed = true;
                self.status_message = Some("Config changed — press 'a' to reload".to_string());
            }
            LifecycleEvent::HealthCheckPassed { id, .. } => {
                self.health_states
                    .insert(id.to_string(), DisplayHealth::Healthy);
            }
            LifecycleEvent::HealthCheckFailed { id, reason, .. } => {
                self.health_states
                    .insert(id.to_string(), DisplayHealth::Unhealthy { reason });
            }
            LifecycleEvent::DependencyWaiting { .. }
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

    /// Count filtered logs for the currently selected service.
    pub fn filtered_log_count(&self) -> usize {
        let service = self.selected_service();
        match &self.active_filter {
            Some(filter) => self
                .log_store
                .filtered_count_with_predicate(service, filter),
            None => self.log_store.filtered_count(service),
        }
    }

    /// Get currently selected service name.
    pub fn selected_service(&self) -> Option<&str> {
        self.service_names.get(self.selected).map(|s| s.as_str())
    }

    /// Gather detail info for the currently selected service.
    pub fn gather_detail(&mut self) {
        let name = match self.selected_service() {
            Some(n) => n.to_string(),
            None => return,
        };
        let state = self
            .service_states
            .get(&name)
            .cloned()
            .unwrap_or(DisplayState::Pending);
        let uptime = self.started_at.get(&name).map(|t| t.elapsed());

        self.detail = Some(ServiceDetail {
            name: name.clone(),
            state,
            health: self.health(&name).clone(),
            port: self.service_ports.get(&name).copied(),
            uptime,
            restart_count: *self.restart_counts.get(&name).unwrap_or(&0),
            command: None,
            dependencies: Vec::new(),
        });
    }

    /// Handle keyboard input (returns action to take).
    pub fn handle_key(&mut self, key: KeyCode) -> Option<TuiAction> {
        match &self.mode {
            UiMode::Normal => self.handle_key_normal(key),
            UiMode::Search { .. } => self.handle_key_search(key),
            UiMode::Filter { .. } => self.handle_key_filter(key),
            UiMode::Detail => self.handle_key_detail(key),
            UiMode::Confirm { .. } => self.handle_key_confirm(key),
            UiMode::Help => self.handle_key_help(key),
            UiMode::Wizard(_) => self.handle_key_wizard(key),
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
            KeyCode::Char('r') | KeyCode::Char('s') => return self.handle_service_action(key),
            KeyCode::Char('a') if self.config_changed => {
                self.config_changed = false;
                self.status_message = Some("Reloading config...".to_string());
                Some(TuiAction::Reload)
            }
            KeyCode::Char('a') => {
                let available = self.service_names.clone();
                self.mode =
                    UiMode::Wizard(Box::new(super::wizard::WizardState::new_add(available)));
                None
            }
            KeyCode::Char('e') => {
                if let Some(name) = self.selected_service().map(|s| s.to_string()) {
                    let available: Vec<String> = self
                        .service_names
                        .iter()
                        .filter(|n| **n != name)
                        .cloned()
                        .collect();
                    // Load current values from ConfigStore, fall back to local state
                    let fallback_port = self.service_ports.get(&name).copied();
                    let (command, port, deps) = ConfigStore::discover_and_load()
                        .ok()
                        .and_then(|store| store.get_resource(&name).map(|rc| {
                            let cmd = rc.command.clone().unwrap_or_default();
                            let d: Vec<String> =
                                rc.depends_on.iter().map(|d| d.name().to_string()).collect();
                            (cmd, rc.port, d)
                        }))
                        .unwrap_or_else(|| (String::new(), fallback_port, Vec::new()));
                    self.mode = UiMode::Wizard(Box::new(super::wizard::WizardState::new_edit(
                        &name, &command, port, deps, available,
                    )));
                }
                None
            }
            KeyCode::PageUp => {
                let height = 20;
                let total = self.filtered_log_count();
                self.log_scroll.scroll_up(height, total);
                None
            }
            KeyCode::PageDown => {
                self.log_scroll.scroll_down(20);
                None
            }
            KeyCode::Char('c') => {
                self.log_store.clear(None);
                self.log_scroll.jump_to_bottom();
                self.status_message = Some("Logs cleared".to_string());
                None
            }
            KeyCode::Char('/') => {
                self.mode = UiMode::Search {
                    query: String::new(),
                    cursor: 0,
                };
                None
            }
            KeyCode::Char('n') => {
                self.next_match();
                None
            }
            KeyCode::Char('N') => {
                self.prev_match();
                None
            }
            KeyCode::Char('F') => {
                let initial = self.active_filter.clone().unwrap_or_default();
                let cursor = initial.len();
                self.mode = UiMode::Filter {
                    query: initial,
                    cursor,
                };
                None
            }
            KeyCode::Char('S') => return self.handle_service_action(key),
            KeyCode::Char('g') => {
                self.selected = 0;
                None
            }
            KeyCode::Char('G') => {
                self.selected = self.service_names.len().saturating_sub(1);
                None
            }
            KeyCode::Char('d') => {
                if let Some(name) = self.selected_service().map(|s| s.to_string()) {
                    self.mode = UiMode::Confirm {
                        action: ConfirmAction::Delete(name.clone()),
                        message: format!("Delete service '{}'?", name),
                    };
                }
                None
            }
            KeyCode::Enter => {
                self.gather_detail();
                self.mode = UiMode::Detail;
                None
            }
            KeyCode::Char('?') => {
                self.mode = UiMode::Help;
                None
            }
            _ => None,
        }
    }

    fn handle_key_help(&mut self, key: KeyCode) -> Option<TuiAction> {
        match key {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = UiMode::Normal;
                None
            }
            _ => None,
        }
    }

    fn handle_key_wizard(&mut self, key: KeyCode) -> Option<TuiAction> {
        use super::wizard::WizardAction;

        if let UiMode::Wizard(ref mut state) = self.mode {
            match state.handle_key(key) {
                WizardAction::Continue => None,
                WizardAction::Cancel => {
                    self.mode = UiMode::Normal;
                    self.status_message = Some("Wizard cancelled".to_string());
                    None
                }
                WizardAction::Complete(result) => {
                    let is_edit = state.is_edit;
                    let original_name = state.original_name.clone();
                    self.mode = UiMode::Normal;
                    if is_edit {
                        let name = original_name.unwrap_or_else(|| result.name.clone());
                        self.status_message = Some(format!("Updating {}...", name));
                        Some(TuiAction::EditService(name, result))
                    } else {
                        self.status_message = Some(format!("Adding {}...", result.name));
                        Some(TuiAction::AddService(result))
                    }
                }
            }
        } else {
            None
        }
    }

    fn handle_key_confirm(&mut self, key: KeyCode) -> Option<TuiAction> {
        match key {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let UiMode::Confirm { action, .. } =
                    std::mem::replace(&mut self.mode, UiMode::Normal)
                {
                    match action {
                        ConfirmAction::Delete(name) => {
                            self.status_message = Some(format!("Deleting {}...", name));
                            return Some(TuiAction::Delete(name));
                        }
                    }
                }
                None
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.mode = UiMode::Normal;
                self.status_message = Some("Cancelled".to_string());
                None
            }
            _ => None,
        }
    }

    /// Edit text input in the current mode (shared by Search and Filter).
    /// Returns true if the key was handled as a text-editing key.
    fn handle_text_input(&mut self, key: KeyCode) -> bool {
        let (query, cursor) = match self.mode {
            UiMode::Search {
                ref mut query,
                ref mut cursor,
            }
            | UiMode::Filter {
                ref mut query,
                ref mut cursor,
            } => (query, cursor),
            _ => return false,
        };
        match key {
            KeyCode::Backspace => {
                if *cursor > 0 {
                    query.remove(*cursor - 1);
                    *cursor -= 1;
                }
                true
            }
            KeyCode::Char(c) => {
                query.insert(*cursor, c);
                *cursor += 1;
                true
            }
            KeyCode::Left => {
                *cursor = cursor.saturating_sub(1);
                true
            }
            KeyCode::Right => {
                if *cursor < query.len() {
                    *cursor += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn handle_key_search(&mut self, key: KeyCode) -> Option<TuiAction> {
        match key {
            KeyCode::Enter => {
                self.execute_search();
                None
            }
            KeyCode::Esc => {
                self.mode = UiMode::Normal;
                None
            }
            _ => {
                self.handle_text_input(key);
                None
            }
        }
    }

    fn handle_key_filter(&mut self, key: KeyCode) -> Option<TuiAction> {
        match key {
            KeyCode::Enter => {
                if let UiMode::Filter { ref query, .. } = self.mode {
                    if query.is_empty() {
                        self.active_filter = None;
                        self.status_message = Some("Filter cleared".to_string());
                    } else {
                        self.active_filter = Some(query.clone());
                        self.status_message = Some(format!("Filter: {}", query));
                    }
                }
                self.log_scroll.jump_to_bottom();
                self.mode = UiMode::Normal;
                None
            }
            KeyCode::Esc => {
                self.active_filter = None;
                self.status_message = Some("Filter cleared".to_string());
                self.log_scroll.jump_to_bottom();
                self.mode = UiMode::Normal;
                None
            }
            _ => {
                self.handle_text_input(key);
                None
            }
        }
    }

    /// Handle service control keys (r/s/S) — shared by Normal and Detail modes.
    fn handle_service_action(&mut self, key: KeyCode) -> Option<TuiAction> {
        match key {
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
            KeyCode::Char('S') => {
                if let Some(name) = self.selected_service().map(|s| s.to_string()) {
                    if matches!(
                        self.service_states.get(&name),
                        Some(
                            DisplayState::Stopped
                                | DisplayState::Completed { .. }
                                | DisplayState::Failed { .. }
                        )
                    ) {
                        self.status_message = Some(format!("Starting {}...", name));
                        return Some(TuiAction::Start(name));
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn handle_key_detail(&mut self, key: KeyCode) -> Option<TuiAction> {
        match key {
            KeyCode::Esc => {
                self.detail = None;
                self.mode = UiMode::Normal;
                None
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.gather_detail();
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected < self.service_names.len().saturating_sub(1) {
                    self.selected += 1;
                    self.gather_detail();
                }
                None
            }
            KeyCode::Char('r') | KeyCode::Char('s') | KeyCode::Char('S') => {
                self.handle_service_action(key)
            }
            _ => None,
        }
    }

    fn execute_search(&mut self) {
        let query = match &self.mode {
            UiMode::Search { query, .. } => query.clone(),
            _ => return,
        };
        if query.is_empty() {
            self.search_state = None;
            self.mode = UiMode::Normal;
            return;
        }
        let query_lower = query.to_lowercase();
        let selected_service = self.selected_service().map(|s| s.to_string());
        let matches: Vec<usize> = self
            .log_store
            .get_visible(selected_service.as_deref(), 0, usize::MAX)
            .iter()
            .enumerate()
            .filter(|(_, log)| log.content.to_lowercase().contains(&query_lower))
            .map(|(i, _)| i)
            .collect();

        if matches.is_empty() {
            self.status_message = Some(format!("No matches for '{}'", query));
            self.search_state = None;
        } else {
            let count = matches.len();
            self.search_state = Some(SearchState {
                query,
                matches,
                current_match: 0,
            });
            self.status_message = Some(format!("{} match(es) found", count));
            self.jump_to_current_match();
        }
        self.mode = UiMode::Normal;
    }

    fn jump_to_current_match(&mut self) {
        if let Some(ref state) = self.search_state {
            if let Some(&match_idx) = state.matches.get(state.current_match) {
                let total = self.filtered_log_count();
                self.log_scroll.offset_from_bottom = total.saturating_sub(match_idx + 1);
                self.log_scroll.following = self.log_scroll.offset_from_bottom == 0;
            }
        }
    }

    fn next_match(&mut self) {
        if let Some(ref mut state) = self.search_state {
            if !state.matches.is_empty() {
                state.current_match = (state.current_match + 1) % state.matches.len();
                let current = state.current_match;
                let total_matches = state.matches.len();
                self.status_message = Some(format!("Match {}/{}", current + 1, total_matches));
            }
        }
        self.jump_to_current_match();
    }

    fn prev_match(&mut self) {
        if let Some(ref mut state) = self.search_state {
            if !state.matches.is_empty() {
                state.current_match = if state.current_match == 0 {
                    state.matches.len() - 1
                } else {
                    state.current_match - 1
                };
                let current = state.current_match;
                let total_matches = state.matches.len();
                self.status_message = Some(format!("Match {}/{}", current + 1, total_matches));
            }
        }
        self.jump_to_current_match();
    }

    /// Get health status for a service.
    pub fn health(&self, name: &str) -> &DisplayHealth {
        static UNKNOWN: DisplayHealth = DisplayHealth::Unknown;
        self.health_states.get(name).unwrap_or(&UNKNOWN)
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
    Start(String),
    Delete(String),
    Reload,
    AddService(super::wizard::WizardResult),
    EditService(String, super::wizard::WizardResult),
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

/// Build a ResourceConfig from a WizardResult (used by AddService and EditService).
fn wizard_result_to_resource_config(
    result: &super::wizard::WizardResult,
) -> dtx_core::config::schema::ResourceConfig {
    use dtx_core::config::schema::DependencyConfig;
    let mut rc = dtx_core::config::schema::ResourceConfig {
        command: Some(result.command.clone()),
        port: result.port,
        ..Default::default()
    };
    if !result.deps.is_empty() {
        rc.depends_on = result
            .deps
            .iter()
            .map(|d| DependencyConfig::Simple(d.clone()))
            .collect();
    }
    rc
}

/// Merge nix env into a ProcessResourceConfig (user env takes precedence).
fn apply_nix_env(config: &mut ProcessResourceConfig, nix_env: &HashMap<String, String>) {
    let user_env = std::mem::take(&mut config.environment);
    config.environment = nix_env.clone();
    config.environment.extend(user_env);
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
            apply_nix_env(&mut config, env);
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
    let dtx_dir = project_dir.join(".dtx");
    let log_dir = dtx_dir.join("logs");
    let mut app = App::new(service_names, Some(log_dir));

    for service in &enabled_services {
        if let Some(port) = service.port {
            app.set_port(&service.name, port);
        }
    }

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
    let mut tick_count: u64 = 0;

    loop {
        // Poll events from ResourceEventBus
        app.poll_events(&mut subscriber);

        // Poll all processes for output (they publish to EventBus)
        orchestrator.poll().await;

        // Sync state from orchestrator (every ~1s, not every tick)
        tick_count += 1;
        if tick_count % 10 == 0 {
            app.sync_from_orchestrator(&orchestrator).await;
        }

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
                TuiAction::Start(name) => {
                    let id = ResourceId::new(&name);
                    if let Some(resource) = orchestrator.get_resource(&id) {
                        let mut resource = resource.write().await;
                        let ctx = Context::new();
                        match resource.start(&ctx).await {
                            Ok(()) => app.set_status(format!("Started {}", name)),
                            Err(e) => app.set_status(format!("Failed to start: {}", e)),
                        }
                    } else {
                        app.set_status(format!("Resource {} not found", name));
                    }
                }
                TuiAction::Delete(name) => {
                    let id = ResourceId::new(&name);
                    if let Some(resource) = orchestrator.get_resource(&id) {
                        let mut resource = resource.write().await;
                        let ctx = Context::new();
                        let _ = resource.stop(&ctx).await;
                    }

                    match ConfigStore::discover_and_load() {
                        Ok(mut store) => match store.remove_resource(&name) {
                            Ok(_) => {
                                if let Err(e) = store.save() {
                                    app.set_status(format!("Failed to save: {}", e));
                                } else {
                                    app.remove_service(&name);
                                    app.set_status(format!("Deleted {}", name));
                                }
                            }
                            Err(e) => {
                                app.set_status(format!("Failed to delete: {}", e))
                            }
                        },
                        Err(e) => {
                            app.set_status(format!("Failed to load config: {}", e))
                        }
                    }
                }
                TuiAction::AddService(result) => {
                    match ConfigStore::discover_and_load() {
                        Ok(mut store) => {
                            let rc = wizard_result_to_resource_config(&result);
                            if let Err(e) = store.add_resource(&result.name, rc) {
                                app.set_status(format!("Failed to add: {}", e));
                            } else if let Err(e) = store.save() {
                                app.set_status(format!("Failed to save: {}", e));
                            } else {
                                let svc = dtx_core::model::Service {
                                    name: result.name.clone(),
                                    command: result.command.clone(),
                                    package: None,
                                    port: result.port,
                                    working_dir: None,
                                    environment: None,
                                    depends_on: if result.deps.is_empty() {
                                        None
                                    } else {
                                        Some(
                                            result
                                                .deps
                                                .iter()
                                                .map(|d| dtx_core::model::Dependency {
                                                    service: d.clone(),
                                                    condition:
                                                        dtx_core::model::DependencyCondition::ProcessStarted,
                                                })
                                                .collect(),
                                        )
                                    },
                                    health_check: None,
                                    shutdown_command: None,
                                    enabled: true,
                                };
                                let mut config = service_to_config(&svc, &project_dir);
                                if let Some(ref env) = nix_env {
                                    apply_nix_env(&mut config, env);
                                }
                                orchestrator.add_resource(config);
                                app.add_service(result.name.clone());
                                if let Some(port) = result.port {
                                    app.set_port(&result.name, port);
                                }

                                let rid = ResourceId::new(&result.name);
                                if let Some(resource) = orchestrator.get_resource(&rid) {
                                    let mut resource = resource.write().await;
                                    let ctx = Context::new();
                                    if let Err(e) = resource.start(&ctx).await {
                                        app.set_status(format!(
                                            "Added {} but failed to start: {}",
                                            result.name, e
                                        ));
                                    } else {
                                        app.set_status(format!(
                                            "Added and started {}",
                                            result.name
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to load config: {}", e))
                        }
                    }
                }
                TuiAction::EditService(original_name, result) => {
                    match ConfigStore::discover_and_load() {
                        Ok(mut store) => {
                            let _ = store.remove_resource(&original_name);
                            let rc = wizard_result_to_resource_config(&result);
                            if let Err(e) = store.add_resource(&result.name, rc) {
                                app.set_status(format!("Failed to update: {}", e));
                            } else if let Err(e) = store.save() {
                                app.set_status(format!("Failed to save: {}", e));
                            } else {
                                app.set_status(format!(
                                    "Updated {} — restart to apply",
                                    original_name
                                ));
                            }
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to load config: {}", e))
                        }
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
                                    apply_nix_env(&mut config, env);
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
