from __future__ import annotations

from dataclasses import dataclass, field
from enum import IntEnum
import time
from typing import Sequence

from damon.core.types import Message, ModelResponse
from damon.models.base import Model


class ModelTier(IntEnum):
    LOCAL_SMALL = 0
    LOCAL_LARGE = 1
    CLOUD_FREE = 2
    CLOUD_PAID = 3


@dataclass(frozen=True, slots=True)
class ModelCandidate:
    """One model route, ordered from cheapest/preferred to strongest fallback."""

    name: str
    model: Model
    tier: ModelTier = ModelTier.LOCAL_SMALL
    local: bool = True


@dataclass(frozen=True, slots=True)
class RouteDecision:
    model: str
    tier: ModelTier
    reason: str
    timestamp: float


@dataclass(slots=True)
class RouterPolicy:
    """Small, deterministic escalation policy suitable for local models."""

    local_only: bool = False
    tool_failures_before_escalation: int = 2
    generation_failures_before_escalation: int = 1
    max_escalations: int = 3

    def __post_init__(self) -> None:
        if self.tool_failures_before_escalation < 1:
            raise ValueError("tool_failures_before_escalation must be >= 1")
        if self.generation_failures_before_escalation < 1:
            raise ValueError("generation_failures_before_escalation must be >= 1")
        if self.max_escalations < 0:
            raise ValueError("max_escalations must be >= 0")


@dataclass(slots=True)
class RouterStats:
    generations: int = 0
    escalations: int = 0
    generation_failures: int = 0
    tool_failures: int = 0
    decisions: list[RouteDecision] = field(default_factory=list)


class ModelRouter:
    """Local-first model router with bounded, evidence-driven escalation.

    Candidates are supplied in preferred order. The router starts at the cheapest
    eligible candidate and moves upward only after deterministic execution or
    provider failures. It never de-escalates during a run, avoiding oscillation.
    """

    def __init__(
        self,
        candidates: Sequence[ModelCandidate],
        *,
        policy: RouterPolicy | None = None,
    ) -> None:
        self.policy = policy or RouterPolicy()
        self._candidates = [c for c in candidates if not self.policy.local_only or c.local]
        if not self._candidates:
            raise ValueError("model router has no eligible candidates")
        self._index = 0
        self._consecutive_tool_failures = 0
        self._consecutive_generation_failures = 0
        self.stats = RouterStats()
        self._record_decision("initial route")

    @property
    def current(self) -> ModelCandidate:
        return self._candidates[self._index]

    @property
    def candidates(self) -> tuple[ModelCandidate, ...]:
        return tuple(self._candidates)

    def reset_run(self) -> None:
        """Start a new request at the cheapest eligible route."""
        self._index = 0
        self._consecutive_tool_failures = 0
        self._consecutive_generation_failures = 0
        self._record_decision("new run")

    async def generate(self, messages: list[Message], tools: list[dict]) -> ModelResponse:
        while True:
            candidate = self.current
            self.stats.generations += 1
            try:
                response = await candidate.model.generate(messages, tools)
            except Exception:
                self.stats.generation_failures += 1
                self._consecutive_generation_failures += 1
                if (
                    self._consecutive_generation_failures >= self.policy.generation_failures_before_escalation
                    and self._escalate("provider generation failure")
                ):
                    self._consecutive_generation_failures = 0
                    continue
                raise
            self._consecutive_generation_failures = 0
            return response

    def record_tool_result(self, *, ok: bool) -> None:
        if ok:
            self._consecutive_tool_failures = 0
            return
        self.stats.tool_failures += 1
        self._consecutive_tool_failures += 1
        if self._consecutive_tool_failures >= self.policy.tool_failures_before_escalation:
            if self._escalate("repeated tool execution failure"):
                self._consecutive_tool_failures = 0

    def _escalate(self, reason: str) -> bool:
        if self.stats.escalations >= self.policy.max_escalations:
            return False
        if self._index + 1 >= len(self._candidates):
            return False
        self._index += 1
        self.stats.escalations += 1
        self._record_decision(reason)
        return True

    def _record_decision(self, reason: str) -> None:
        candidate = self.current
        self.stats.decisions.append(
            RouteDecision(
                model=candidate.name,
                tier=candidate.tier,
                reason=reason,
                timestamp=time.time(),
            )
        )
