use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use fs2::FileExt;
use regex::Regex;
use tracing::debug;

use crate::error::{MemoryError, Result};
use crate::types::{Memory, MemoryMeta};

static KEBAB_CASE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$").expect("valid regex"));

pub struct MemoryStore {
    root: PathBuf,
}

impl MemoryStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn memories_dir(&self) -> &Path {
        &self.root
    }

    pub fn memory_path(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.md"))
    }

    pub fn validate_name(name: &str) -> Result<()> {
        if name.len() < 2 || name.len() > 63 || !KEBAB_CASE_RE.is_match(name) {
            return Err(MemoryError::InvalidName(format!(
                "'{name}' must be 2-63 chars, kebab-case (lowercase alphanumeric + hyphens, no leading/trailing hyphens)"
            )));
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<MemoryMeta>> {
        let dir = self.memories_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut metas = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let text = std::fs::read_to_string(&path)?;
                match Memory::from_file_content(&text) {
                    Ok(mem) => metas.push(mem.meta),
                    Err(e) => {
                        debug!("Skipping malformed memory file {}: {}", path.display(), e);
                    }
                }
            }
        }
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(metas)
    }

    pub fn read(&self, name: &str) -> Result<Memory> {
        let path = self.memory_path(name);
        if !path.exists() {
            return Err(MemoryError::NotFound(name.to_string()));
        }
        let text = std::fs::read_to_string(&path)?;
        Memory::from_file_content(&text)
    }

    pub fn write(&self, memory: &Memory) -> Result<()> {
        Self::validate_name(&memory.meta.name)?;

        let path = self.memory_path(&memory.meta.name);
        let tmp_path = path.with_extension("md.tmp");
        let lock_path = path.with_extension("md.lock");

        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        lock_file.lock_exclusive()?;

        // Preserve created_at from existing memory on overwrite
        let to_write = match std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| Memory::from_file_content(&t).ok())
        {
            Some(existing) => {
                let mut updated = memory.clone();
                updated.meta.created_at = existing.meta.created_at;
                updated
            }
            None => memory.clone(),
        };

        let content = to_write.to_file_content();
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &path)?;

        lock_file.unlock()?;
        let _ = std::fs::remove_file(&lock_path);

        dtx_core::events::socket::notify_memory_changed_sync("", &memory.meta.name);

        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.memory_path(name);
        if !path.exists() {
            return Err(MemoryError::NotFound(name.to_string()));
        }

        std::fs::remove_file(&path)?;

        dtx_core::events::socket::notify_memory_changed_sync("", name);

        Ok(())
    }
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

    fn make_memory(name: &str) -> Memory {
        Memory {
            meta: MemoryMeta {
                name: name.to_string(),
                kind: MemoryKind::Project,
                description: Some("test".to_string()),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                tags: vec![],
            },
            content: "Hello world".to_string(),
        }
    }

    #[test]
    fn validate_name_accepts_valid() {
        assert!(MemoryStore::validate_name("ab").is_ok());
        assert!(MemoryStore::validate_name("my-memory").is_ok());
        assert!(MemoryStore::validate_name("test-123-foo").is_ok());
        assert!(MemoryStore::validate_name("a1").is_ok());
    }

    #[test]
    fn validate_name_rejects_invalid() {
        assert!(MemoryStore::validate_name("a").is_err()); // too short
        assert!(MemoryStore::validate_name("A").is_err()); // uppercase
        assert!(MemoryStore::validate_name("My-Memory").is_err()); // uppercase
        assert!(MemoryStore::validate_name("-leading").is_err());
        assert!(MemoryStore::validate_name("trailing-").is_err());
        assert!(MemoryStore::validate_name("has space").is_err());
        assert!(MemoryStore::validate_name("has_underscore").is_err());
        assert!(MemoryStore::validate_name("").is_err());
    }

    #[test]
    fn crud_lifecycle() {
        let (_dir, store) = make_store();
        let mem = make_memory("test-mem");

        // Write
        store.write(&mem).expect("write");

        // List
        let metas = store.list().expect("list");
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].name, "test-mem");

        // Read
        let read = store.read("test-mem").expect("read");
        assert_eq!(read.meta.name, "test-mem");
        assert_eq!(read.content, "Hello world");

        // Delete
        store.delete("test-mem").expect("delete");
        assert!(store.read("test-mem").is_err());
        assert_eq!(store.list().expect("list").len(), 0);
    }

    #[test]
    fn read_not_found() {
        let (_dir, store) = make_store();
        match store.read("nonexistent") {
            Err(MemoryError::NotFound(_)) => {}
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn delete_not_found() {
        let (_dir, store) = make_store();
        match store.delete("nonexistent") {
            Err(MemoryError::NotFound(_)) => {}
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn write_creates_parent_dir() {
        let dir = TempDir::new().expect("create temp dir");
        let nested = dir.path().join("a").join("b").join("memories");
        let store = MemoryStore::new(nested.clone()).expect("create store");
        assert!(nested.exists());

        let mem = make_memory("nested-test");
        store.write(&mem).expect("write");
        let read = store.read("nested-test").expect("read");
        assert_eq!(read.meta.name, "nested-test");
    }

    #[test]
    fn list_sorts_by_name() {
        let (_dir, store) = make_store();
        store.write(&make_memory("zz-last")).expect("write");
        store.write(&make_memory("aa-first")).expect("write");
        store.write(&make_memory("mm-middle")).expect("write");

        let metas = store.list().expect("list");
        let names: Vec<_> = metas.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["aa-first", "mm-middle", "zz-last"]);
    }

    #[test]
    fn overwrite_existing() {
        let (_dir, store) = make_store();
        let mut mem = make_memory("overwrite-me");
        store.write(&mem).expect("write");

        mem.content = "Updated content".to_string();
        store.write(&mem).expect("overwrite");

        let read = store.read("overwrite-me").expect("read");
        assert_eq!(read.content, "Updated content");
    }
}
