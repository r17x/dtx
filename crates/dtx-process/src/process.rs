//! ProcessResource - Implementation of Resource trait for OS processes.

use async_trait::async_trait;
use chrono::Utc;
use std::any::Any;
use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tracing::{debug, error, info, warn};

use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::resource::{
    Context, HealthStatus, LogEntry, LogStream, LogStreamKind, Resource, ResourceError, ResourceId,
    ResourceKind, ResourceResult, ResourceState,
};

use crate::config::{ProcessResourceConfig, RestartPolicy, Signal};
use crate::ProbeRunner;

/// Log buffer capacity.
const LOG_BUFFER_CAPACITY: usize = 1000;

/// A process resource implementing the Resource trait.
///
/// ProcessResource manages an OS process with support for:
/// - Environment variables and working directory
/// - Configurable shutdown (signal + timeout)
/// - Restart policies with backoff
/// - Health probes (readiness + liveness)
/// - Log capture (stdout + stderr)
pub struct ProcessResource {
    /// Configuration.
    config: ProcessResourceConfig,
    /// Current state.
    state: ResourceState,
    /// Running child process.
    child: Option<Child>,
    /// Event bus for publishing lifecycle events.
    event_bus: Arc<ResourceEventBus>,
    /// Restart state tracking.
    restart_state: RestartState,
    /// Probe runner for health checks.
    probe_runner: Option<ProbeRunner>,
    /// Captured logs.
    logs: VecDeque<LogEntry>,
    /// Buffered reader for stdout.
    stdout_reader: Option<tokio::io::BufReader<tokio::process::ChildStdout>>,
    /// Buffered reader for stderr.
    stderr_reader: Option<tokio::io::BufReader<tokio::process::ChildStderr>>,
    /// When the process started running healthily (for restart counter reset).
    restart_healthy_since: Option<std::time::Instant>,
}

/// Tracks restart attempts for backoff calculation.
#[derive(Clone, Debug, Default)]
struct RestartState {
    /// Number of restart attempts.
    attempts: u32,
    /// Whether a restart is scheduled.
    scheduled: bool,
    /// When the next restart should occur.
    next_restart_at: Option<std::time::Instant>,
}

impl RestartState {
    fn increment(&mut self) {
        self.attempts += 1;
    }

    fn reset(&mut self) {
        self.attempts = 0;
        self.scheduled = false;
        self.next_restart_at = None;
    }
}

/// Strip ANSI escape sequences and normalize control characters.
///
/// Handles CSI sequences (`ESC[...X`), OSC sequences (`ESC]...ST`),
/// simple two-byte escapes (`ESC X`), and carriage returns (keeps only
/// content after the last `\r` to simulate terminal overwrite behavior).
fn strip_ansi(s: &str) -> String {
    // Handle \r first: keep only content after the last \r per line
    let s = if let Some(pos) = s.rfind('\r') {
        &s[pos + 1..]
    } else {
        s
    };

    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    // CSI: consume until final byte (0x40..=0x7E)
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: consume until ST (ESC \ or BEL)
                    let mut prev = '\0';
                    for c in chars.by_ref() {
                        if c == '\x07' || (prev == '\x1b' && c == '\\') {
                            break;
                        }
                        prev = c;
                    }
                }
                Some(_) => {} // two-byte escape, already consumed
                None => break,
            }
        } else if c.is_ascii_control() && c != '\t' && c != '\n' {
            continue;
        } else {
            out.push(c);
        }
    }
    out
}

