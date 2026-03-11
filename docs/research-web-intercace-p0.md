  Design Philosophy

  Create a dark-mode operational dashboard inspired by sci-fi command centers, industrial control rooms, and network operation centers. The aesthetic blends
 cyberpunk visual language with functional data density, prioritizing at-a-glance status monitoring and real-time system awareness.

  ---
  Visual Language Principles

  Atmosphere

  - Mood: Technical, precise, professional yet futuristic
  - Metaphor: You are the operator of a complex system - the UI is your command interface
  - Information Density: High density, low clutter - every element earns its space
  - Hierarchy: Status > Actions > Details

  Color System

  | Role           | Color                  | Usage                                      |
  |----------------|------------------------|--------------------------------------------|
  | Canvas         | Deep navy (#0a1628)    | Background, creates depth                  |
  | Surface        | Slate blue (#1e293b)   | Cards, panels, elevated elements           |
  | Primary        | Cyan (#00d4ff)         | Active states, primary actions, highlights |
  | Focal Point    | Amber/Orange (#f5a623) | Central entity, warnings, attention        |
  | Success        | Green (#4ade80)        | Healthy, running, complete                 |
  | Info           | Cyan (#22d3ee)         | Waiting, ready, idle                       |
  | Warning        | Orange (#f59e0b)       | Processing, busy, caution                  |
  | Error          | Magenta (#ec4899)      | Failed, disconnected, alert                |
  | Text Primary   | White (#ffffff)        | Headlines, active labels                   |
  | Text Secondary | Slate (#94a3b8)        | Descriptions, metadata                     |
  | Borders        | Cyan with glow         | Interactive boundaries, emphasis           |

  Visual Effects

  - Glow: Subtle outer glow on active/selected elements (2-4px blur, accent color at 30% opacity)
  - Grid: Faint background grid or crosshair markers suggest precision
  - Lines: Thin (1px) connection lines between related entities
  - Depth: Cards float above canvas with subtle shadow or border highlight

  ---
  Layout Architecture

  ┌─────────────────────────────────────────────────────────────────┐
  │ HEADER: Identity | Metrics Bar | Global Actions                 │
  ├──────────────┬──────────────────────────────────────────────────┤
  │              │                                                  │
  │  SIDEBAR     │              MAIN CANVAS                         │
  │  - Feed      │         (Visualization Area)                     │
  │  - Filters   │                                                  │
  │  - Widgets   │     [Interactive Graph / Map / Grid]             │
  │              │                                                  │
  │              │         [Cluster Labels]                         │
  │              │         [Entity Nodes]                           │
  │              │         [Relationship Lines]                     │
  │              │                                                  │
  └──────────────┴──────────────────────────────────────────────────┘

  Header Bar

  - Left Zone: System identity (logo, name, version), global status indicators
  - Center Zone: Key metrics as large numbers with small labels below (e.g., "42 NODES | 12 ACTIVE | 8 PENDING")
  - Right Zone: Primary actions with icon+label buttons, outlined style with accent border

  Sidebar (Left, ~280-320px)

  - Activity Feed: Chronological event stream, collapsible, shows recent system activity
  - Filters/Search: Quick search input + toggle/segmented controls for view modes
  - Contextual Widgets: Floating cards for auxiliary features (integrations, quick actions)

  Main Canvas

  - Primary visualization fills remaining space
  - Supports: network graphs, node maps, dependency trees, or grid layouts
  - Interactive: pan, zoom, select, hover for details
  - Entity nodes with status-driven styling
  - Cluster labels as bordered text boxes grouping related entities
  - Connection lines showing relationships

  ---
  Component Patterns

  Metric Card

  ┌─────────────┐
  │     42      │  ← Large number (24-32px, bold)
  │   ACTIVE    │  ← Small label (10-12px, uppercase, muted)
  └─────────────┘

  Activity Feed Item

  ┌─────────────────────────────────────┐
  │ [icon] Entity Name        5m ago   │
  │        Action description          │
  └─────────────────────────────────────┘
  - Icon indicates event type
  - Timestamp right-aligned, muted
  - Clickable for details

  Filter Toggle Group

  ┌──────────────┬──────────────┐
  │ ▣ OPTION A   │   OPTION B   │  ← Active has filled background
  └──────────────┴──────────────┘

  Entity Node (Graph)

  - Central/Primary: 50-60px circle, amber fill, outer glow ring
  - Standard: 20-30px circle, status-colored fill
  - Label: Appears beside or below node, small text
  - Hover: Highlight connections, show tooltip

  Cluster Label

  ┌─────────────────────┐
  │  WORKSTREAM NAME    │  ← Bordered box, accent color border
  └─────────────────────┘

  Action Button

  ┌─────────────────┐
  │  </>  CODE      │  ← Icon + uppercase label
  └─────────────────┘     Outlined, accent border, hover fills

  ---
  Typography System

  | Element         | Style                                     |
  |-----------------|-------------------------------------------|
  | System Title    | Monospace, uppercase, 14-16px             |
  | Metrics Number  | Sans-serif, bold, 24-32px                 |
  | Metrics Label   | Uppercase, 10-12px, letter-spacing +0.5px |
  | Section Headers | Uppercase, 12-14px, medium weight         |
  | Body Text       | Sans-serif, 13-14px, regular              |
  | Node Labels     | Sans-serif, 11-12px                       |
  | Timestamps      | Monospace or tabular nums, 11px, muted    |

  Recommended Fonts:
  - Monospace: JetBrains Mono, Space Mono, IBM Plex Mono
  - Sans-serif: Inter, system-ui, -apple-system

  ---
  Animation Principles

  | Element              | Animation                                 |
  |----------------------|-------------------------------------------|
  | Active nodes         | Subtle pulse (scale 1.0→1.05, 2s loop)    |
  | Selection            | Glow intensity increase (0.3→0.6 opacity) |
  | New feed items       | Slide in from top (200ms ease-out)        |
  | Graph updates        | Smooth position transitions (300ms)       |
  | Hover states         | Fast response (100-150ms)                 |
  | Connection highlight | Opacity/color shift on related node hover |

  ---
  Interaction Model

  1. Observe: Dashboard provides passive real-time awareness
  2. Filter: Narrow view to relevant subset
  3. Inspect: Click entity for detail panel/modal
  4. Act: Execute commands via action buttons
  5. Monitor: Activity feed confirms system responses

  ---
  Implementation Notes

  Graph Visualization Libraries:
  - D3.js (force-directed)
  - vis.js / vis-network
  - react-force-graph
  - Cytoscape.js

  Real-time Updates:
  - Server-Sent Events (SSE) for activity feed
  - WebSocket for graph state changes

  Responsive Behavior:
  - Sidebar collapses to icons on narrow screens
  - Graph remains primary focus
  - Metrics stack vertically on mobile

  ---
  Adaptable Elements

  This pattern applies to any domain requiring:
  - Entity monitoring: Agents, servers, services, devices, users
  - Relationship visualization: Dependencies, connections, hierarchies
  - Real-time status: Health, activity, throughput
  - Operational control: Start/stop, configure, intervene

  Simply replace:
  - Entity names and types
  - Status definitions and colors
  - Metric categories
  - Action verbs
  - Domain terminology

  ---
  This creates a professional operational interface that feels powerful without overwhelming, dense without cluttering, and futuristic while remaining funct
ional.

