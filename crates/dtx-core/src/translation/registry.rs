//! Translator registry for type-based lookup.
//!
//! This module provides a registry that stores translators and allows
//! looking them up by source and target types.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use super::context::TranslationContext;
use super::contextual::ContextualTranslator;
use super::error::{TranslationError, TranslationResult};
use super::traits::{Translator, TranslatorMetadata};

/// Type-erased translator wrapper.
trait AnyTranslator: Send + Sync {
    fn translate_any(
        &self,
        from: &dyn Any,
        ctx: &TranslationContext,
    ) -> TranslationResult<Box<dyn Any + Send>>;

    fn reverse_any(
        &self,
        to: &dyn Any,
        ctx: &TranslationContext,
    ) -> TranslationResult<Box<dyn Any + Send>>;

    fn supports_reverse(&self) -> bool;
    fn metadata(&self) -> TranslatorMetadata;
}

/// Wrapper to implement AnyTranslator for concrete types.
struct TranslatorWrapper<T, From, To> {
    translator: T,
    from_type_name: &'static str,
    to_type_name: &'static str,
    _marker: PhantomData<fn(From) -> To>,
}

impl<T, From, To> AnyTranslator for TranslatorWrapper<T, From, To>
where
    T: ContextualTranslator<From, To> + 'static,
    From: 'static + Send,
    To: 'static + Send,
{
    fn translate_any(
        &self,
        from: &dyn Any,
        ctx: &TranslationContext,
    ) -> TranslationResult<Box<dyn Any + Send>> {
        let from = from.downcast_ref::<From>().ok_or_else(|| {
            TranslationError::failed(format!(
                "type mismatch: expected {}, got unknown type",
                self.from_type_name
            ))
        })?;
        let result = self.translator.translate(from, ctx)?;
        Ok(Box::new(result))
    }

    fn reverse_any(
        &self,
        to: &dyn Any,
        ctx: &TranslationContext,
    ) -> TranslationResult<Box<dyn Any + Send>> {
        let to = to.downcast_ref::<To>().ok_or_else(|| {
            TranslationError::failed(format!(
                "type mismatch: expected {}, got unknown type",
                self.to_type_name
            ))
        })?;
        let result = self.translator.reverse(to, ctx)?;
        Ok(Box::new(result))
    }

    fn supports_reverse(&self) -> bool {
        self.translator.supports_reverse()
    }

    fn metadata(&self) -> TranslatorMetadata {
        self.translator.metadata()
    }
}

/// Key for translator lookup.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct TranslatorKey {
    from: TypeId,
    to: TypeId,
}

impl TranslatorKey {
    fn new<From: 'static, To: 'static>() -> Self {
        Self {
            from: TypeId::of::<From>(),
            to: TypeId::of::<To>(),
        }
    }
}

/// Registry of translators.
///
/// Provides type-based lookup of translators for converting between
/// resource types.
///
/// # Example
///
/// ```ignore
/// let mut registry = TranslatorRegistry::new();
/// registry.register(ProcessToContainerTranslator);
///
/// let process = ProcessConfig::new("api", "cargo run");
/// let container: ContainerConfig = registry.translate(&process)?;
/// ```
pub struct TranslatorRegistry {
    translators: HashMap<TranslatorKey, Arc<dyn AnyTranslator>>,
}

