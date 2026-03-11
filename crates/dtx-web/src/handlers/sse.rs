//! Server-Sent Events (SSE) handlers for real-time updates.

use axum::{
    extract::{Path, State},
    response::sse::{Event, Sse},
};
use dtx_core::events::{EventFilter, LifecycleEvent};
use dtx_core::resource::ResourceState;
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;

use crate::sse::{event, KEEPALIVE_INTERVAL};
use crate::state::AppState;

/// Status update event data
#[derive(serde::Serialize)]
struct StatusUpdate {
    running: bool,
    services: Vec<ServiceStatus>,
    timestamp: String,
}

/// Service status information
#[derive(serde::Serialize)]
struct ServiceStatus {
    name: String,
    status: String,
    is_running: bool,
    pid: u32,
    restarts: u32,
}

pub async fn status_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let _guard = state.sse_tracker.connect();
    let shutdown_token = state.shutdown_token.clone();

    let stream = async_stream::stream! {
        let mut interval = tokio::time::interval(Duration::from_secs(2));

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    yield event("shutdown", serde_json::json!({
                        "message": "Server shutting down",
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }));
                    break;
                }
                _ = interval.tick() => {}
            }

            let update = {
                let orch_guard = state.orchestrator.read().await;

                if let Some(ref orchestrator) = *orch_guard {
                    let mut services = Vec::new();
                    let statuses = orchestrator.status().await;

                    for (id, resource_state) in &statuses {
                        let (status, is_running, pid) = match resource_state {
                            ResourceState::Pending => ("Pending".to_string(), false, 0),
                            ResourceState::Starting { .. } => ("Starting".to_string(), false, 0),
                            ResourceState::Running { pid, .. } => {
                                ("Running".to_string(), true, pid.unwrap_or(0))
                            }
                            ResourceState::Stopping { .. } => ("Stopping".to_string(), false, 0),
                            ResourceState::Stopped { exit_code, .. } => {
                                let code = exit_code.unwrap_or(0);
                                (format!("Stopped ({})", code), false, 0)
                            }
                            ResourceState::Failed { error, exit_code, .. } => {
                                let code_str = exit_code.map(|c| format!(" ({})", c)).unwrap_or_default();
                                (format!("Failed: {}{}", error, code_str), false, 0)
                            }
                        };
                        services.push(ServiceStatus {
                            name: id.to_string(),
                            status,
                            is_running,
                            pid,
                            restarts: 0,
                        });
                    }
                    StatusUpdate {
                        running: orchestrator.is_running(),
                        services,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    }
                } else {
                    StatusUpdate {
                        running: false,
                        services: vec![],
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    }
                }
            };

            yield event("status", &update);
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(KEEPALIVE_INTERVAL)
            .text("keepalive"),
    )
}

/// Log entry for SSE
#[derive(serde::Serialize)]
struct LogEntry {
    service: String,
    message: String,
    timestamp: String,
    level: String,
}

/// Stream logs for a specific service via ResourceEventBus subscription.
pub async fn logs_stream(
    State(state): State<AppState>,
    Path(service): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let _guard = state.sse_tracker.connect();
    let shutdown_token = state.shutdown_token.clone();

    let filter = EventFilter::new().resource(service.as_str()).with_logs();
    let mut subscriber = state.event_bus.subscribe_filtered(filter.clone());

    let buffered = state.event_bus.replay(&filter);

    let stream = async_stream::stream! {
        yield event(
            "info",
            serde_json::json!({
                "message": format!("Streaming logs for '{}'", service),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        );

        // Replay buffered events
        for lifecycle_event in &buffered {
            if let LifecycleEvent::Log { id, line, timestamp, .. } = lifecycle_event {
                yield event("log", &LogEntry {
                    service: id.to_string(),
                    message: line.clone(),
                    timestamp: timestamp.to_rfc3339(),
                    level: "info".to_string(),
                });
            }
        }

        // Live stream
        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    yield event("shutdown", serde_json::json!({
                        "message": "Server shutting down",
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }));
                    break;
                }
                maybe_event = subscriber.recv() => {
                    match maybe_event {
                        Some(lifecycle_event) => {
                            match &lifecycle_event {
                                LifecycleEvent::Log { id, line, timestamp, stream } => {
                                    let level = match stream {
                                        dtx_core::resource::LogStreamKind::Stderr => "error",
                                        _ => "info",
                                    };
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: line.clone(),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: level.to_string(),
                                    });
                                }
                                LifecycleEvent::Starting { id, timestamp, .. } => {
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: "Service starting".to_string(),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "info".to_string(),
                                    });
                                }
                                LifecycleEvent::Running { id, timestamp, .. } => {
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: "Service running".to_string(),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "info".to_string(),
                                    });
                                }
                                LifecycleEvent::Stopped { id, exit_code, timestamp, .. } => {
                                    let msg = match exit_code {
                                        Some(code) => format!("Service stopped (exit code: {})", code),
                                        None => "Service stopped".to_string(),
                                    };
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: msg,
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "info".to_string(),
                                    });
                                }
                                LifecycleEvent::Failed { id, error, timestamp, .. } => {
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: format!("Service failed: {}", error),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "error".to_string(),
                                    });
                                }
                                LifecycleEvent::Restarting { id, attempt, max_attempts, timestamp, .. } => {
                                    let max = max_attempts.map(|m| m.to_string()).unwrap_or_else(|| "unlimited".to_string());
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: format!("Restarting ({}/{})", attempt, max),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "warn".to_string(),
                                    });
                                }
                                _ => {}
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    // Keepalive handled by SSE keep_alive config
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(KEEPALIVE_INTERVAL)
            .text("keepalive"),
    )
}

