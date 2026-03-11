//! AI middleware for intelligent suggestions and error explanations.

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info};

use dtx_core::middleware::{Middleware, Next, Operation, Response};
use dtx_core::resource::{Context, Result};

use super::provider::{AIProvider, AIRequest};

/// Configuration for AI middleware.
#[derive(Clone, Debug)]
pub struct AIConfig {
    /// Enable error explanations.
    pub explain_failures: bool,
    /// Enable configuration suggestions.
    pub suggest_configs: bool,
    /// Maximum tokens for explanations.
    pub max_tokens: u32,
    /// Temperature for AI responses.
    pub temperature: f32,
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            explain_failures: true,
            suggest_configs: true,
            max_tokens: 500,
            temperature: 0.3,
        }
    }
}

impl AIConfig {
    /// Create a minimal config with all features disabled.
    pub fn disabled() -> Self {
        Self {
            explain_failures: false,
            suggest_configs: false,
            max_tokens: 100,
            temperature: 0.0,
        }
    }

    /// Enable only error explanations.
    pub fn explain_only() -> Self {
        Self {
            explain_failures: true,
            suggest_configs: false,
            ..Default::default()
        }
    }
}

/// AI middleware that provides intelligent error explanations.
pub struct AIMiddleware {
    provider: Arc<dyn AIProvider>,
    config: AIConfig,
}

impl AIMiddleware {
    /// Create a new AI middleware with the given provider.
    pub fn new(provider: Arc<dyn AIProvider>) -> Self {
        Self {
            provider,
            config: AIConfig::default(),
        }
    }

    /// Set the configuration.
    pub fn with_config(mut self, config: AIConfig) -> Self {
        self.config = config;
        self
    }

    /// Explain an error in simple terms.
    async fn explain_error(&self, operation: &str, error: &str) -> Option<String> {
        let prompt = format!(
            r#"You are a helpful DevOps assistant. Explain this dtx orchestration error in simple terms and suggest a fix.

Operation: {}
Error: {}

Provide a brief explanation (2-3 sentences) and a concrete fix if possible.
Format: "Error explanation. Suggested fix: <action>""#,
            operation, error
        );

        let request = AIRequest::new(prompt)
            .with_system("You are a DevOps expert helping debug service orchestration issues.")
            .with_max_tokens(self.config.max_tokens)
            .with_temperature(self.config.temperature);

        match self.provider.complete(request).await {
            Ok(response) if !response.text.is_empty() => {
                debug!(explanation = %response.text, "AI error explanation generated");
                Some(response.text)
            }
            Ok(_) => None,
            Err(e) => {
                debug!(error = %e, "Failed to generate AI explanation");
                None
            }
        }
    }
}

#[async_trait]
impl Middleware for AIMiddleware {
    fn name(&self) -> &'static str {
        "ai"
    }

    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
        let op_name = op.name().to_string();
        let result = next.run(op, ctx).await;

        // Generate explanation on error if enabled
        if let Err(ref error) = result {
            if self.config.explain_failures {
                if let Some(explanation) = self.explain_error(&op_name, &error.to_string()).await {
                    info!(
                        operation = %op_name,
                        explanation = %explanation,
                        "AI explanation"
                    );
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtx_core::middleware::{MiddlewareStack, NoopHandler};

    struct MockProvider {
        response: String,
    }

    #[async_trait]
    impl AIProvider for MockProvider {
        async fn complete(
            &self,
            _request: AIRequest,
        ) -> std::result::Result<super::super::AIResponse, super::super::AIError> {
            Ok(super::super::AIResponse {
                text: self.response.clone(),
                usage: None,
                model: Some("mock".to_string()),
            })
        }
    }

    #[test]
    fn config_default() {
        let config = AIConfig::default();
        assert!(config.explain_failures);
        assert!(config.suggest_configs);
    }

    #[test]
    fn config_disabled() {
        let config = AIConfig::disabled();
        assert!(!config.explain_failures);
        assert!(!config.suggest_configs);
    }

    #[tokio::test]
    async fn middleware_passes_through() {
        let provider = Arc::new(MockProvider {
            response: "test".to_string(),
        });
        let middleware = AIMiddleware::new(provider);

        let chain = MiddlewareStack::new().layer(middleware).build(NoopHandler);

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn middleware_name() {
        let provider = Arc::new(MockProvider {
            response: String::new(),
        });
        let middleware = AIMiddleware::new(provider);
        assert_eq!(middleware.name(), "ai");
    }
}