impl TranslatorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            translators: HashMap::new(),
        }
    }

    /// Register a contextual translator.
    pub fn register<From, To, TR>(&mut self, translator: TR) -> &mut Self
    where
        From: 'static + Send,
        To: 'static + Send,
        TR: ContextualTranslator<From, To> + 'static,
    {
        let key = TranslatorKey::new::<From, To>();
        let wrapper = TranslatorWrapper {
            translator,
            from_type_name: std::any::type_name::<From>(),
            to_type_name: std::any::type_name::<To>(),
            _marker: PhantomData,
        };
        self.translators.insert(key, Arc::new(wrapper));
        self
    }

    /// Register a simple translator (wraps with ContextAdapter).
    pub fn register_simple<From, To, TR>(&mut self, translator: TR) -> &mut Self
    where
        From: 'static + Send,
        To: 'static + Send,
        TR: Translator<From, To> + 'static,
    {
        self.register(super::contextual::ContextAdapter(translator))
    }

    /// Translate with default context.
    pub fn translate<From: 'static, To: 'static>(&self, from: &From) -> TranslationResult<To> {
        self.translate_with_context(from, &TranslationContext::default())
    }

    /// Translate with context.
    pub fn translate_with_context<From: 'static, To: 'static>(
        &self,
        from: &From,
        ctx: &TranslationContext,
    ) -> TranslationResult<To> {
        let key = TranslatorKey::new::<From, To>();
        let translator = self.translators.get(&key).ok_or_else(|| {
            TranslationError::no_translator(
                std::any::type_name::<From>(),
                std::any::type_name::<To>(),
            )
        })?;

        let result = translator.translate_any(from, ctx)?;
        result
            .downcast::<To>()
            .map(|b| *b)
            .map_err(|_| TranslationError::failed("downcast failed after translation"))
    }

    /// Reverse translate with default context.
    pub fn reverse<From: 'static, To: 'static>(&self, to: &To) -> TranslationResult<From> {
        self.reverse_with_context(to, &TranslationContext::default())
    }

    /// Reverse translate with context.
    pub fn reverse_with_context<From: 'static, To: 'static>(
        &self,
        to: &To,
        ctx: &TranslationContext,
    ) -> TranslationResult<From> {
        let key = TranslatorKey::new::<From, To>();
        let translator = self.translators.get(&key).ok_or_else(|| {
            TranslationError::no_translator(
                std::any::type_name::<From>(),
                std::any::type_name::<To>(),
            )
        })?;

        if !translator.supports_reverse() {
            return Err(TranslationError::incompatible(format!(
                "reverse translation not supported for {} -> {}",
                std::any::type_name::<From>(),
                std::any::type_name::<To>()
            )));
        }

        let result = translator.reverse_any(to, ctx)?;
        result
            .downcast::<From>()
            .map(|b| *b)
            .map_err(|_| TranslationError::failed("downcast failed after reverse translation"))
    }

    /// Check if a translator is registered.
    pub fn has_translator<From: 'static, To: 'static>(&self) -> bool {
        let key = TranslatorKey::new::<From, To>();
        self.translators.contains_key(&key)
    }

    /// Check if reverse translation is supported.
    pub fn supports_reverse<From: 'static, To: 'static>(&self) -> bool {
        let key = TranslatorKey::new::<From, To>();
        self.translators
            .get(&key)
            .map(|t| t.supports_reverse())
            .unwrap_or(false)
    }

    /// Get translator metadata.
    pub fn metadata<From: 'static, To: 'static>(&self) -> Option<TranslatorMetadata> {
        let key = TranslatorKey::new::<From, To>();
        self.translators.get(&key).map(|t| t.metadata())
    }

    /// Get the number of registered translators.
    pub fn len(&self) -> usize {
        self.translators.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.translators.is_empty()
    }
}

impl Default for TranslatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a registered translator.
#[derive(Clone, Debug)]
pub struct TranslatorInfo {
    pub metadata: TranslatorMetadata,
    pub supports_reverse: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct TypeA(i32);

    #[derive(Clone, Debug, PartialEq)]
    struct TypeB(String);

    struct AToB;

    impl ContextualTranslator<TypeA, TypeB> for AToB {
        fn translate(&self, from: &TypeA, _ctx: &TranslationContext) -> TranslationResult<TypeB> {
            Ok(TypeB(from.0.to_string()))
        }

        fn reverse(&self, to: &TypeB, _ctx: &TranslationContext) -> TranslationResult<TypeA> {
            to.0.parse()
                .map(TypeA)
                .map_err(|_| TranslationError::failed("parse error"))
        }

        fn supports_reverse(&self) -> bool {
            true
        }

        fn metadata(&self) -> TranslatorMetadata {
            TranslatorMetadata::new("AToB").with_description("Converts A to B")
        }
    }

