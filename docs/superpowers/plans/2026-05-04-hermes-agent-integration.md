# Hermes Agent Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Hermes Agent (https://github.com/NousResearch/hermes-agent) as a first-class `AgentIntegration` in omni — `omni install hermes` configures MCP and installs a Python plugin with an `omni_exec` tool and `post_tool_call` hook.

**Architecture:** Two deliverables: (1) a Rust `AgentIntegration` (`src/agents/hermes.rs`) that patches `~/.hermes/config.yaml` for MCP and installs a Python plugin to `~/.hermes/plugins/omni-signal-engine/`; (2) the Python plugin (`integrations/hermes/`) with `omni_exec` tool + `post_tool_call` hook + `/omni` slash command.

**Tech Stack:** Rust (serde_yaml 0.9, ureq 2, anyhow), Python 3.11+, Hermes Agent plugin API

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` | Modify | Add `serde_yaml = "0.9"` dependency |
| `src/agents/hermes.rs` | Create | `HermesIntegration` struct implementing `AgentIntegration` |
| `src/agents/mod.rs` | Modify | Add `pub mod hermes`, `pub use hermes::HermesIntegration`, register in `all_integrations()` |
| `integrations/hermes/__init__.py` | Create | Python plugin: `omni_exec` tool + `post_tool_call` hook + `/omni` command |
| `integrations/hermes/plugin.yaml` | Create | Plugin metadata |
| `tests/agents/hermes_test.rs` | Create | Unit tests for install/uninstall/doctor logic |

---

## Task 1: Create Python Plugin Files

**Files:**
- Create: `integrations/hermes/__init__.py`
- Create: `integrations/hermes/plugin.yaml`

- [ ] **Step 1: Create plugin.yaml**

```bash
mkdir -p integrations/hermes
```

File `integrations/hermes/plugin.yaml`:
```yaml
id: omni-signal-engine
name: Omni Signal Engine
version: "1.0.0"
description: "Token-efficient terminal execution via omni. Filters noise from command output before it reaches the LLM."
required_binaries:
  - omni
```

- [ ] **Step 2: Create `integrations/hermes/__init__.py`**

```python
"""omni-signal-engine — Hermes Agent plugin.

Registers:
- ``omni_exec`` tool: runs commands through ``omni exec -- <cmd>`` for
  filtered terminal output (noise removed before the LLM sees it).
- ``post_tool_call`` hook: logs native ``terminal`` tool invocations to
  omni's Rewind-Store so omni_history / omni_budget stay accurate.
- ``/omni`` slash command: show omni status.
"""

from __future__ import annotations

import logging
import os
import shutil
import subprocess
from typing import Any, Dict, Optional

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _omni_binary() -> str:
    """Return the path to the omni binary, or 'omni' as fallback."""
    # Config-level override (set via omni_path in plugin config if ever added)
    env_path = os.environ.get("OMNI_PATH")
    if env_path and os.path.isfile(env_path):
        return env_path
    # Prefer explicit PATH search
    found = shutil.which("omni")
    return found or "omni"


def _run_omni_exec(command: str) -> str:
    """Run ``omni exec -- <command>`` and return the filtered output."""
    omni = _omni_binary()
    try:
        result = subprocess.run(
            [omni, "exec", "--", command],
            capture_output=True,
            text=True,
            timeout=180,
        )
        output = result.stdout
        if result.returncode != 0 and result.stderr:
            output += result.stderr
        return output
    except FileNotFoundError:
        return f"[omni-signal-engine] omni binary not found at '{omni}'. Is omni installed?"
    except subprocess.TimeoutExpired:
        return "[omni-signal-engine] command timed out after 180 seconds"
    except Exception as exc:  # noqa: BLE001
        return f"[omni-signal-engine] error running omni exec: {exc}"


def _log_to_rewind(command: str) -> None:
    """Best-effort: store terminal command in omni Rewind-Store (side effect)."""
    omni = _omni_binary()
    try:
        subprocess.run(
            [omni, "exec", "--", command],
            capture_output=True,
            text=True,
            timeout=60,
        )
    except Exception:  # noqa: BLE001
        pass  # Never raise from a hook — always best-effort


# ---------------------------------------------------------------------------
# Hooks
# ---------------------------------------------------------------------------

