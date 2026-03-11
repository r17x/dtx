//! Protocol request handler and dispatch.
//!
//! This module provides:
//! - `ProtocolHandler` trait for implementing protocol operations
//! - `dispatch` function for routing requests to handler methods

use async_trait::async_trait;
use serde_json::Value;

use crate::jsonrpc::{ErrorObject, Request, Response};
use crate::methods::{
    LogEntry, LogsParams, ResourceListResult, ResourceParams, ResourceStatusResult,
    SubscribeParams, SubscribeResult, EVENTS_SUBSCRIBE, EVENTS_UNSUBSCRIBE, RESOURCE_KILL,
    RESOURCE_LIST, RESOURCE_LOGS, RESOURCE_RESTART, RESOURCE_START, RESOURCE_STATUS, RESOURCE_STOP,
    START_ALL, STOP_ALL,
};

/// Trait for handling protocol requests.
///
/// Implementors handle the actual business logic for each protocol method.
/// The `dispatch` function routes incoming requests to the appropriate method.
#[async_trait]
pub trait ProtocolHandler: Send + Sync {
    // Resource lifecycle

    /// Start a resource.
    async fn resource_start(&self, params: ResourceParams) -> Result<Value, ErrorObject>;

    /// Stop a resource gracefully.
    async fn resource_stop(&self, params: ResourceParams) -> Result<Value, ErrorObject>;

    /// Restart a resource.
    async fn resource_restart(&self, params: ResourceParams) -> Result<Value, ErrorObject>;

    /// Force kill a resource.
    async fn resource_kill(&self, params: ResourceParams) -> Result<Value, ErrorObject>;

    // Resource query

    /// Get resource status.
    async fn resource_status(
        &self,
        params: ResourceParams,
    ) -> Result<ResourceStatusResult, ErrorObject>;

    /// List all resources.
    async fn resource_list(&self) -> Result<ResourceListResult, ErrorObject>;

    /// Get resource logs.
    async fn resource_logs(&self, params: LogsParams) -> Result<Vec<LogEntry>, ErrorObject>;

    // Batch operations

    /// Start all resources.
    async fn start_all(&self) -> Result<Value, ErrorObject>;

    /// Stop all resources.
    async fn stop_all(&self) -> Result<Value, ErrorObject>;

    // Events

    /// Subscribe to events.
    async fn events_subscribe(
        &self,
        params: SubscribeParams,
    ) -> Result<SubscribeResult, ErrorObject>;

    /// Unsubscribe from events.
    async fn events_unsubscribe(&self, subscription_id: String) -> Result<Value, ErrorObject>;
}

/// Dispatch a request to the appropriate handler method.
///
/// Returns the response to send back to the client.
pub async fn dispatch<H: ProtocolHandler>(handler: &H, request: Request) -> Response {
    let id = request.id.clone();

    let result = dispatch_inner(handler, &request).await;

    match result {
        Ok(value) => Response::success(id, value),
        Err(error) => Response::error(id, error),
    }
}

