//! Codebase inference for automatic project detection.
//!
//! Analyzes project directories to detect:
//! - Project type (Rust, Node, Python, Go, etc.)
//! - Required Nix packages
//! - Suggested services

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

/// Inference confidence level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceConfidence {
    /// Exact match found (e.g., Cargo.toml -> rustc).
    High,
    /// Inferred from ecosystem (e.g., npm dependency -> system lib).
    Medium,
    /// Heuristic guess.
    Low,
}

/// Detected project type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Java,
    Ruby,
    Php,
    Mixed(Vec<ProjectType>),
    Unknown,
}

impl fmt::Display for ProjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rust => write!(f, "Rust"),
            Self::Node => write!(f, "Node.js"),
            Self::Python => write!(f, "Python"),
            Self::Go => write!(f, "Go"),
            Self::Java => write!(f, "Java"),
            Self::Ruby => write!(f, "Ruby"),
            Self::Php => write!(f, "PHP"),
            Self::Mixed(types) => {
                let names: Vec<_> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "Mixed ({})", names.join(", "))
            }
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// A detected package suggestion.
#[derive(Debug, Clone)]
pub struct DetectedPackage {
    /// Human-readable name.
    pub name: String,
    /// Nix package name.
    pub nixpkg: String,
    /// Detection confidence.
    pub confidence: InferenceConfidence,
    /// Source file that triggered detection.
    pub source: String,
}

impl DetectedPackage {
    /// Create a high-confidence package.
    pub fn high(
        name: impl Into<String>,
        nixpkg: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            nixpkg: nixpkg.into(),
            confidence: InferenceConfidence::High,
            source: source.into(),
        }
    }

    /// Create a medium-confidence package.
    pub fn medium(
        name: impl Into<String>,
        nixpkg: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            nixpkg: nixpkg.into(),
            confidence: InferenceConfidence::Medium,
            source: source.into(),
        }
    }

    /// Create a low-confidence package.
    pub fn low(
        name: impl Into<String>,
        nixpkg: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            nixpkg: nixpkg.into(),
            confidence: InferenceConfidence::Low,
            source: source.into(),
        }
    }
}

/// A suggested service based on project type.
#[derive(Debug, Clone)]
pub struct SuggestedService {
    /// Service name.
    pub name: String,
    /// Command to run.
    pub command: String,
    /// Suggested port (if applicable).
    pub port: Option<u16>,
    /// Description.
    pub description: String,
}

/// Result of codebase inference.
#[derive(Debug, Clone)]
pub struct ProjectInference {
    /// Detected project type.
    pub project_type: ProjectType,
    /// Detected packages with confidence levels.
    pub detected_packages: Vec<DetectedPackage>,
    /// Suggested services.
    pub suggested_services: Vec<SuggestedService>,
}

impl ProjectInference {
    /// Create an unknown project inference.
    pub fn unknown() -> Self {
        Self {
            project_type: ProjectType::Unknown,
            detected_packages: Vec::new(),
            suggested_services: Vec::new(),
        }
    }

    /// Check if inference found anything.
    pub fn is_empty(&self) -> bool {
        self.project_type == ProjectType::Unknown && self.detected_packages.is_empty()
    }

    /// Get only high-confidence packages.
    pub fn high_confidence_packages(&self) -> Vec<&DetectedPackage> {
        self.detected_packages
            .iter()
            .filter(|p| p.confidence == InferenceConfidence::High)
            .collect()
    }
}

/// Inference errors.
#[derive(Debug, Error)]
pub enum InferenceError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Path does not exist.
    #[error("path does not exist: {0}")]
    PathNotFound(String),

    /// Path is not a directory.
    #[error("path is not a directory: {0}")]
    NotADirectory(String),
}

/// Codebase analyzer for inferring project configuration.
pub struct CodebaseInferrer {
    npm_mappings: HashMap<String, String>,
    // TODO: Implement pypi dependency scanning similar to npm_mappings
    _pypi_mappings: HashMap<String, String>,
}

impl CodebaseInferrer {
    /// Create a new codebase inferrer with default mappings.
    pub fn new() -> Self {
        Self {
            npm_mappings: Self::default_npm_mappings(),
            _pypi_mappings: Self::default_pypi_mappings(),
        }
    }

