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
