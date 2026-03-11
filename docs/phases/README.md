# dtx v2 Implementation Phases

> Ship incrementally. Validate continuously. Never skip verification.

---

## Philosophy

```
USER FIRST     - Every phase delivers user value
FAST ITERATION - Small batches, quick feedback
VERIFICATION   - Nothing merges without tests
CONSISTENCY    - Same patterns everywhere
```

---

## Status Tracking

| Phase | Sub | Name | Status | Completed |
|-------|-----|------|--------|-----------|
| **1** | | **FOUNDATION** | ✅ | |
| | 1.1 | Resource Trait & Lifecycle | ✅ Completed | 57 tests |
| | 1.2 | EventBus v2 | ✅ Completed | 64 tests |
| | 1.3 | Context & Error Handling | ✅ Completed | 71 tests |
| **2** | | **MIDDLEWARE** | ✅ | |
| | 2.1 | Middleware Trait & Stack | ✅ Completed | 17 tests |
| | 2.2 | Logging Middleware | ✅ Completed | 3 tests |
| | 2.3 | Metrics Middleware | ✅ Completed | 12 tests |
| | 2.4 | Timeout & Retry Middleware | ✅ Completed | 16 tests |
| **3** | | **PROTOCOL** | ✅ | |
| | 3.1 | Protocol Core | ✅ Completed | 12 tests |
| | 3.2 | Transport Layer | ✅ Completed | 4 tests |
| | 3.3 | MCP Resources & Tools | ✅ Completed | 15 tests |
| **4** | | **PROCESS BACKEND** | ✅ | |
| | 4.1 | Process Resource | ✅ Completed | 22 tests |
| | 4.2 | Orchestrator v2 | ✅ Completed | 10 tests |
| | 4.3 | Migration & Cleanup | ✅ Completed | CLI uses dtx-process |
| | 4.4 | Process Improvements | ✅ Completed | 52 tests + codebase inference (new) |
| **5** | | **AI INTEGRATION** | ✅ | |
| | 5.1 | AI Middleware | ✅ Completed | 17 tests |
| | 5.2 | Natural Language Commands | ✅ Completed | 14 tests |
| | 5.3 | MCP Server Mode | ✅ Completed | CLI cmd |
| **6** | | **TRANSLATION** | ✅ | |
| | 6.1 | Translation Trait & Registry | ✅ Completed | 89 tests |
| | 6.2 | Process ↔ Container | ✅ Completed | ContainerConfig, inference |
| | 6.3 | Export Formats | ✅ Completed | 48 tests |
| | 6.4 | Configuration Import | ✅ Completed | CLI import command |
| **7** | | **PLUGIN SYSTEM** | ✅ | |
| | 7.1 | Plugin Manifest & Loader | ✅ Completed | dtx-plugin crate |
| | 7.2 | Backend Plugins | ✅ Completed | BackendPlugin trait |
| | 7.3 | Middleware Plugins | ✅ Completed | MiddlewarePlugin trait |
| | 7.4 | Plugin Sandbox (WASM) | ✅ Completed | WASM runtime, capabilities |
| **8** | | **ADDITIONAL BACKENDS** | ✅ | |
| | 8.1 | Container Backend | ✅ Completed | 69 tests |
| | 8.2 | VM Backend | ✅ Completed | 93 tests (QEMU, Firecracker, NixOS) |
| | 8.3 | Agent Backend | ✅ Completed | 97 tests (Claude, OpenAI, Ollama) |
**Legend:** 🔲 Not Started | 🔄 In Progress | ✅ Completed | ⏸️ Blocked

---

