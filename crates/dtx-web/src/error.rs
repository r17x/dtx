//! Error handling for web server.

use askama_axum::Template;
use axum::{
    http::{header, HeaderName, HeaderValue, StatusCode},
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

/// For /api/* handlers — renders as JSON `{"error": "...", "code": N}`.
pub struct JsonError(pub AppError);

/// For /htmx/* handlers — renders as HTML error toast partial.
pub struct HtmlError(pub AppError);

/// Result type for API handlers returning JSON errors.
pub type ApiResult<T> = Result<T, JsonError>;

/// Result type for HTMX handlers returning HTML error toasts.
pub type HtmxResult<T> = Result<T, HtmlError>;

impl From<AppError> for JsonError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl From<AppError> for HtmlError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl IntoResponse for JsonError {
    fn into_response(self) -> Response {
        let body = Json(ErrorResponse {
            error: self.0.message,
            code: self.0.code.as_u16(),
        });
        (self.0.code, body).into_response()
    }
}

#[derive(Template)]
#[template(path = "partials/error_toast.html")]
struct ErrorToastTemplate<'a> {
    message: &'a str,
}

impl IntoResponse for HtmlError {
    fn into_response(self) -> Response {
        let status = self.0.code;
        let template = ErrorToastTemplate {
            message: &self.0.message,
        };
        let body = template.render().unwrap_or_else(|_| self.0.message.clone());
        (
            status,
            [
                (header::CONTENT_TYPE, HeaderValue::from_static("text/html")),
                (
                    HeaderName::from_static("hx-retarget"),
                    HeaderValue::from_static("body"),
                ),
                (
                    HeaderName::from_static("hx-reswap"),
                    HeaderValue::from_static("beforeend"),
                ),
            ],
            body,
        )
            .into_response()
    }
}

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

    #[test]
    fn test_json_error_from_app_error() {
        let app_err = AppError::not_found("missing");
        let json_err = JsonError::from(app_err);
        assert_eq!(json_err.0.code, StatusCode::NOT_FOUND);
        assert_eq!(json_err.0.message, "missing");
    }

    #[test]
    fn test_html_error_from_app_error() {
        let app_err = AppError::bad_request("bad input");
        let html_err = HtmlError::from(app_err);
        assert_eq!(html_err.0.code, StatusCode::BAD_REQUEST);
        assert_eq!(html_err.0.message, "bad input");
    }

    #[test]
    fn test_json_error_response_is_json() {
        let err = JsonError(AppError::not_found("gone"));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let content_type = response.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(content_type.to_str().unwrap().contains("application/json"));
    }

    #[test]
    fn test_html_error_response_has_htmx_headers() {
        let err = HtmlError(AppError::internal("oops"));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers().get("hx-retarget").unwrap(), "body");
        assert_eq!(response.headers().get("hx-reswap").unwrap(), "beforeend");
        let content_type = response.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(content_type.to_str().unwrap().contains("text/html"));
    }
}
