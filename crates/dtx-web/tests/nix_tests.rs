//! Integration tests for Nix environment endpoints.

use dtx_web::{create_router, AppState};
use tempfile::tempdir;

fn setup_test_state_with_dir(temp_dir: &std::path::Path) -> AppState {
    let dtx_dir = temp_dir.join(".dtx");
    std::fs::create_dir_all(&dtx_dir).unwrap();
    std::fs::write(
        dtx_dir.join("config.yaml"),
        "project:\n  name: test-project\nresources: {}\n",
    )
    .unwrap();

    let store = dtx_core::store::ConfigStore::load(dtx_dir.join("config.yaml")).unwrap();
    AppState::new(store, dtx_web::config::WebConfig::default())
}

#[tokio::test]
async fn test_nix_status_endpoint() {
    let temp_dir = tempdir().unwrap();
    let state = setup_test_state_with_dir(temp_dir.path());
    let app = create_router(state);
    let client = axum_test::TestServer::new(app).unwrap();

    // Get Nix status
    let response = client.get("/api/nix/status").await;

    response.assert_status_ok();
    let status: serde_json::Value = response.json();

    assert!(!status["has_flake"].as_bool().unwrap());
    assert!(!status["has_envrc"].as_bool().unwrap());
    assert!(status["packages"].is_array());
}

#[tokio::test]
async fn test_nix_init_endpoint() {
    let temp_dir = tempdir().unwrap();

    // Create config with a service that has a package
    let dtx_dir = temp_dir.path().join(".dtx");
    std::fs::create_dir_all(&dtx_dir).unwrap();
    std::fs::write(
        dtx_dir.join("config.yaml"),
        r#"project:
  name: test-project
resources:
  db:
    command: postgres
    nix:
      packages: [postgresql]
"#,
    )
    .unwrap();

    let store = dtx_core::store::ConfigStore::load(dtx_dir.join("config.yaml")).unwrap();
    let state = AppState::new(store, dtx_web::config::WebConfig::default());
    let app = create_router(state);
    let client = axum_test::TestServer::new(app).unwrap();

    // Initialize Nix environment
    let response = client.post("/api/nix/init").await;

    response.assert_status_ok();
    let result: serde_json::Value = response.json();

    assert_eq!(result["status"].as_str().unwrap(), "success");
    assert!(result["files"].is_array());
    assert_eq!(result["files"].as_array().unwrap().len(), 2);

    // Verify files were created
    assert!(temp_dir.path().join("flake.nix").exists());
    assert!(temp_dir.path().join(".envrc").exists());

    // Verify content
    let flake_content = std::fs::read_to_string(temp_dir.path().join("flake.nix")).unwrap();
    assert!(flake_content.contains("postgresql"));
    assert!(flake_content.contains("process-compose"));

    let envrc_content = std::fs::read_to_string(temp_dir.path().join(".envrc")).unwrap();
    assert!(envrc_content.contains("use flake"));
    assert!(envrc_content.contains("PGDATA"));
}

#[tokio::test]
async fn test_nix_envrc_endpoint() {
    let temp_dir = tempdir().unwrap();
    let state = setup_test_state_with_dir(temp_dir.path());
    let app = create_router(state);
    let client = axum_test::TestServer::new(app).unwrap();

    // Generate .envrc only
    let response = client.post("/api/nix/envrc").await;

    response.assert_status_ok();
    let result: serde_json::Value = response.json();

    assert_eq!(result["status"].as_str().unwrap(), "success");
    assert!(temp_dir.path().join(".envrc").exists());
}

#[tokio::test]
async fn test_download_flake_endpoint() {
    let temp_dir = tempdir().unwrap();

    // Create config with a service
    let dtx_dir = temp_dir.path().join(".dtx");
    std::fs::create_dir_all(&dtx_dir).unwrap();
    std::fs::write(
        dtx_dir.join("config.yaml"),
        r#"project:
  name: test-project
resources:
  cache:
    command: redis-server
    nix:
      packages: [redis]
"#,
    )
    .unwrap();

    let store = dtx_core::store::ConfigStore::load(dtx_dir.join("config.yaml")).unwrap();
    let state = AppState::new(store, dtx_web::config::WebConfig::default());
    let app = create_router(state);
    let client = axum_test::TestServer::new(app).unwrap();

    // Download flake.nix
    let response = client.get("/api/nix/flake").await;

    response.assert_status_ok();
    let flake_content = response.text();

    assert!(flake_content.contains("redis"));
    assert!(flake_content.contains("process-compose"));
    assert!(flake_content.contains("test-project"));
}

#[tokio::test]
async fn test_nix_panel_htmx_partial() {
    let temp_dir = tempdir().unwrap();
    let state = setup_test_state_with_dir(temp_dir.path());
    let app = create_router(state);
    let client = axum_test::TestServer::new(app).unwrap();

    // Get Nix panel partial
    let response = client.get("/htmx/partials/nix-panel").await;

    response.assert_status_ok();
    let body = response.text();

    assert!(body.contains("nix-panel"));
}
