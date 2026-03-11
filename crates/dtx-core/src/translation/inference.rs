//! Image inference heuristics for process-to-container translation.
//!
//! This module provides functions to infer appropriate container images
//! from process commands and Nix package names.

use std::collections::HashMap;

/// Infer a container image from a process command.
///
/// Uses heuristics based on common patterns to suggest appropriate
/// base images for containerization.
///
/// # Example
///
/// ```ignore
/// let result = infer_image("node server.js");
/// assert!(result.image.starts_with("node:"));
/// assert_eq!(result.confidence, Confidence::High);
/// ```
pub fn infer_image(command: &str) -> InferredImage {
    let cmd_lower = command.to_lowercase();
    let first_word = command.split_whitespace().next().unwrap_or("");
    let first_word_lower = first_word.to_lowercase();

    // Check explicit runtime commands
    if first_word_lower == "node" || first_word_lower == "npm" || first_word_lower == "npx" {
        return InferredImage::confident("node:20-alpine", "Node.js command detected");
    }

    if first_word_lower == "python"
        || first_word_lower == "python3"
        || first_word_lower == "pip"
        || first_word_lower == "pip3"
    {
        return InferredImage::confident("python:3.12-slim", "Python command detected");
    }

    if first_word_lower == "ruby" || first_word_lower == "bundle" || first_word_lower == "rails" {
        return InferredImage::confident("ruby:3.3-slim", "Ruby command detected");
    }

    if first_word_lower == "go" || first_word.ends_with("/go") {
        return InferredImage::confident("golang:1.22-alpine", "Go command detected");
    }

    if first_word_lower == "java"
        || first_word_lower == "mvn"
        || first_word_lower == "gradle"
        || first_word_lower == "mvnw"
        || first_word_lower == "gradlew"
    {
        return InferredImage::confident("eclipse-temurin:21-jdk", "Java command detected");
    }

    if first_word_lower == "cargo" || first_word_lower == "rustc" {
        return InferredImage::confident("rust:1.75-slim", "Rust command detected");
    }

    if first_word_lower == "php" || first_word_lower == "composer" {
        return InferredImage::confident("php:8.3-cli", "PHP command detected");
    }

    if first_word_lower == "dotnet" {
        return InferredImage::confident(
            "mcr.microsoft.com/dotnet/sdk:8.0",
            ".NET command detected",
        );
    }

    if first_word_lower == "deno" {
        return InferredImage::confident("denoland/deno:alpine", "Deno command detected");
    }

    if first_word_lower == "bun" {
        return InferredImage::confident("oven/bun:alpine", "Bun command detected");
    }

    // Check for common database commands
    if cmd_lower.contains("postgres") || first_word_lower == "psql" || first_word_lower == "pg_" {
        return InferredImage::confident("postgres:16-alpine", "PostgreSQL detected");
    }

    if cmd_lower.contains("mysql") || cmd_lower.contains("mariadb") {
        return InferredImage::confident("mysql:8", "MySQL detected");
    }

    if cmd_lower.contains("redis") || first_word_lower == "redis-server" {
        return InferredImage::confident("redis:7-alpine", "Redis detected");
    }

    if cmd_lower.contains("mongo") {
        return InferredImage::confident("mongo:7", "MongoDB detected");
    }

    // Check for web servers
    if first_word_lower == "nginx" {
        return InferredImage::confident("nginx:alpine", "Nginx detected");
    }

    if first_word_lower == "httpd" || first_word_lower == "apache2" {
        return InferredImage::confident("httpd:alpine", "Apache detected");
    }

    if first_word_lower == "caddy" {
        return InferredImage::confident("caddy:2-alpine", "Caddy detected");
    }

    // Check file extensions for script files
    if command.ends_with(".js") || command.ends_with(".mjs") || command.ends_with(".cjs") {
        return InferredImage::probable("node:20-alpine", "JavaScript file extension");
    }

    if command.ends_with(".ts") || command.ends_with(".tsx") {
        return InferredImage::probable("node:20-alpine", "TypeScript file extension");
    }

    if command.ends_with(".py") {
        return InferredImage::probable("python:3.12-slim", "Python file extension");
    }

    if command.ends_with(".rb") {
        return InferredImage::probable("ruby:3.3-slim", "Ruby file extension");
    }

    if command.ends_with(".sh") {
        return InferredImage::probable("alpine:3.19", "Shell script extension");
    }

    if command.ends_with(".php") {
        return InferredImage::probable("php:8.3-cli", "PHP file extension");
    }

    // Default fallback
    InferredImage::fallback(
        "alpine:3.19",
        "No specific runtime detected, using minimal base",
    )
}

