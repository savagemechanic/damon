from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any

from damon.core.agent import Agent
from damon.models.base import Model
from damon.runtime.events import EventBus
from damon.runtime.executor import ToolExecutor, ToolResult
from damon.tools.registry import ToolRegistry


CODING_SYSTEM_PROMPT = """You are Damon, a local-first coding agent.
The Python runtime has already inspected the repository and will verify your work after you finish.
Inspect before editing. Search narrowly. Make the smallest correct change. Prefer patches over rewrites.
Use deterministic tools instead of guessing. Do not commit, push, delete, or use privileged operations.
When the task is complete, stop and summarize what changed. Do not claim tests passed unless tool evidence says so."""


@dataclass(slots=True)
class CodingAttempt:
    number: int
    response: str
    verification: dict[str, Any]
    git_diff: dict[str, Any]
    git_status: dict[str, Any]

    @property
    def verification_failed(self) -> bool:
        return self.verification.get("checks_run", 0) > 0 and not bool(self.verification.get("passed"))


@dataclass(slots=True)
class CodingOutcome:
    request: str
    response: str
    preflight: dict[str, Any]
    attempts: list[CodingAttempt] = field(default_factory=list)

    @property
    def verification_passed(self) -> bool | None:
        if not self.attempts:
            return None
        verification = self.attempts[-1].verification
        if verification.get("checks_run", 0) == 0:
            return None
        return bool(verification.get("passed"))

    @property
    def repaired(self) -> bool:
        return len(self.attempts) > 1

    def as_dict(self) -> dict[str, Any]:
        final = self.attempts[-1] if self.attempts else None
        return {
            "request": self.request,
            "response": self.response,
            "attempts": len(self.attempts),
            "repaired": self.repaired,
            "verification_passed": self.verification_passed,
            "preflight": self.preflight,
            "verification": final.verification if final else {},
            "git_diff": final.git_diff if final else {},
            "git_status": final.git_status if final else {},
        }


class CodingAgent:
    """Bounded coding harness that keeps task state outside the model.

    Each attempt is a fresh agent run over the current filesystem state. Python performs
    deterministic preflight, verification, and Git evidence collection between attempts.
    Failed verification is fed back as compact repair evidence; successful verification
    ends the loop immediately.
    """

    def __init__(
        self,
        model: Model,
        registry: ToolRegistry,
        executor: ToolExecutor,
        *,
        max_attempts: int = 3,
        max_agent_steps: int = 16,
        evidence_chars: int = 12_000,
        events: EventBus | None = None,
    ) -> None:
        if max_attempts < 1:
            raise ValueError("max_attempts must be >= 1")
        if evidence_chars < 1:
            raise ValueError("evidence_chars must be >= 1")
        self.model = model
        self.registry = registry
        self.executor = executor
        self.max_attempts = max_attempts
        self.max_agent_steps = max_agent_steps
        self.evidence_chars = evidence_chars
        self.events = events or EventBus()

    async def _tool(self, name: str, arguments: dict[str, Any] | None = None) -> Any:
        result: ToolResult = await self.executor.execute(name, arguments or {})
        if not result.ok:
            raise RuntimeError(f"deterministic tool {name} failed: {result.error}")
        return result.output

    def _compact(self, value: Any) -> str:
        text = json.dumps(value, default=str, separators=(",", ":"))
        if len(text) <= self.evidence_chars:
            return text
        return text[-self.evidence_chars :]

    async def _preflight(self) -> dict[str, Any]:
        return {
            "project": await self._tool("inspect_project"),
            "checks": await self._tool("discover_checks"),
            "git_status": await self._tool("git_status"),
        }

    async def _evidence(self) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
        verification = await self._tool("verify_project")
        git_diff = await self._tool("git_diff")
        git_status = await self._tool("git_status")
        return verification, git_diff, git_status

    def _prompt(self, request: str, preflight: dict[str, Any], previous: CodingAttempt | None) -> str:
        prompt = [
            f"TASK:\n{request}",
            f"DETERMINISTIC PREFLIGHT:\n{self._compact(preflight)}",
        ]
        if previous is not None:
            repair = {
                "verification": previous.verification,
                "git_diff": previous.git_diff,
                "git_status": previous.git_status,
            }
            prompt.extend([
                "The previous attempt failed deterministic verification. Repair the existing workspace; do not restart or broaden the change unnecessarily.",
                f"LATEST FAILURE EVIDENCE:\n{self._compact(repair)}",
            ])
        return "\n\n".join(prompt)

    async def run(self, request: str) -> CodingOutcome:
        preflight = await self._preflight()
        attempts: list[CodingAttempt] = []
        previous: CodingAttempt | None = None
        response = ""

        for number in range(1, self.max_attempts + 1):
            agent = Agent(
                self.model,
                self.registry,
                self.executor,
                max_steps=self.max_agent_steps,
                events=self.events,
                system_prompt=CODING_SYSTEM_PROMPT,
            )
            response = await agent.run(self._prompt(request, preflight, previous))
            verification, git_diff, git_status = await self._evidence()
            attempt = CodingAttempt(number, response, verification, git_diff, git_status)
            attempts.append(attempt)
            if not attempt.verification_failed:
                break
            previous = attempt

        return CodingOutcome(request=request, response=response, preflight=preflight, attempts=attempts)
