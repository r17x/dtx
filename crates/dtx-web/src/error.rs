//! Error handling for web server.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// Application error type.
#[derive(Debug)]
pub struct AppError {
    pub code: StatusCode,
    pub message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: u16,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = Json(ErrorResponse {
            error: self.message,
            code: self.code.as_u16(),
        });
        (self.code, body).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        tracing::error!("Internal error: {:?}", err);
        Self {
            code: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Internal server error".to_string(),
        }
    }
}

impl From<dtx_core::StoreError> for AppError {
    fn from(err: dtx_core::StoreError) -> Self {
        tracing::warn!("Store error: {:?}", err);
        match err {
            dtx_core::StoreError::ResourceNotFound(_) | dtx_core::StoreError::ProjectNotFound => {
                Self {
                    code: StatusCode::NOT_FOUND,
                    message: err.to_string(),
                }
            }
            dtx_core::StoreError::DuplicateResource(_) => Self {
                code: StatusCode::CONFLICT,
                message: err.to_string(),
            },
            _ => Self {
                code: StatusCode::INTERNAL_SERVER_ERROR,
                message: err.to_string(),
            },
        }
    }
}

impl From<dtx_core::NixError> for AppError {
    fn from(err: dtx_core::NixError) -> Self {
        tracing::warn!("Nix error: {:?}", err);
        match err {
            dtx_core::NixError::NixNotInstalled => Self {
                code: StatusCode::SERVICE_UNAVAILABLE,
                message: err.to_string(),
            },
            dtx_core::NixError::PackageNotFound(_) => Self {
                code: StatusCode::NOT_FOUND,
                message: err.to_string(),
            },
            _ => Self {
                code: StatusCode::BAD_REQUEST,
                message: err.to_string(),
            },
        }
    }
}

impl From<dtx_core::CoreError> for AppError {
    fn from(err: dtx_core::CoreError) -> Self {
        tracing::warn!("Core error: {:?}", err);
        match err {
            dtx_core::CoreError::PortConflict(ref conflict) => Self {
                code: StatusCode::CONFLICT,
                message: conflict.to_string(),
            },
            dtx_core::CoreError::Timeout(_) => Self {
                code: StatusCode::GATEWAY_TIMEOUT,
                message: err.to_string(),
            },
            dtx_core::CoreError::ProcessCompose(_) => Self {
                code: StatusCode::SERVICE_UNAVAILABLE,
                message: err.to_string(),
            },
            dtx_core::CoreError::Nix(nix_err) => nix_err.into(),
            _ => Self {
                code: StatusCode::BAD_REQUEST,
                message: err.to_string(),
            },
        }
    }
}

impl AppError {
    /// Creates a 404 Not Found error.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            code: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
    }

    /// Creates a 400 Bad Request error.
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            code: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    /// Creates a 409 Conflict error.
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self {
            code: StatusCode::CONFLICT,
            message: msg.into(),
        }
    }

    /// Creates a 500 Internal Server Error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

/// Result type for web handlers.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_error_not_found() {
        let err = AppError::not_found("Resource not found");
        assert_eq!(err.code, StatusCode::NOT_FOUND);
        assert_eq!(err.message, "Resource not found");
    }

    #[test]
    fn test_app_error_bad_request() {
        let err = AppError::bad_request("Invalid input");
        assert_eq!(err.code, StatusCode::BAD_REQUEST);
        assert_eq!(err.message, "Invalid input");
    }
}