/// Drain available complete lines from an async reader without blocking.
///
/// Only reads when a complete line (`\n`) is available in the buffer.
/// This prevents partial reads from consuming data that would be lost
/// on the next call (since `read_line` consumes from BufReader's internal
/// buffer even when it returns Pending).
fn drain_reader<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut Option<tokio::io::BufReader<R>>,
    stream_kind: LogStreamKind,
    logs: &mut Vec<LogEntry>,
) {
    use tokio::io::AsyncBufReadExt;

    let reader = match reader.as_mut() {
        Some(r) => r,
        None => return,
    };

    let mut line = String::new();
    loop {
        // Peek at the buffer — only proceed if a complete line is available.
        let has_newline = {
            let pinned = std::pin::pin!(reader.fill_buf());
            match futures_lite::future::block_on(futures_lite::future::poll_once(pinned)) {
                Some(Ok(buf)) if !buf.is_empty() => buf.contains(&b'\n'),
                _ => false,
            }
        };

        if !has_newline {
            break;
        }

        line.clear();
        let pinned_read = std::pin::pin!(reader.read_line(&mut line));
        if let Some(Ok(n)) =
            futures_lite::future::block_on(futures_lite::future::poll_once(pinned_read))
        {
            if n > 0 {
                logs.push(LogEntry {
                    timestamp: Utc::now(),
                    stream: stream_kind,
                    line: strip_ansi(line.trim_end()),
                });
            }
        } else {
            // Should not happen since we verified \n exists, but be safe
            break;
        }
    }
}

impl ProcessResource {
    /// Create a new process resource.
    pub fn new(config: ProcessResourceConfig, event_bus: Arc<ResourceEventBus>) -> Self {
        Self {
            config,
            state: ResourceState::Pending,
            child: None,
            event_bus,
            restart_state: RestartState::default(),
            probe_runner: None,
            logs: VecDeque::with_capacity(LOG_BUFFER_CAPACITY),
            stdout_reader: None,
            stderr_reader: None,
            restart_healthy_since: None,
        }
    }

    /// Get the process configuration.
    pub fn config(&self) -> &ProcessResourceConfig {
        &self.config
    }

    /// Get the child process if running.
    pub fn child(&self) -> Option<&Child> {
        self.child.as_ref()
    }

    /// Check if a restart should be attempted.
    pub fn should_restart(&self) -> bool {
        match &self.config.restart {
            RestartPolicy::No => false,
            RestartPolicy::Always { max_retries, .. }
            | RestartPolicy::OnFailure { max_retries, .. } => {
                if let Some(max) = max_retries {
                    self.restart_state.attempts < *max
                } else {
                    true
                }
            }
        }
    }

    /// Schedule a restart with backoff.
    pub fn schedule_restart(&mut self) {
        if let Some(backoff) = self.config.restart.backoff() {
            let delay = backoff.delay_for_attempt(self.restart_state.attempts);
            self.restart_state.next_restart_at = Some(std::time::Instant::now() + delay);
            self.restart_state.scheduled = true;

            info!(
                id = %self.config.id,
                attempt = self.restart_state.attempts + 1,
                delay_secs = delay.as_secs_f64(),
                "Scheduling restart"
            );
        }
    }

    /// Check if it's time to execute a scheduled restart.
    pub fn is_restart_due(&self) -> bool {
        if !self.restart_state.scheduled {
            return false;
        }
        self.restart_state
            .next_restart_at
            .map(|t| std::time::Instant::now() >= t)
            .unwrap_or(false)
    }

    /// Execute a scheduled restart.
    pub async fn execute_restart(&mut self, ctx: &Context) -> ResourceResult<()> {
        self.restart_state.increment();
        self.restart_state.scheduled = false;
        self.restart_state.next_restart_at = None;

        // Publish restart event
        self.event_bus.publish(LifecycleEvent::Restarting {
            id: self.config.id.clone(),
            kind: ResourceKind::Process,
            attempt: self.restart_state.attempts,
            max_attempts: self.config.restart.max_retries(),
            timestamp: Utc::now(),
        });

        self.start(ctx).await
    }

