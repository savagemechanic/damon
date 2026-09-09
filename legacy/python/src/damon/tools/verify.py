from __future__ import annotations

from pathlib import Path

from damon.runtime.commands import CommandRunner
from damon.runtime.projects import discover_verification_commands
from damon.tools.registry import tool


def make_verification_tools(root: Path, runner: CommandRunner | None = None):
    commands = runner or CommandRunner(root)

    @tool(permission="filesystem.read")
    def discover_checks() -> list[dict]:
        """Discover deterministic test, lint, typecheck, format, and build checks for the repository."""
        return [check.as_dict() for check in discover_verification_commands(root)]

    @tool(permission="process.execute", timeout=300)
    async def verify_project(timeout_per_check: float = 120.0, max_checks: int = 8) -> dict:
        """Run available repository verification checks discovered from project configuration."""
        if timeout_per_check <= 0:
            raise ValueError("timeout_per_check must be > 0")
        if max_checks < 1 or max_checks > 20:
            raise ValueError("max_checks must be between 1 and 20")

        checks = discover_verification_commands(root)[:max_checks]
        results: list[dict] = []
        skipped: list[dict] = []
        for check in checks:
            if not check.available:
                skipped.append(check.as_dict())
                continue
            result = await commands.run(check.argv, timeout=timeout_per_check)
            results.append({
                "kind": check.kind,
                "source": check.source,
                **result.compact(),
            })

        passed = bool(results) and all(bool(result["exit_code"] == 0 and not result["timed_out"]) for result in results)
        return {
            "passed": passed,
            "checks_run": len(results),
            "checks_skipped": len(skipped),
            "results": results,
            "skipped": skipped,
        }

    return [discover_checks, verify_project]