    #[test]
    fn registry_new() {
        let registry = TranslatorRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn registry_register() {
        let mut registry = TranslatorRegistry::new();
        registry.register(AToB);
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registry_translate() {
        let mut registry = TranslatorRegistry::new();
        registry.register(AToB);

        let a = TypeA(42);
        let b: TypeB = registry.translate(&a).unwrap();
        assert_eq!(b.0, "42");
    }

    #[test]
    fn registry_translate_with_context() {
        let mut registry = TranslatorRegistry::new();
        registry.register(AToB);

        let a = TypeA(100);
        let ctx = TranslationContext::new();
        let b: TypeB = registry.translate_with_context(&a, &ctx).unwrap();
        assert_eq!(b.0, "100");
    }

    #[test]
    fn registry_reverse() {
        let mut registry = TranslatorRegistry::new();
        registry.register(AToB);

        let b = TypeB("123".to_string());
        let a: TypeA = registry.reverse(&b).unwrap();
        assert_eq!(a.0, 123);
    }

    #[test]
    fn registry_roundtrip() {
        let mut registry = TranslatorRegistry::new();
        registry.register(AToB);

        let original = TypeA(999);
        let translated: TypeB = registry.translate(&original).unwrap();
        let recovered: TypeA = registry.reverse(&translated).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn registry_no_translator() {
        let registry = TranslatorRegistry::new();
        let result: TranslationResult<TypeB> = registry.translate(&TypeA(1));
        assert!(matches!(result, Err(TranslationError::NoTranslator { .. })));
    }

    #[test]
    fn registry_has_translator() {
        let mut registry = TranslatorRegistry::new();
        assert!(!registry.has_translator::<TypeA, TypeB>());

        registry.register(AToB);
        assert!(registry.has_translator::<TypeA, TypeB>());
        assert!(!registry.has_translator::<TypeB, TypeA>()); // Reverse direction not registered
    }

    #[test]
    fn registry_supports_reverse() {
        let mut registry = TranslatorRegistry::new();
        assert!(!registry.supports_reverse::<TypeA, TypeB>());

        registry.register(AToB);
        assert!(registry.supports_reverse::<TypeA, TypeB>());
    }

    #[test]
    fn registry_metadata() {
        let mut registry = TranslatorRegistry::new();
        assert!(registry.metadata::<TypeA, TypeB>().is_none());

        registry.register(AToB);
        let meta = registry.metadata::<TypeA, TypeB>().unwrap();
        assert_eq!(meta.name, Some("AToB".to_string()));
    }

    // Test simple translator registration
    struct SimpleDoubler;

    impl Translator<i32, i32> for SimpleDoubler {
        fn translate(&self, from: &i32) -> TranslationResult<i32> {
            Ok(from * 2)
        }

        fn reverse(&self, to: &i32) -> TranslationResult<i32> {
            Ok(to / 2)
        }

        fn supports_reverse(&self) -> bool {
            true
        }
    }

    #[test]
    fn registry_register_simple() {
        let mut registry = TranslatorRegistry::new();
        registry.register_simple(SimpleDoubler);

        let result: i32 = registry.translate(&5).unwrap();
        assert_eq!(result, 10);
    }

    #[test]
    fn registry_register_simple_reverse() {
        let mut registry = TranslatorRegistry::new();
        registry.register_simple(SimpleDoubler);

        let result: i32 = registry.reverse(&10).unwrap();
        assert_eq!(result, 5);
    }

    // Test non-reversible translator
    struct OneWay;

    impl ContextualTranslator<String, usize> for OneWay {
        fn translate(&self, from: &String, _ctx: &TranslationContext) -> TranslationResult<usize> {
            Ok(from.len())
        }
    }

    #[test]
    fn registry_no_reverse_support() {
        let mut registry = TranslatorRegistry::new();
        registry.register(OneWay);

        assert!(!registry.supports_reverse::<String, usize>());
        let result: TranslationResult<String> = registry.reverse(&5usize);
        assert!(matches!(result, Err(TranslationError::Incompatible(_))));
    }

    #[test]
    fn registry_chained_registration() {
        let mut registry = TranslatorRegistry::new();
        registry.register(AToB).register_simple(SimpleDoubler);

        assert_eq!(registry.len(), 2);
        assert!(registry.has_translator::<TypeA, TypeB>());
        assert!(registry.has_translator::<i32, i32>());
    }

    #[test]
    fn registry_default() {
        let registry = TranslatorRegistry::default();
        assert!(registry.is_empty());
    }
}
