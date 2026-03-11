  PROMPT: Multi-Agent Orchestration Dashboard (Gas Town Style)

  Design Aesthetic

  Create a cyberpunk/industrial control room dashboard for multi-agent AI orchestration. The visual langu
age draws from sci-fi command centers, gas/oil industry control panels, and network operation centers.

  Color Palette

  - Background: Deep navy/dark blue (#0a1628 or similar)
  - Primary Accent: Cyan/teal (#00d4ff) for active states and highlights
  - Secondary Accent: Orange/amber (#f5a623) for the central "Mayor" node and warnings
  - Status Colors:
    - Green (#4ade80) - Active/Running agents
    - Cyan (#22d3ee) - Waiting/Ready agents
    - Magenta/Pink (#ec4899) - Error/Alert states
    - Orange (#f59e0b) - Processing/Busy states
  - Text: Light gray (#94a3b8) for secondary, white for primary
  - Borders/Lines: Subtle cyan glow effects, thin connection lines

  Layout Structure

  Header Bar (Top)

  - Left: Logo area with "MULTI-AGENT ORCHESTRATION" label, product name "GAS TOWN V.01", status indicato
rs (SECTOR: GLOBAL // STATUS: ONLINE)
  - Center: Metrics dashboard showing counts (33 AGENTS | 9 ACTIVE | 9 WAITING) with dropdown filter "All
 Rigs"
  - Right: Action buttons with icons - "CODE" (code brackets icon) and "CONNECT" (terminal icon) with cya
n border styling

  Left Sidebar

  1. Activity Feed Panel (collapsible)
    - Header: "ACTIVITY FEED" with count badge (20)
    - Scrollable list of agent activity items
    - Each item shows: Agent icon, name, timestamp ("5m ago"), action description
    - Activity types: status changes, handoffs, work started, hook updates, convoy joins
  2. Signal Filter Panel
    - Search input with placeholder "Search by name..."
    - Toggle buttons: "ALL AGENTS" (active) | "MAYOR ONLY"
  3. Voice AI Widget (floating card)
    - xAI logo integration
    - "VOICE AI" header with status indicator
    - "Ready - tap to start" status text
    - Large "START" button with microphone icon
    - Footer: "POWERED BY GROK VOICE AI"

  Main Canvas (Center)

  Interactive Network Graph Visualization
  - Force-directed graph showing agent relationships
  - Central Hub Node: "The Mayor" - larger orange/amber circle with glow effect, serves as orchestrator
  - Agent Nodes: Smaller circles with status-based coloring (cyan, green, magenta, orange)
  - Connection Lines: Thin lines showing relationships/handoffs between agents
  - Cluster Labels: Boxed labels for workstreams:
    - "Test Coverage" (top)
    - "System Overhaul" (left)
    - "Performance Optimization" (right)
    - "API v2 Migration" (bottom-right)
  - Node labels appear on hover/always visible with agent names
  - Subtle grid pattern or crosshair markers in background

  Agent Node Types (Visual Hierarchy)

  1. Mayor/Orchestrator: Large (50-60px), orange with outer glow ring
  2. Active Workers: Medium (20-30px), bright cyan or green
  3. Waiting/Idle: Medium, muted cyan
  4. Processing: Medium, orange with pulse animation
  5. Error State: Medium, magenta/pink

  Typography

  - Headers: Uppercase, monospace/tech font (like "Space Mono", "JetBrains Mono", or "Orbitron")
  - Body: Clean sans-serif (Inter, system-ui)
  - Metrics: Large bold numbers with small uppercase labels below

  Interactive Elements

  - Nodes are clickable (show agent details)
  - Activity feed items are clickable (expand/navigate)
  - Graph is pannable and zoomable
  - Hover states show connection highlights
  - Real-time updates via SSE/WebSocket

  Key UI Components to Build

  1. <HeaderMetrics> - Stats bar with counts
  2. <ActivityFeed> - Scrollable event list
  3. <SignalFilter> - Search + toggle filters
  4. <VoiceAIWidget> - Voice interface card
  5. <NetworkGraph> - Force-directed visualization (use D3.js, vis.js, or react-force-graph)
  6. <AgentNode> - Individual node component with status styling
  7. <ClusterLabel> - Workstream/group labels

  Animation & Effects

  - Subtle pulse on active nodes
  - Glow effects on selected/hover states
  - Smooth transitions on graph updates
  - Activity feed items slide in from top
  - Connection lines animate on handoff events

  ---
  This dashboard visualizes a multi-agent AI system where "The Mayor" orchestrates worker agents across d
ifferent workstreams, with real-time activity monitoring and voice AI integration.

