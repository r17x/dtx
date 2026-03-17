# Web UI Guide

> Using the dtx web interface for service management.

---

## Overview

dtx provides a web-based UI for managing services with real-time updates, log viewing, and visual dependency graphs.

---

## Starting the Web UI

```bash
# Start on default port 3000
dtx web

# Custom port
dtx web --port 8080

# Open browser automatically
dtx web --open
```

Navigate to `http://localhost:3000` (or your custom port).

---

## Dashboard

The main dashboard shows:

```
┌─────────────────────────────────────────────────────────────┐
│  dtx - myapp                                    [Status: ●] │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Services                          │  Activity              │
│  ┌────────────────────────────┐    │  ┌──────────────────┐  │
│  │ ● postgres    5432  healthy│    │  │ 10:30 api started│  │
│  │ ● redis       6379  healthy│    │  │ 10:29 postgres   │  │
│  │ ● api         3000  healthy│    │  │       healthy    │  │
│  │ ○ worker      -     stopped│    │  │ 10:28 redis      │  │
│  └────────────────────────────┘    │  │       started    │  │
│                                    │  └──────────────────┘  │
│  [Start All] [Stop All]            │                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Service Status Indicators

| Icon | Meaning |
|------|---------|
| ● (green) | Running and healthy |
| ● (yellow) | Running, health check pending |
| ● (red) | Running but unhealthy |
| ○ (gray) | Stopped |
| ◐ (blue) | Starting |

---

## Service Management

### Starting Services

1. Click a service row to select it
2. Click **Start** button, or
3. Use **Start All** to start all services

Services start in dependency order automatically.

### Stopping Services

1. Select service(s)
2. Click **Stop** button, or
3. Use **Stop All** to stop all services

Services stop in reverse dependency order.

### Restarting Services

1. Select a service
2. Click **Restart** button

The service will be stopped and started again.

---

## Logs Panel

View real-time logs for any service:

```
┌─────────────────────────────────────────────────────────────┐
│  Logs: api                              [Clear] [Download]  │
├─────────────────────────────────────────────────────────────┤
│  10:30:15 [INFO] Server listening on port 3000              │
│  10:30:16 [INFO] Config loaded                               │
│  10:30:17 [DEBUG] Health check passed                       │
│  10:30:20 [INFO] GET /api/users 200 15ms                    │
│  10:30:21 [INFO] POST /api/login 200 42ms                   │
│  █                                                          │
└─────────────────────────────────────────────────────────────┘
```

### Features

- **Real-time streaming**: Logs appear as they're generated
- **Search**: Filter logs with text search
- **Level filtering**: Show only errors, warnings, etc.
- **Download**: Export logs as text file
- **Clear**: Clear the log display

### Viewing Logs

1. Click a service in the list
2. The logs panel shows that service's output
3. Logs stream in real-time via SSE

---

## Dependency Graph

Visual representation of service dependencies:

```
        ┌──────────┐
        │ frontend │
        └────┬─────┘
             │
        ┌────▼─────┐
        │   api    │
        └────┬─────┘
             │
     ┌───────┴───────┐
     │               │
┌────▼─────┐   ┌─────▼────┐
│ postgres │   │  redis   │
└──────────┘   └──────────┘
```

### Interactions

- **Hover**: Show service details
- **Click**: Select service
- **Drag**: Rearrange layout
- **Zoom**: Scroll to zoom in/out

---

## Service Details

Click a service to view details:

```
┌─────────────────────────────────────────────────────────────┐
│  postgres                                                   │
├─────────────────────────────────────────────────────────────┤
│  Kind:        process                                       │
│  Status:      running (pid: 12345)                          │
│  Port:        5432                                          │
│  Health:      healthy (last check: 2s ago)                  │
│  Uptime:      1h 23m 45s                                    │
│  Restarts:    0                                             │
│                                                             │
│  Command:     postgres -D /data/postgres                    │
│  Working Dir: /home/user/myapp                              │
│                                                             │
│  Environment:                                               │
│    PGDATA=/data/postgres                                    │
│    POSTGRES_USER=app                                        │
│                                                             │
│  Depends On:  (none)                                        │
│  Depended By: api, worker                                   │
│                                                             │
│  [Start] [Stop] [Restart] [View Logs] [Edit]                │
└─────────────────────────────────────────────────────────────┘
```

---

## Real-time Updates

The web UI uses Server-Sent Events (SSE) for real-time updates:

- Service state changes appear immediately
- Health check results update live
- Logs stream as they're generated
- No page refresh needed

### Connection Status

The status indicator in the header shows connection state:
- **●** Connected
- **○** Disconnected (will auto-reconnect)

---

## Configuration

### Web Server Settings

In `.dtx/config.yaml`:

```yaml
web:
  port: 3000
  host: 127.0.0.1
```

Or via command line:
```bash
dtx web --port 8080
```

### Theme

The UI uses a cyberpunk-inspired dark theme optimized for developer environments.

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate services up/down |
| `Enter` | Select service |
| `s` | Start selected service |
| `x` | Stop selected service |
| `r` | Restart selected service |
| `l` | Focus logs panel |
| `/` | Search logs |
| `Esc` | Clear selection |
| `?` | Show help |

---

## API Endpoints

The web server exposes these endpoints:

| Endpoint | Description |
|----------|-------------|
| `GET /` | Web UI |
| `GET /api/services` | List services |
| `GET /api/services/:id` | Get service details |
| `POST /api/services/:id/start` | Start service |
| `POST /api/services/:id/stop` | Stop service |
| `GET /api/services/:id/logs` | Get logs |
| `GET /sse/status` | SSE status stream |
| `GET /sse/events` | SSE event stream |
| `GET /sse/logs/:id` | SSE log stream |

---

## Troubleshooting

### Port Already in Use

```bash
# Use a different port
dtx web --port 8080

# Or find what's using the port
lsof -i :3000
```

### UI Not Updating

1. Check browser console for errors
2. Verify SSE connection (Network tab)
3. Refresh the page
4. Restart `dtx web`

### Cannot Connect

1. Check firewall settings
2. Verify dtx web is running
3. Try `127.0.0.1` instead of `localhost`

---

## See Also

- [Quick Start](./quick-start.md) - Get running quickly
- [CLI Reference](./cli-reference.md) - Command line usage
- [Configuration](./configuration.md) - Configuration options