/// Result of image inference.
#[derive(Clone, Debug)]
pub struct InferredImage {
    /// Suggested image.
    pub image: String,
    /// Reason for suggestion.
    pub reason: String,
    /// Confidence level.
    pub confidence: Confidence,
}

impl InferredImage {
    /// High confidence inference.
    pub fn confident(image: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            reason: reason.into(),
            confidence: Confidence::High,
        }
    }

    /// Medium confidence inference.
    pub fn probable(image: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            reason: reason.into(),
            confidence: Confidence::Medium,
        }
    }

    /// Low confidence fallback.
    pub fn fallback(image: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            reason: reason.into(),
            confidence: Confidence::Low,
        }
    }
}

/// Confidence level of inference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// Confident based on explicit command match.
    High,
    /// Probable based on file extension or pattern.
    Medium,
    /// Fallback, may need user confirmation.
    Low,
}

/// Image mappings for common tools.
pub fn common_images() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();

    // Databases
    map.insert("postgres", "postgres:16-alpine");
    map.insert("postgresql", "postgres:16-alpine");
    map.insert("mysql", "mysql:8");
    map.insert("mariadb", "mariadb:11");
    map.insert("redis", "redis:7-alpine");
    map.insert("mongo", "mongo:7");
    map.insert("mongodb", "mongo:7");
    map.insert("elasticsearch", "elasticsearch:8.12.0");
    map.insert("sqlite", "alpine:3.19"); // SQLite is just a library

    // Message queues
    map.insert("rabbitmq", "rabbitmq:3-management-alpine");
    map.insert("kafka", "confluentinc/cp-kafka:7.5.0");
    map.insert("nats", "nats:2-alpine");

    // Runtimes
    map.insert("node", "node:20-alpine");
    map.insert("nodejs", "node:20-alpine");
    map.insert("python", "python:3.12-slim");
    map.insert("python3", "python:3.12-slim");
    map.insert("ruby", "ruby:3.3-slim");
    map.insert("go", "golang:1.22-alpine");
    map.insert("golang", "golang:1.22-alpine");
    map.insert("rust", "rust:1.75-slim");
    map.insert("java", "eclipse-temurin:21-jdk");
    map.insert("openjdk", "eclipse-temurin:21-jdk");
    map.insert("php", "php:8.3-cli");
    map.insert("dotnet", "mcr.microsoft.com/dotnet/sdk:8.0");
    map.insert("deno", "denoland/deno:alpine");
    map.insert("bun", "oven/bun:alpine");

    // Web servers
    map.insert("nginx", "nginx:alpine");
    map.insert("apache", "httpd:alpine");
    map.insert("httpd", "httpd:alpine");
    map.insert("caddy", "caddy:2-alpine");

    map
}

