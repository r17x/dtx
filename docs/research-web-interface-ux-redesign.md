# UX Research Report: dtx-web Redesign

> **Target Research**: Gas Town UI POC (https://gas-town-ui-poc.onrender.com/)
> **Target Redesign**: dtx-web
> **Date**: 2026-01-25
> **Methodology**: Interactive exploration via Chrome DevTools MCP

---

## Summary

### Executive Overview

This research documents the complete UX analysis of the Gas Town UI POC—a sophisticated multi-agent orchestration dashboard—and provides prescriptive redesign guidance for transforming dtx-web from a conventional web application into a command-and-control interface optimized for developer tooling.

### Key Findings

| Dimension | Gas Town UI | Current dtx-web | Gap Severity |
|-----------|-------------|-----------------|--------------|
| Visual Identity | Cyberpunk industrial aesthetic | Generic enterprise | **High** |
| Information Density | High-density data visualization | Low-density cards | **High** |
| Real-time Feedback | Pervasive live updates | Polling-based status | **Medium** |
| Interaction Model | Multi-modal (graph, feed, terminal) | Tab-based navigation | **High** |
| State Communication | Rich status hierarchy | Binary running/stopped | **High** |
| Professional Feel | "Mission control" authority | "Admin dashboard" utility | **High** |

### Recommended Approach

Transform dtx-web into a **"Dev Ops Command Center"** by adopting:
1. Dark theme with accent color system
2. Force-directed service graph visualization
3. Real-time activity feed with rich events
4. Terminal-style command interface
5. Multi-state service status indicators

---

## Design System

### 1. Color Palette

#### Gas Town Reference Palette

```css
/* Primary Background */
--bg-primary: #0d1117;      /* Near-black slate */
--bg-secondary: #161b22;    /* Dark panel backgrounds */
--bg-tertiary: #1a1f26;     /* Card/container backgrounds */

/* Accent Colors - Semantic Status */
--accent-cyan: #22d3ee;     /* Active/connected/online */
--accent-green: #22c55e;    /* Running/success/approved */
--accent-orange: #f59e0b;   /* Processing/warning/coordinator */
--accent-magenta: #ec4899;  /* Error/critical/attention */
--accent-purple: #a855f7;   /* Special/ephemeral states */

/* Text Hierarchy */
--text-primary: #e5e7eb;    /* Main content */
--text-secondary: #9ca3af;  /* Supporting text */
--text-muted: #6b7280;      /* Timestamps, metadata */

/* Borders & Dividers */
--border-subtle: #2d333b;   /* Panel borders */
--border-focus: #3b82f6;    /* Focus indicators */
--border-glow: rgba(34, 211, 238, 0.3); /* Hover glow effects */
```

#### Proposed dtx Palette Mapping

| Gas Town | dtx Semantic Use |
|----------|------------------|
| Cyan | Service running, healthy |
| Green | Build success, tests passing |
| Orange | Service starting, compiling |
| Magenta | Service stopped, error state |
| Purple | Nix environment, package info |

### 2. Typography

```css
/* Monospace Stack - Technical Authority */
--font-mono: 'JetBrains Mono', 'Fira Code', 'SF Mono', 'Consolas', monospace;

/* System Sans - UI Elements */
--font-sans: 'Inter', 'SF Pro Display', -apple-system, sans-serif;

/* Scale */
--text-xs: 0.75rem;    /* Timestamps, badges */
--text-sm: 0.875rem;   /* Body, secondary */
--text-base: 1rem;     /* Primary content */
--text-lg: 1.125rem;   /* Section headers */
--text-xl: 1.25rem;    /* Panel titles */
--text-2xl: 1.5rem;    /* Page titles */
```

### 3. Component Specifications

#### 3.1 Status Badge

**Gas Town Pattern**: Pill-shaped badges with glow effects and color-coded semantics.

```html
<!-- Status Badge Component -->
<span class="status-badge status-running">
  <span class="status-indicator"></span>
  RUNNING
</span>

<style>
.status-badge {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.25rem 0.75rem;
  border-radius: 9999px;
  font-size: 0.75rem;
  font-weight: 500;
  font-family: var(--font-mono);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.status-indicator {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  animation: pulse 2s infinite;
}

.status-running {
  background: rgba(34, 197, 94, 0.15);
  color: #22c55e;
  border: 1px solid rgba(34, 197, 94, 0.3);
}

.status-running .status-indicator {
  background: #22c55e;
  box-shadow: 0 0 8px #22c55e;
}

.status-stopped {
  background: rgba(107, 114, 128, 0.15);
  color: #9ca3af;
  border: 1px solid rgba(107, 114, 128, 0.3);
}

.status-starting {
  background: rgba(245, 158, 11, 0.15);
  color: #f59e0b;
  border: 1px solid rgba(245, 158, 11, 0.3);
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}
</style>
```

#### 3.2 Activity Feed Item

**Gas Town Pattern**: Compact event rows with agent identification, timestamp, and action description.

```html
<!-- Activity Feed Item -->
<button class="activity-item">
  <div class="activity-agent">
    <span class="agent-icon">&#x2699;</span>
    <span class="agent-name">postgresql</span>
    <span class="agent-status"></span>
  </div>
  <span class="activity-timestamp">5m ago</span>
  <p class="activity-message">Service started on port 5432</p>
</button>

<style>
.activity-item {
  display: grid;
  grid-template-columns: 1fr auto;
  grid-template-rows: auto auto;
  gap: 0.25rem 1rem;
  padding: 0.75rem 1rem;
  background: transparent;
  border: none;
  border-bottom: 1px solid var(--border-subtle);
  cursor: pointer;
  text-align: left;
  transition: background 0.15s ease;
}

.activity-item:hover {
  background: rgba(255, 255, 255, 0.02);
}

.activity-agent {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.agent-name {
  color: var(--accent-cyan);
  font-weight: 500;
}

.agent-status {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent-green);
}

.activity-timestamp {
  color: var(--text-muted);
  font-size: var(--text-xs);
}

.activity-message {
  grid-column: 1 / -1;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}
</style>
```

#### 3.3 Command Panel / Terminal

**Gas Town Pattern**: Full-featured command input with autocomplete, keyboard hints, and mode toggles.

```html
<!-- Command Terminal Panel -->
<div class="terminal-panel">
  <div class="terminal-header">
    <div class="terminal-title">
      <span class="terminal-icon">&gt;_</span>
      DTX TERMINAL
      <span class="terminal-status online">ONLINE</span>
    </div>
    <div class="terminal-actions">
      <button class="terminal-action">HISTORY</button>
      <button class="terminal-close">&times;</button>
    </div>
  </div>

  <div class="terminal-hint">
    <kbd>ESC</kbd> to focus or unfocus terminal
  </div>

  <div class="terminal-input-row">
    <span class="terminal-prompt">/</span>
    <input type="text"
           class="terminal-input"
           placeholder="Enter command..."
           autocomplete="off" />
    <button class="terminal-submit" disabled>
      <span class="submit-arrow">&rarr;</span>
    </button>
  </div>
</div>
```

#### 3.4 Service Graph Node

**Gas Town Pattern**: Circular nodes with color-coded rings, connection lines, and hover tooltips.

```css
/* SVG Node Styles */
.graph-node {
  cursor: pointer;
  transition: transform 0.2s ease;
}

.graph-node:hover {
  transform: scale(1.1);
}

.graph-node-circle {
  fill: var(--bg-secondary);
  stroke-width: 3;
}

.graph-node-circle.running {
  stroke: var(--accent-cyan);
  filter: drop-shadow(0 0 6px var(--accent-cyan));
}

.graph-node-circle.stopped {
  stroke: var(--text-muted);
}

.graph-node-circle.coordinator {
  stroke: var(--accent-orange);
  stroke-width: 4;
  filter: drop-shadow(0 0 10px var(--accent-orange));
}

.graph-edge {
  stroke: var(--border-subtle);
  stroke-width: 1.5;
  fill: none;
}

.graph-label {
  fill: var(--text-primary);
  font-size: 12px;
  font-family: var(--font-mono);
  text-anchor: middle;
}
```

---

## Interaction Patterns

### 1. Click Interactions

| Element | Gas Town Behavior | Recommendation for dtx |
|---------|-------------------|------------------------|
| Activity Feed Item | Opens detail panel/modal | Open service detail panel |
| Graph Node | Highlights node, shows tooltip, selects for detail | Same |
| Dropdown Button | Expands with rich options (icon + title + description) | Filter projects/services |
| Tab Button | Switches content with underline animation | Same pattern |
| Action Button | Immediate feedback with loading state | Same pattern |

### 2. Hover States

```css
/* Hover Feedback Patterns */

/* Subtle lift effect */
.card:hover {
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
}

/* Border glow */
.interactive-element:hover {
  border-color: var(--accent-cyan);
  box-shadow: 0 0 0 1px var(--accent-cyan),
              0 0 20px rgba(34, 211, 238, 0.1);
}

/* Background fade */
.list-item:hover {
  background: rgba(255, 255, 255, 0.03);
}
```

### 3. State Transitions

```css
/* Smooth transitions for all interactive states */
* {
  transition-property: background-color, border-color, color,
                       box-shadow, transform, opacity;
  transition-duration: 150ms;
  transition-timing-function: ease-out;
}

/* Loading spinner */
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.loading-spinner {
  animation: spin 1s linear infinite;
}
```

### 4. Feedback Patterns

| State | Visual Feedback |
|-------|-----------------|
| Loading | Pulsing opacity + spinner icon |
| Success | Green flash + checkmark |
| Error | Red border + shake animation |
| Focus | Cyan border glow |
| Disabled | 50% opacity + cursor not-allowed |

---

## Layout Structures

### 1. Overall Layout (Gas Town)

```
┌─────────────────────────────────────────────────────────────┐
│ HEADER: Logo │ Stats │ Filters │ Actions                    │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────┐                                    ┌────────┐ │
│  │ ACTIVITY │     MAIN VISUALIZATION             │ DETAIL │ │
│  │   FEED   │        (Graph/Grid)                │ PANEL  │ │
│  │          │                                    │        │ │
│  │  ┌────┐  │                                    │        │ │
│  │  │item│  │         ┌───┐   ┌───┐              │        │ │
│  │  ├────┤  │        /│ ● │───│ ● │\             │        │ │
│  │  │item│  │       / └───┘   └───┘ \            │        │ │
│  │  ├────┤  │      /                 \           │        │ │
│  │  │item│  │    ┌───┐    ┌───┐    ┌───┐        │        │ │
│  │  └────┘  │    │ ● │────│ ● │────│ ● │        │        │ │
│  │          │    └───┘    └───┘    └───┘        │        │ │
│  └──────────┘                                    └────────┘ │
│                                                             │
│  ┌────────────────┐  ┌─────────────────────────────────────┐│
│  │ SIGNAL FILTER  │  │         VOICE/TERMINAL              ││
│  │  [search...]   │  │  >_ Enter command...                ││
│  │  [ALL] [MAYOR] │  └─────────────────────────────────────┘│
│  └────────────────┘                                         │
└─────────────────────────────────────────────────────────────┘
```

### 2. Proposed dtx Layout

```
┌─────────────────────────────────────────────────────────────┐
│ dtx │ STATUS: ONLINE │ 5 SERVICES │ 3 RUNNING │ [TERMINAL] │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────┐                                    ┌────────┐ │
│  │  EVENTS  │     SERVICE GRAPH                  │SERVICE │ │
│  │          │                                    │ DETAIL │ │
│  │ postgres │         ┌─────┐                    │        │ │
│  │ ● started│        /│redis│\                   │ name   │ │
│  │ 2m ago   │       / └─────┘ \                  │ status │ │
│  │          │      /           \                 │ port   │ │
│  │ redis    │   ┌─────┐     ┌─────┐             │ nixpkg │ │
│  │ ● ready  │   │ api │─────│ web │             │ logs   │ │
│  │ 5m ago   │   └─────┘     └─────┘             │        │ │
│  │          │       \         /                  │[START] │ │
│  │ api      │        \       /                   │[STOP]  │ │
│  │ ● health │         ┌─────┐                    │        │ │
│  │ passed   │         │postgres                  └────────┘ │
│  │          │         └─────┘                               │
│  └──────────┘                                               │
│                                                             │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ >_ dtx start api --port 3000                            ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

---

## UI/UX Audit: Current dtx-web

### Strengths

1. **HTMX Foundation**: Already uses HTMX for dynamic updates—good base for real-time features
2. **SSE Support**: Has SSE infrastructure for live updates
3. **Mermaid Graphs**: Dependency graph visualization exists
4. **Tab Navigation**: Organized content structure

### Gaps & Issues

| Issue | Current State | Impact | Priority |
|-------|---------------|--------|----------|
| **Light theme** | Gray-100 background, white cards | Lacks "command center" authority | P0 |
| **Binary status** | Only running/stopped | Missing nuanced states (starting, unhealthy, etc.) | P0 |
| **No activity feed** | Static service list | No sense of system liveliness | P1 |
| **Basic graph** | Mermaid static diagram | No interactive exploration | P1 |
| **No terminal** | Buttons only | Missing power-user interface | P1 |
| **Generic typography** | System fonts | Lacks technical character | P2 |
| **Minimal hover states** | Basic color change | Weak interaction feedback | P2 |
| **No keyboard shortcuts** | None | Poor power-user efficiency | P2 |

### Component-by-Component Gap Analysis

#### Header (`base.html`)
- **Current**: White nav bar, indigo logo, text links
- **Gap**: No status metrics, no global actions
- **Fix**: Add service count badges, global status indicator, terminal toggle

#### Status Badge (`status_panel.html`)
- **Current**: Single span with green/gray bg
- **Gap**: No pulse animation, no intermediate states
- **Fix**: Full status badge component with indicator dot and glow

#### Service List (`project_detail.html`)
- **Current**: Flat list with delete buttons
- **Gap**: No real-time status, no rich actions
- **Fix**: Card-based layout with status, logs preview, action buttons

#### Dependency Graph (`dependency_graph.html`)
- **Current**: Mermaid static rendering
- **Gap**: Not interactive, no node selection
- **Fix**: D3.js force-directed graph with click-to-select

---

## Prescriptive Redesign Guidance

### Phase 1: Foundation (Dark Theme + Typography)

**Files to modify**: `base.html`, add `styles/theme.css`

```html
<!-- Updated base.html -->
<body class="bg-slate-950 text-slate-200 min-h-screen font-mono">
```

**Tailwind config additions**:
```javascript
module.exports = {
  theme: {
    extend: {
      colors: {
        'dtx-bg': '#0d1117',
        'dtx-panel': '#161b22',
        'dtx-border': '#2d333b',
        'dtx-cyan': '#22d3ee',
        'dtx-green': '#22c55e',
        'dtx-orange': '#f59e0b',
        'dtx-magenta': '#ec4899',
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'Fira Code', 'monospace'],
      }
    }
  }
}
```

### Phase 2: Component Upgrade (Status + Activity)

**New partial**: `partials/status_badge.html`
```html
<span class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium tracking-wider
             {% if status == 'running' %}bg-emerald-500/15 text-emerald-400 border border-emerald-500/30{% endif %}
             {% if status == 'stopped' %}bg-slate-500/15 text-slate-400 border border-slate-500/30{% endif %}
             {% if status == 'starting' %}bg-amber-500/15 text-amber-400 border border-amber-500/30{% endif %}">
  <span class="w-1.5 h-1.5 rounded-full {% if status == 'running' %}bg-emerald-400 animate-pulse{% else %}bg-current{% endif %}"></span>
  {{ status|upper }}
