from __future__ import annotations

import subprocess
from pathlib import Path

from damon.tools.registry import tool


def make_patch_tool(root: Path):
    @tool(permission="workspace.write", timeout=30)
    def apply_patch(patch: str) -> dict:
        """Apply a minimal unified diff to the Git workspace after a dry-run check."""
        check = subprocess.run(["git", "apply", "--check", "-"], cwd=root, input=patch, text=True, capture_output=True, timeout=20, check=False)
        if check.returncode != 0:
            return {"applied": False, "error": check.stderr[-6000:]}
        result = subprocess.run(["git", "apply", "-"], cwd=root, input=patch, text=True, capture_output=True, timeout=20, check=False)
        return {"applied": result.returncode == 0, "exit_code": result.returncode, "stderr": result.stderr[-6000:]}
    return apply_patch
