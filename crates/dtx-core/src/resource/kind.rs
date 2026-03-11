//! ResourceKind - The type of resource being orchestrated.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The type of resource being orchestrated.
///
/// This enum defines the different categories of resources that dtx can manage.
/// Each kind may have different backends and capabilities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "lowercase")]
pub enum ResourceKind {
    /// Native OS process.
    #[default]
    Process,

    /// Container (Docker, Podman, etc.).
    Container,

    /// Virtual machine (QEMU, Nix VMs, Firecracker).
    #[serde(rename = "vm")]
    VM,

    /// AI agent or LLM worker.
    Agent,

    /// Plugin-defined resource type.
    ///
    /// The u16 identifies the plugin that defines this resource type.
    Custom(u16),
}

impl ResourceKind {
    /// Get a string representation of the kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Container => "container",
            Self::VM => "vm",
            Self::Agent => "agent",
            Self::Custom(_) => "custom",
        }
    }

    /// Check if this is a built-in kind (not custom).
    pub fn is_builtin(&self) -> bool {
        !matches!(self, Self::Custom(_))
    }

    /// Get the custom plugin ID if this is a custom kind.
    pub fn custom_id(&self) -> Option<u16> {
        match self {
            Self::Custom(id) => Some(*id),
            _ => None,
        }
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Custom(id) => write!(f, "custom:{}", id),
            _ => write!(f, "{}", self.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_kind_as_str() {
        assert_eq!(ResourceKind::Process.as_str(), "process");
        assert_eq!(ResourceKind::Container.as_str(), "container");
        assert_eq!(ResourceKind::VM.as_str(), "vm");
        assert_eq!(ResourceKind::Agent.as_str(), "agent");
        assert_eq!(ResourceKind::Custom(42).as_str(), "custom");
    }

    #[test]
    fn resource_kind_display() {
        assert_eq!(ResourceKind::Process.to_string(), "process");
        assert_eq!(ResourceKind::VM.to_string(), "vm");
        assert_eq!(ResourceKind::Custom(42).to_string(), "custom:42");
    }

    #[test]
    fn resource_kind_default() {
        assert_eq!(ResourceKind::default(), ResourceKind::Process);
    }

    #[test]
    fn resource_kind_is_builtin() {
        assert!(ResourceKind::Process.is_builtin());
        assert!(ResourceKind::Container.is_builtin());
        assert!(!ResourceKind::Custom(1).is_builtin());
    }

    #[test]
    fn resource_kind_custom_id() {
        assert_eq!(ResourceKind::Process.custom_id(), None);
        assert_eq!(ResourceKind::Custom(42).custom_id(), Some(42));
    }

    #[test]
    fn resource_kind_serde_process() {
        let kind = ResourceKind::Process;
        let json = serde_json::to_value(kind).unwrap();
        assert_eq!(json, serde_json::json!({"type": "process"}));

        let parsed: ResourceKind = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, kind);
    }

    #[test]
    fn resource_kind_serde_custom() {
        let kind = ResourceKind::Custom(42);
        let json = serde_json::to_value(kind).unwrap();
        assert_eq!(json, serde_json::json!({"type": "custom", "id": 42}));

        let parsed: ResourceKind = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, kind);
    }

    #[test]
    fn resource_kind_equality() {
        assert_eq!(ResourceKind::Process, ResourceKind::Process);
        assert_ne!(ResourceKind::Process, ResourceKind::Container);
        assert_eq!(ResourceKind::Custom(1), ResourceKind::Custom(1));
        assert_ne!(ResourceKind::Custom(1), ResourceKind::Custom(2));
    }
}