/// Stream logs from all services via ResourceEventBus subscription.
pub async fn all_logs_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let _guard = state.sse_tracker.connect();
    let shutdown_token = state.shutdown_token.clone();

    let filter = EventFilter::all();
    let mut subscriber = state.event_bus.subscribe_filtered(filter.clone());

    let buffered = state.event_bus.replay(&EventFilter::new().with_logs());

    let stream = async_stream::stream! {
        yield event(
            "info",
            serde_json::json!({
                "message": format!("Streaming all service logs ({} buffered)", buffered.len()),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        );

        // Replay buffered log events
        for lifecycle_event in &buffered {
            if let LifecycleEvent::Log { id, line, timestamp, .. } = lifecycle_event {
                yield event("log", &LogEntry {
                    service: id.to_string(),
                    message: line.clone(),
                    timestamp: timestamp.to_rfc3339(),
                    level: "info".to_string(),
                });
            }
        }

        // Live stream
        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    yield event("shutdown", serde_json::json!({
                        "message": "Server shutting down",
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }));
                    break;
                }
                maybe_event = subscriber.recv() => {
                    match maybe_event {
                        Some(lifecycle_event) => {
                            match &lifecycle_event {
                                LifecycleEvent::Log { id, line, timestamp, stream } => {
                                    let level = match stream {
                                        dtx_core::resource::LogStreamKind::Stderr => "error",
                                        _ => "info",
                                    };
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: line.clone(),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: level.to_string(),
                                    });
                                }
                                LifecycleEvent::Starting { id, timestamp, .. } => {
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: "Service starting".to_string(),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "info".to_string(),
                                    });
                                }
                                LifecycleEvent::Running { id, timestamp, .. } => {
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: "Service running".to_string(),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "info".to_string(),
                                    });
                                }
                                LifecycleEvent::Stopped { id, exit_code, timestamp, .. } => {
                                    let msg = match exit_code {
                                        Some(code) => format!("Service stopped (exit code: {})", code),
                                        None => "Service stopped".to_string(),
                                    };
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: msg,
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "info".to_string(),
                                    });
                                }
                                LifecycleEvent::Failed { id, error, timestamp, .. } => {
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: format!("Service failed: {}", error),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "error".to_string(),
                                    });
                                }
                                LifecycleEvent::Restarting { id, attempt, max_attempts, timestamp, .. } => {
                                    let max = max_attempts.map(|m| m.to_string()).unwrap_or_else(|| "unlimited".to_string());
                                    yield event("log", &LogEntry {
                                        service: id.to_string(),
                                        message: format!("Restarting ({}/{})", attempt, max),
                                        timestamp: timestamp.to_rfc3339(),
                                        level: "warn".to_string(),
                                    });
                                }
                                _ => {}
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    // Keepalive handled by SSE keep_alive config
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(KEEPALIVE_INTERVAL)
            .text("keepalive"),
    )
}

/// Stream lifecycle events via SSE.
pub async fn events_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let _guard = state.sse_tracker.connect();
    let shutdown_token = state.shutdown_token.clone();
    let mut subscriber = state.event_bus.subscribe();

    let stream = async_stream::stream! {
        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    yield event("shutdown", serde_json::json!({
                        "message": "Server shutting down",
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }));
                    break;
                }
                maybe_event = subscriber.recv() => {
                    match maybe_event {
                        Some(lifecycle_event) => {
                            let event_type = lifecycle_event.event_type();
                            yield event(event_type, &lifecycle_event);
                        }
                        None => {
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(15)) => {
                    yield event("keepalive", serde_json::json!({
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }));
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(KEEPALIVE_INTERVAL)
            .text("keepalive"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_update_serialization() {
        let update = StatusUpdate {
            running: true,
            services: vec![ServiceStatus {
                name: "test".to_string(),
                status: "Running".to_string(),
                is_running: true,
                pid: 1234,
                restarts: 0,
            }],
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("running"));
        assert!(json.contains("test"));
    }

    #[test]
    fn test_log_entry_serialization() {
        let entry = LogEntry {
            service: "postgres".to_string(),
            message: "Starting database".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            level: "info".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("postgres"));
        assert!(json.contains("Starting database"));
    }
}
