//! AI provider trait and implementations.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::AIError;

/// Request to an AI provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AIRequest {
    /// The prompt/question to send.
    pub prompt: String,
    /// System message for context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Temperature for randomness (0.0 - 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

impl AIRequest {
    /// Create a new request with just a prompt.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            system: None,
            max_tokens: None,
            temperature: None,
        }
    }

    /// Set system message.
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set max tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature.clamp(0.0, 1.0));
        self
    }
}

/// Response from an AI provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AIResponse {
    /// The generated text.
    pub text: String,
    /// Usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Model used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Token usage statistics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt.
    pub prompt_tokens: u32,
    /// Tokens in the completion.
    pub completion_tokens: u32,
    /// Total tokens.
    pub total_tokens: u32,
}

/// AI provider trait for making completions.
#[async_trait]
pub trait AIProvider: Send + Sync {
    /// Complete a prompt.
    async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError>;

    /// Analyze content and answer a question.
    async fn analyze(&self, context: &str, question: &str) -> Result<String, AIError> {
        let prompt = format!("Context:\n{}\n\nQuestion: {}\n\nAnswer:", context, question);
        let request = AIRequest::new(prompt);
        let response = self.complete(request).await?;
        Ok(response.text)
    }
}

/// No-op provider that returns empty responses.
pub struct NoopProvider;

#[async_trait]
impl AIProvider for NoopProvider {
    async fn complete(&self, _request: AIRequest) -> Result<AIResponse, AIError> {
        Ok(AIResponse {
            text: String::new(),
            usage: None,
            model: None,
        })
    }
}

// ============================================================================
// Feature-gated provider implementations
// ============================================================================

#[cfg(feature = "ai")]
mod providers {
    use super::*;
    use tracing::debug;

    /// OpenAI API provider.
    pub struct OpenAIProvider {
        client: reqwest::Client,
        api_key: String,
        model: String,
        base_url: String,
    }

    impl OpenAIProvider {
        /// Create a new OpenAI provider.
        pub fn new(api_key: impl Into<String>) -> Self {
            Self {
                client: reqwest::Client::new(),
                api_key: api_key.into(),
                model: "gpt-4o-mini".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
            }
        }

        /// Set the model to use.
        pub fn with_model(mut self, model: impl Into<String>) -> Self {
            self.model = model.into();
            self
        }

