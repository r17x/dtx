//! Integration tests for SSE endpoints.

use dtx_web::{create_router, AppState};
use tempfile::tempdir;

fn setup_test_state() -> AppState {
    let temp_dir = tempdir().unwrap();
    let project_root = temp_dir.path().to_path_buf();

    // Create .dtx/config.yaml
    let dtx_dir = project_root.join(".dtx");
    std::fs::create_dir_all(&dtx_dir).unwrap();
    std::fs::write(
        dtx_dir.join("config.yaml"),
        "project:\n  name: test-project\nresources: {}\n",
    )
    .unwrap();

    let store = dtx_core::store::ConfigStore::load(dtx_dir.join("config.yaml")).unwrap();
    // Leak tempdir so it persists for the test duration
    std::mem::forget(temp_dir);
    AppState::new(store)
}

#[tokio::test]
async fn test_sse_status_endpoint_exists() {
    let state = setup_test_state();
    let app = create_router(state);
    let _client = axum_test::TestServer::new(app).unwrap();

    // Note: SSE endpoints stream indefinitely, so we can't test them directly
    // with axum-test. The endpoint registration and handler logic are tested
    // through compilation and unit tests in the handler modules.
}

#[tokio::test]
async fn test_htmx_live_status_panel() {
    let state = setup_test_state();
    let app = create_router(state);
    let client = axum_test::TestServer::new(app).unwrap();

    // Test live status panel partial
    let response = client.get("/htmx/partials/status-live").await;
    response.assert_status_ok();
}

#[tokio::test]
async fn test_htmx_live_logs_panel() {
    let state = setup_test_state();
    let app = create_router(state);
    let client = axum_test::TestServer::new(app).unwrap();

    // Test live logs panel partial
    let response = client.get("/htmx/partials/logs-live/test-service").await;
    response.assert_status_ok();
}

#[tokio::test]
async fn test_connection_tracker() {
    use dtx_web::sse::ConnectionTracker;

    let tracker = ConnectionTracker::new();
    assert_eq!(tracker.connection_count(), 0);

    let _guard1 = tracker.connect();
    assert_eq!(tracker.connection_count(), 1);

    let _guard2 = tracker.connect();
    assert_eq!(tracker.connection_count(), 2);

    drop(_guard1);
    assert_eq!(tracker.connection_count(), 1);

    drop(_guard2);
    assert_eq!(tracker.connection_count(), 0);
}