async fn dispatch_inner<H: ProtocolHandler>(
    handler: &H,
    request: &Request,
) -> Result<Value, ErrorObject> {
    match request.method.as_str() {
        RESOURCE_START => {
            let params = parse_params(request)?;
            handler.resource_start(params).await
        }
        RESOURCE_STOP => {
            let params = parse_params(request)?;
            handler.resource_stop(params).await
        }
        RESOURCE_RESTART => {
            let params = parse_params(request)?;
            handler.resource_restart(params).await
        }
        RESOURCE_KILL => {
            let params = parse_params(request)?;
            handler.resource_kill(params).await
        }
        RESOURCE_STATUS => {
            let params = parse_params(request)?;
            let result = handler.resource_status(params).await?;
            serde_json::to_value(result).map_err(|e| ErrorObject::internal_error(e.to_string()))
        }
        RESOURCE_LIST => {
            let result = handler.resource_list().await?;
            serde_json::to_value(result).map_err(|e| ErrorObject::internal_error(e.to_string()))
        }
        RESOURCE_LOGS => {
            let params = parse_params(request)?;
            let result = handler.resource_logs(params).await?;
            serde_json::to_value(result).map_err(|e| ErrorObject::internal_error(e.to_string()))
        }
        START_ALL => handler.start_all().await,
        STOP_ALL => handler.stop_all().await,
        EVENTS_SUBSCRIBE => {
            let params = parse_params_or_default(request);
            let result = handler.events_subscribe(params).await?;
            serde_json::to_value(result).map_err(|e| ErrorObject::internal_error(e.to_string()))
        }
        EVENTS_UNSUBSCRIBE => {
            let params: ResourceParams = parse_params(request)?;
            handler.events_unsubscribe(params.id).await
        }
        _ => Err(ErrorObject::method_not_found(&request.method)),
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(request: &Request) -> Result<T, ErrorObject> {
    let params = request
        .params
        .as_ref()
        .ok_or_else(|| ErrorObject::invalid_params("Missing params"))?;

    serde_json::from_value(params.clone())
        .map_err(|e| ErrorObject::invalid_params(format!("Invalid params: {}", e)))
}

fn parse_params_or_default<T: serde::de::DeserializeOwned + Default>(request: &Request) -> T {
    request
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockHandler {
        call_count: AtomicUsize,
    }

    impl MockHandler {
        fn new() -> Self {
            Self {
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ProtocolHandler for MockHandler {
        async fn resource_start(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"status": "started"}))
        }

        async fn resource_stop(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"status": "stopped"}))
        }

        async fn resource_restart(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"status": "restarted"}))
        }

        async fn resource_kill(&self, _params: ResourceParams) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"status": "killed"}))
        }

        async fn resource_status(
            &self,
            params: ResourceParams,
        ) -> Result<ResourceStatusResult, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(ResourceStatusResult {
                id: params.id,
                kind: "process".to_string(),
                state: "running".to_string(),
                pid: Some(1234),
                healthy: Some(true),
                started_at: None,
                stopped_at: None,
                exit_code: None,
            })
        }

        async fn resource_list(&self) -> Result<ResourceListResult, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(ResourceListResult { resources: vec![] })
        }

        async fn resource_logs(&self, _params: LogsParams) -> Result<Vec<LogEntry>, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        }

        async fn start_all(&self) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"count": 0}))
        }

        async fn stop_all(&self) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"count": 0}))
        }

        async fn events_subscribe(
            &self,
            _params: SubscribeParams,
        ) -> Result<SubscribeResult, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(SubscribeResult {
                subscription_id: "sub-1".to_string(),
            })
        }

        async fn events_unsubscribe(&self, _subscription_id: String) -> Result<Value, ErrorObject> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({}))
        }
    }

    #[tokio::test]
    async fn dispatch_resource_start() {
        let handler = MockHandler::new();
        let request = Request::with_id(
            RESOURCE_START,
            Some(serde_json::json!({"id": "postgres"})),
            1.into(),
        );

        let response = dispatch(&handler, request).await;
        assert!(response.is_success());
        assert_eq!(handler.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_unknown_method() {
        let handler = MockHandler::new();
        let request = Request::with_id("unknown/method", None, 1.into());

        let response = dispatch(&handler, request).await;
        assert!(response.is_error());
        assert_eq!(
            response.error.unwrap().code,
            crate::jsonrpc::error_codes::METHOD_NOT_FOUND
        );
    }

    #[tokio::test]
    async fn dispatch_missing_params() {
        let handler = MockHandler::new();
        let request = Request::with_id(RESOURCE_START, None, 1.into());

        let response = dispatch(&handler, request).await;
        assert!(response.is_error());
        assert_eq!(
            response.error.unwrap().code,
            crate::jsonrpc::error_codes::INVALID_PARAMS
        );
    }
}
