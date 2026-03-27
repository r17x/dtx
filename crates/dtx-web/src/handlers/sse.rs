//! Server-Sent Events (SSE) handlers for real-time updates.

use axum::{
    extract::{Path, State},
    response::sse::{Event, Sse},
};
use dtx_core::events::EventFilter;
use dtx_core::resource::ResourceState;
use futures::stream::Stream;
use std::convert::Infallible;

use crate::sse::{event, lifecycle_to_log_entry};
use crate::state::AppState;
use crate::types::{LogEntry, ServiceStatus, StatusUpdate};

pub async fn status_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let guard = state.sse_tracker().connect();
    let shutdown_token = state.shutdown_token().clone();
    let config = state.config().clone();
    let keepalive_interval = config.sse_keepalive_interval;

    let orchestrator_handle = state.orchestrator_handle().clone();

    let stream = async_stream::stream! {
        let _guard = guard;
        let mut interval = tokio::time::interval(config.status_poll_interval);

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
                let statuses = orchestrator_handle.status().await.ok().flatten();

                match statuses {
                    Some(statuses) => {
                        let mut services = Vec::new();
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

                        let any_running = statuses.values().any(|s| {
                            matches!(s, ResourceState::Running { .. } | ResourceState::Starting { .. })
                        });

                        StatusUpdate {
                            running: any_running,
                            services,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        }
                    }
                    None => StatusUpdate {
                        running: false,
                        services: vec![],
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    },
                }
            };

            yield event("status", &update);
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(keepalive_interval)
            .text("keepalive"),
    )
}

/// Stream logs for a specific service via ResourceEventBus subscription.
pub async fn logs_stream(
    State(state): State<AppState>,
    Path(service): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let guard = state.sse_tracker().connect();
    let shutdown_token = state.shutdown_token().clone();
    let config = state.config().clone();
    let keepalive_interval = config.sse_keepalive_interval;

    let filter = EventFilter::new().resource(service.as_str()).with_logs();
    let mut subscriber = state.event_bus().subscribe_filtered(filter.clone());

    let buffered = state.event_bus().replay(&filter);

    let stream = async_stream::stream! {
        let _guard = guard;

        yield event(
            "info",
            serde_json::json!({
                "message": format!("Streaming logs for '{}'", service),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        );

        // Replay buffered events
        for lifecycle_event in &buffered {
            if let Some(entry) = lifecycle_to_log_entry(lifecycle_event) {
                yield event("log", &LogEntry {
                    service: entry.service,
                    message: entry.message,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    level: entry.level,
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
                            if let Some(entry) = lifecycle_to_log_entry(&lifecycle_event) {
                                yield event("log", &LogEntry {
                                    service: entry.service,
                                    message: entry.message,
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                    level: entry.level,
                                });
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(config.sse_stream_timeout) => {
                    // Keepalive handled by SSE keep_alive config
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(keepalive_interval)
            .text("keepalive"),
    )
}

/// Stream logs from all services via ResourceEventBus subscription.
pub async fn all_logs_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let guard = state.sse_tracker().connect();
    let shutdown_token = state.shutdown_token().clone();
    let config = state.config().clone();
    let keepalive_interval = config.sse_keepalive_interval;

    let filter = EventFilter::all();
    let mut subscriber = state.event_bus().subscribe_filtered(filter);

    let buffered = state.event_bus().replay(&EventFilter::new().with_logs());

    let stream = async_stream::stream! {
        let _guard = guard;

        yield event(
            "info",
            serde_json::json!({
                "message": format!("Streaming all service logs ({} buffered)", buffered.len()),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        );

        // Replay buffered log events
        for lifecycle_event in &buffered {
            if let Some(entry) = lifecycle_to_log_entry(lifecycle_event) {
                yield event("log", &LogEntry {
                    service: entry.service,
                    message: entry.message,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    level: entry.level,
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
                            if let Some(entry) = lifecycle_to_log_entry(&lifecycle_event) {
                                yield event("log", &LogEntry {
                                    service: entry.service,
                                    message: entry.message,
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                    level: entry.level,
                                });
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(config.sse_stream_timeout) => {
                    // Keepalive handled by SSE keep_alive config
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(keepalive_interval)
            .text("keepalive"),
    )
}

/// Stream logs for a specific service with project_id prefix (CLI compatibility).
/// The project_id is ignored; the service name is extracted from the second path segment.
pub async fn logs_stream_with_project(
    state: State<AppState>,
    Path((_project_id, service)): Path<(String, String)>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    logs_stream(state, Path(service)).await
}

/// Stream lifecycle events via SSE.
pub async fn events_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let guard = state.sse_tracker().connect();
    let shutdown_token = state.shutdown_token().clone();
    let config = state.config().clone();
    let keepalive_interval = config.sse_keepalive_interval;
    let mut subscriber = state.event_bus().subscribe();

    let stream = async_stream::stream! {
        let _guard = guard;

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

                            // Graph-affecting events also emit a graph_changed hint
                            match event_type {
                                "config_changed" | "memory_changed" => {
                                    yield event("graph_changed", serde_json::json!({
                                        "reason": event_type,
                                        "view_hint": "knowledge"
                                    }));
                                }
                                _ => {}
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(config.sse_keepalive_interval) => {
                    yield event("keepalive", serde_json::json!({
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }));
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(keepalive_interval)
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