</span>
```

**New partial**: `partials/activity_feed.html`
```html
<aside class="w-80 bg-dtx-panel border-r border-dtx-border overflow-y-auto">
  <header class="sticky top-0 bg-dtx-panel px-4 py-3 border-b border-dtx-border">
    <h2 class="flex items-center gap-2 text-sm font-medium">
      <span class="text-dtx-cyan">&#x2699;</span>
      ACTIVITY FEED
      <span class="ml-auto px-2 py-0.5 rounded bg-dtx-cyan/20 text-dtx-cyan text-xs">{{ events|length }}</span>
    </h2>
  </header>
  <ul hx-ext="sse" sse-connect="/api/events" sse-swap="message">
    {% for event in events %}
    <li class="px-4 py-3 border-b border-dtx-border hover:bg-white/[0.02] cursor-pointer transition-colors">
      <div class="flex items-center justify-between">
        <span class="text-dtx-cyan font-medium">{{ event.service }}</span>
        <time class="text-xs text-slate-500">{{ event.time_ago }}</time>
      </div>
      <p class="text-sm text-slate-400 mt-1">{{ event.message }}</p>
    </li>
    {% endfor %}
  </ul>
</aside>
```

### Phase 3: Interactive Graph (D3.js Force Layout)

**New partial**: `partials/service_graph.html`

Replace Mermaid with D3.js force-directed graph:
- Nodes represent services
- Edges represent dependencies
- Node color = status
- Click to select and show detail panel
- Drag to rearrange
- Zoom/pan support

### Phase 4: Terminal Interface

**New partial**: `partials/terminal.html`

```html
<div id="terminal" class="fixed bottom-0 left-0 right-0 bg-dtx-panel border-t border-dtx-border transform translate-y-full transition-transform" data-open="false">
  <header class="flex items-center justify-between px-4 py-2 border-b border-dtx-border">
    <span class="flex items-center gap-2 text-sm">
      <span class="text-dtx-cyan">&gt;_</span>
      DTX TERMINAL
      <span class="px-2 py-0.5 rounded text-xs bg-emerald-500/20 text-emerald-400">ONLINE</span>
    </span>
    <button onclick="toggleTerminal()" class="text-slate-400 hover:text-white">&times;</button>
  </header>
  <div class="px-4 py-3">
    <div class="flex items-center gap-2">
      <span class="text-dtx-cyan">/</span>
      <input type="text"
             id="terminal-input"
             class="flex-1 bg-transparent border-none outline-none text-slate-200 placeholder-slate-500"
             placeholder="start, stop, status, logs..."
             hx-post="/api/terminal"
             hx-trigger="keyup[key=='Enter']"
             hx-target="#terminal-output" />
    </div>
  </div>
  <div id="terminal-output" class="px-4 pb-4 max-h-64 overflow-y-auto font-mono text-sm text-slate-400"></div>
