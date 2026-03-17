# Memory System

> Cross-session persistent memory for AI agents.

---

## Overview

dtx provides a file-backed memory system through the `dtx-memory` crate. Memories persist across conversations, enabling AI agents to maintain context about users, projects, decisions, and feedback over time.

---

## Memory Kinds

| Kind | Purpose | Example |
|------|---------|---------|
| `user` | User role, preferences, expertise | "Senior Rust engineer, prefers minimal comments" |
| `project` | Ongoing work, goals, decisions | "Auth rewrite driven by compliance, deadline March 5" |
| `feedback` | Corrections and guidance | "Don't mock the database in integration tests" |
| `reference` | Pointers to external systems | "Pipeline bugs tracked in Linear project INGEST" |

---

## CLI Usage

```bash
# List all memories
dtx memory list

# Filter by kind
dtx memory list --kind project

# Read a specific memory
dtx memory read onboarding

# Write a new memory
dtx memory write auth-decision

# Edit an existing memory
dtx memory edit auth-decision

# Delete a memory
dtx memory delete old-note
```

---

## MCP Tools

When running as an MCP server (`dtx mcp`), 7 memory tools + 2 meta-cognitive tools are available:

### CRUD Operations

- **`list_memories`** — List all memories with optional kind/tag filters
- **`read_memory`** — Read a specific memory by name
- **`write_memory`** — Create a new memory (name, content, kind, description, tags)
- **`edit_memory`** — Update an existing memory's content or metadata
- **`delete_memory`** — Delete a memory by name

### Meta-Cognitive Tools

- **`reflect`** — Synthesize the entire memory landscape into actionable insights. Useful for understanding project context at the start of a session.
- **`checkpoint`** — Save current session progress as a timestamped memory. Auto-tagged with `checkpoint` and `session`.

### Onboarding

- **`onboarding`** — Analyze project structure (files, languages, frameworks) and save findings as an `onboarding` memory. Subsequent calls update the existing memory.
- **`initial_instructions`** — Return project-specific instructions for AI agent bootstrap. Reads CLAUDE.md, existing memories, and project config.

---

## Memory Structure

Each memory is stored as a file with metadata:

```rust
pub struct Memory {
    pub name: String,          // kebab-case identifier
    pub content: String,       // Free-form content
    pub kind: MemoryKind,      // User | Project | Feedback | Reference
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

---

## Storage

Memories are stored in a project-local directory (`.dtx/memory/`) as individual files. The store supports:

- **CRUD**: Create, read, update, delete by name
- **Search**: Filter by kind, tags, or text content
- **Atomic writes**: File-level atomicity for concurrent safety

---

## Recommended Workflow

```
1. Start session → call `initial_instructions` for project context
2. If new project → call `onboarding` to analyze and bootstrap
3. If returning → call `reflect` to synthesize recent memories
4. During work → `write_memory` for decisions, context, feedback
5. End session → call `checkpoint` to save progress
```

---

## See Also

- [MCP Integration](./mcp-integration.md) — Full MCP tool reference
- [CLI Reference](./cli-reference.md) — `dtx memory` commands
