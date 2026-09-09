from __future__ import annotations

import asyncio
import inspect
from dataclasses import dataclass
from typing import Any

from damon.policy.engine import Policy
from damon.tools.registry import ToolRegistry


@dataclass(slots=True)
class ToolResult:
    name: str
    ok: bool
    output: Any = None
    error: str | None = None


class ToolExecutor:
    def __init__(self, registry: ToolRegistry, policy: Policy | None = None) -> None:
        self.registry = registry
        self.policy = policy or Policy()

    async def execute(self, name: str, arguments: dict[str, Any], *, approved: bool = False) -> ToolResult:
        spec = self.registry.get(name)
        try:
            self.policy.enforce(spec.permission, approved=approved)
            async with asyncio.timeout(spec.timeout):
                if inspect.iscoroutinefunction(spec.function):
                    output = await spec.function(**arguments)
                else:
                    output = await asyncio.to_thread(spec.function, **arguments)
            return ToolResult(name=name, ok=True, output=output)
        except Exception as exc:
            return ToolResult(name=name, ok=False, error=f"{type(exc).__name__}: {exc}")