</div>

<script>
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') toggleTerminal();
  if (e.key === '/' && !e.target.matches('input, textarea')) {
    e.preventDefault();
    toggleTerminal(true);
  }
});

function toggleTerminal(forceOpen) {
  const terminal = document.getElementById('terminal');
  const isOpen = terminal.dataset.open === 'true';
  const shouldOpen = forceOpen ?? !isOpen;

  terminal.dataset.open = shouldOpen;
  terminal.classList.toggle('translate-y-full', !shouldOpen);
  terminal.classList.toggle('translate-y-0', shouldOpen);

  if (shouldOpen) {
    document.getElementById('terminal-input').focus();
  }
}
</script>
```

---

## Implementation Checklist

### P0 - Critical (Week 1)

- [ ] Switch to dark theme (bg-slate-950)
- [ ] Update color palette in Tailwind config
- [ ] Implement multi-state status badges
- [ ] Add monospace font stack
- [ ] Update header with status metrics

### P1 - High Priority (Week 2)

- [ ] Create activity feed component
- [ ] Wire SSE for real-time events
- [ ] Replace Mermaid with D3.js graph
- [ ] Add service detail slide-over panel
- [ ] Implement terminal interface

### P2 - Enhancement (Week 3)

- [ ] Add keyboard shortcuts
- [ ] Implement hover glow effects
- [ ] Add loading skeletons
- [ ] Create toast notifications
- [ ] Polish animations and transitions

### P3 - Polish (Week 4)

- [ ] Accessibility audit (ARIA labels, focus management)
- [ ] Mobile responsive adjustments
- [ ] Performance optimization
- [ ] User preference persistence (theme, layout)

---

## Additional Patterns Discovered

### Signal Filter Component

**Purpose**: Quick filtering of visible services/nodes in the graph by text search and category toggles.

**Structure**:
```html
<!-- Signal Filter Panel -->
<div class="signal-filter">
  <div class="filter-label">
    <span class="label-badge">UI Refresh</span>  <!-- Current context/mode -->
  </div>

  <div class="filter-controls">
    <div class="search-row">
      <span class="search-icon">●</span>
      <input type="text" placeholder="Search in nodes..." />
    </div>

    <div class="toggle-buttons">
      <button class="toggle active">All Services</button>
      <button class="toggle">Dependencies</button>
    </div>
  </div>