        /// Set a custom base URL (for Azure or proxies).
        pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
            self.base_url = url.into();
            self
        }
    }

    #[async_trait]
    impl AIProvider for OpenAIProvider {
        async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError> {
            debug!(model = %self.model, "OpenAI completion request");

            let mut messages = Vec::new();
            if let Some(system) = &request.system {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": system
                }));
            }
            messages.push(serde_json::json!({
                "role": "user",
                "content": request.prompt
            }));

            let mut body = serde_json::json!({
                "model": self.model,
                "messages": messages,
            });

            if let Some(max_tokens) = request.max_tokens {
                body["max_tokens"] = serde_json::json!(max_tokens);
            }
            if let Some(temperature) = request.temperature {
                body["temperature"] = serde_json::json!(temperature);
            }

            let response = self
                .client
                .post(format!("{}/chat/completions", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok());
                return Err(AIError::RateLimited { retry_after });
            }

            if !response.status().is_success() {
                let error: serde_json::Value = response.json().await?;
                return Err(AIError::Api {
                    code: error["error"]["type"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    message: error["error"]["message"]
                        .as_str()
                        .unwrap_or("Unknown error")
                        .to_string(),
                });
            }

            let data: serde_json::Value = response.json().await?;
            let text = data["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();

            let usage = data.get("usage").map(|u| Usage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
            });

            Ok(AIResponse {
                text,
                usage,
                model: Some(self.model.clone()),
            })
        }
    }

    /// Claude (Anthropic) API provider.
    pub struct ClaudeProvider {
        client: reqwest::Client,
        api_key: String,
        model: String,
    }

    impl ClaudeProvider {
        /// Create a new Claude provider.
        pub fn new(api_key: impl Into<String>) -> Self {
            Self {
                client: reqwest::Client::new(),
                api_key: api_key.into(),
                model: "claude-3-haiku-20240307".to_string(),
            }
        }

        /// Set the model to use.
        pub fn with_model(mut self, model: impl Into<String>) -> Self {
            self.model = model.into();
            self
        }
    }

    #[async_trait]
    impl AIProvider for ClaudeProvider {
        async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError> {
            debug!(model = %self.model, "Claude completion request");

            let mut body = serde_json::json!({
                "model": self.model,
                "messages": [{
                    "role": "user",
                    "content": request.prompt
                }],
                "max_tokens": request.max_tokens.unwrap_or(1024),
            });

            if let Some(system) = &request.system {
                body["system"] = serde_json::json!(system);
            }
            if let Some(temperature) = request.temperature {
                body["temperature"] = serde_json::json!(temperature);
            }

            let response = self
                .client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(AIError::RateLimited { retry_after: None });
            }

            if !response.status().is_success() {
                let error: serde_json::Value = response.json().await?;
                return Err(AIError::Api {
                    code: error["error"]["type"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    message: error["error"]["message"]
                        .as_str()
                        .unwrap_or("Unknown error")
                        .to_string(),
                });
            }

            let data: serde_json::Value = response.json().await?;
            let text = data["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string();

            let usage = data.get("usage").map(|u| Usage {
                prompt_tokens: u["input_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["output_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: (u["input_tokens"].as_u64().unwrap_or(0)
                    + u["output_tokens"].as_u64().unwrap_or(0))
                    as u32,
            });

            Ok(AIResponse {
                text,
                usage,
                model: Some(self.model.clone()),
            })
        }
    }

    /// Local LLM provider (compatible with OpenAI API format).
    pub struct LocalProvider {
        client: reqwest::Client,
        endpoint: String,
        model: String,
    }

    impl LocalProvider {
        /// Create a new local provider.
        pub fn new(endpoint: impl Into<String>) -> Self {
            Self {
                client: reqwest::Client::new(),
                endpoint: endpoint.into(),
                model: "default".to_string(),
            }
        }

        /// Set the model name.
        pub fn with_model(mut self, model: impl Into<String>) -> Self {
            self.model = model.into();
            self
        }
    }

    #[async_trait]
    impl AIProvider for LocalProvider {
        async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError> {
            debug!(endpoint = %self.endpoint, "Local LLM completion request");

            let mut messages = Vec::new();
            if let Some(system) = &request.system {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": system
                }));
            }
            messages.push(serde_json::json!({
                "role": "user",
                "content": request.prompt
            }));

            let mut body = serde_json::json!({
                "model": self.model,
                "messages": messages,
            });

            if let Some(max_tokens) = request.max_tokens {
                body["max_tokens"] = serde_json::json!(max_tokens);
            }
            if let Some(temperature) = request.temperature {
                body["temperature"] = serde_json::json!(temperature);
            }

            let response = self
                .client
                .post(format!("{}/v1/chat/completions", self.endpoint))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(AIError::Api {
                    code: "local_error".to_string(),
                    message: error_text,
                });
            }

            let data: serde_json::Value = response.json().await?;
            let text = data["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();

            Ok(AIResponse {
                text,
                usage: None,
                model: Some(self.model.clone()),
            })
        }
    }
}

#[cfg(feature = "ai")]
pub use providers::{ClaudeProvider, LocalProvider, OpenAIProvider};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_request_builder() {
        let request = AIRequest::new("Hello")
            .with_system("You are helpful")
            .with_max_tokens(100)
            .with_temperature(0.7);

        assert_eq!(request.prompt, "Hello");
        assert_eq!(request.system, Some("You are helpful".to_string()));
        assert_eq!(request.max_tokens, Some(100));
        assert_eq!(request.temperature, Some(0.7));
    }

    #[test]
    fn temperature_clamped() {
        let request = AIRequest::new("test").with_temperature(2.0);
        assert_eq!(request.temperature, Some(1.0));

        let request = AIRequest::new("test").with_temperature(-1.0);
        assert_eq!(request.temperature, Some(0.0));
    }

    #[tokio::test]
    async fn noop_provider() {
        let provider = NoopProvider;
        let response = provider.complete(AIRequest::new("test")).await.unwrap();
        assert!(response.text.is_empty());
    }
}
