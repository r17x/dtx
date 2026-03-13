use crate::store::MemoryStore;
use crate::types::{Memory, MemoryKind};

pub struct MemoryFilter {
    pub kind: Option<MemoryKind>,
    pub name_contains: Option<String>,
    pub content_contains: Option<String>,
    pub tags: Vec<String>,
}

impl Default for MemoryFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryFilter {
    pub fn new() -> Self {
        Self {
            kind: None,
            name_contains: None,
            content_contains: None,
            tags: vec![],
        }
    }

    pub fn kind(mut self, kind: MemoryKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn name_contains(mut self, s: impl Into<String>) -> Self {
        self.name_contains = Some(s.into());
        self
    }

    pub fn content_contains(mut self, s: impl Into<String>) -> Self {
        self.content_contains = Some(s.into());
        self
    }

    pub fn tag(mut self, t: impl Into<String>) -> Self {
        self.tags.push(t.into());
        self
    }

    fn matches_meta(&self, meta: &crate::types::MemoryMeta) -> bool {
        if let Some(kind) = self.kind {
            if meta.kind != kind {
                return false;
            }
        }
        if let Some(ref substr) = self.name_contains {
            if !meta.name.contains(substr.as_str()) {
                return false;
            }
        }
        if !self.tags.is_empty() && !self.tags.iter().any(|t| meta.tags.contains(t)) {
            return false;
        }
        true
    }

    fn matches_content(&self, content: &str) -> bool {
        if let Some(ref substr) = self.content_contains {
            if !content.contains(substr.as_str()) {
                return false;
            }
        }
        true
    }
}

pub fn search(store: &MemoryStore, filter: &MemoryFilter) -> crate::Result<Vec<Memory>> {
    let dir = store.memories_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut results = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let memory = match crate::types::Memory::from_file_content(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if filter.matches_meta(&memory.meta) && filter.matches_content(&memory.content) {
            results.push(memory);
        }
    }
    results.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MemoryKind, MemoryMeta};
    use chrono::Utc;
    use tempfile::TempDir;

    fn make_store() -> (TempDir, MemoryStore) {
        let dir = TempDir::new().expect("create temp dir");
        let store = MemoryStore::new(dir.path().join("memories")).expect("create store");
        (dir, store)
    }

    fn make_memory(name: &str, kind: MemoryKind, tags: Vec<&str>, content: &str) -> Memory {
        Memory {
            meta: MemoryMeta {
                name: name.to_string(),
                kind,
                description: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                tags: tags.into_iter().map(String::from).collect(),
            },
            content: content.to_string(),
        }
    }

    fn populate_store(store: &MemoryStore) {
        store
            .write(&make_memory(
                "user-prefs",
                MemoryKind::User,
                vec!["config"],
                "User prefers dark mode",
            ))
            .expect("write");
        store
            .write(&make_memory(
                "project-setup",
                MemoryKind::Project,
                vec!["rust", "nix"],
                "Project uses Nix flakes",
            ))
            .expect("write");
        store
            .write(&make_memory(
                "feedback-workflow",
                MemoryKind::Feedback,
                vec!["workflow"],
                "Always use atomic commits",
            ))
            .expect("write");
    }

    #[test]
    fn search_no_filter_returns_all() {
        let (_dir, store) = make_store();
        populate_store(&store);

        let results = search(&store, &MemoryFilter::new()).expect("search");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_by_kind() {
        let (_dir, store) = make_store();
        populate_store(&store);

        let filter = MemoryFilter::new().kind(MemoryKind::User);
        let results = search(&store, &filter).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meta.name, "user-prefs");
    }

    #[test]
    fn search_by_name_contains() {
        let (_dir, store) = make_store();
        populate_store(&store);

        let filter = MemoryFilter::new().name_contains("project");
        let results = search(&store, &filter).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meta.name, "project-setup");
    }

    #[test]
    fn search_by_content_contains() {
        let (_dir, store) = make_store();
        populate_store(&store);

        let filter = MemoryFilter::new().content_contains("atomic commits");
        let results = search(&store, &filter).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meta.name, "feedback-workflow");
    }

    #[test]
    fn search_by_tag() {
        let (_dir, store) = make_store();
        populate_store(&store);

        let filter = MemoryFilter::new().tag("rust");
        let results = search(&store, &filter).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meta.name, "project-setup");
    }

    #[test]
    fn search_combined_filters() {
        let (_dir, store) = make_store();
        populate_store(&store);

        let filter = MemoryFilter::new()
            .kind(MemoryKind::Project)
            .content_contains("Nix");
        let results = search(&store, &filter).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meta.name, "project-setup");
    }

    #[test]
    fn search_no_matches() {
        let (_dir, store) = make_store();
        populate_store(&store);

        let filter = MemoryFilter::new().name_contains("nonexistent");
        let results = search(&store, &filter).expect("search");
        assert!(results.is_empty());
    }
}
