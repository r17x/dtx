//! Translator traits for type conversion.
//!
//! This module defines the core traits for translating between resource types.

use super::error::{TranslationError, TranslationResult};

/// Synchronous translator between two types.
///
/// Translators convert resources from one type to another, supporting
/// bidirectional translation where possible.
///
/// # Type Parameters
///
/// - `From`: Source type
/// - `To`: Target type
///
/// # Example
///
/// ```ignore
/// struct ProcessToContainer;
///
/// impl Translator<ProcessConfig, ContainerConfig> for ProcessToContainer {
///     fn translate(&self, from: &ProcessConfig) -> TranslationResult<ContainerConfig> {
///         Ok(ContainerConfig {
///             id: from.id.clone(),
///             image: infer_image(&from.command)?,
///             // ...
///         })
///     }
///
///     fn reverse(&self, to: &ContainerConfig) -> TranslationResult<ProcessConfig> {
///         Ok(ProcessConfig {
///             id: to.id.clone(),
///             command: to.command.join(" "),
///             // ...
///         })
///     }
///
///     fn supports_reverse(&self) -> bool {
///         true
///     }
/// }
/// ```
pub trait Translator<From, To>: Send + Sync {
    /// Translate from source to target type.
    fn translate(&self, from: &From) -> TranslationResult<To>;

    /// Reverse translate from target to source type.
    ///
    /// Not all translations are reversible. Implementations should return
    /// `TranslationError::Incompatible` if reverse is not supported.
    fn reverse(&self, to: &To) -> TranslationResult<From> {
        let _ = to;
        Err(TranslationError::incompatible(
            "reverse translation not supported",
        ))
    }

    /// Check if this translator supports reverse translation.
    fn supports_reverse(&self) -> bool {
        false
    }

    /// Get translator metadata.
    fn metadata(&self) -> TranslatorMetadata {
        TranslatorMetadata::default()
    }
}

/// Async translator for operations requiring I/O.
///
/// Use when translation requires network calls, file reads, or other
/// async operations (e.g., fetching container image metadata).
#[async_trait::async_trait]
pub trait AsyncTranslator<From, To>: Send + Sync
where
    From: Send + Sync,
    To: Send + Sync,
{
    /// Translate from source to target type asynchronously.
    async fn translate(&self, from: &From) -> TranslationResult<To>;

    /// Reverse translate asynchronously.
    async fn reverse(&self, to: &To) -> TranslationResult<From> {
        let _ = to;
        Err(TranslationError::incompatible(
            "reverse translation not supported",
        ))
    }

    /// Check if this translator supports reverse translation.
    fn supports_reverse(&self) -> bool {
        false
    }

    /// Get translator metadata.
    fn metadata(&self) -> TranslatorMetadata {
        TranslatorMetadata::default()
    }
}

/// Metadata about a translator.
#[derive(Clone, Debug, Default)]
pub struct TranslatorMetadata {
    /// Human-readable name.
    pub name: Option<String>,
    /// Description of what this translator does.
    pub description: Option<String>,
    /// Whether translation may be lossy.
    pub lossy: bool,
    /// Version of the translator.
    pub version: Option<String>,
}

impl TranslatorMetadata {
    /// Create new metadata with a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Default::default()
        }
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Mark as lossy.
    pub fn lossy(mut self) -> Self {
        self.lossy = true;
        self
    }

    /// Set version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DoubleTranslator;

    impl Translator<i32, i32> for DoubleTranslator {
        fn translate(&self, from: &i32) -> TranslationResult<i32> {
            Ok(from * 2)
        }

        fn reverse(&self, to: &i32) -> TranslationResult<i32> {
            Ok(to / 2)
        }

        fn supports_reverse(&self) -> bool {
            true
        }

        fn metadata(&self) -> TranslatorMetadata {
            TranslatorMetadata::new("DoubleTranslator")
                .with_description("Doubles integers")
                .with_version("1.0")
        }
    }

    #[test]
    fn translator_translate() {
        let t = DoubleTranslator;
        assert_eq!(t.translate(&5).unwrap(), 10);
    }

    #[test]
    fn translator_reverse() {
        let t = DoubleTranslator;
        assert_eq!(t.reverse(&10).unwrap(), 5);
    }

    #[test]
    fn translator_roundtrip() {
        let t = DoubleTranslator;
        let original = 10;
        let translated = t.translate(&original).unwrap();
        let reversed = t.reverse(&translated).unwrap();
        assert_eq!(original, reversed);
    }

    #[test]
    fn translator_supports_reverse() {
        let t = DoubleTranslator;
        assert!(t.supports_reverse());
    }

    #[test]
    fn translator_metadata() {
        let t = DoubleTranslator;
        let meta = t.metadata();
        assert_eq!(meta.name, Some("DoubleTranslator".to_string()));
        assert_eq!(meta.description, Some("Doubles integers".to_string()));
        assert_eq!(meta.version, Some("1.0".to_string()));
        assert!(!meta.lossy);
    }

    struct OneWayTranslator;

    impl Translator<String, usize> for OneWayTranslator {
        fn translate(&self, from: &String) -> TranslationResult<usize> {
            Ok(from.len())
        }

        fn metadata(&self) -> TranslatorMetadata {
            TranslatorMetadata::new("StringLength").lossy()
        }
    }

    #[test]
    fn translator_no_reverse() {
        let t = OneWayTranslator;
        assert!(!t.supports_reverse());
        assert!(t.reverse(&5).is_err());
    }

    #[test]
    fn translator_lossy_metadata() {
        let t = OneWayTranslator;
        assert!(t.metadata().lossy);
    }

    #[test]
    fn metadata_builder() {
        let meta = TranslatorMetadata::new("Test")
            .with_description("A test translator")
            .with_version("2.0")
            .lossy();

        assert_eq!(meta.name, Some("Test".to_string()));
        assert_eq!(meta.description, Some("A test translator".to_string()));
        assert_eq!(meta.version, Some("2.0".to_string()));
        assert!(meta.lossy);
    }

    #[test]
    fn metadata_default() {
        let meta = TranslatorMetadata::default();
        assert!(meta.name.is_none());
        assert!(meta.description.is_none());
        assert!(meta.version.is_none());
        assert!(!meta.lossy);
    }
}
