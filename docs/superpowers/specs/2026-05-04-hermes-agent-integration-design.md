# Design: Hermes Agent Integration

**Date:** 2026-05-04  
**Branch:** main-fork (fork: tomraider4720/omni)  
**Status:** Approved

---

## Overview

Add Hermes Agent (https://github.com/NousResearch/hermes-agent) as a first-class `AgentIntegration` in omni, enabling `omni install hermes`. Hermes is the successor to OpenClaw and supports both MCP servers and a Python plugin system with hooks.

The integration has two parts:
1. **Rust `AgentIntegration`** — installs/uninstalls via `omni install hermes`
2. **Python plugin** — provides `omni_exec` tool + `post_tool_call` hook

---

## Architecture

### Part 1: Rust AgentIntegration (`src/agents/hermes.rs`)

Implements the `AgentIntegration` trait.

**`install(exe_path: &str)`:**
1. Parse `~/.hermes/config.yaml` (create if missing)
2. Add `omni` entry under `mcp_servers:` (idempotent)
3. Download Python plugin files to `~/.hermes/plugins/omni-signal-engine/`
   - `__init__.py`
   - `plugin.yaml`

**`uninstall()`:**
1. Remove `mcp_servers.omni` from `~/.hermes/config.yaml`
2. Remove `~/.hermes/plugins/omni-signal-engine/` directory

**`doctor_check(fix_mode, warnings)`:**
1. Verify `mcp_servers.omni` exists in `~/.hermes/config.yaml`
2. Verify `~/.hermes/plugins/omni-signal-engine/__init__.py` exists

**Config patch format (`~/.hermes/config.yaml`):**
```yaml
mcp_servers:
  omni:
    command: "/path/to/omni"
    args: ["--mcp"]
```

**New Cargo dependency:** `serde_yaml = "0.9"`

---

### Part 2: Python Plugin (`integrations/hermes/`)

Three files installed to `~/.hermes/plugins/omni-signal-engine/`.

#### `plugin.yaml`
```yaml
id: omni-signal-engine
name: Omni Signal Engine
version: "1.0.0"
description: "Token-efficient terminal execution via omni. Filters noise from command output before it reaches the LLM."
required_binaries:
  - omni
```

#### `__init__.py`

**`register(ctx)` registers:**

1. **`omni_exec` tool** — Wraps terminal calls through `omni exec -- <command>`:
   - Runs `omni exec -- <command>` via subprocess
   - Returns filtered stdout (noise removed, signal preserved)
   - Compatible with omni's Rewind-Store for `omni_retrieve`

2. **`post_tool_call` hook** — Passively observes `terminal` tool calls:
   - When `tool_name == "terminal"`, pipes the command through `omni exec` for rewind logging
   - Does NOT modify the live result (Hermes hooks are fire-and-forget)
   - Enables `omni_history` and `omni_budget` to track terminal activity

3. **`/omni` slash command** — Status overview:
   - Shows omni version, filter stats, budget usage

---

## Data Flow

```
Hermes Agent
    │
    ├── [MCP] omni_retrieve, omni_learn, omni_history, omni_budget, omni_agents, omni_knowledge
    │         └── omni --mcp (stdio MCP server)
    │
    ├── [Tool] omni_exec <command>
    │         └── omni exec -- <command>  →  filtered output → LLM
    │
    └── [Hook] post_tool_call on "terminal"
              └── omni exec -- <command>  →  stored in Rewind-Store (side effect only)
```

---

## Limitations

- **No automatic terminal interception:** Unlike Claude Code (which has `PreToolUse`/`PostToolUse` hooks), Hermes has no mechanism to intercept the built-in `terminal` tool's output before the LLM sees it. The `omni_exec` tool must be used explicitly.
- **Workaround:** Users can add to their `SOUL.md`: `"Prefer omni_exec over terminal for shell commands to reduce token usage."`
- **Hook result mutation:** Hermes `post_tool_call` hooks are fire-and-forget. They cannot modify the tool result. Rewind logging via hooks is therefore best-effort / async.

---

## Files Changed

| File | Change |
|------|--------|
| `Cargo.toml` | Add `serde_yaml = "0.9"` |
| `src/agents/hermes.rs` | New: AgentIntegration impl |
| `src/agents/mod.rs` | Register `HermesAgent` in `all_integrations()` |
| `integrations/hermes/__init__.py` | New: Python plugin |
| `integrations/hermes/plugin.yaml` | New: Plugin metadata |

---

## Testing

- `omni install hermes` on a machine with Hermes installed → verify config.yaml patched, plugin dir created
- `omni uninstall hermes` → verify both removed cleanly
- `omni doctor --hermes` → verify health check passes
- In Hermes session: verify omni MCP tools appear, `omni_exec` tool available, `/omni` command works
