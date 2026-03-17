# Code Intelligence

> Symbol-aware code navigation, search, and editing via ast-grep.

---

## Overview

dtx provides language-aware code intelligence through the `dtx-code` crate, powered by [ast-grep](https://ast-grep.github.io/). This enables AI agents (via MCP) and CLI users to navigate, search, and safely edit code with structural understanding rather than plain text matching.

---

## Capabilities

| Feature | Description |
|---------|-------------|
| Symbol Overview | File structure showing functions, structs, impls with line ranges |
| Symbol Search | Find symbols by name path (e.g., `MyStruct/method`) |
| Reference Finding | Cross-file usage tracking with context |
| Safe Editing | Optimistic locking via SHA256 content hashes |
| Cross-file Rename | Rename a symbol across all files in the workspace |
| Pattern Search | Regex search with configurable context lines |

---

## CLI Usage

```bash
# Show file structure
dtx code symbols src/main.rs

# Find a symbol by path
dtx code find "Resource/start"

# Find all references to a symbol
dtx code references "ResourceId" src/lib.rs

# Search with regex
dtx code search "async fn.*start"

# Rename across files
dtx code rename "old_name" "new_name" src/lib.rs
```

---

## MCP Tools

When running as an MCP server (`dtx mcp`), 13 code intelligence tools are available:

### Navigation

- **`get_symbols_overview`** — Show file structure with line ranges. Start here before reading files.
- **`find_symbol`** — Search by name path (e.g., `MyStruct/method`). Returns symbol definition with source.
- **`find_references`** — Find all usages of a symbol across the codebase.
- **`find_referencing_symbols`** — Like find_references but returns the containing function for each reference.
- **`find_file`** — Find files by glob pattern.
- **`list_dir`** — List directory contents with file type and size metadata.

### Search

- **`search_pattern`** — Regex search with configurable context lines and file filtering.

### Editing

All editing tools use optimistic locking: you provide a content hash from when you read the file, and the edit only applies if the file hasn't changed since.

- **`replace_symbol_body`** — Replace a symbol's entire body. Requires content hash.
- **`insert_before_symbol`** — Insert code before a symbol definition.
- **`insert_after_symbol`** — Insert code after a symbol definition.
- **`insert_at_line`** — Insert code at a specific line number.
- **`replace_lines`** — Replace a range of lines.
- **`rename_symbol`** — Rename a symbol across all files in the workspace.

---

## Optimistic Locking

To prevent concurrent edit conflicts, all editing operations use SHA256 content hashes:

```
1. Agent reads file → dtx returns content + SHA256 hash
2. Agent sends edit with the hash it received
3. dtx verifies hash matches current file content
4. Edit applies only if hashes match
5. If mismatch → error, agent must re-read and retry
```

This ensures safe concurrent editing without file locks.

---

## Supported Languages

Language detection is automatic based on file extension. ast-grep supports:

- Rust, Go, Python, JavaScript, TypeScript, C, C++, Java, Ruby, Kotlin, Swift, Lua, and more

---

## Architecture

```
dtx-code/
├── workspace.rs   # WorkspaceIndex: file discovery, indexing
├── symbol.rs      # Symbol tree: kinds, overview, line ranges
├── parser.rs      # AST parsing via ast-grep
├── language.rs    # File extension → language detection
├── references.rs  # Cross-file reference finding
├── rename.rs      # Cross-file rename with reference tracking
├── edit.rs        # Symbol-aware editing with content hashing
├── search.rs      # Regex pattern search
├── patterns.rs    # AST pattern definitions
└── index.rs       # File content indexing
```

---

## See Also

- [MCP Integration](./mcp-integration.md) — Full MCP tool reference
- [CLI Reference](./cli-reference.md) — `dtx code` commands