    /// Poll the process for output and status.
    ///
    /// This should be called periodically to capture logs and detect
    /// process exit.
    pub fn poll(&mut self) -> Option<i32> {
        // Collect log entries first to avoid borrow conflicts
        let mut pending_logs: Vec<LogEntry> = Vec::new();

        // Read stdout and stderr non-blocking
        drain_reader(
            &mut self.stdout_reader,
            LogStreamKind::Stdout,
            &mut pending_logs,
        );
        drain_reader(
            &mut self.stderr_reader,
            LogStreamKind::Stderr,
            &mut pending_logs,
        );

        // Now add all collected logs
        for entry in pending_logs {
            self.add_log(entry);
        }

        // Handle restart counter reset after sustained healthy operation (30 seconds)
        const RESTART_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(30);
        if self.state.is_running() {
            if self.restart_healthy_since.is_none() {
                self.restart_healthy_since = Some(std::time::Instant::now());
            }

            if let Some(since) = self.restart_healthy_since {
                if since.elapsed() >= RESTART_GRACE_PERIOD && self.restart_state.attempts > 0 {
                    debug!(
                        id = %self.config.id,
                        attempts = self.restart_state.attempts,
                        "Resetting restart counter after grace period"
                    );
                    self.restart_state.reset();
                    self.restart_healthy_since = None;
                }
            }
        }

        // Check if process has exited
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let exit_code = status.code();
                    let started_at = self.state.started_at().unwrap_or_else(Utc::now);

                    // Clear the healthy timer on exit
                    self.restart_healthy_since = None;

                    // Clear stdout/stderr readers
                    self.stdout_reader = None;
                    self.stderr_reader = None;

                    if status.success() {
                        self.state = ResourceState::Stopped {
                            exit_code,
                            started_at,
                            stopped_at: Utc::now(),
                        };
                        self.event_bus.publish(LifecycleEvent::Stopped {
                            id: self.config.id.clone(),
                            kind: ResourceKind::Process,
                            exit_code,
                            timestamp: Utc::now(),
                        });
                    } else {
                        let error = match exit_code {
                            Some(code) => format!("exit code {}", code),
                            None => "terminated by signal".to_string(),
                        };
                        debug!(
                            id = %self.config.id,
                            exit_code = ?exit_code,
                            restart_attempts = self.restart_state.attempts,
                            "Process failed"
                        );
                        self.state = ResourceState::Failed {
                            error: error.clone(),
                            exit_code,
                            started_at: Some(started_at),
                            failed_at: Utc::now(),
                        };
                        self.event_bus.publish(LifecycleEvent::Failed {
                            id: self.config.id.clone(),
                            kind: ResourceKind::Process,
                            error,
                            exit_code,
                            timestamp: Utc::now(),
                        });
                    }

                    self.child = None;
                    return exit_code;
                }
                Ok(None) => {
                    // Still running
                }
                Err(e) => {
                    warn!(id = %self.config.id, error = %e, "Failed to check process status");
                }
            }
        }
        None
    }

    /// Spawn the main process.
    async fn spawn_process(&mut self) -> ResourceResult<()> {
        let working_dir = self.config.effective_working_dir();
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&self.config.command)
            .current_dir(&working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Set environment
        for (key, value) in &self.config.environment {
            cmd.env(key, value);
        }

        // Isolate the child into its own process group so signals
        // (SIGTERM, SIGKILL) reach the entire tree of descendants.
        #[cfg(unix)]
        {
            // SAFETY: setpgid is async-signal-safe per POSIX
            unsafe {
                cmd.pre_exec(|| {
                    libc::setpgid(0, 0);
                    Ok(())
                });
            }
        }

        let mut child = cmd.spawn().map_err(|e| {
            Box::new(std::io::Error::other(format!(
                "Failed to spawn process: {}",
                e
            ))) as ResourceError
        })?;

        let pid = child.id();

        // Capture stdout/stderr for log streaming
        if let Some(stdout) = child.stdout.take() {
            self.stdout_reader = Some(tokio::io::BufReader::new(stdout));
        }
        if let Some(stderr) = child.stderr.take() {
            self.stderr_reader = Some(tokio::io::BufReader::new(stderr));
        }

        self.child = Some(child);
        self.state = ResourceState::Running {
            pid,
            started_at: Utc::now(),
        };

        // Publish running event
        self.event_bus.publish(LifecycleEvent::Running {
            id: self.config.id.clone(),
            kind: ResourceKind::Process,
            pid,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Send a signal to the process.
    #[cfg(unix)]
    fn send_signal(&self, signal: Signal) -> ResourceResult<()> {
        if let Some(ref child) = self.child {
            if let Some(pid) = child.id() {
                let sig = signal.as_libc();
                unsafe {
                    if libc::kill(-(pid as i32), sig) != 0 {
                        return Err(Box::new(std::io::Error::last_os_error()));
                    }
                }
            }
        }
        Ok(())
    }

    /// Add a log entry.
    fn add_log(&mut self, entry: LogEntry) {
        if self.logs.len() >= LOG_BUFFER_CAPACITY {
            self.logs.pop_front();
        }

        // Publish log event
        self.event_bus.publish(LifecycleEvent::Log {
            id: self.config.id.clone(),
            stream: entry.stream,
            line: entry.line.clone(),
            timestamp: entry.timestamp,
        });

        self.logs.push_back(entry);
    }
}

#[async_trait]
impl Resource for ProcessResource {
    fn id(&self) -> &ResourceId {
        &self.config.id
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::Process
    }

    fn state(&self) -> &ResourceState {
        &self.state
    }

    async fn start(&mut self, _ctx: &Context) -> ResourceResult<()> {
        // Check current state
        if self.state.is_running() {
            return Ok(());
        }

        info!(id = %self.config.id, cmd = %self.config.command, "Starting process");

        // Transition to Starting
        self.state = ResourceState::Starting {
            started_at: Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Starting {
            id: self.config.id.clone(),
            kind: ResourceKind::Process,
            timestamp: Utc::now(),
        });

        // Spawn the main process
        if let Err(e) = self.spawn_process().await {
            let error = format!("Failed to spawn process: {}", e);
            self.state = ResourceState::Failed {
                error: error.clone(),
                exit_code: None,
                started_at: None,
                failed_at: Utc::now(),
            };
            self.event_bus.publish(LifecycleEvent::Failed {
                id: self.config.id.clone(),
                kind: ResourceKind::Process,
                error,
                exit_code: None,
                timestamp: Utc::now(),
            });
            return Err(e);
        }

        // BUG FIX: Do NOT reset restart state here.
        // Previously this caused infinite restart loops because the counter was reset
        // on every start, including restarts.
        // Instead, the restart counter is reset in poll() after 30 seconds of healthy operation.
        // Just clear the healthy timer so it starts fresh.
        self.restart_healthy_since = None;

        // Initialize probe runner if configured
        if self.config.readiness_probe.is_some() || self.config.liveness_probe.is_some() {
            self.probe_runner = Some(ProbeRunner::new(
                self.config.readiness_probe.clone(),
                self.config.liveness_probe.clone(),
            ));
        }

        Ok(())
    }

    async fn stop(&mut self, _ctx: &Context) -> ResourceResult<()> {
        if !self.state.is_running() {
            return Ok(());
        }

        info!(id = %self.config.id, "Stopping process");

        // Transition to Stopping
        let started_at = self.state.started_at().unwrap_or_else(Utc::now);
        self.state = ResourceState::Stopping {
            started_at,
            stopping_at: Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Stopping {
            id: self.config.id.clone(),
            kind: ResourceKind::Process,
            timestamp: Utc::now(),
        });

        // Execute shutdown command if configured
        if let Some(shutdown_cmd) = &self.config.shutdown.command {
            debug!(id = %self.config.id, cmd = %shutdown_cmd, "Running shutdown command");
            let output = Command::new("sh")
                .arg("-c")
                .arg(shutdown_cmd)
                .output()
                .await;

            if let Err(e) = output {
                warn!(id = %self.config.id, error = %e, "Shutdown command failed");
            }
        }

        // Send signal
        #[cfg(unix)]
        {
            if let Err(e) = self.send_signal(self.config.shutdown.signal) {
                warn!(id = %self.config.id, error = %e, "Failed to send signal");
            }
        }

        // Wait for process to exit with timeout
        let timeout = self.config.shutdown.timeout;
        if let Some(ref mut child) = self.child {
            let wait_result = tokio::time::timeout(timeout, child.wait()).await;

            match wait_result {
                Ok(Ok(status)) => {
                    let exit_code = status.code();
                    self.state = ResourceState::Stopped {
                        exit_code,
                        started_at,
                        stopped_at: Utc::now(),
                    };
                    self.event_bus.publish(LifecycleEvent::Stopped {
                        id: self.config.id.clone(),
                        kind: ResourceKind::Process,
                        exit_code,
                        timestamp: Utc::now(),
                    });
                }
                Ok(Err(e)) => {
                    error!(id = %self.config.id, error = %e, "Error waiting for process");
                }
                Err(_) => {
                    // Timeout - force kill the entire process group
                    warn!(id = %self.config.id, "Graceful shutdown timed out, killing process group");
                    #[cfg(unix)]
                    {
                        if let Some(pid) = child.id() {
                            unsafe {
                                libc::kill(-(pid as i32), libc::SIGKILL);
                            }
                        }
                        let _ = child.wait().await;
                    }
                    #[cfg(not(unix))]
                    {
                        if let Err(e) = child.kill().await {
                            error!(id = %self.config.id, error = %e, "Failed to kill process");
                        }
                    }
                    self.state = ResourceState::Stopped {
                        exit_code: None,
                        started_at,
                        stopped_at: Utc::now(),
                    };
                    self.event_bus.publish(LifecycleEvent::Stopped {
                        id: self.config.id.clone(),
                        kind: ResourceKind::Process,
                        exit_code: None,
                        timestamp: Utc::now(),
                    });
                }
            }
        }

        self.child = None;
        self.probe_runner = None;

        Ok(())
    }

    async fn kill(&mut self, _ctx: &Context) -> ResourceResult<()> {
        if let Some(ref mut child) = self.child {
            #[cfg(unix)]
            {
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
                child.wait().await.map_err(|e| {
                    Box::new(std::io::Error::other(format!(
                        "Failed to reap process: {}",
                        e
                    ))) as ResourceError
                })?;
            }
            #[cfg(not(unix))]
            child.kill().await.map_err(|e| {
                Box::new(std::io::Error::other(format!(
                    "Failed to kill process: {}",
                    e
                ))) as ResourceError
            })?;

            let started_at = self.state.started_at().unwrap_or_else(Utc::now);
            self.state = ResourceState::Stopped {
                exit_code: None,
                started_at,
                stopped_at: Utc::now(),
            };
            self.event_bus.publish(LifecycleEvent::Stopped {
                id: self.config.id.clone(),
                kind: ResourceKind::Process,
                exit_code: None,
                timestamp: Utc::now(),
            });
            self.child = None;
        }
        Ok(())
    }

    async fn health(&self) -> HealthStatus {
        // If not running, unhealthy
        if !self.state.is_running() {
            return HealthStatus::Unhealthy {
                reason: format!("Process not running (state: {})", self.state),
            };
        }

        // If no probes configured, assume healthy if running
        let Some(ref probe_runner) = self.probe_runner else {
            return HealthStatus::Healthy;
        };

        // Run liveness probe if configured
        probe_runner.check_health().await
    }

    fn logs(&self) -> Option<Box<dyn LogStream>> {
        Some(Box::new(ProcessLogStream {
            logs: self.logs.clone(),
            position: 0,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Log stream implementation for ProcessResource.
struct ProcessLogStream {
    logs: VecDeque<LogEntry>,
    position: usize,
}

impl LogStream for ProcessLogStream {
    fn try_recv(&mut self) -> Option<LogEntry> {
        if self.position < self.logs.len() {
            let entry = self.logs.get(self.position).cloned();
            self.position += 1;
            entry
        } else {
            None
        }
    }

    fn is_open(&self) -> bool {
        self.position < self.logs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_config(id: &str, cmd: &str) -> ProcessResourceConfig {
        ProcessResourceConfig::new(id, cmd)
    }

    fn make_resource(config: ProcessResourceConfig) -> ProcessResource {
        let bus = Arc::new(ResourceEventBus::new());
        ProcessResource::new(config, bus)
    }

    #[test]
    fn process_resource_new() {
        let config = make_config("api", "echo hello");
        let resource = make_resource(config);

        assert_eq!(resource.id().as_str(), "api");
        assert_eq!(resource.kind(), ResourceKind::Process);
        assert!(resource.state().is_pending());
    }

    #[tokio::test]
    async fn process_resource_start_stop() {
        let config = make_config("test", "sleep 10");
        let mut resource = make_resource(config);
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();
        assert!(resource.state().is_running());
        assert!(resource.child().is_some());

        resource.stop(&ctx).await.unwrap();
        assert!(resource.state().is_stopped());
        assert!(resource.child().is_none());
    }

    #[tokio::test]
    async fn process_resource_quick_exit() {
        let config = make_config("test", "echo done");
        let mut resource = make_resource(config);
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();

        // Wait for process to complete
        tokio::time::sleep(Duration::from_millis(100)).await;
        resource.poll();

        // Process should have stopped
        assert!(resource.state().is_stopped());
    }

    #[tokio::test]
    async fn process_resource_health_not_running() {
        let config = make_config("test", "echo hello");
        let resource = make_resource(config);

        let health = resource.health().await;
        assert!(health.is_unhealthy());
    }

    #[tokio::test]
    async fn process_resource_health_running() {
        let config = make_config("test", "sleep 10");
        let mut resource = make_resource(config);
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();
        let health = resource.health().await;
        assert!(health.is_healthy());

        resource.stop(&ctx).await.unwrap();
    }

    #[test]
    fn process_resource_should_restart() {
        let config = make_config("test", "exit 1").with_restart(RestartPolicy::OnFailure {
            max_retries: Some(3),
            backoff: Default::default(),
        });
        let resource = make_resource(config);

        assert!(resource.should_restart());
    }

    #[test]
    fn process_resource_no_restart() {
        let config = make_config("test", "exit 0").with_restart(RestartPolicy::No);
        let resource = make_resource(config);

        assert!(!resource.should_restart());
    }

    #[tokio::test]
    async fn process_resource_working_dir() {
        let config = make_config("test", "pwd").with_working_dir("/tmp");
        let mut resource = make_resource(config);
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        resource.stop(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn process_resource_environment() {
        let config = make_config("test", "echo $MY_VAR").with_env("MY_VAR", "hello");
        let mut resource = make_resource(config);
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        resource.stop(&ctx).await.unwrap();
    }

    #[test]
    fn process_resource_logs() {
        let config = make_config("test", "echo hello");
        let resource = make_resource(config);

        // Initially no logs
        let logs = resource.logs();
        assert!(logs.is_some());
    }

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        assert_eq!(strip_ansi("hello"), "hello");
        assert_eq!(strip_ansi("\x1b[32mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("\x1b[1;31mERROR\x1b[0m: fail"), "ERROR: fail");
        assert_eq!(strip_ansi("\x1b]0;title\x07rest"), "rest");
        assert_eq!(strip_ansi("\x1b[2K\x1b[1Gprogress"), "progress");
    }

    #[test]
    fn strip_ansi_handles_carriage_return() {
        // \r overwrites from start of line — keep content after last \r
        assert_eq!(strip_ansi("progress 50%\rprogress 100%"), "progress 100%");
        assert_eq!(strip_ansi("old text\rnew"), "new");
        // No \r: pass through normally
        assert_eq!(strip_ansi("no cr here"), "no cr here");
        // Tab preserved, other controls stripped
        assert_eq!(strip_ansi("a\tb"), "a\tb");
    }

    #[test]
    fn process_resource_downcast() {
        let config = make_config("test", "echo hello");
        let resource = make_resource(config);

        let any = resource.as_any();
        let downcasted = any.downcast_ref::<ProcessResource>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().id().as_str(), "test");
    }
}
