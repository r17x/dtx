//! AI provider and middleware for intelligent suggestions.
//!
//! This module provides AI-assisted features:
//! - Error explanation
//! - Configuration suggestions
//! - Health check recommendations
//!
//! # Feature Flag
//!
//! Enable the `ai` feature for full provider support:
//!
//! ```toml
//! [dependencies]
//! dtx-middleware = { version = "0.1", features = ["ai"] }
//! ```

mod error;
mod middleware;
mod provider;
mod suggestions;

pub use error::AIError;
pub use middleware::{AIConfig, AIMiddleware};
pub use provider::{AIProvider, AIRequest, AIResponse};
pub use suggestions::Suggestions;

#[cfg(feature = "ai")]
pub use provider::{ClaudeProvider, LocalProvider, OpenAIProvider};

/// Create an AI provider from environment configuration.
///
/// Checks `DTX_AI_PROVIDER` environment variable:
/// - `openai` - OpenAI provider (requires `DTX_OPENAI_KEY`)
/// - `claude` - Claude provider (requires `DTX_ANTHROPIC_KEY`)
/// - `local` - Local provider (requires `DTX_LOCAL_AI_ENDPOINT`)
///
/// Returns `None` if no provider is configured.
#[cfg(feature = "ai")]
pub fn create_provider_from_env() -> Option<Box<dyn AIProvider>> {
    let provider = std::env::var("DTX_AI_PROVIDER").ok()?;

    match provider.to_lowercase().as_str() {
        "openai" => {
            let api_key = std::env::var("DTX_OPENAI_KEY").ok()?;
            Some(Box::new(OpenAIProvider::new(api_key)))
        }
        "claude" | "anthropic" => {
            let api_key = std::env::var("DTX_ANTHROPIC_KEY").ok()?;
            Some(Box::new(ClaudeProvider::new(api_key)))
        }
        "local" => {
            let endpoint = std::env::var("DTX_LOCAL_AI_ENDPOINT").ok()?;
            Some(Box::new(LocalProvider::new(endpoint)))
        }
        _ => None,
    }
}

/// Create a no-op provider that always returns empty responses.
pub fn noop_provider() -> impl AIProvider {
    provider::NoopProvider
}
