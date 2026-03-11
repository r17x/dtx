//! Tool executor and built-in tools.
//!
//! This module provides the tool execution infrastructure for agents,
//! including built-in tools and support for custom tools.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use crate::config::ToolConfig;
use crate::error::{AgentError, Result};
use crate::message::{ToolCall, ToolResult};

/// Executor for agent tools.
///
/// The tool executor manages tool registration and execution,
/// supporting both built-in and custom tools.
pub struct ToolExecutor {
    tools: HashMap<String, ToolConfig>,
    builtins: HashMap<String, Box<dyn BuiltinTool>>,
}

impl ToolExecutor {
    /// Create a new tool executor with the given tool configurations.
    pub fn new(tools: Vec<ToolConfig>) -> Self {
        let tool_map = tools
            .into_iter()
            .map(|t| (Self::tool_name(&t), t))
            .collect();

        let mut executor = Self {
            tools: tool_map,
            builtins: HashMap::new(),
        };

        // Register default built-in tools
        executor.register_builtin(Box::new(ReadFileTool));
        executor.register_builtin(Box::new(WriteFileTool));
        executor.register_builtin(Box::new(ExecuteCommandTool));
        executor.register_builtin(Box::new(ListFilesTool));
        executor.register_builtin(Box::new(SearchFilesTool));
        executor.register_builtin(Box::new(GetCurrentTimeTool));

        executor
    }

    /// Register a built-in tool.
    pub fn register_builtin(&mut self, tool: Box<dyn BuiltinTool>) {
        self.builtins.insert(tool.name().to_string(), tool);
    }

    fn tool_name(tool: &ToolConfig) -> String {
        match tool {
            ToolConfig::Builtin { name, .. } => name.clone(),
            ToolConfig::Command { name, .. } => name.clone(),
            ToolConfig::Http { name, .. } => name.clone(),
            ToolConfig::Resource { name, .. } => name.clone(),
            ToolConfig::Mcp { server, tool } => format!("{}:{}", server, tool),
            ToolConfig::Custom { name, .. } => name.clone(),
        }
    }

    /// Execute a tool call.
    pub async fn execute(&self, tool_call: &ToolCall) -> Result<ToolResult> {
        let tool = self.tools.get(&tool_call.name);

        let result = match tool {
            Some(ToolConfig::Builtin { name, .. }) => {
                self.execute_builtin(name, &tool_call.input).await
            }
            Some(ToolConfig::Command {
                command, timeout, ..
            }) => {
                self.execute_command(command, &tool_call.input, *timeout)
                    .await
            }
            Some(ToolConfig::Http {
                url,
                method,
                headers,
                ..
            }) => {
                self.execute_http(url, method, headers, &tool_call.input)
                    .await
            }
            Some(ToolConfig::Custom { handler, .. }) => {
                self.execute_custom(handler, &tool_call.input).await
            }
            Some(ToolConfig::Resource { .. }) => {
                // Resource tools require orchestrator integration
                Err(AgentError::tool_execution(
                    &tool_call.name,
                    "Resource tools not yet implemented",
                ))
            }
            Some(ToolConfig::Mcp { .. }) => {
                // MCP tools require MCP client integration
                Err(AgentError::tool_execution(
                    &tool_call.name,
                    "MCP tools not yet implemented",
                ))
            }
            None => {
                // Try as a builtin by name
                self.execute_builtin(&tool_call.name, &tool_call.input)
                    .await
            }
        };

        match result {
            Ok(content) => Ok(ToolResult::success(&tool_call.id, content)),
            Err(e) => Ok(ToolResult::error(&tool_call.id, e.to_string())),
        }
    }

    async fn execute_builtin(&self, name: &str, input: &serde_json::Value) -> Result<String> {
        if let Some(builtin) = self.builtins.get(name) {
            builtin.execute(input.clone()).await
        } else {
            Err(AgentError::tool_execution(name, "Unknown builtin tool"))
        }
    }

