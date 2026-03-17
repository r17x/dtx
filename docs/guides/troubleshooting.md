# Troubleshooting Guide

> Common issues and solutions for dtx.

---

## Quick Diagnostics

```bash
# Check dtx version
dtx --version

# Verbose output
dtx --verbose <command>

# Check service status
dtx status

# View logs for a service
dtx logs <service> -f
```

---

## Startup Issues

### "No services found"

**Problem:** Project has no services configured.

**Solutions:**
1. Add a service:
   ```bash
   dtx add postgres
   ```
2. Import from existing config:
   ```bash
   dtx import process-compose.yaml
   ```
3. Check `.dtx/config.yaml` exists and has resources

### Service won't start

**Problem:** Service fails to start or exits immediately.

**Diagnostics:**
```bash
# Check status
dtx status <service>

# View logs
dtx logs <service>

# Run in foreground for detailed output
dtx start <service> -f
```

**Common causes:**
- Command not found: Check the command is in PATH or use Nix package
- Port already in use: See [Port Conflicts](#port-conflicts)
- Missing dependencies: Check `depends_on` services are healthy
- Permission denied: Check file permissions

---

## Health Check Issues

### Health check always fails

**Problem:** Service starts but health check never passes.

**Diagnostics:**
```bash
# Check service logs
dtx logs <service>

# Try health check manually
# For exec:
pg_isready -h 127.0.0.1 -p 5432

# For HTTP:
curl http://localhost:3000/health

# For TCP:
nc -z 127.0.0.1 5432
```

**Solutions:**

1. **Increase initial delay** - Service needs more time to start:
   ```yaml
   health:
     exec: pg_isready
     initial_delay: 10s  # Increase this
   ```

2. **Fix health check command** - Command returns non-zero:
   ```yaml
   health:
     exec: curl -sf http://localhost:3000/health
     # -s = silent, -f = fail on HTTP errors
   ```

3. **Check endpoint exists** - For HTTP checks:
   ```yaml
   health:
     http:
       path: /health    # Must exist
       port: 3000
   ```

### Service marked unhealthy intermittently

**Problem:** Health check passes sometimes, fails other times.

**Solutions:**

1. **Increase timeout**:
   ```yaml
   health:
     timeout: 10s  # Increase from default
   ```

2. **Increase failure threshold**:
   ```yaml
   health:
     retries: 5  # More retries before marking unhealthy
   ```

3. **Check service load** - Service may be overloaded

---

## Dependency Issues

### Circular dependency detected

**Problem:** Services depend on each other in a cycle.

**Error:**
```
Error: Circular dependency detected: api -> worker -> api
```

**Solution:** Restructure dependencies. Common patterns:

```yaml
# Instead of circular:
# api -> worker -> api

# Use shared dependency:
resources:
  shared-state:
    kind: process
    command: redis-server

  api:
    depends_on:
      - shared-state: healthy

  worker:
    depends_on:
      - shared-state: healthy
```

### Dependency never becomes healthy

**Problem:** Service waits forever for dependency.

**Diagnostics:**
```bash
# Check dependency status
dtx status <dependency>

# View dependency logs
dtx logs <dependency>
```

**Solutions:**
1. Fix the dependency's health check
2. Use `started` instead of `healthy`:
   ```yaml
   depends_on:
     - postgres: started  # Don't wait for health check
   ```

---

## Port Conflicts

### Port already in use

**Problem:** Service can't bind to its port.

**Error:**
```
Error: Address already in use (os error 48)
```

**Diagnostics:**
```bash
# Find what's using the port
lsof -i :3000

# Or on Linux
ss -tlnp | grep 3000
```

**Solutions:**

1. **Stop the other process:**
   ```bash
   kill <pid>
   ```

2. **Use a different port:**
   ```yaml
   resources:
     api:
       port: 3001  # Change port
       environment:
         PORT: "3001"
   ```

3. **Stop all dtx services first:**
   ```bash
   dtx stop
   ```

---

## Nix Issues

### Package not found

**Problem:** Nix can't find the specified package.

**Error:**
```
error: attribute 'nonexistent-package' missing
```

**Solutions:**

1. **Search for correct name:**
   ```bash
   dtx search postgres
   ```

2. **Check nixpkgs:**
   ```bash
   nix search nixpkgs postgres
   ```

3. **Use full attribute path:**
   ```yaml
   nix:
     packages:
       - nixpkgs#postgresql_16
   ```

### Nix shell slow

**Problem:** `dtx nix shell` or service startup is slow.

**Solutions:**

1. **Enable Nix cache:**
   ```bash
   # Add to /etc/nix/nix.conf or ~/.config/nix/nix.conf
   substituters = https://cache.nixos.org
   trusted-public-keys = cache.nixos.org-1:...
   ```

2. **Use flake lock:**
   ```bash
   dtx nix init  # Generates flake.lock
   ```

3. **Pre-build environment:**
   ```bash
   nix develop --build
   ```

### direnv not working

**Problem:** Environment not loaded automatically.

**Solutions:**

1. **Allow direnv:**
   ```bash
   direnv allow
   ```

2. **Regenerate .envrc:**
   ```bash
   dtx nix envrc
   ```

3. **Check direnv is hooked in shell:**
   ```bash
   # Add to .bashrc or .zshrc
   eval "$(direnv hook bash)"  # or zsh/fish
   ```

---

## Performance Issues

### Services start slowly

**Solutions:**

1. **Parallel startup** - Reduce unnecessary dependencies
2. **Faster health checks** - Use TCP instead of HTTP where possible

### High memory usage

**Solutions:**

1. **Check for memory leaks** in your services
2. **Limit service resources** (future feature)
3. **Reduce log retention**

---

## Web UI Issues

### Cannot connect to web UI

**Diagnostics:**
```bash
# Check if server is running
curl http://localhost:3000

# Check port
lsof -i :3000
```

**Solutions:**
1. Ensure `dtx web` is running
2. Check firewall settings
3. Try different port: `dtx web --port 8080`

### UI not updating

**Problem:** Service status doesn't update in real-time.

**Solutions:**
1. Check browser console for errors
2. Verify SSE connection (Network tab → EventStream)
3. Hard refresh (Cmd+Shift+R / Ctrl+Shift+R)
4. Restart `dtx web`

---

## Getting Help

### Collect Debug Information

```bash
# Version info
dtx --version

# Verbose logs
dtx --verbose start -f 2>&1 | tee dtx-debug.log

# System info
uname -a
nix --version
```

### Report Issues

1. Search existing issues: https://github.com/r17x/dtx/issues
2. Create new issue with:
   - dtx version
   - OS/platform
   - Steps to reproduce
   - Error messages
   - Relevant config (redact secrets!)

---

## See Also

- [Quick Start](./quick-start.md) - Getting started
- [Configuration](./configuration.md) - Configuration reference
- [CLI Reference](./cli-reference.md) - Command reference
