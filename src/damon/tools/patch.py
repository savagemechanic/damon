from __future__ import annotations

from pathlib import Path

from damon.runtime.commands import CommandRunner
from damon.tools.registry import tool


def make_patch_tool(root: Path, runner: CommandRunner | None = None):
    commands = runner or CommandRunner(root)

    @tool(permission="workspace.write", timeout=30)
    async def apply_patch(patch: str) -> dict:
        """Apply a minimal unified diff to the Git workspace after a dry-run check."""
        check = await commands.run(["git", "apply", "--check", "-"], stdin=patch, timeout=20)
        if not check.ok:
            return {"applied": False, "error": check.stderr[-6000:]}
        result = await commands.run(["git", "apply", "-"], stdin=patch, timeout=20)
        return {
            "applied": result.ok,
            "exit_code": result.returncode,
            "stderr": result.stderr[-6000:],
        }

    return apply_patch
