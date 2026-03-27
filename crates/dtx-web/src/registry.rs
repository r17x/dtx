//! Project registry for multi-project web server.
//!
//! Manages multiple project states within a single web server instance.
//! Each project has its own `ConfigStore`, `OrchestratorHandle`, `WorkspaceIndex`,
//! and optionally a `MemoryStore`.
//!
//! Uses `std::sync::RwLock` (not tokio) because operations are purely in-memory
//! map lookups — no async work happens under the lock.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use dtx_code::WorkspaceIndex;
use dtx_core::events::ResourceEventBus;
use dtx_core::store::ConfigStore;
use dtx_memory::MemoryStore;
use indexmap::IndexMap;
use tokio::sync::RwLock as TokioRwLock;

use crate::config::WebConfig;
use crate::service::OrchestratorHandle;

/// Per-project state.
#[derive(Clone)]
pub struct ProjectState {
    id: String,
    root: PathBuf,
    store: Arc<TokioRwLock<ConfigStore>>,
    orchestrator_handle: Arc<OrchestratorHandle>,
    workspace_index: Arc<WorkspaceIndex>,
    memory_store: Option<Arc<MemoryStore>>,
}

impl ProjectState {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn store(&self) -> &Arc<TokioRwLock<ConfigStore>> {
        &self.store
    }

    pub fn orchestrator_handle(&self) -> &Arc<OrchestratorHandle> {
        &self.orchestrator_handle
    }

    pub fn workspace_index(&self) -> &Arc<WorkspaceIndex> {
        &self.workspace_index
    }

    pub fn memory_store(&self) -> Option<&Arc<MemoryStore>> {
        self.memory_store.as_ref()
    }
}

/// Internal state protected by a single lock.
struct RegistryInner {
    projects: IndexMap<String, ProjectState>,
    active_id: String,
}

/// Registry of all projects served by this web server instance.
pub struct ProjectRegistry {
    inner: RwLock<RegistryInner>,
}

impl ProjectRegistry {
    /// Create a registry with an initial project.
    pub fn new(initial: ProjectState) -> Self {
        let id = initial.id.clone();
        let mut projects = IndexMap::new();
        projects.insert(id.clone(), initial);
        Self {
            inner: RwLock::new(RegistryInner {
                projects,
                active_id: id,
            }),
        }
    }

    /// Register a new project. Uses `build_project_state` internally.
    pub fn add(
        &self,
        store: ConfigStore,
        event_bus: &Arc<ResourceEventBus>,
        config: &Arc<WebConfig>,
    ) -> Result<String, String> {
        let state = build_project_state(store, event_bus, config);
        let id = state.id.clone();

        let mut inner = self.inner.write().unwrap();
        inner.projects.entry(id.clone()).or_insert(state);
        Ok(id)
    }

    /// Remove a project by ID. Returns the removed state.
    pub fn remove(&self, id: &str) -> Option<ProjectState> {
        let mut inner = self.inner.write().unwrap();
        if id == inner.active_id {
            return None; // Can't remove active project
        }
        inner.projects.shift_remove(id)
    }

    /// Get a project by ID.
    pub fn get(&self, id: &str) -> Option<ProjectState> {
        self.inner.read().unwrap().projects.get(id).cloned()
    }

    /// Get the active project.
    pub fn active(&self) -> ProjectState {
        let inner = self.inner.read().unwrap();
        inner
            .projects
            .get(&inner.active_id)
            .cloned()
            .expect("active project must exist in registry")
    }

    /// Set the active project. Returns error if project not found.
    pub fn set_active(&self, id: &str) -> Result<(), String> {
        let mut inner = self.inner.write().unwrap();
        if !inner.projects.contains_key(id) {
            return Err(format!("project not found: {}", id));
        }
        inner.active_id = id.to_string();
        Ok(())
    }

    /// List all projects as (id, root, is_active).
    pub fn list(&self) -> Vec<(String, PathBuf, bool)> {
        let inner = self.inner.read().unwrap();
        inner
            .projects
            .iter()
            .map(|(id, state)| (id.clone(), state.root.clone(), id == &inner.active_id))
            .collect()
    }

    /// Resolve a project query: if `query` is Some, look up by ID; otherwise return active.
    pub fn resolve(&self, query: Option<&str>) -> ProjectState {
        let inner = self.inner.read().unwrap();
        let id = match query {
            Some(id) if inner.projects.contains_key(id) => id,
            _ => &inner.active_id,
        };
        inner
            .projects
            .get(id)
            .cloned()
            .expect("resolved project must exist")
    }
}

/// Generate a short (6-char) hex project ID from a path.
pub fn project_id(root: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    format!("{:06x}", hasher.finish() & 0xFFFFFF)
}

/// Build a `ProjectState` from a `ConfigStore` and shared resources.
pub fn build_project_state(
    store: ConfigStore,
    event_bus: &Arc<ResourceEventBus>,
    config: &Arc<WebConfig>,
) -> ProjectState {
    let root = store.project_root().to_path_buf();
    let id = project_id(&root);

    let orchestrator_handle = Arc::new(OrchestratorHandle::new(event_bus.clone(), config.clone()));
    let workspace_index = Arc::new(WorkspaceIndex::new(root.clone()));
    let memory_store = MemoryStore::new(root.join(".dtx/memories"))
        .ok()
        .map(Arc::new);

    ProjectState {
        id,
        root,
        store: Arc::new(TokioRwLock::new(store)),
        orchestrator_handle,
        workspace_index,
        memory_store,
    }
}
