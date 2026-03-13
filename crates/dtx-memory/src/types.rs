use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    User,
    Project,
    Feedback,
    Reference,
}

impl std::fmt::Display for MemoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
            Self::Feedback => write!(f, "feedback"),
            Self::Reference => write!(f, "reference"),
        }
    }
}

impl std::str::FromStr for MemoryKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(Self::User),
            "project" => Ok(Self::Project),
            "feedback" => Ok(Self::Feedback),
            "reference" => Ok(Self::Reference),
            _ => Err(format!("Unknown memory kind: {s}")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryMeta {
    pub name: String,
    pub kind: MemoryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Memory {
    pub meta: MemoryMeta,
    pub content: String,
}

impl Memory {
    pub fn to_file_content(&self) -> String {
        let frontmatter = serde_yaml::to_string(&self.meta).unwrap_or_default();
        format!("---\n{}---\n\n{}\n", frontmatter, self.content.trim())
    }

    /// Parse a memory from file content. If frontmatter is present, parse it.
    /// Otherwise treat the entire content as the body with a default meta derived from `name`.
    pub fn from_file_content(text: &str, name: &str) -> crate::Result<Self> {
        let trimmed = text.trim_start();
        if let Some(after_marker) = trimmed.strip_prefix("---") {
            let after_first = after_marker.trim_start_matches('\n');
            let end = after_first
                .find("\n---")
                .ok_or_else(|| crate::MemoryError::Frontmatter("Missing closing ---".into()))?;
            let yaml_str = &after_first[..end];
            let content_start = end + 4; // skip "\n---"
            let content = after_first[content_start..].trim().to_string();
            let meta: MemoryMeta = serde_yaml::from_str(yaml_str)
                .map_err(|e| crate::MemoryError::Frontmatter(e.to_string()))?;
            Ok(Self { meta, content })
        } else {
            let now = chrono::Utc::now();
            Ok(Self {
                meta: MemoryMeta {
                    name: name.to_string(),
                    kind: MemoryKind::Project,
                    description: None,
                    created_at: now,
                    updated_at: now,
                    tags: vec![],
                },
                content: trimmed.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_memory() -> Memory {
        Memory {
            meta: MemoryMeta {
                name: "test-memory".to_string(),
                kind: MemoryKind::Project,
                description: Some("A test memory".to_string()),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                tags: vec!["rust".to_string(), "test".to_string()],
            },
            content: "This is the body content.\n\nWith multiple paragraphs.".to_string(),
        }
    }

    #[test]
    fn frontmatter_roundtrip() {
        let original = sample_memory();
        let serialized = original.to_file_content();
        let parsed =
            Memory::from_file_content(&serialized, "test-memory").expect("parse should succeed");

        assert_eq!(parsed.meta.name, original.meta.name);
        assert_eq!(parsed.meta.kind, original.meta.kind);
        assert_eq!(parsed.meta.description, original.meta.description);
        assert_eq!(parsed.meta.tags, original.meta.tags);
        assert_eq!(parsed.content, original.content.trim());
    }

    #[test]
    fn frontmatter_no_optional_fields() {
        let mem = Memory {
            meta: MemoryMeta {
                name: "minimal".to_string(),
                kind: MemoryKind::User,
                description: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                tags: vec![],
            },
            content: "Just content.".to_string(),
        };
        let serialized = mem.to_file_content();
        let parsed =
            Memory::from_file_content(&serialized, "minimal").expect("parse should succeed");
        assert_eq!(parsed.meta.name, "minimal");
        assert!(parsed.meta.description.is_none());
        assert!(parsed.meta.tags.is_empty());
    }

    #[test]
    fn plain_markdown_without_frontmatter() {
        let result = Memory::from_file_content("no frontmatter here", "my-note");
        let mem = result.expect("should succeed for plain markdown");
        assert_eq!(mem.meta.name, "my-note");
        assert_eq!(mem.meta.kind, MemoryKind::Project);
        assert_eq!(mem.content, "no frontmatter here");
    }

    #[test]
    fn frontmatter_missing_closing() {
        let result = Memory::from_file_content("---\nname: test\n", "test");
        assert!(result.is_err());
    }

    #[test]
    fn memory_kind_display_and_parse() {
        for kind in [
            MemoryKind::User,
            MemoryKind::Project,
            MemoryKind::Feedback,
            MemoryKind::Reference,
        ] {
            let s = kind.to_string();
            let parsed: MemoryKind = s.parse().expect("should parse");
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn memory_kind_parse_unknown() {
        let result = "unknown".parse::<MemoryKind>();
        assert!(result.is_err());
    }
}
