//! Context-aware translator types.
//!
//! This module provides translators that accept context for configuration.

use super::context::TranslationContext;
use super::error::{TranslationError, TranslationResult};
use super::traits::{Translator, TranslatorMetadata};

/// Translator that uses context for configuration.
///
/// Extends the basic Translator trait with context awareness,
/// allowing translations to be influenced by mappings and options.
pub trait ContextualTranslator<From, To>: Send + Sync {
    /// Translate with context.
    fn translate(&self, from: &From, ctx: &TranslationContext) -> TranslationResult<To>;

    /// Reverse translate with context.
    fn reverse(&self, to: &To, ctx: &TranslationContext) -> TranslationResult<From> {
        let _ = (to, ctx);
        Err(TranslationError::incompatible(
            "reverse translation not supported",
        ))
    }

    /// Check if reverse is supported.
    fn supports_reverse(&self) -> bool {
        false
    }

    /// Get metadata.
    fn metadata(&self) -> TranslatorMetadata {
        TranslatorMetadata::default()
    }
}

/// Async contextual translator.
#[async_trait::async_trait]
pub trait AsyncContextualTranslator<From, To>: Send + Sync
where
    From: Send + Sync,
    To: Send + Sync,
{
    /// Translate with context asynchronously.
    async fn translate(&self, from: &From, ctx: &TranslationContext) -> TranslationResult<To>;

    /// Reverse translate with context asynchronously.
    async fn reverse(&self, to: &To, ctx: &TranslationContext) -> TranslationResult<From> {
        let _ = (to, ctx);
        Err(TranslationError::incompatible(
            "reverse translation not supported",
        ))
    }

    /// Check if reverse is supported.
    fn supports_reverse(&self) -> bool {
        false
    }

    /// Get metadata.
    fn metadata(&self) -> TranslatorMetadata {
        TranslatorMetadata::default()
    }
}

/// Adapt a simple Translator to ContextualTranslator.
///
/// This wrapper allows non-contextual translators to be used
/// where a contextual translator is expected. The context is
/// simply ignored.
///
/// # Example
///
/// ```ignore
/// let simple = SimpleTranslator;
/// let contextual = ContextAdapter(simple);
///
/// // Can now use with context (which is ignored)
/// let result = contextual.translate(&input, &TranslationContext::new())?;
/// ```
pub struct ContextAdapter<T>(pub T);

impl<T, From, To> ContextualTranslator<From, To> for ContextAdapter<T>
where
    T: Translator<From, To>,
{
    fn translate(&self, from: &From, _ctx: &TranslationContext) -> TranslationResult<To> {
        self.0.translate(from)
    }

    fn reverse(&self, to: &To, _ctx: &TranslationContext) -> TranslationResult<From> {
        self.0.reverse(to)
    }

    fn supports_reverse(&self) -> bool {
        self.0.supports_reverse()
    }

    fn metadata(&self) -> TranslatorMetadata {
        self.0.metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple translator for testing
    struct StringToLen;

    impl Translator<String, usize> for StringToLen {
        fn translate(&self, from: &String) -> TranslationResult<usize> {
            Ok(from.len())
        }

        fn metadata(&self) -> TranslatorMetadata {
            TranslatorMetadata::new("StringToLen")
        }
    }

    // Contextual translator that uses context
    struct ContextualMultiplier;

    impl ContextualTranslator<i32, i32> for ContextualMultiplier {
        fn translate(&self, from: &i32, ctx: &TranslationContext) -> TranslationResult<i32> {
            let multiplier: i32 = ctx.get_default("multiplier").unwrap_or(1);
            Ok(from * multiplier)
        }

        fn reverse(&self, to: &i32, ctx: &TranslationContext) -> TranslationResult<i32> {
            let multiplier: i32 = ctx.get_default("multiplier").unwrap_or(1);
            if multiplier == 0 {
                return Err(TranslationError::failed("cannot divide by zero"));
            }
            Ok(to / multiplier)
        }

        fn supports_reverse(&self) -> bool {
            true
        }

        fn metadata(&self) -> TranslatorMetadata {
            TranslatorMetadata::new("ContextualMultiplier")
                .with_description("Multiplies by context value")
        }
    }

    #[test]
    fn context_adapter_translate() {
        let adapted = ContextAdapter(StringToLen);
        let ctx = TranslationContext::new();
        let result = adapted.translate(&"hello".to_string(), &ctx).unwrap();
        assert_eq!(result, 5);
    }

    #[test]
    fn context_adapter_ignores_context() {
        let adapted = ContextAdapter(StringToLen);
        // Context values are ignored
        let ctx = TranslationContext::new()
            .default_value("irrelevant", 42i32)
            .map_field("ignored", "also_ignored");
        let result = adapted.translate(&"test".to_string(), &ctx).unwrap();
        assert_eq!(result, 4);
    }

    #[test]
    fn context_adapter_no_reverse() {
        let adapted = ContextAdapter(StringToLen);
        let ctx = TranslationContext::new();
        assert!(!adapted.supports_reverse());
        assert!(adapted.reverse(&5, &ctx).is_err());
    }

    #[test]
    fn context_adapter_metadata() {
        let adapted = ContextAdapter(StringToLen);
        let meta = adapted.metadata();
        assert_eq!(meta.name, Some("StringToLen".to_string()));
    }

    #[test]
    fn contextual_translator_uses_context() {
        let translator = ContextualMultiplier;
        let ctx = TranslationContext::new().default_value("multiplier", 3i32);
        let result = translator.translate(&10, &ctx).unwrap();
        assert_eq!(result, 30);
    }

    #[test]
    fn contextual_translator_default_context() {
        let translator = ContextualMultiplier;
        let ctx = TranslationContext::new(); // No multiplier set
        let result = translator.translate(&10, &ctx).unwrap();
        assert_eq!(result, 10); // Uses default multiplier of 1
    }

    #[test]
    fn contextual_translator_reverse() {
        let translator = ContextualMultiplier;
        let ctx = TranslationContext::new().default_value("multiplier", 5i32);
        let result = translator.reverse(&50, &ctx).unwrap();
        assert_eq!(result, 10);
    }

    #[test]
    fn contextual_translator_supports_reverse() {
        let translator = ContextualMultiplier;
        assert!(translator.supports_reverse());
    }

    #[test]
    fn contextual_translator_metadata() {
        let translator = ContextualMultiplier;
        let meta = translator.metadata();
        assert_eq!(meta.name, Some("ContextualMultiplier".to_string()));
        assert!(meta.description.is_some());
    }

    // Test default trait implementations
    struct MinimalContextual;

    impl ContextualTranslator<i32, String> for MinimalContextual {
        fn translate(&self, from: &i32, _ctx: &TranslationContext) -> TranslationResult<String> {
            Ok(from.to_string())
        }
    }

    #[test]
    fn default_reverse_returns_error() {
        let translator = MinimalContextual;
        let ctx = TranslationContext::new();
        assert!(!translator.supports_reverse());
        let result = translator.reverse(&"42".to_string(), &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn default_metadata_is_empty() {
        let translator = MinimalContextual;
        let meta = translator.metadata();
        assert!(meta.name.is_none());
        assert!(meta.description.is_none());
    }
}
