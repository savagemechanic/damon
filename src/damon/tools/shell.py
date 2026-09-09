from __future__ import annotations

from pathlib import Path

from damon.runtime.commands import CommandRunner, classify_command
from damon.tools.registry import tool

_BLOCKED = {
    "sudo", "su", "rm", "rmdir", "mkfs", "diskutil", "dd", "shutdown", "reboot",
    "halt", "killall", "launchctl", "chmod", "chown", "ssh", "scp", "curl", "wget",
}


def make_shell_tool(root: Path, runner: CommandRunner | None = None):
    commands = runner or CommandRunner(root)

    @tool(permission="process.execute", timeout=60)
    async def run_command(command: list[str], timeout: float = 30.0) -> dict:
        """Run a non-shell command in the workspace; high-risk executables are blocked."""
        if not command:
            raise ValueError("command cannot be empty")
        executable = Path(command[0]).name
        if executable in _BLOCKED:
            raise PermissionError(f"executable blocked by shell policy: {executable}")
        spec = classify_command(command)
        if spec.capability != "process.execute" or spec.mutates or spec.network or spec.privileged:
            reason = spec.capability
            if spec.mutates:
                reason += "/mutation"
            if spec.network:
                reason += "/network"
            if spec.privileged:
                reason += "/privileged"
            raise PermissionError(
                f"command requires structured policy ({reason}); use a purpose-built tool"
            )
        result = await commands.run(command, timeout=timeout)
        return result.compact()

    return run_command
