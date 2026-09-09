from __future__ import annotations

import subprocess
from pathlib import Path

from damon.tools.registry import tool


def _git(root: Path, *args: str) -> dict:
    result = subprocess.run(["git", *args], cwd=root, text=True, capture_output=True, timeout=20, check=False)
    return {"exit_code": result.returncode, "stdout": result.stdout[-12000:], "stderr": result.stderr[-12000:]}


def make_git_tools(root: Path):
    @tool(permission="git.read")
    def git_status() -> dict:
        """Show concise Git status for the workspace."""
        return _git(root, "status", "--short", "--branch")

    @tool(permission="git.read")
    def git_diff() -> dict:
        """Show the current unstaged Git diff."""
        return _git(root, "diff", "--", ".")

    @tool(permission="git.read")
    def git_diff_staged() -> dict:
        """Show the staged Git diff."""
        return _git(root, "diff", "--cached", "--", ".")

    return [git_status, git_diff, git_diff_staged]
