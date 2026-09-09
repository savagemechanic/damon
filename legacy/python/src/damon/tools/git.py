from __future__ import annotations

from pathlib import Path

from damon.runtime.commands import CommandRunner
from damon.tools.registry import tool


def make_git_tools(root: Path, runner: CommandRunner | None = None):
    commands = runner or CommandRunner(root)

    async def _git(*args: str) -> dict:
        return (await commands.run(["git", *args], timeout=20)).compact()

    @tool(permission="git.read")
    async def git_status() -> dict:
        """Show concise Git status for the workspace."""
        return await _git("status", "--short", "--branch")

    @tool(permission="git.read")
    async def git_diff() -> dict:
        """Show the current unstaged Git diff."""
        return await _git("diff", "--", ".")

    @tool(permission="git.read")
    async def git_diff_staged() -> dict:
        """Show the staged Git diff."""
        return await _git("diff", "--cached", "--", ".")

    return [git_status, git_diff, git_diff_staged]
