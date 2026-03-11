//! Kubernetes manifest exporter.
//!
//! Exports dtx projects to Kubernetes Deployment and Service manifests.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::error::{ExportError, ExportResult};
use super::types::{ExportFormat, ExportableProject, ExportableService, Exporter};
use crate::translation::{ContainerHealthCheck, HealthCheckTest, PortMapping, ResourceLimits};

/// Kubernetes manifest exporter.
#[derive(Debug, Clone, Default)]
pub struct KubernetesExporter {
    /// Namespace for resources.
    namespace: Option<String>,
    /// Additional labels to add.
    labels: HashMap<String, String>,
    /// Image pull policy.
    image_pull_policy: Option<String>,
}

impl KubernetesExporter {
    /// Create a new Kubernetes exporter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set namespace.
    pub fn with_namespace(mut self, ns: impl Into<String>) -> Self {
        self.namespace = Some(ns.into());
        self
    }

    /// Add a label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Set image pull policy.
    pub fn with_image_pull_policy(mut self, policy: impl Into<String>) -> Self {
        self.image_pull_policy = Some(policy.into());
        self
    }

    /// Generate manifests for a service.
    fn service_to_manifests(
        &self,
        service: &ExportableService,
        project_name: &str,
    ) -> ExportResult<Vec<K8sResource>> {
        let container = service
            .container
            .as_ref()
            .ok_or_else(|| ExportError::missing("container config"))?;

        let name = service.id.as_str();
        let labels = common_labels(project_name, name, &self.labels);
        let selector = K8sSelector {
            match_labels: labels.clone(),
        };

        // Build container spec
        let k8s_container = K8sContainer {
            name: name.to_string(),
            image: container.image.clone(),
            image_pull_policy: self.image_pull_policy.clone(),
            command: container.command.clone(),
            args: None,
            working_dir: container.working_dir.clone(),
            env: if container.environment.is_empty() {
                None
            } else {
                Some(
                    container
                        .environment
                        .iter()
                        .map(|(k, v)| K8sEnvVar {
                            name: k.clone(),
                            value: Some(v.clone()),
                            value_from: None,
                        })
                        .collect(),
                )
            },
            ports: if container.ports.is_empty() {
                None
            } else {
                Some(container.ports.iter().map(port_to_k8s).collect())
            },
            liveness_probe: container.health_check.as_ref().map(health_to_probe),
            readiness_probe: container.health_check.as_ref().map(health_to_probe),
            resources: container.resources.as_ref().map(resources_to_k8s),
        };

        // Build pod spec
        let pod_spec = K8sPodSpec {
            containers: vec![k8s_container],
            restart_policy: None, // Deployment handles restart
        };

        // Build deployment
        let deployment = K8sResource::Deployment(K8sDeployment {
            api_version: "apps/v1".to_string(),
            kind: "Deployment".to_string(),
            metadata: K8sMetadata {
                name: name.to_string(),
                namespace: self.namespace.clone(),
                labels: Some(labels.clone()),
                annotations: None,
            },
            spec: K8sDeploymentSpec {
                replicas: Some(1),
                selector,
                template: K8sPodTemplateSpec {
                    metadata: K8sMetadata {
                        name: name.to_string(),
                        namespace: None,
                        labels: Some(labels.clone()),
                        annotations: None,
                    },
                    spec: pod_spec,
                },
            },
        });

        let mut resources = vec![deployment];

        // Build service if ports are exposed
        if !container.ports.is_empty() {
            let service_ports: Vec<K8sServicePort> = container
                .ports
                .iter()
                .enumerate()
                .map(|(i, p)| K8sServicePort {
                    name: Some(format!("port-{}", i)),
                    port: p.host as i32,
                    target_port: Some(K8sIntOrString::Int(p.container as i32)),
                    protocol: Some("TCP".to_string()),
                })
                .collect();

            let k8s_service = K8sResource::Service(K8sService {
                api_version: "v1".to_string(),
                kind: "Service".to_string(),
                metadata: K8sMetadata {
                    name: name.to_string(),
                    namespace: self.namespace.clone(),
                    labels: Some(labels.clone()),
                    annotations: None,
                },
                spec: K8sServiceSpec {
                    selector: Some(labels),
                    ports: service_ports,
                    service_type: None,
                },
            });

            resources.push(k8s_service);
        }

        Ok(resources)
    }
}

impl Exporter for KubernetesExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Kubernetes
    }

    fn export(&self, project: &ExportableProject) -> ExportResult<String> {
        let mut all_resources = Vec::new();

        for service in project.services.iter().filter(|s| s.enabled) {
            let resources = self.service_to_manifests(service, &project.name)?;
            all_resources.extend(resources);
        }

        // Serialize each resource and join with ---
        let yamls: Vec<String> = all_resources
            .iter()
            .map(|r| {
                serde_yaml::to_string(r).map_err(|e| ExportError::serialization(e.to_string()))
            })
            .collect::<ExportResult<_>>()?;

        Ok(yamls.join("---\n"))
    }
}