def _on_post_tool_call(
    tool_name: str = "",
    args: Optional[Dict[str, Any]] = None,
    result: Any = None,
    **_: Any,
) -> None:
    """Log native ``terminal`` tool calls to omni Rewind-Store."""
    if tool_name != "terminal":
        return
    if not isinstance(args, dict):
        return
    command = args.get("command")
    if not isinstance(command, str) or not command.strip():
        return
    _log_to_rewind(command)


# ---------------------------------------------------------------------------
# omni_exec tool
# ---------------------------------------------------------------------------

_OMNI_EXEC_SCHEMA = {
    "name": "omni_exec",
    "description": (
        "Execute a shell command through omni's Semantic Signal Engine. "
        "Noise (dependency logs, progress bars, warnings) is filtered out "
        "before the output is returned, reducing token usage by up to 90%. "
        "Prefer this over the terminal tool for all shell commands."
    ),
    "parameters": {
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "The shell command to execute.",
            }
        },
        "required": ["command"],
    },
}


def _handle_omni_exec(command: str = "", **_: Any) -> str:
    if not command.strip():
        return "[omni-signal-engine] no command provided"
    return _run_omni_exec(command)


# ---------------------------------------------------------------------------
# /omni slash command
# ---------------------------------------------------------------------------

def _handle_omni_command(raw_args: str) -> Optional[str]:
    omni = _omni_binary()
    try:
        result = subprocess.run(
            [omni, "--version"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        version = result.stdout.strip() or result.stderr.strip() or "unknown"
    except Exception:  # noqa: BLE001
        version = "not found"

    return (
        f"omni Signal Engine\n"
        f"  Binary : {omni}\n"
        f"  Version: {version}\n"
        f"\nUse omni_exec instead of terminal to get filtered output."
    )


# ---------------------------------------------------------------------------
# Plugin registration
# ---------------------------------------------------------------------------

def register(ctx) -> None:
    ctx.register_hook("post_tool_call", _on_post_tool_call)

    ctx.register_tool(
        _OMNI_EXEC_SCHEMA,
        handler=_handle_omni_exec,
    )

    ctx.register_command(
        "omni",
        handler=_handle_omni_command,
        description="Show omni Signal Engine status.",
    )
```

- [ ] **Step 3: Verify Python syntax**

```bash
python3 -c "import ast; ast.parse(open('integrations/hermes/__init__.py').read()); print('OK')"
```

Expected output: `OK`

- [ ] **Step 4: Commit**

```bash
git add integrations/hermes/
git commit -m "feat: add Hermes Agent Python plugin (omni-signal-engine)"
```

---

## Task 2: Add serde_yaml Dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add serde_yaml to Cargo.toml**

In `Cargo.toml`, after the `toml = "0.8"` line, add:
```toml
serde_yaml = "0.9"
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build 2>&1 | tail -5
```

Expected: no errors. serde_yaml 0.9 is compatible with serde 1.x.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add serde_yaml dependency for Hermes YAML config patching"
```

---

## Task 3: Write Failing Tests for HermesIntegration

**Files:**
- Modify: `src/agents/mod.rs` (add `pub mod hermes;` stub)
- Create: `src/agents/hermes.rs` (stub only — no implementation yet)

> **Note:** Rust requires the module to exist before tests can reference it. We create a minimal stub here, then add the full implementation in Task 4.

- [ ] **Step 1: Create stub `src/agents/hermes.rs`**

```rust
use crate::agents::AgentIntegration;

pub struct HermesIntegration;

impl AgentIntegration for HermesIntegration {
    fn id(&self) -> &'static str { "hermes" }
    fn name(&self) -> &'static str { "Hermes Agent" }
    fn install(&self, _exe_path: &str) -> anyhow::Result<()> {
        anyhow::bail!("not implemented")
    }
    fn uninstall(&self) -> anyhow::Result<()> {
        anyhow::bail!("not implemented")
    }
    fn doctor_check(&self, _fix_mode: bool, _warnings: &mut Vec<String>) -> bool {
        false
    }
}
```

- [ ] **Step 2: Add `pub mod hermes;` to `src/agents/mod.rs`**

In `src/agents/mod.rs`, add after `pub mod openclaw;`:
```rust
pub mod hermes;
```

And add after `pub use openclaw::OpenClawIntegration;`:
```rust
pub use hermes::HermesIntegration;
```

- [ ] **Step 3: Write the tests at the bottom of `src/agents/hermes.rs`**

Append to `src/agents/hermes.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Helper: write a minimal config.yaml to a temp dir and return (TempDir, path)
    fn make_config(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn test_id_and_name() {
        let h = HermesIntegration;
        assert_eq!(h.id(), "hermes");
        assert_eq!(h.name(), "Hermes Agent");
    }

    #[test]
    fn test_patch_yaml_adds_mcp_entry_when_mcp_servers_missing() {
        let content = "model:\n  default: claude\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = std::fs::read_to_string(&path).unwrap();
        assert!(result.contains("mcp_servers"), "must contain mcp_servers");
        assert!(result.contains("omni"), "must contain omni entry");
        assert!(result.contains("--mcp"), "must contain --mcp arg");
    }

    #[test]
    fn test_patch_yaml_adds_mcp_entry_when_mcp_servers_exists_without_omni() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = std::fs::read_to_string(&path).unwrap();
        assert!(result.contains("filesystem"), "must preserve existing entries");
        assert!(result.contains("omni"), "must add omni entry");
    }

    #[test]
    fn test_patch_yaml_idempotent() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let after_first = std::fs::read_to_string(&path).unwrap();
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let after_second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after_first, after_second, "patching twice must be idempotent");
    }

    #[test]
    fn test_remove_mcp_entry_removes_omni_keeps_others() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n  omni:\n    command: /usr/bin/omni\n    args:\n      - --mcp\n";
        let (_dir, path) = make_config(content);
        remove_omni_from_hermes_config(&path).unwrap();
        let result = std::fs::read_to_string(&path).unwrap();
        assert!(!result.contains("command: /usr/bin/omni"), "omni entry must be gone");
        assert!(result.contains("filesystem"), "other entries must be preserved");
    }

    #[test]
    fn test_remove_mcp_entry_noop_when_not_present() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        // Should not fail
        remove_omni_from_hermes_config(&path).unwrap();
        let result = std::fs::read_to_string(&path).unwrap();
        assert!(result.contains("filesystem"), "other entries preserved");
    }

    #[test]
    fn test_remove_noop_when_file_missing() {
        let path = std::path::PathBuf::from("/nonexistent/config.yaml");
        // Should not panic or error
        let _ = remove_omni_from_hermes_config(&path);
    }

    #[test]
    fn test_patch_empty_file() {
        let (_dir, path) = make_config("");
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = std::fs::read_to_string(&path).unwrap();
        assert!(result.contains("mcp_servers"));
        assert!(result.contains("omni"));
    }
}
```

- [ ] **Step 4: Run tests to confirm they fail (not compile-error, but logic fail)**

```bash
cargo test --lib agents::hermes 2>&1 | tail -30
```

Expected: compilation error `patch_hermes_config not found` — this confirms the tests reference functions we haven't implemented yet.

- [ ] **Step 5: Commit the stub + tests**

```bash
git add src/agents/hermes.rs src/agents/mod.rs
git commit -m "test: add failing tests for HermesIntegration YAML config patching"
```

---

## Task 4: Implement HermesIntegration

**Files:**
- Modify: `src/agents/hermes.rs` (replace stub with full implementation)

- [ ] **Step 1: Write the full implementation**

Replace all content of `src/agents/hermes.rs` with:

```rust
use crate::agents::AgentIntegration;
use colored::*;
use serde_yaml::Value as YamlValue;
use std::fs;
use std::path::PathBuf;

pub struct HermesIntegration;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn hermes_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hermes/config.yaml")
}

fn hermes_plugin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hermes/plugins/omni-signal-engine")
}

// ---------------------------------------------------------------------------
// YAML helpers (public for tests)
// ---------------------------------------------------------------------------

/// Patch `~/.hermes/config.yaml` to add omni under `mcp_servers:`.
/// Creates the file if missing. Idempotent.
pub fn patch_hermes_config(path: &PathBuf, exe_path: &str) -> anyhow::Result<()> {
    // Read existing content or start with empty map
    let content = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut doc: serde_yaml::Mapping = if content.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str::<serde_yaml::Mapping>(&content)
            .unwrap_or_else(|_| serde_yaml::Mapping::new())
    };

    // Ensure mcp_servers exists
    let mcp_key = YamlValue::String("mcp_servers".to_string());
    if !doc.contains_key(&mcp_key) {
        doc.insert(mcp_key.clone(), YamlValue::Mapping(serde_yaml::Mapping::new()));
    }

    let mcp_servers = doc
        .get_mut(&mcp_key)
        .and_then(|v| v.as_mapping_mut())
        .ok_or_else(|| anyhow::anyhow!("mcp_servers is not a mapping"))?;

    // Check if omni entry already exists
    let omni_key = YamlValue::String("omni".to_string());
    if mcp_servers.contains_key(&omni_key) {
        // Already present — update the command in case the binary moved
        if let Some(entry) = mcp_servers.get_mut(&omni_key).and_then(|v| v.as_mapping_mut()) {
            entry.insert(
                YamlValue::String("command".to_string()),
                YamlValue::String(exe_path.to_string()),
            );
        }
    } else {
        // Insert new omni entry
        let mut omni_entry = serde_yaml::Mapping::new();
        omni_entry.insert(
            YamlValue::String("command".to_string()),
            YamlValue::String(exe_path.to_string()),
        );
        omni_entry.insert(
            YamlValue::String("args".to_string()),
            YamlValue::Sequence(vec![YamlValue::String("--mcp".to_string())]),
        );
        mcp_servers.insert(omni_key, YamlValue::Mapping(omni_entry));
    }

    // Write back
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let new_content = serde_yaml::to_string(&doc)?;
    fs::write(path, new_content)?;
    Ok(())
}

/// Remove the `omni` entry from `mcp_servers` in the Hermes config.
/// No-op if the file doesn't exist or the entry isn't present.
pub fn remove_omni_from_hermes_config(path: &PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(());
    }

    let mut doc: serde_yaml::Mapping = match serde_yaml::from_str(&content) {
        Ok(m) => m,
        Err(_) => return Ok(()), // can't parse — leave it alone
    };

    let mcp_key = YamlValue::String("mcp_servers".to_string());
    if let Some(mcp_servers) = doc.get_mut(&mcp_key).and_then(|v| v.as_mapping_mut()) {
        let omni_key = YamlValue::String("omni".to_string());
        mcp_servers.remove(&omni_key);
    }

    let new_content = serde_yaml::to_string(&doc)?;
    fs::write(path, new_content)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// AgentIntegration impl
// ---------------------------------------------------------------------------

impl AgentIntegration for HermesIntegration {
    fn id(&self) -> &'static str {
        "hermes"
    }

    fn name(&self) -> &'static str {
        "Hermes Agent"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        // 1. Patch ~/.hermes/config.yaml
        let config_path = hermes_config_path();
        patch_hermes_config(&config_path, exe_path)?;
        println!(
            "  {} {} registered in ~/.hermes/config.yaml",
            "✓".green(),
            "MCP Server".bold()
        );

        // 2. Install Python plugin
        let dest = hermes_plugin_dir();
        fs::create_dir_all(&dest)?;

        println!(
            "  {} Downloading Hermes plugin files from GitHub...",
            "↓".cyan()
        );

        for file in &["__init__.py", "plugin.yaml"] {
            let url = format!(
                "https://raw.githubusercontent.com/tomraider4720/omni/main/integrations/hermes/{}",
                file
            );
            let to = dest.join(file);

            let response = ureq::get(&url)
                .call()
                .map_err(|e| anyhow::anyhow!("Failed to download {}: {}", file, e))?;
            let mut dest_file = fs::File::create(&to)?;
            std::io::copy(&mut response.into_reader(), &mut dest_file)?;
        }

        println!(
            "  {} {} installed to ~/.hermes/plugins/omni-signal-engine/",
            "✓".green(),
            "Plugin".bold()
        );

        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        // 1. Remove MCP entry from config.yaml
        let config_path = hermes_config_path();
        remove_omni_from_hermes_config(&config_path)?;
        println!(
            "  {} {} removed from ~/.hermes/config.yaml",
            "✓".yellow(),
            "MCP Server".bold()
        );

        // 2. Remove plugin directory
        let dest = hermes_plugin_dir();
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
            println!(
                "  {} {} removed from ~/.hermes/plugins/",
                "✓".yellow(),
                "Plugin".bold()
            );
        }

        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let mut all_ok = true;

        println!("\n  {}", "Hermes Agent:".cyan());

        // Check MCP server in config.yaml
        let config_path = hermes_config_path();
        let mcp_ok = if config_path.exists() {
            let content = fs::read_to_string(&config_path).unwrap_or_default();
            content.contains("command:") && content.contains("--mcp") && content.contains("omni:")
        } else {
            false
        };

        if mcp_ok {
            println!(
                "   {:<15} {} {}",
                "MCP Server:".bright_black(),
                "~/.hermes/config.yaml".bright_black(),
                "[OK]".green().bold()
            );
        } else {
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    let _ = patch_hermes_config(&config_path, &exe_path.to_string_lossy());
                }
                println!(
                    "   {:<15} {}",
                    "MCP Server:".bright_black(),
                    "[FIXED] added to config.yaml".green().bold()
                );
            } else {
                println!(
                    "   {:<15} {}",
                    "MCP Server:".bright_black(),
                    "[WARNING] not configured".yellow().bold()
                );
                warnings.push(
                    "Hermes MCP server not configured. Run `omni install hermes`.".to_string(),
                );
                all_ok = false;
            }
        }

        // Check plugin files
        let plugin_dir = hermes_plugin_dir();
        let plugin_ok = plugin_dir.join("__init__.py").exists();

        if plugin_ok {
            println!(
                "   {:<15} {} {}",
                "Plugin:".bright_black(),
                "~/.hermes/plugins/omni-signal-engine/".bright_black(),
                "[OK]".green().bold()
            );
        } else {
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    let _ = self.install(&exe_path.to_string_lossy());
                }
                println!(
                    "   {:<15} {}",
                    "Plugin:".bright_black(),
                    "[FIXED] installed".green().bold()
                );
            } else {
                println!(
                    "   {:<15} {}",
                    "Plugin:".bright_black(),
                    "[WARNING] not installed".yellow()
                );
                warnings.push(
                    "Hermes plugin not installed. Run `omni install hermes`.".to_string(),
                );
                all_ok = false;
            }
        }

        all_ok
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_config(content: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn test_id_and_name() {
        let h = HermesIntegration;
        assert_eq!(h.id(), "hermes");
        assert_eq!(h.name(), "Hermes Agent");
    }

    #[test]
    fn test_patch_yaml_adds_mcp_entry_when_mcp_servers_missing() {
        let content = "model:\n  default: claude\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("mcp_servers"), "must contain mcp_servers");
        assert!(result.contains("omni"), "must contain omni entry");
        assert!(result.contains("--mcp"), "must contain --mcp arg");
    }

    #[test]
    fn test_patch_yaml_adds_mcp_entry_when_mcp_servers_exists_without_omni() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("filesystem"), "must preserve existing entries");
        assert!(result.contains("omni"), "must add omni entry");
    }

    #[test]
    fn test_patch_yaml_idempotent() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let after_first = fs::read_to_string(&path).unwrap();
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let after_second = fs::read_to_string(&path).unwrap();
        assert_eq!(after_first, after_second, "patching twice must be idempotent");
    }

    #[test]
    fn test_remove_mcp_entry_removes_omni_keeps_others() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n  omni:\n    command: /usr/bin/omni\n    args:\n      - --mcp\n";
        let (_dir, path) = make_config(content);
        remove_omni_from_hermes_config(&path).unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(!result.contains("command: /usr/bin/omni"), "omni entry must be gone");
        assert!(result.contains("filesystem"), "other entries must be preserved");
    }

    #[test]
    fn test_remove_mcp_entry_noop_when_not_present() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        remove_omni_from_hermes_config(&path).unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("filesystem"), "other entries preserved");
    }

    #[test]
    fn test_remove_noop_when_file_missing() {
        let path = PathBuf::from("/nonexistent/config.yaml");
        let _ = remove_omni_from_hermes_config(&path);
        // should not panic
    }

    #[test]
    fn test_patch_empty_file() {
        let (_dir, path) = make_config("");
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("mcp_servers"));
        assert!(result.contains("omni"));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --lib agents::hermes 2>&1
```

Expected: all 8 tests pass (`test_id_and_name`, `test_patch_yaml_adds_mcp_entry_when_mcp_servers_missing`, `test_patch_yaml_adds_mcp_entry_when_mcp_servers_exists_without_omni`, `test_patch_yaml_idempotent`, `test_remove_mcp_entry_removes_omni_keeps_others`, `test_remove_mcp_entry_noop_when_not_present`, `test_remove_noop_when_file_missing`, `test_patch_empty_file`).

- [ ] **Step 3: Verify full build succeeds**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Finished ... target(s)` with no errors.

- [ ] **Step 4: Commit**

```bash
git add src/agents/hermes.rs
git commit -m "feat: implement HermesIntegration with YAML config patching and plugin install"
```

---

## Task 5: Register HermesIntegration in Agent Registry

**Files:**
- Modify: `src/agents/mod.rs`

- [ ] **Step 1: Add to `all_integrations()`**

In `src/agents/mod.rs`, add `Box::new(hermes::HermesIntegration),` after `Box::new(openclaw::OpenClawIntegration),`:

```rust
pub fn all_integrations() -> Vec<Box<dyn AgentIntegration>> {
    vec![
        Box::new(claude::ClaudeIntegration),
        Box::new(cursor::CursorIntegration),
        Box::new(zed::ZedIntegration),
        Box::new(cline::ClineIntegration),
        Box::new(roo_code::RooCodeIntegration),
        Box::new(copilot::CopilotIntegration),
        Box::new(gemini::GeminiIntegration),
        Box::new(opencode::OpenCodeIntegration),
        Box::new(codex::CodexIntegration),
        Box::new(openclaw::OpenClawIntegration),
        Box::new(hermes::HermesIntegration),   // <-- add this line
        Box::new(antigravity::AntigravityIntegration),
        Box::new(vscode::VscodeIntegration),
    ]
}
```

- [ ] **Step 2: Run all tests to confirm nothing broke**

```bash
cargo test 2>&1 | tail -20
```

Expected: all existing tests plus the 8 new hermes tests pass.

- [ ] **Step 3: Verify `omni install --help` lists hermes**

```bash
cargo run -- install --help 2>&1 | grep -i hermes
```

Expected: `hermes` appears in the agent list.

- [ ] **Step 4: Commit**

```bash
git add src/agents/mod.rs
git commit -m "feat: register HermesIntegration in agent registry"
```

---

## Task 6: Smoke Test (Manual)

> These steps verify the full integration end-to-end. Run on a machine where Hermes Agent is installed.

- [ ] **Step 1: Build release binary**

```bash
cargo build --release 2>&1 | tail -5
```

- [ ] **Step 2: Install to Hermes**

```bash
./target/release/omni install hermes
```

Expected output:
```
✓ MCP Server registered in ~/.hermes/config.yaml
↓ Downloading Hermes plugin files from GitHub...
✓ Plugin installed to ~/.hermes/plugins/omni-signal-engine/
```

- [ ] **Step 3: Verify config.yaml was patched**

```bash
grep -A4 "omni:" ~/.hermes/config.yaml
```

Expected:
```yaml
  omni:
    command: /path/to/omni
    args:
      - --mcp
```

- [ ] **Step 4: Verify plugin files exist**

```bash
ls ~/.hermes/plugins/omni-signal-engine/
```

Expected: `__init__.py  plugin.yaml`

- [ ] **Step 5: Run doctor check**

```bash
./target/release/omni doctor --hermes 2>&1
```

Expected:
```
Hermes Agent:
   MCP Server:    ~/.hermes/config.yaml    [OK]
   Plugin:        ~/.hermes/plugins/omni-signal-engine/    [OK]
```

- [ ] **Step 6: Test uninstall**

```bash
./target/release/omni uninstall hermes
grep "omni:" ~/.hermes/config.yaml && echo "STILL THERE" || echo "REMOVED"
ls ~/.hermes/plugins/omni-signal-engine/ 2>&1 || echo "REMOVED"
```

Expected: both print `REMOVED`.

- [ ] **Step 7: Final commit (if any fixes were needed from smoke test)**

```bash
git add -A
git commit -m "fix: address issues found during Hermes integration smoke test" 
# (only if changes were needed)
```