</div>
```

**Behavior**:
- Search input filters graph nodes in real-time
- Toggle buttons switch between view modes (all vs dependencies only)
- Active toggle has cyan border glow
- Panel floats above graph on bottom-left

### Slide-Over Detail Panel

**Key Behavior**: The detail panel is **hidden by default** and slides in from the right when:
- User clicks an activity feed item
- User clicks a graph node
- Detail panel has close button (×) in header

**Implementation Pattern**:
```css
.detail-panel {
  position: fixed;
  right: 0;
  top: 0;
  bottom: 0;
  width: 360px;
  transform: translateX(100%);  /* Hidden by default */
  transition: transform 0.3s ease;
}

.detail-panel.open {
  transform: translateX(0);
}
```

### Project Dropdown with Rich Items

**Pattern**: Dropdown items contain more than just text—they have icon, title, and description.

```html
<div class="dropdown-item">
  <div class="item-icon">◆</div>  <!-- Unique per project -->
  <div class="item-content">
    <div class="item-title">Project Name</div>
    <div class="item-description">Brief context or path</div>
  </div>
</div>
```

### Graph Grid Pattern Overlay

**Purpose**: Subtle grid pattern gives the graph area a "radar screen" or technical schematic feel.

**Implementation**:
```css
.graph-container {
  background-image:
    linear-gradient(rgba(45, 51, 59, 0.3) 1px, transparent 1px),
    linear-gradient(90deg, rgba(45, 51, 59, 0.3) 1px, transparent 1px);
  background-size: 40px 40px;
}
```

### Header Meta Information Line

**Pattern**: Below main header, a secondary line shows sector/environment info.

```
SECTOR: LOCAL // STATUS: ONLINE // UPTIME: 24h
```

- Uses muted text color
- Monospace font
- `//` separator between fields
- Reinforces "command center" aesthetic