/// Suggest an image from Nix package name.
pub fn image_from_nixpkg(nixpkg: &str) -> Option<InferredImage> {
    let images = common_images();
    let nixpkg_lower = nixpkg.to_lowercase();

    // Direct match
    if let Some(&image) = images.get(nixpkg_lower.as_str()) {
        return Some(InferredImage::confident(
            image,
            format!("Nix package: {}", nixpkg),
        ));
    }

    // Try common suffixes
    for suffix in ["_server", "-server", "_service", "-service", "_db", "-db"] {
        if let Some(base) = nixpkg_lower.strip_suffix(suffix) {
            if let Some(&image) = images.get(base) {
                return Some(InferredImage::confident(
                    image,
                    format!("Nix package: {}", nixpkg),
                ));
            }
        }
    }

    // Try common prefixes
    for prefix in ["lib", "python3.", "nodePackages.", "rubyGems."] {
        if let Some(base) = nixpkg_lower.strip_prefix(prefix) {
            if let Some(&image) = images.get(base) {
                return Some(InferredImage::probable(
                    image,
                    format!("Nix package prefix: {}", nixpkg),
                ));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_node() {
        let result = infer_image("node server.js");
        assert!(result.image.starts_with("node:"));
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn infer_npm() {
        let result = infer_image("npm start");
        assert!(result.image.starts_with("node:"));
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn infer_python() {
        let result = infer_image("python main.py");
        assert!(result.image.starts_with("python:"));
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn infer_python3() {
        let result = infer_image("python3 -m flask run");
        assert!(result.image.starts_with("python:"));
    }

    #[test]
    fn infer_ruby() {
        let result = infer_image("bundle exec rails server");
        assert!(result.image.starts_with("ruby:"));
    }

    #[test]
    fn infer_go() {
        let result = infer_image("go run main.go");
        assert!(result.image.starts_with("golang:"));
    }

    #[test]
    fn infer_rust() {
        let result = infer_image("cargo run");
        assert!(result.image.starts_with("rust:"));
    }

    #[test]
    fn infer_java() {
        let result = infer_image("java -jar app.jar");
        assert!(result.image.contains("temurin") || result.image.contains("jdk"));
    }

    #[test]
    fn infer_postgres() {
        let result = infer_image("postgres -D $PGDATA");
        assert!(result.image.starts_with("postgres:"));
    }

    #[test]
    fn infer_redis() {
        let result = infer_image("redis-server /etc/redis.conf");
        assert!(result.image.starts_with("redis:"));
    }

    #[test]
    fn infer_nginx() {
        let result = infer_image("nginx -g 'daemon off;'");
        assert!(result.image.starts_with("nginx:"));
    }

    #[test]
    fn infer_from_js_extension() {
        let result = infer_image("./app.js");
        assert!(result.image.starts_with("node:"));
        assert_eq!(result.confidence, Confidence::Medium);
    }

    #[test]
    fn infer_from_py_extension() {
        let result = infer_image("./scripts/process.py");
        assert!(result.image.starts_with("python:"));
        assert_eq!(result.confidence, Confidence::Medium);
    }

    #[test]
    fn infer_from_sh_extension() {
        let result = infer_image("./start.sh");
        assert!(result.image.starts_with("alpine:"));
        assert_eq!(result.confidence, Confidence::Medium);
    }

    #[test]
    fn infer_unknown_fallback() {
        let result = infer_image("./my-custom-binary");
        assert!(result.image.starts_with("alpine:"));
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn image_from_nix_direct() {
        let result = image_from_nixpkg("postgresql").unwrap();
        assert!(result.image.starts_with("postgres:"));
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn image_from_nix_with_suffix() {
        let result = image_from_nixpkg("redis-server").unwrap();
        assert!(result.image.starts_with("redis:"));
    }

    #[test]
    fn image_from_nix_unknown() {
        let result = image_from_nixpkg("my-custom-pkg");
        assert!(result.is_none());
    }

    #[test]
    fn common_images_has_databases() {
        let images = common_images();
        assert!(images.contains_key("postgres"));
        assert!(images.contains_key("mysql"));
        assert!(images.contains_key("redis"));
        assert!(images.contains_key("mongo"));
    }

    #[test]
    fn common_images_has_runtimes() {
        let images = common_images();
        assert!(images.contains_key("node"));
        assert!(images.contains_key("python"));
        assert!(images.contains_key("ruby"));
        assert!(images.contains_key("go"));
    }

    #[test]
    fn inferred_image_constructors() {
        let high = InferredImage::confident("img:1", "reason");
        assert_eq!(high.confidence, Confidence::High);

        let medium = InferredImage::probable("img:2", "reason");
        assert_eq!(medium.confidence, Confidence::Medium);

        let low = InferredImage::fallback("img:3", "reason");
        assert_eq!(low.confidence, Confidence::Low);
    }
}