/// Common labels for Kubernetes resources.
fn common_labels(
    app: &str,
    component: &str,
    extra: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut labels = HashMap::new();
    labels.insert("app.kubernetes.io/name".to_string(), app.to_string());
    labels.insert(
        "app.kubernetes.io/component".to_string(),
        component.to_string(),
    );
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "dtx".to_string(),
    );
    labels.extend(extra.clone());
    labels
}

/// Convert port mapping to K8s container port.
fn port_to_k8s(port: &PortMapping) -> K8sContainerPort {
    K8sContainerPort {
        container_port: port.container as i32,
        name: None,
        protocol: Some("TCP".to_string()),
    }
}

/// Convert health check to K8s probe.
fn health_to_probe(check: &ContainerHealthCheck) -> K8sProbe {
    let (exec, http_get) = match &check.test {
        HealthCheckTest::CmdShell(cmd) => (
            Some(K8sExecAction {
                command: vec!["/bin/sh".to_string(), "-c".to_string(), cmd.clone()],
            }),
            None,
        ),
        HealthCheckTest::Cmd(args) => (
            Some(K8sExecAction {
                command: args.clone(),
            }),
            None,
        ),
    };

    // Parse interval/timeout (assume format like "30s")
    fn parse_seconds(s: &str) -> i32 {
        s.trim_end_matches('s').parse().unwrap_or(10)
    }

    K8sProbe {
        exec,
        http_get,
        initial_delay_seconds: check.start_period.as_ref().map(|s| parse_seconds(s)),
        period_seconds: Some(parse_seconds(&check.interval)),
        timeout_seconds: Some(parse_seconds(&check.timeout)),
        failure_threshold: Some(check.retries as i32),
        success_threshold: Some(1),
    }
}

/// Convert resource limits to K8s format.
fn resources_to_k8s(limits: &ResourceLimits) -> K8sResources {
    K8sResources {
        limits: Some(K8sResourceLimits {
            cpu: limits.cpus.clone(),
            memory: limits.memory.clone(),
        }),
        requests: None,
    }
}

/// K8s resource types (for multi-document output).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum K8sResource {
    /// Deployment resource.
    Deployment(K8sDeployment),
    /// Service resource.
    Service(K8sService),
}

/// Kubernetes Deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sDeployment {
    /// API version.
    pub api_version: String,
    /// Resource kind.
    pub kind: String,
    /// Metadata.
    pub metadata: K8sMetadata,
    /// Deployment spec.
    pub spec: K8sDeploymentSpec,
}

/// Deployment spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sDeploymentSpec {
    /// Number of replicas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<i32>,
    /// Pod selector.
    pub selector: K8sSelector,
    /// Pod template.
    pub template: K8sPodTemplateSpec,
}

/// Pod template spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sPodTemplateSpec {
    /// Metadata.
    pub metadata: K8sMetadata,
    /// Pod spec.
    pub spec: K8sPodSpec,
}

/// Pod spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sPodSpec {
    /// Containers.
    pub containers: Vec<K8sContainer>,
    /// Restart policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_policy: Option<String>,
}

/// Container spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sContainer {
    /// Container name.
    pub name: String,
    /// Image.
    pub image: String,
    /// Image pull policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_pull_policy: Option<String>,
    /// Command override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Args.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Environment variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<K8sEnvVar>>,
    /// Container ports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<K8sContainerPort>>,
    /// Liveness probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness_probe: Option<K8sProbe>,
    /// Readiness probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness_probe: Option<K8sProbe>,
    /// Resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<K8sResources>,
}

/// Environment variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sEnvVar {
    /// Variable name.
    pub name: String,
    /// Direct value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Value from source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_from: Option<K8sEnvVarSource>,
}

/// Environment variable source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sEnvVarSource {
    /// Secret key ref.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key_ref: Option<K8sSecretKeyRef>,
}

/// Secret key reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sSecretKeyRef {
    /// Secret name.
    pub name: String,
    /// Key in secret.
    pub key: String,
}

/// Container port.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sContainerPort {
    /// Container port.
    pub container_port: i32,
    /// Port name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// Probe configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sProbe {
    /// Exec action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<K8sExecAction>,
    /// HTTP get action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_get: Option<K8sHttpGetAction>,
    /// Initial delay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_delay_seconds: Option<i32>,
    /// Period.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_seconds: Option<i32>,
    /// Timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    /// Failure threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_threshold: Option<i32>,
    /// Success threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_threshold: Option<i32>,
}

/// Exec probe action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sExecAction {
    /// Command to run.
    pub command: Vec<String>,
}

/// HTTP get probe action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sHttpGetAction {
    /// Path.
    pub path: String,
    /// Port.
    pub port: K8sIntOrString,
    /// Scheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
}

/// Int or string type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum K8sIntOrString {
    /// Integer value.
    Int(i32),
    /// String value.
    String(String),
}

/// Resource configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sResources {
    /// Resource limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<K8sResourceLimits>,
    /// Resource requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<K8sResourceLimits>,
}