    async fn execute_command(
        &self,
        command: &str,
        input: &serde_json::Value,
        timeout: Option<Duration>,
    ) -> Result<String> {
        // Interpolate input into command
        let mut cmd = command.to_string();
        if let Some(obj) = input.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{}}}", key);
                let value_str = match value {
                    serde_json::Value::String(s) => s.clone(),
                    v => v.to_string(),
                };
                cmd = cmd.replace(&placeholder, &value_str);
            }
        }

        let child = Command::new("sh")
            .args(["-c", &cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AgentError::tool_execution(&cmd, e.to_string()))?;

        let output = match timeout {
            Some(t) => {
                match tokio::time::timeout(t, child.wait_with_output()).await {
                    Ok(result) => {
                        result.map_err(|e| AgentError::tool_execution(&cmd, e.to_string()))?
                    }
                    Err(_) => {
                        // Timeout occurred - process is left running (but will be cleaned up when dropped)
                        return Err(AgentError::timeout(t));
                    }
                }
            }
            None => child
                .wait_with_output()
                .await
                .map_err(|e| AgentError::tool_execution(&cmd, e.to_string()))?,
        };

        Ok(format!(
            "Exit: {}\n{}{}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }

    async fn execute_http(
        &self,
        url: &str,
        method: &str,
        headers: &HashMap<String, String>,
        input: &serde_json::Value,
    ) -> Result<String> {
        let client = reqwest::Client::new();

        let mut request = match method.to_uppercase().as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url).json(input),
            "PUT" => client.put(url).json(input),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url).json(input),
            _ => {
                return Err(AgentError::tool_execution(
                    url,
                    format!("Unknown HTTP method: {}", method),
                ))
            }
        };

        for (key, value) in headers {
            request = request.header(key, value);
        }

        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await?;

        Ok(format!("Status: {}\n{}", status, body))
    }

    async fn execute_custom(&self, handler: &Path, input: &serde_json::Value) -> Result<String> {
        let input_json = serde_json::to_string(input)?;

        let output = Command::new(handler)
            .arg(&input_json)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                AgentError::tool_execution(handler.display().to_string(), e.to_string())
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(AgentError::tool_execution(
                handler.display().to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    /// Get the JSON schema for a tool.
    pub fn tool_schema(&self, name: &str) -> Option<serde_json::Value> {
        if let Some(builtin) = self.builtins.get(name) {
            return Some(serde_json::json!({
                "name": builtin.name(),
                "description": builtin.description(),
                "input_schema": builtin.parameters(),
            }));
        }

        self.tools.get(name).map(|t| match t {
            ToolConfig::Builtin { name, .. } => self
                .builtins
                .get(name)
                .map(|b| {
                    serde_json::json!({
                        "name": b.name(),
                        "description": b.description(),
                        "input_schema": b.parameters(),
                    })
                })
                .unwrap_or_default(),
            ToolConfig::Command {
                name,
                description,
                args_schema,
                ..
            } => serde_json::json!({
                "name": name,
                "description": description,
                "input_schema": args_schema.clone().unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {}
                })),
            }),
            ToolConfig::Http {
                name,
                description,
                body_schema,
                ..
            } => serde_json::json!({
                "name": name,
                "description": description,
                "input_schema": body_schema.clone().unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {}
                })),
            }),
            ToolConfig::Custom {
                name,
                description,
                schema,
                ..
            } => serde_json::json!({
                "name": name,
                "description": description,
                "input_schema": schema.clone().unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {}
                })),
            }),
            _ => serde_json::json!({}),
        })
    }

    /// Get all tool schemas.
    pub fn all_tool_schemas(&self) -> Vec<serde_json::Value> {
        let mut schemas = Vec::new();

        // Add configured tools
        for name in self.tools.keys() {
            if let Some(schema) = self.tool_schema(name) {
                schemas.push(schema);
            }
        }

        schemas
    }
}

/// Trait for built-in tools.
#[async_trait]
pub trait BuiltinTool: Send + Sync {
    /// Tool name.
    fn name(&self) -> &str;

    /// Tool description.
    fn description(&self) -> &str;

    /// JSON Schema for parameters.
    fn parameters(&self) -> serde_json::Value;

    /// Execute the tool with given arguments.
    async fn execute(&self, args: serde_json::Value) -> Result<String>;
}

// === Built-in Tool Implementations ===

/// Read file contents.
struct ReadFileTool;

#[async_trait]
impl BuiltinTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the specified path"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AgentError::tool_execution("read_file", "Missing path"))?;

        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AgentError::tool_execution("read_file", e.to_string()))
    }
}

/// Write file contents.
struct WriteFileTool;

#[async_trait]
impl BuiltinTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file at the specified path"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AgentError::tool_execution("write_file", "Missing path"))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| AgentError::tool_execution("write_file", "Missing content"))?;

        tokio::fs::write(path, content)
            .await
            .map_err(|e| AgentError::tool_execution("write_file", e.to_string()))?;

        Ok(format!("Written {} bytes to {}", content.len(), path))
    }
}

/// Execute shell command.
struct ExecuteCommandTool;

#[async_trait]
impl BuiltinTool for ExecuteCommandTool {
    fn name(&self) -> &str {
        "execute_command"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| AgentError::tool_execution("execute_command", "Missing command"))?;

        let output = Command::new("sh")
            .args(["-c", command])
            .output()
            .await
            .map_err(|e| AgentError::tool_execution("execute_command", e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!(
            "Exit code: {}\nStdout:\n{}\nStderr:\n{}",
            output.status.code().unwrap_or(-1),
            stdout,
            stderr
        ))
    }
}

/// List files in a directory.
struct ListFilesTool;

#[async_trait]
impl BuiltinTool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files and directories in the specified path"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (defaults to current directory)"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or(".");

        let mut entries = tokio::fs::read_dir(path)
            .await
            .map_err(|e| AgentError::tool_execution("list_files", e.to_string()))?;

        let mut files = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AgentError::tool_execution("list_files", e.to_string()))?
        {
            let metadata = entry.metadata().await.ok();
            let file_type = metadata
                .map(|m| if m.is_dir() { "dir" } else { "file" })
                .unwrap_or("unknown");
            files.push(format!(
                "[{}] {}",
                file_type,
                entry.file_name().to_string_lossy()
            ));
        }

        Ok(files.join("\n"))
    }
}