### Keyboard Shortcuts Summary

| Key | Action |
|-----|--------|
| `ESC` | Toggle terminal / close panels |
| `/` | Focus terminal input |
| `?` | Show keyboard shortcut help (optional) |

---

## Appendix: Gas Town Component Inventory

### Observed Components

| Component | Description | Interaction |
|-----------|-------------|-------------|
| Header Stats | "41 AGENTS / 7 ACTIVE / 9 WAITING" | Static display |
| Header Meta Line | "SECTOR: LOCAL // STATUS: ONLINE" | Static context info |
| Rig Dropdown | Project/repo selector with rich items (icon + title + desc) | Click to open, select to filter |
| CODE Button | Opens code panel with file tree | Click to toggle panel |
| CONNECT Button | Opens terminal/command panel | Click to toggle, ESC to close |
| Activity Feed | Scrollable event list | Click item to open detail slide-over |
| Signal Filter | Floating panel with search + toggle buttons | Type to filter nodes, click toggles to switch mode |
| Voice AI Card | Microphone with status | Click START to activate |
| Network Graph | Force-directed D3 visualization with grid overlay | Click node to select, drag to move, zoom/pan |
| Detail Panel | Slide-over (hidden by default), opens from right | Tab navigation, × to close, ESC shortcut |
| Code Panel | File tree + diff viewer | Click file, approve/reject buttons |
| Terminal | Collapsible command input with history | Type, Enter to execute, / to focus, ESC to close |

### Observed States

| State | Visual Treatment | dtx Mapping |
|-------|------------------|-------------|
| ACTIVE | Cyan text, glowing indicator | Service responding to requests |
| RUNNING | Green badge, pulse animation | Process executing normally |
| WAITING | Gray badge | Service idle, awaiting work |
| STARTING | Orange badge, pulse animation | Service initializing |
| PROCESSING | Orange badge, spin animation | Active task in progress |
| COMPLETED | Green checkmark icon | Task/operation finished |
| PENDING | Orange/amber badge | Queued for execution |
| APPROVED | Green badge | Configuration validated |
| STOPPED | Gray text, muted indicator | Service not running |
| ERROR | Magenta/red badge, alert icon | Failure state requiring attention |
| COORDINATOR | Orange ring, larger node | Central/orchestrator service |
| EPHEMERAL | Purple accent, dashed border | Temporary/dev-mode service |

---

*Document generated from interactive Chrome DevTools exploration of Gas Town UI POC.*
