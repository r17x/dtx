//! Resource orchestrator for dependency-aware lifecycle management.
//!
//! The orchestrator manages a collection of resources, handling:
//! - Dependency-ordered startup (topological sort)
//! - Reverse-order shutdown
//! - Health check coordination
//! - Restart handling

use chrono::Utc;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use dtx_core::events::{DependencyCondition, LifecycleEvent, ResourceEventBus};
use dtx_core::resource::{Context, HealthStatus, Resource, ResourceId, ResourceState};

use crate::config::ProcessResourceConfig;
use crate::ProcessResource;

/// Result of starting all resources.
#[derive(Clone, Debug)]
pub struct StartAllResult {
    /// Resources that started successfully.
    pub started: Vec<ResourceId>,
    /// Resources that failed to start with error messages.
    pub failed: Vec<(ResourceId, String)>,
    /// Resources that were skipped due to failed dependencies.
    pub skipped: Vec<ResourceId>,
}

impl StartAllResult {
    fn new() -> Self {
        Self {
            started: Vec::new(),
            failed: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// Check if all resources started successfully.
    pub fn is_success(&self) -> bool {
        self.failed.is_empty() && self.skipped.is_empty()
    }
}

/// Dependency specification for a resource.
#[derive(Clone, Debug)]
pub struct Dependency {
    /// The resource this depends on.
    pub resource_id: ResourceId,
    /// The condition that must be met.
    pub condition: DependencyCondition,
}

impl Dependency {
    /// Create a new dependency that waits for started.
    pub fn started(id: impl Into<ResourceId>) -> Self {
        Self {
            resource_id: id.into(),
            condition: DependencyCondition::Started,
        }
    }

    /// Create a new dependency that waits for healthy.
    pub fn healthy(id: impl Into<ResourceId>) -> Self {
        Self {
            resource_id: id.into(),
            condition: DependencyCondition::Healthy,
        }
    }

    /// Create a new dependency that waits for completion.
    pub fn completed(id: impl Into<ResourceId>) -> Self {
        Self {
            resource_id: id.into(),
            condition: DependencyCondition::Completed,
        }
    }
}

/// Orchestrates resource lifecycle with dependency ordering.
pub struct ResourceOrchestrator {
    /// Resources being managed.
    resources: HashMap<ResourceId, Arc<RwLock<ProcessResource>>>,
    /// Dependency graph: resource -> dependencies.
    dependencies: HashMap<ResourceId, Vec<Dependency>>,
    /// Event bus for lifecycle events.
    event_bus: Arc<ResourceEventBus>,
    /// Startup order (topologically sorted).
    startup_order: Vec<ResourceId>,
    /// Whether the orchestrator is running.
    running: bool,
}

impl ResourceOrchestrator {
    /// Create a new orchestrator.
    pub fn new(event_bus: Arc<ResourceEventBus>) -> Self {
        Self {
            resources: HashMap::new(),
            dependencies: HashMap::new(),
            event_bus,
            startup_order: Vec::new(),
            running: false,
        }
    }

    /// Add a resource to the orchestrator.
    pub fn add_resource(&mut self, config: ProcessResourceConfig) {
        let id = config.id.clone();
        let depends_on = config.depends_on.clone();
        let deps: Vec<Dependency> = depends_on.into_iter().map(Dependency::started).collect();

        let resource = ProcessResource::new(config, self.event_bus.clone());
        self.resources
            .insert(id.clone(), Arc::new(RwLock::new(resource)));
        self.dependencies.insert(id, deps);
    }
    /// Get a resource by ID.
    pub fn get_resource(&self, id: &ResourceId) -> Option<Arc<RwLock<ProcessResource>>> {
        self.resources.get(id).cloned()
    }

    /// Get all resource IDs.
    pub fn resource_ids(&self) -> impl Iterator<Item = &ResourceId> {
        self.resources.keys()
    }

    /// Compute topological order for startup.
    fn compute_startup_order(&mut self) -> Result<(), String> {
        let mut in_degree: HashMap<&ResourceId, usize> = HashMap::new();
        let mut graph: HashMap<&ResourceId, Vec<&ResourceId>> = HashMap::new();

        // Initialize
        for id in self.resources.keys() {
            in_degree.insert(id, 0);
            graph.insert(id, Vec::new());
        }

        // Build graph (reverse edges for startup order)
        for (id, deps) in &self.dependencies {
            for dep in deps {
                if let Some(edges) = graph.get_mut(&dep.resource_id) {
                    edges.push(id);
                }
                if let Some(count) = in_degree.get_mut(id) {
                    *count += 1;
                }
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<&ResourceId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| *id)
            .collect();

        let mut order = Vec::new();

        while let Some(id) = queue.pop_front() {
            order.push(id.clone());

            if let Some(neighbors) = graph.get(id) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        if order.len() != self.resources.len() {
            return Err("Circular dependency detected".to_string());
        }

        self.startup_order = order;
        Ok(())
    }

    /// Start all resources in dependency order.
    pub async fn start_all(&mut self) -> Result<StartAllResult, String> {
        self.compute_startup_order()?;

        info!(
            order = ?self.startup_order.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            "Starting resources in order"
        );

        let mut result = StartAllResult::new();
        let mut failed_resources: HashSet<ResourceId> = HashSet::new();
        let ctx = Context::new();

        for id in &self.startup_order {
            // Check if any dependency failed
            let deps = self.dependencies.get(id).cloned().unwrap_or_default();
            let has_failed_dep = deps
                .iter()
                .any(|d| failed_resources.contains(&d.resource_id));

            if has_failed_dep {
                info!(id = %id, "Skipping resource due to failed dependency");
                result.skipped.push(id.clone());
                failed_resources.insert(id.clone());
                continue;
            }

            // Wait for dependencies
            for dep in &deps {
                self.event_bus.publish(LifecycleEvent::DependencyWaiting {
                    id: id.clone(),
                    dependency: dep.resource_id.clone(),
                    condition: dep.condition,
                    timestamp: Utc::now(),
                });

                if let Err(e) = self
                    .wait_for_condition(&dep.resource_id, dep.condition)
                    .await
                {
                    warn!(
                        id = %id,
                        dependency = %dep.resource_id,
                        error = %e,
                        "Dependency wait failed"
                    );
                    result.skipped.push(id.clone());
                    failed_resources.insert(id.clone());
                    continue;
                }

                self.event_bus.publish(LifecycleEvent::DependencyResolved {
                    id: id.clone(),
                    dependency: dep.resource_id.clone(),
                    timestamp: Utc::now(),
                });
            }

            // Start the resource
            let resource = self.resources.get(id).unwrap().clone();
            let mut resource = resource.write().await;

            match resource.start(&ctx).await {
                Ok(_) => {
                    info!(id = %id, "Resource started");
                    result.started.push(id.clone());
                }
                Err(e) => {
                    error!(id = %id, error = %e, "Resource failed to start");
                    result.failed.push((id.clone(), e.to_string()));
                    failed_resources.insert(id.clone());
                }
            }
        }

        self.running = !result.started.is_empty();
        Ok(result)
    }

    /// Stop all resources in reverse dependency order.
    pub async fn stop_all(&mut self) -> Result<(), String> {
        let shutdown_order: Vec<ResourceId> = self.startup_order.iter().rev().cloned().collect();

        info!(
            order = ?shutdown_order.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            "Stopping resources in order"
        );

        let ctx = Context::new();

        for id in &shutdown_order {
            let resource = self.resources.get(id).unwrap().clone();
            let mut resource = resource.write().await;

            if resource.state().is_running() {
                match resource.stop(&ctx).await {
                    Ok(_) => {
                        info!(id = %id, "Resource stopped");
                    }
                    Err(e) => {
                        error!(id = %id, error = %e, "Resource failed to stop");
                    }
                }
            }
        }

        self.running = false;
        Ok(())
    }

    /// Wait for a resource to meet a condition.
    async fn wait_for_condition(
        &self,
        id: &ResourceId,
        condition: DependencyCondition,
    ) -> Result<(), String> {
        let Some(resource_arc) = self.resources.get(id) else {
            return Err(format!("Resource {} not found", id));
        };

        let timeout = std::time::Duration::from_secs(60);
        let start = std::time::Instant::now();

        loop {
            let (met, is_failed) = {
                let resource = resource_arc.read().await;
                let state = resource.state();

                let met = match condition {
                    DependencyCondition::Started => state.is_running(),
                    DependencyCondition::Healthy => {
                        if !state.is_running() {
                            false
                        } else {
                            // Release the lock before async health check
                            drop(resource);
                            let resource = resource_arc.read().await;
                            resource.health().await.is_healthy()
                        }
                    }
                    DependencyCondition::Completed => {
                        state.is_stopped() && state.exit_code() == Some(0)
                    }
                };

                // Re-check state for failure
                let resource = resource_arc.read().await;
                let is_failed = resource.state().is_failed();

                (met, is_failed)
            };

            if met {
                return Ok(());
            }

            // Check for failure
            if is_failed {
                return Err(format!("Resource {} failed", id));
            }

            // Check timeout
            if start.elapsed() > timeout {
                return Err(format!(
                    "Timeout waiting for {} to reach {:?}",
                    id, condition
                ));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Poll all resources for status updates and restarts.
    pub async fn poll(&mut self) {
        for (id, resource) in &self.resources {
            let mut resource = resource.write().await;

            // Poll for exit
            if let Some(exit_code) = resource.poll() {
                debug!(id = %id, exit_code = exit_code, "Resource exited");

                // Check if restart is needed
                if resource.should_restart() {
                    resource.schedule_restart();
                }
            }

            // Execute scheduled restart
            if resource.is_restart_due() {
                let ctx = Context::new();
                if let Err(e) = resource.execute_restart(&ctx).await {
                    error!(id = %id, error = %e, "Restart failed");
                }
            }
        }
    }

    /// Get the status of all resources.
    pub async fn status(&self) -> HashMap<ResourceId, ResourceState> {
        let mut statuses = HashMap::new();
        for (id, resource) in &self.resources {
            let resource = resource.read().await;
            statuses.insert(id.clone(), resource.state().clone());
        }
        statuses
    }

    /// Get health status of all resources.
    pub async fn health(&self) -> HashMap<ResourceId, HealthStatus> {
        let mut health = HashMap::new();
        for (id, resource) in &self.resources {
            let resource = resource.read().await;
            let status = resource.health().await;
            health.insert(id.clone(), status);
        }
        health
    }

    /// Check if the orchestrator is running.
    pub fn is_running(&self) -> bool {
        self.running
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProcessResourceConfig;

    fn make_config(id: &str, cmd: &str) -> ProcessResourceConfig {
        ProcessResourceConfig::new(id, cmd)
    }

    #[test]
    fn orchestrator_new() {
        let bus = Arc::new(ResourceEventBus::new());
        let orchestrator = ResourceOrchestrator::new(bus);
        assert!(!orchestrator.is_running());
    }

    #[test]
    fn orchestrator_add_resource() {
        let bus = Arc::new(ResourceEventBus::new());
        let mut orchestrator = ResourceOrchestrator::new(bus);

        orchestrator.add_resource(make_config("api", "echo api"));
        assert!(orchestrator.get_resource(&ResourceId::new("api")).is_some());
    }

    #[test]
    fn orchestrator_compute_order_simple() {
        let bus = Arc::new(ResourceEventBus::new());
        let mut orchestrator = ResourceOrchestrator::new(bus);

        orchestrator.add_resource(make_config("api", "echo api"));
        orchestrator.add_resource(make_config("db", "echo db"));

        orchestrator.compute_startup_order().unwrap();
        assert_eq!(orchestrator.startup_order.len(), 2);
    }

    #[test]
    fn orchestrator_compute_order_with_deps() {
        let bus = Arc::new(ResourceEventBus::new());
        let mut orchestrator = ResourceOrchestrator::new(bus);

        orchestrator.add_resource(make_config("api", "echo api").depends_on("db"));
        orchestrator.add_resource(make_config("db", "echo db"));

        orchestrator.compute_startup_order().unwrap();

        // db should come before api
        let db_pos = orchestrator
            .startup_order
            .iter()
            .position(|id| id.as_str() == "db")
            .unwrap();
        let api_pos = orchestrator
            .startup_order
            .iter()
            .position(|id| id.as_str() == "api")
            .unwrap();

        assert!(db_pos < api_pos);
    }

    #[test]
    fn orchestrator_detect_cycle() {
        let bus = Arc::new(ResourceEventBus::new());
        let mut orchestrator = ResourceOrchestrator::new(bus);

        orchestrator.add_resource(make_config("a", "echo a").depends_on("b"));
        orchestrator.add_resource(make_config("b", "echo b").depends_on("a"));

        let result = orchestrator.compute_startup_order();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular"));
    }

    #[tokio::test]
    async fn orchestrator_start_stop() {
        let bus = Arc::new(ResourceEventBus::new());
        let mut orchestrator = ResourceOrchestrator::new(bus);

        orchestrator.add_resource(make_config("test", "sleep 10"));

        let result = orchestrator.start_all().await.unwrap();
        assert!(result.is_success());
        assert_eq!(result.started.len(), 1);

        orchestrator.stop_all().await.unwrap();
        assert!(!orchestrator.is_running());
    }

    #[tokio::test]
    async fn orchestrator_status() {
        let bus = Arc::new(ResourceEventBus::new());
        let mut orchestrator = ResourceOrchestrator::new(bus);

        orchestrator.add_resource(make_config("test", "sleep 10"));

        let status = orchestrator.status().await;
        assert!(status.get(&ResourceId::new("test")).unwrap().is_pending());

        orchestrator.start_all().await.unwrap();

        let status = orchestrator.status().await;
        assert!(status.get(&ResourceId::new("test")).unwrap().is_running());

        orchestrator.stop_all().await.unwrap();
    }

    #[test]
    fn dependency_constructors() {
        let started = Dependency::started("api");
        assert_eq!(started.condition, DependencyCondition::Started);

        let healthy = Dependency::healthy("api");
        assert_eq!(healthy.condition, DependencyCondition::Healthy);

        let completed = Dependency::completed("api");
        assert_eq!(completed.condition, DependencyCondition::Completed);
    }

    #[tokio::test]
    async fn orchestrator_partial_start_is_running() {
        let bus = Arc::new(ResourceEventBus::new());
        let mut orchestrator = ResourceOrchestrator::new(bus);

        // One valid resource that will start successfully
        orchestrator.add_resource(make_config("good", "sleep 10"));
        // One resource with a nonexistent working directory — spawn will fail
        orchestrator.add_resource(
            make_config("bad", "sleep 10")
                .with_working_dir("/nonexistent_dir_dtx_test_partial_start"),
        );

        let result = orchestrator.start_all().await.unwrap();
        assert!(!result.is_success(), "not all resources succeeded");
        assert!(!result.started.is_empty(), "some resources started");
        assert!(!result.failed.is_empty(), "some resources failed");
        assert!(
            orchestrator.is_running(),
            "orchestrator should be running when at least one resource started"
        );

        orchestrator.stop_all().await.unwrap();
    }

    #[test]
    fn start_all_result() {
        let mut result = StartAllResult::new();
        assert!(result.is_success());

        result
            .failed
            .push((ResourceId::new("bad"), "error".to_string()));
        assert!(!result.is_success());
    }
}