## Phase Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 1: FOUNDATION                                                     │
│ Core abstractions that everything builds on                             │
├─────────────────────────────────────────────────────────────────────────┤
│ 1.1 Resource Trait & Lifecycle       │ The universal abstraction       │
│ 1.2 EventBus v2                      │ Central nervous system          │
│ 1.3 Context & Error Handling         │ Request context, typed errors   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 2: MIDDLEWARE                                                     │
│ Tower-style composition layer                                           │
├─────────────────────────────────────────────────────────────────────────┤
│ 2.1 Middleware Trait & Stack         │ Core middleware abstraction     │
│ 2.2 Logging Middleware               │ Structured tracing              │
│ 2.3 Metrics Middleware               │ Prometheus/OTLP export          │
│ 2.4 Timeout & Retry Middleware       │ Resilience patterns             │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 3: PROTOCOL                                                       │
│ MCP-compatible communication layer                                      │
├─────────────────────────────────────────────────────────────────────────┤
│ 3.1 Protocol Core                    │ JSON-RPC message types          │
│ 3.2 Transport Layer                  │ stdio, HTTP, WebSocket          │
│ 3.3 MCP Resources & Tools            │ AI agent integration            │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 4: PROCESS BACKEND                                                │
│ Migrate current process management to new architecture                  │
├─────────────────────────────────────────────────────────────────────────┤
│ 4.1 Process Resource                 │ Process implements Resource     │
│ 4.2 Orchestrator v2                  │ New orchestrator on middleware  │
│ 4.3 Migration & Cleanup              │ Remove deprecated code          │
│ 4.4 Process Improvements             │ Log streaming, restart, detect  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 5: AI INTEGRATION                                                 │
│ AI assistance and natural language                                      │
├─────────────────────────────────────────────────────────────────────────┤
│ 5.1 AI Middleware                    │ Suggestions, explanations       │
│ 5.2 Natural Language Commands        │ "start postgres with redis"     │
│ 5.3 MCP Server Mode                  │ dtx as MCP server for AI agents │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 6: TRANSLATION                                                    │
│ Resource type conversion                                                │
├─────────────────────────────────────────────────────────────────────────┤
│ 6.1 Translation Trait & Registry     │ A ↔ B conversion framework     │
│ 6.2 Process ↔ Container              │ First translator               │
│ 6.3 Export Formats                   │ docker-compose, k8s manifests  │
│ 6.4 Configuration Import             │ Import from external configs   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 7: PLUGIN SYSTEM                                                  │
│ Community extensibility                                                 │
├─────────────────────────────────────────────────────────────────────────┤
│ 7.1 Plugin Manifest & Loader         │ Dynamic plugin loading          │
│ 7.2 Backend Plugins                  │ Custom resource backends        │
│ 7.3 Middleware Plugins               │ Custom middleware               │
│ 7.4 Plugin Sandbox (WASM)            │ Safe plugin execution           │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 8: ADDITIONAL BACKENDS                                            │
│ Expand resource types                                                   │
├─────────────────────────────────────────────────────────────────────────┤
│ 8.1 Container Backend                │ Docker/Podman support           │
│ 8.2 VM Backend                       │ Nix VMs, QEMU                   │
│ 8.3 Agent Backend                    │ AI agent orchestration          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Phase Rules

### Every Phase MUST

```
1. DEFINE      - Clear scope, no ambiguity
2. BREAKDOWN   - Tasks ≤ 4 hours each
3. VERIFY      - Tests for every feature
4. DOCUMENT    - Update docs with changes
5. DEMO        - User-visible improvement
```

### Every Task MUST

```
1. ACCEPTANCE  - Clear done criteria
2. TESTS       - Unit + integration where applicable
3. EXAMPLES    - Code examples in docs
4. MIGRATION   - Path from previous state
5. REVIEW      - Code review checklist
```

### Verification Checklist

```
□ cargo check passes
□ cargo test passes
□ New interfaces verified (CLI commands, protocols, APIs)
□ Documentation updated
□ CHANGELOG entry added
□ Example code compiles
□ Migration guide (if breaking)

Note: fmt/clippy checks are deferred to final release stage.
Focus on delivery - verify the feature works, not formatting.
```

---

## Dependencies

```
Phase 1 ──► Phase 2 ──► Phase 4
   │           │
   │           └──► Phase 3 ──► Phase 5
   │
   └──► Phase 6

Phase 4 + Phase 7 ──► Phase 8
```

---

## Timeline (Estimated)

```
Phase 1: 2 weeks   (Foundation)
Phase 2: 2 weeks   (Middleware)
Phase 3: 2 weeks   (Protocol)
Phase 4: 3 weeks   (Process Backend)
Phase 5: 2 weeks   (AI Integration)
Phase 6: 2 weeks   (Translation)
Phase 7: 3 weeks   (Plugin System)
Phase 8: 4 weeks   (Additional Backends)
─────────────────────────────────
Total:   20 weeks
```

---

## Files

```
docs/v2/phases/
├── README.md           # This file
├── 1.1-resource-trait.md
├── 1.2-eventbus-v2.md
├── 1.3-context-errors.md
├── 2.1-middleware-trait.md
├── 2.2-logging-middleware.md
├── 2.3-metrics-middleware.md
├── 2.4-timeout-retry.md
├── 3.1-protocol-core.md
├── 3.2-transport-layer.md
├── 3.3-mcp-integration.md
├── 4.1-process-resource.md
├── 4.2-orchestrator-v2.md
├── 4.3-migration-cleanup.md
├── 4.4-process-improvements.md
├── 5.1-ai-middleware.md
├── 5.2-natural-language.md
├── 5.3-mcp-server.md
├── 6.1-translation-trait.md
├── 6.2-process-container.md
├── 6.3-export-formats.md
├── 6.4-config-import.md
├── 7.1-plugin-loader.md
├── 7.2-backend-plugins.md
├── 7.3-middleware-plugins.md
├── 7.4-plugin-sandbox.md
├── 8.1-container-backend.md
├── 8.2-vm-backend.md
└── 8.3-agent-backend.md
```