    /// Analyze a directory and infer project configuration.
    pub fn infer(&self, project_path: &Path) -> Result<ProjectInference, InferenceError> {
        if !project_path.exists() {
            return Err(InferenceError::PathNotFound(
                project_path.display().to_string(),
            ));
        }

        if !project_path.is_dir() {
            return Err(InferenceError::NotADirectory(
                project_path.display().to_string(),
            ));
        }

        let mut packages = Vec::new();
        let mut project_types = Vec::new();

        // Check for Rust
        if project_path.join("Cargo.toml").exists() {
            project_types.push(ProjectType::Rust);
            packages.extend(self.infer_rust(project_path));
        }

        // Check for Node.js
        if project_path.join("package.json").exists() {
            project_types.push(ProjectType::Node);
            packages.extend(self.infer_node(project_path));
        }

        // Check for Python
        if project_path.join("pyproject.toml").exists()
            || project_path.join("requirements.txt").exists()
            || project_path.join("setup.py").exists()
        {
            project_types.push(ProjectType::Python);
            packages.extend(self.infer_python(project_path));
        }

        // Check for Go
        if project_path.join("go.mod").exists() {
            project_types.push(ProjectType::Go);
            packages.extend(self.infer_go(project_path));
        }

        // Check for Java
        if project_path.join("pom.xml").exists()
            || project_path.join("build.gradle").exists()
            || project_path.join("build.gradle.kts").exists()
        {
            project_types.push(ProjectType::Java);
            packages.extend(self.infer_java(project_path));
        }

        // Check for Ruby
        if project_path.join("Gemfile").exists() {
            project_types.push(ProjectType::Ruby);
            packages.extend(self.infer_ruby(project_path));
        }

        // Check for PHP
        if project_path.join("composer.json").exists() {
            project_types.push(ProjectType::Php);
            packages.extend(self.infer_php(project_path));
        }

        let project_type = match project_types.len() {
            0 => ProjectType::Unknown,
            1 => project_types.remove(0),
            _ => ProjectType::Mixed(project_types.clone()),
        };

        let suggested_services = self.suggest_services(&project_type, project_path);
        let packages = self.deduplicate_packages(packages);

        Ok(ProjectInference {
            project_type,
            detected_packages: packages,
            suggested_services,
        })
    }

    fn infer_rust(&self, _project_path: &Path) -> Vec<DetectedPackage> {
        vec![
            DetectedPackage::high("rustc", "rustc", "Cargo.toml"),
            DetectedPackage::high("cargo", "cargo", "Cargo.toml"),
        ]
    }

    fn infer_node(&self, project_path: &Path) -> Vec<DetectedPackage> {
        let mut packages = vec![DetectedPackage::high("nodejs", "nodejs_20", "package.json")];

        // Detect package manager
        if project_path.join("pnpm-lock.yaml").exists() {
            packages.push(DetectedPackage::high("pnpm", "pnpm", "pnpm-lock.yaml"));
        } else if project_path.join("yarn.lock").exists() {
            packages.push(DetectedPackage::high("yarn", "yarn", "yarn.lock"));
        } else if project_path.join("bun.lockb").exists() {
            packages.push(DetectedPackage::high("bun", "bun", "bun.lockb"));
        }

        // Parse package.json for native dependencies
        if let Ok(content) = std::fs::read_to_string(project_path.join("package.json")) {
            if let Ok(pkg) = serde_json::from_str::<PackageJson>(&content) {
                for dep in pkg.dependencies.keys().chain(pkg.dev_dependencies.keys()) {
                    if let Some(nixpkg) = self.npm_mappings.get(dep) {
                        packages.push(DetectedPackage::medium(
                            dep.clone(),
                            nixpkg.clone(),
                            "package.json",
                        ));
                    }
                }
            }
        }

        packages
    }

    fn infer_python(&self, project_path: &Path) -> Vec<DetectedPackage> {
        let mut packages = vec![DetectedPackage::high(
            "python",
            "python3",
            "requirements.txt",
        )];

        // Check for package managers
        if project_path.join("poetry.lock").exists() {
            packages.push(DetectedPackage::high("poetry", "poetry", "poetry.lock"));
        } else if project_path.join("Pipfile.lock").exists() {
            packages.push(DetectedPackage::high("pipenv", "pipenv", "Pipfile.lock"));
        }

        packages
    }

    fn infer_go(&self, _project_path: &Path) -> Vec<DetectedPackage> {
        vec![DetectedPackage::high("go", "go", "go.mod")]
    }

    fn infer_java(&self, project_path: &Path) -> Vec<DetectedPackage> {
        let mut packages = vec![DetectedPackage::high("jdk", "jdk", "pom.xml")];

        if project_path.join("pom.xml").exists() {
            packages.push(DetectedPackage::high("maven", "maven", "pom.xml"));
        } else if project_path.join("build.gradle").exists()
            || project_path.join("build.gradle.kts").exists()
        {
            packages.push(DetectedPackage::high("gradle", "gradle", "build.gradle"));
        }

        packages
    }

