# CLI Output Design System

## 1. Philosophy: Transferential Output

The output's structure mirrors the work's structure. If nix loads 42 vars, you see "42 vars." If a service waits on postgres, you see "waiting: postgres." No spinners obscuring what's happening. No generic "Loading..." hiding five different operations. Every line corresponds to a real action, and its result shows the real outcome.

**Corollary: every line earns its place.** If removing a line loses no information, it shouldn't exist. "Starting services..." followed by "Services started." is two lines where one suffices: `● services ··· 2 started`.

## 2. Visual Language

**Indicators** — state of a single unit of work:

| Glyph | State | ANSI | Meaning |
|-------|-------|------|---------|
| `●` | Done | green `\x1b[32m` | Work completed successfully |
| `◐` | In progress | yellow `\x1b[33m` | Work underway, not yet resolved |
| `◌` | Pending | dim `\x1b[2m` | Queued, waiting for dependency |
| `✕` | Failed | red `\x1b[31m` | Work failed |

**Leader dots** — `·` (U+00B7) fills the gap between label and result. The eye follows the dots like a table of contents. Dot count is dynamic based on terminal width. Below 40 columns, dots are replaced with a single space.

**Tree indentation** — 2-space indent per level. Parent-child relationship visible through nesting. Groups show a header with child count, children indented below:

```
 ● services ································ 2 started
   ● postgres ······························ :5432 pid 4821
   ● api ··································· :3000 pid 4825
```

**Separator** — `───` line marks phase transitions (bootstrap → streaming). Contains contextual text:

```
 ─── logs (ctrl+c to stop) ──────────────────────────────
```

**Timing** — dimmed parenthetical `(1.2s)` appended to result. Only shown when measured.

## 3. Terminal Capabilities (Three Independent Axes)

Capabilities are NOT a hierarchy. They are three independent booleans:

| Capability | Detection | Controls |
|------------|-----------|----------|
| **color** | `is_tty && !NO_COLOR && TERM != "dumb"` | ANSI color codes (green, red, yellow, cyan, dim) |
| **cursor** | `is_tty` | In-place line updates (`\r`, `\x1b[K`), ephemeral lines |
| **width** | `crossterm::terminal::size()` or 80 | Leader dot count, table column sizing |

`NO_COLOR` disables colors but NOT cursor control. A user piping to `less -R` might want cursor control without colors. `TERM=dumb` disables both.

## 4. Non-TTY Mode (First-Class, Not Degraded)

When output is piped (`dtx list | grep api`) or captured, the format changes completely:

```
dtx: nix: 42 vars (1.2s)
dtx: pre-flight: 3/3
dtx: services: 2 started
dtx:   postgres: :5432 pid 4821
dtx:   api: :3000 pid 4825
```

Rules:
- Prefixed with `dtx:` for greppability
- 2-space indent preserved for tree structure
- No indicators, no leader dots, no color
- Colon separates label from result
- Tables become tab-separated (TSV)
- Log lines: `dtx: [service] line`

## 5. Color as Information

Colors encode state, not decoration. Remove all color and the output still makes sense through text:
- Indicator glyphs carry meaning without color
- Error/warning labels (`error:`, `warn:`, `hint:`) are readable without color
- Service names in log prefix `[api]` are identifiable without cyan

## 6. Command Output Categories

| Category | Commands | Pattern | Example |
|----------|----------|---------|---------|
| **Single result** | list, search, status, config | Table or raw output | `dtx list -s` |
| **Multi-step** | init, add, edit, remove, import, export, nix | Sequential steps with indicators | `dtx init --detect` |
| **Streaming** | start -f, logs -f | Bootstrap steps, separator, continuous log stream | `dtx start -f` |
| **Control** | stop | Single step with result | `dtx stop` |
| **Protocol** | mcp | JSON-RPC (no human output) | `dtx mcp` |
| **Pass-through** | completions, export to stdout | Raw output, no formatting | `dtx completions bash` |

## 7. Error Formatting

Errors expand with structured detail:

```
 ✕ pre-flight ······························ 1/3 failed

   error: Command 'redis-server' not found in PATH
     required by: cache
     hint: dtx add cache --package redis
```

Rules:
- `error:` label on its own line, red if color enabled
- Detail lines indented 4 spaces, key-value pairs
- `hint:` label, yellow if color enabled, suggests fix
- Warnings use `warn:` label, same structure but no step failure

## 8. Early Termination

If a bootstrap step fails, subsequent steps don't execute. The output shows exactly what happened:

```
 ● nix ····································· 42 vars (1.2s)
 ✕ pre-flight ······························ 1/3 failed

   error: Command 'redis-server' not found in PATH
     required by: cache
     hint: dtx add cache --package redis
```

No "services" group appears because we never reached that phase. The last line tells the story.

## 9. Implementation

The output system lives in `crates/dtx/src/output/`:
- `mod.rs` — `Output`, `Step`, `Group` types
- `caps.rs` — `Capabilities` detection
- `line.rs` — Line rendering with indicators and leader dots
- `table.rs` — Column-aligned table output
- `stream.rs` — Log-phase rendering

Key design decisions:
- `Output` is `Clone + Send + Sync` (Arc-based)
- `Step::done(self)` and `Step::fail(self)` consume the step — can only resolve once
- Groups buffer children, flush on `.done()` with count in header
- Three independent capability axes (color, cursor, width)
- Non-TTY format is first-class, not degraded
