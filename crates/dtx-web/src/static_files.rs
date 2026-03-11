//! Embedded static files handler.
//!
//! Static files (CSS, JS, fonts) are embedded into the binary at compile time,
//! making the binary self-contained and portable.

use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
};
use rust_embed::Embed;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::Service;

/// Embedded static assets from the `static/` directory.
#[derive(Embed)]
#[folder = "../../static"]
struct Assets;

/// Service that serves embedded static files.
#[derive(Clone)]
pub struct EmbeddedStaticFiles;

impl EmbeddedStaticFiles {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmbeddedStaticFiles {
    fn default() -> Self {
        Self::new()
    }
}

impl<B> Service<Request<B>> for EmbeddedStaticFiles
where
    B: Send + 'static,
{
    type Response = Response<Body>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let path = req.uri().path().trim_start_matches('/').to_string();

        Box::pin(async move {
            match Assets::get(&path) {
                Some(content) => {
                    let mime = mime_guess::from_path(path).first_or_octet_stream();
                    let body = Body::from(content.data.to_vec());

                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, mime.as_ref())
                        .header(header::CACHE_CONTROL, "public, max-age=31536000")
                        .body(body)
                        .unwrap())
                }
                None => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("Not Found"))
                    .unwrap()),
            }
        })
    }
}

/// Handler function for serving a specific static file.
pub async fn serve_static(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    match Assets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}