/// Search for files matching a pattern.
struct SearchFilesTool;

#[async_trait]
impl BuiltinTool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search for files containing a pattern using grep"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (defaults to current directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| AgentError::tool_execution("search_files", "Missing pattern"))?;
        let path = args["path"].as_str().unwrap_or(".");

        let output = Command::new("grep")
            .args(["-r", "-l", "--include=*", pattern, path])
            .output()
            .await
            .map_err(|e| AgentError::tool_execution("search_files", e.to_string()))?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Get current time.
struct GetCurrentTimeTool;

#[async_trait]
impl BuiltinTool for GetCurrentTimeTool {
    fn name(&self) -> &str {
        "get_current_time"
    }

    fn description(&self) -> &str {
        "Get the current date and time in UTC"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<String> {
        Ok(chrono::Utc::now().to_rfc3339())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_executor_new() {
        let tools = vec![ToolConfig::Builtin {
            name: "read_file".to_string(),
            config: serde_json::json!({}),
        }];
        let executor = ToolExecutor::new(tools);
        assert!(executor.tools.contains_key("read_file"));
    }

    #[test]
    fn tool_executor_tool_name() {
        let builtin = ToolConfig::Builtin {
            name: "test".to_string(),
            config: serde_json::json!({}),
        };
        assert_eq!(ToolExecutor::tool_name(&builtin), "test");

        let mcp = ToolConfig::Mcp {
            server: "server1".to_string(),
            tool: "tool1".to_string(),
        };
        assert_eq!(ToolExecutor::tool_name(&mcp), "server1:tool1");
    }

    #[tokio::test]
    async fn builtin_read_file() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
        assert!(!tool.description().is_empty());

        let params = tool.parameters();
        assert!(params["properties"]["path"].is_object());
    }

    #[tokio::test]
    async fn builtin_write_file() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");

        let params = tool.parameters();
        assert!(params["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("path")));
        assert!(params["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("content")));
    }

    #[tokio::test]
    async fn builtin_execute_command() {
        let tool = ExecuteCommandTool;
        assert_eq!(tool.name(), "execute_command");

        // Test actual execution
        let result = tool
            .execute(serde_json::json!({"command": "echo hello"}))
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn builtin_list_files() {
        let tool = ListFilesTool;
        assert_eq!(tool.name(), "list_files");

        let result = tool.execute(serde_json::json!({"path": "."})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn builtin_get_current_time() {
        let tool = GetCurrentTimeTool;
        assert_eq!(tool.name(), "get_current_time");

        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_ok());
        // Should be valid RFC3339
        let time = result.unwrap();
        assert!(time.contains("T"));
        assert!(time.contains(":"));
    }

    #[test]
    fn tool_executor_schemas() {
        let tools = vec![ToolConfig::Builtin {
            name: "read_file".to_string(),
            config: serde_json::json!({}),
        }];
        let executor = ToolExecutor::new(tools);

        let schema = executor.tool_schema("read_file");
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema["name"], "read_file");
    }

    #[tokio::test]
    async fn tool_executor_execute_builtin() {
        let executor = ToolExecutor::new(vec![ToolConfig::Builtin {
            name: "get_current_time".to_string(),
            config: serde_json::json!({}),
        }]);

        let tool_call = ToolCall::new("call-1", "get_current_time", serde_json::json!({}));
        let result = executor.execute(&tool_call).await.unwrap();

        assert!(!result.is_error);
        assert!(!result.content.is_empty());
    }

    #[tokio::test]
    async fn tool_executor_execute_unknown() {
        let executor = ToolExecutor::new(vec![]);

        let tool_call = ToolCall::new("call-1", "unknown_tool", serde_json::json!({}));
        let result = executor.execute(&tool_call).await.unwrap();

        // Should return error result, not fail
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn tool_executor_execute_command_interpolation() {
        let tools = vec![ToolConfig::Command {
            name: "echo_msg".to_string(),
            description: "Echo a message".to_string(),
            command: "echo {message}".to_string(),
            args_schema: None,
            timeout: None,
        }];
        let executor = ToolExecutor::new(tools);

        let tool_call = ToolCall::new(
            "call-1",
            "echo_msg",
            serde_json::json!({"message": "hello world"}),
        );
        let result = executor.execute(&tool_call).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("hello world"));
    }

    #[test]
    fn tool_result_success() {
        let result = ToolResult::success("call-1", "Success!");
        assert!(!result.is_error);
        assert_eq!(result.tool_use_id, "call-1");
        assert_eq!(result.content, "Success!");
    }

    #[test]
    fn tool_result_error() {
        let result = ToolResult::error("call-1", "Failed!");
        assert!(result.is_error);
        assert_eq!(result.content, "Failed!");
    }
}
