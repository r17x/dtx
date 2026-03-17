# MCP Integration Guide

> Use dtx with AI agents via Model Context Protocol.

---

## Overview

dtx implements the [Model Context Protocol](https://modelcontextprotocol.io) (MCP), enabling AI agents like Claude to:

- Start/stop services
- Check service status
- Read logs
- Configure resources

---

## Setup with Claude Desktop

1. Install dtx:
```bash
nix profile install github:r17x/dtx
```

2. Add to Claude Desktop config (`~/.config/claude/claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "dtx": {
      "command": "dtx",
      "args": ["mcp"],
      "env": {
        "DXT_PROJECT": "/path/to/your/project"
      }
    }
  }
}
```

3. Restart Claude Desktop

---

## Available Tools

dtx exposes 30 MCP tools across four categories. Tools are feature-gated — code and memory tools require the respective features enabled at build time.

### Resource Management (8 tools, always enabled)

| Tool | Description |
|------|-------------|
| `start_resource` | Start a service by name |
| `stop_resource` | Stop a service |
| `restart_resource` | Restart a service |
| `get_status` | Check service status |
| `list_resources` | List all services |
| `get_logs` | Get recent logs (with optional line limit and service filter) |
| `start_all` | Start all enabled services |
| `stop_all` | Stop all running services |

### Code Intelligence (13 tools, feature: `code`)

Symbol-aware code navigation and editing powered by ast-grep.

| Tool | Description |
|------|-------------|
| `get_symbols_overview` | Show file structure with functions, structs, impls and line ranges |
| `find_symbol` | Search by name path (e.g., `MyStruct/method`) |
| `find_references` | Find all references to a symbol across the codebase |
| `find_referencing_symbols` | Like find_references but shows the containing function |
| `search_pattern` | Regex search with configurable context lines |
| `replace_symbol_body` | Replace a symbol's body with new content (optimistic locking via content hash) |
| `insert_before_symbol` | Insert code before a symbol |
| `insert_after_symbol` | Insert code after a symbol |
| `insert_at_line` | Insert code at a specific line number |
| `replace_lines` | Replace a range of lines |
| `rename_symbol` | Rename a symbol across all files |
| `find_file` | Find files by glob pattern |
| `list_dir` | List directory contents with metadata |

### Memory Management (7 tools, feature: `memory`)

Persistent cross-session memory for AI agents.

| Tool | Description |
|------|-------------|
| `list_memories` | List memories with optional kind/tag filters |
| `read_memory` | Read a specific memory by name |
| `write_memory` | Create a new memory (name, content, kind, description, tags) |
| `edit_memory` | Update an existing memory's content or metadata |
| `delete_memory` | Delete a memory by name |
| `reflect` | Meta-cognitive: synthesize memory landscape into insights |
| `checkpoint` | Save session progress as a timestamped memory |

Memory kinds: `user`, `project`, `feedback`, `reference`

### Onboarding (2 tools, requires `code` + `memory`)

| Tool | Description |
|------|-------------|
| `onboarding` | Analyze project structure and save context to memory |
| `initial_instructions` | Return project-specific instructions for AI agent bootstrap |

---

## Available Resources

Resources are exposed via MCP URIs:

```
dtx://project/{project_id}              # Project info
dtx://project/{id}/resource/{name}      # Service status
dtx://project/{id}/resource/{name}/logs # Service logs
```

---

## Example Conversations

### Starting Development

```
You: "I want to start working on my project"
Claude: I'll start all the services for you.
[Calls start_all]
All services are now running:
- postgres: healthy (port 5432)
- redis: healthy (port 6379)
- api: healthy (port 3000)
```

### Debugging

```
You: "The API isn't responding"
Claude: Let me check the status and logs.
[Calls get_status for api]
[Calls get_logs for api]
The API is running but showing connection errors to postgres.
Let me check postgres:
[Calls get_status for postgres]
Postgres is stopped. Would you like me to start it?
```

### Configuration Help

```
You: "Add a new worker service that processes background jobs"
Claude: I'll help you configure that. Based on your setup, here's a suggested configuration:
[Provides .dtx/config.yaml snippet]
The worker depends on both postgres and redis. Should I add this?
```

---

## Natural Language Commands

With AI middleware enabled, you can use natural language:

```bash
dtx run "start postgres and redis, then the api"
dtx run "show me what's using port 3000"
dtx run "restart anything that's unhealthy"
```

---

## Custom MCP Server

Run dtx as a standalone MCP server:

```bash
# stdio mode (for MCP clients)
dtx mcp

# HTTP mode
dtx mcp --http --port 8080

# With specific project
dtx mcp --project /path/to/project
```

---

## Programmatic Access

Use the protocol directly:

```python
import json
import subprocess

# Start MCP server
proc = subprocess.Popen(
    ["dtx", "mcp"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
)

# Send request
request = {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "start_resource",
        "arguments": {"id": "postgres"}
    },
    "id": 1
}

proc.stdin.write(json.dumps(request).encode() + b"\n")
proc.stdin.flush()

# Read response
response = json.loads(proc.stdout.readline())
print(response)
```

---

## Troubleshooting

### "MCP server not responding"

1. Check dtx is installed: `which dtx`
2. Check project path exists: `ls $DXT_PROJECT`
3. Run manually: `dtx mcp` and try a command

### "Tool not found"

Ensure you're using dtx v2. Check: `dtx --version`

### "Permission denied"

MCP runs with your user permissions. Ensure:
- You can run `dtx start` manually
- Project directory is accessible