/// Resource limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sResourceLimits {
    /// CPU limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    /// Memory limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

/// Kubernetes Service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sService {
    /// API version.
    pub api_version: String,
    /// Resource kind.
    pub kind: String,
    /// Metadata.
    pub metadata: K8sMetadata,
    /// Service spec.
    pub spec: K8sServiceSpec,
}

/// Service spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sServiceSpec {
    /// Pod selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<HashMap<String, String>>,
    /// Ports.
    pub ports: Vec<K8sServicePort>,
    /// Service type.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub service_type: Option<String>,
}

/// Service port.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sServicePort {
    /// Port name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Service port.
    pub port: i32,
    /// Target port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_port: Option<K8sIntOrString>,
    /// Protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// Kubernetes metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sMetadata {
    /// Resource name.
    pub name: String,
    /// Namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    /// Annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
}

/// Selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct K8sSelector {
    /// Match labels.
    pub match_labels: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::ExportableService;
    use crate::translation::ContainerConfig;

    fn make_test_project() -> ExportableProject {
        let container = ContainerConfig::new("api", "node:20-alpine")
            .with_port_same(3000)
            .with_env("NODE_ENV", "production");

        ExportableProject::new("test-app")
            .with_service(ExportableService::from_container(container))
    }

    #[test]
    fn export_basic() {
        let exporter = KubernetesExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("apiVersion: apps/v1"));
        assert!(yaml.contains("kind: Deployment"));
        assert!(yaml.contains("kind: Service"));
    }

    #[test]
    fn export_with_namespace() {
        let exporter = KubernetesExporter::new().with_namespace("production");
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("namespace: production"));
    }

    #[test]
    fn export_with_labels() {
        let exporter = KubernetesExporter::new().with_label("version", "v1.0");
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("version: v1.0"));
    }

    #[test]
    fn export_has_common_labels() {
        let exporter = KubernetesExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("app.kubernetes.io/name"));
        assert!(yaml.contains("app.kubernetes.io/component"));
        assert!(yaml.contains("app.kubernetes.io/managed-by: dtx"));
    }

    #[test]
    fn export_deployment_spec() {
        let exporter = KubernetesExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("replicas: 1"));
        assert!(yaml.contains("image: node:20-alpine"));
    }

    #[test]
    fn export_environment() {
        let exporter = KubernetesExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("env:"));
        assert!(yaml.contains("NODE_ENV"));
        assert!(yaml.contains("production"));
    }

    #[test]
    fn export_ports() {
        let exporter = KubernetesExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("containerPort: 3000"));
        assert!(yaml.contains("port: 3000"));
    }

    #[test]
    fn export_multi_document() {
        let exporter = KubernetesExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        // Should have Deployment and Service separated by ---
        assert!(yaml.contains("---"));
    }

    #[test]
    fn export_with_image_pull_policy() {
        let exporter = KubernetesExporter::new().with_image_pull_policy("Always");
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("imagePullPolicy: Always"));
    }

    #[test]
    fn export_with_health_check() {
        let container = ContainerConfig::new("api", "node:20-alpine")
            .with_port_same(3000)
            .with_health_check(ContainerHealthCheck {
                test: HealthCheckTest::shell("curl -f http://localhost:3000/health"),
                interval: "30s".to_string(),
                timeout: "10s".to_string(),
                retries: 3,
                start_period: Some("5s".to_string()),
            });

        let project = ExportableProject::new("test")
            .with_service(ExportableService::from_container(container));

        let exporter = KubernetesExporter::new();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("livenessProbe:"));
        assert!(yaml.contains("readinessProbe:"));
        assert!(yaml.contains("exec:"));
    }

    #[test]
    fn exporter_format() {
        let exporter = KubernetesExporter::new();
        assert_eq!(exporter.format(), ExportFormat::Kubernetes);
    }

    #[test]
    fn common_labels_includes_app() {
        let labels = common_labels("myapp", "web", &HashMap::new());
        assert_eq!(
            labels.get("app.kubernetes.io/name"),
            Some(&"myapp".to_string())
        );
        assert_eq!(
            labels.get("app.kubernetes.io/component"),
            Some(&"web".to_string())
        );
        assert_eq!(
            labels.get("app.kubernetes.io/managed-by"),
            Some(&"dtx".to_string())
        );
    }

    #[test]
    fn common_labels_merges_extra() {
        let mut extra = HashMap::new();
        extra.insert("version".to_string(), "v1".to_string());

        let labels = common_labels("myapp", "web", &extra);
        assert_eq!(labels.get("version"), Some(&"v1".to_string()));
    }

    #[test]
    fn disabled_services_skipped() {
        let container = ContainerConfig::new("api", "node:20-alpine");
        let service = ExportableService::from_container(container).with_enabled(false);

        let project = ExportableProject::new("test").with_service(service);

        let exporter = KubernetesExporter::new();
        let yaml = exporter.export(&project).unwrap();

        assert!(!yaml.contains("Deployment"));
    }
}