    fn infer_ruby(&self, _project_path: &Path) -> Vec<DetectedPackage> {
        vec![
            DetectedPackage::high("ruby", "ruby", "Gemfile"),
            DetectedPackage::high("bundler", "bundler", "Gemfile"),
        ]
    }

    fn infer_php(&self, _project_path: &Path) -> Vec<DetectedPackage> {
        vec![
            DetectedPackage::high("php", "php", "composer.json"),
            DetectedPackage::high("composer", "php83Packages.composer", "composer.json"),
        ]
    }

    fn suggest_services(
        &self,
        project_type: &ProjectType,
        project_path: &Path,
    ) -> Vec<SuggestedService> {
        match project_type {
            ProjectType::Rust => vec![SuggestedService {
                name: "app".to_string(),
                command: "cargo run".to_string(),
                port: None,
                description: "Run Rust application".to_string(),
            }],
            ProjectType::Node => {
                // Try to detect scripts
                if let Ok(content) = std::fs::read_to_string(project_path.join("package.json")) {
                    if let Ok(pkg) = serde_json::from_str::<PackageJson>(&content) {
                        if pkg.scripts.contains_key("dev") {
                            return vec![SuggestedService {
                                name: "dev".to_string(),
                                command: "npm run dev".to_string(),
                                port: Some(3000),
                                description: "Development server".to_string(),
                            }];
                        }
                    }
                }
                vec![SuggestedService {
                    name: "app".to_string(),
                    command: "npm start".to_string(),
                    port: Some(3000),
                    description: "Start application".to_string(),
                }]
            }
            ProjectType::Python => vec![SuggestedService {
                name: "app".to_string(),
                command: "python main.py".to_string(),
                port: None,
                description: "Run Python application".to_string(),
            }],
            ProjectType::Go => vec![SuggestedService {
                name: "app".to_string(),
                command: "go run .".to_string(),
                port: None,
                description: "Run Go application".to_string(),
            }],
            _ => vec![],
        }
    }

    fn deduplicate_packages(&self, packages: Vec<DetectedPackage>) -> Vec<DetectedPackage> {
        let mut seen: HashMap<String, DetectedPackage> = HashMap::new();

        for pkg in packages {
            seen.entry(pkg.nixpkg.clone())
                .and_modify(|existing| {
                    if pkg.confidence == InferenceConfidence::High
                        && existing.confidence != InferenceConfidence::High
                    {
                        *existing = pkg.clone();
                    }
                })
                .or_insert(pkg);
        }

        seen.into_values().collect()
    }

    fn default_npm_mappings() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("sharp".to_string(), "vips".to_string());
        m.insert("canvas".to_string(), "cairo".to_string());
        m.insert("bcrypt".to_string(), "openssl".to_string());
        m.insert("sqlite3".to_string(), "sqlite".to_string());
        m.insert("pg".to_string(), "postgresql".to_string());
        m
    }

    fn default_pypi_mappings() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("psycopg2".to_string(), "postgresql".to_string());
        m.insert("mysqlclient".to_string(), "mysql".to_string());
        m.insert("pillow".to_string(), "python3Packages.pillow".to_string());
        m
    }
}

impl Default for CodebaseInferrer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: HashMap<String, String>,
    #[serde(default)]
    scripts: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_project() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp dir")
    }

    #[test]
    fn test_detect_rust_project() {
        let dir = create_temp_project();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let inferrer = CodebaseInferrer::new();
        let result = inferrer.infer(dir.path()).unwrap();

        assert_eq!(result.project_type, ProjectType::Rust);
        assert!(result.detected_packages.iter().any(|p| p.nixpkg == "rustc"));
    }

    #[test]
    fn test_detect_node_project() {
        let dir = create_temp_project();
        fs::write(dir.path().join("package.json"), r#"{"name": "test"}"#).unwrap();

        let inferrer = CodebaseInferrer::new();
        let result = inferrer.infer(dir.path()).unwrap();

        assert_eq!(result.project_type, ProjectType::Node);
    }

    #[test]
    fn test_detect_unknown_project() {
        let dir = create_temp_project();

        let inferrer = CodebaseInferrer::new();
        let result = inferrer.infer(dir.path()).unwrap();

        assert_eq!(result.project_type, ProjectType::Unknown);
    }
}
