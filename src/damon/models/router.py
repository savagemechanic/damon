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
    input_usd_per_million: float = 0.0
    output_usd_per_million: float = 0.0
    request_cost_ceiling_usd: float = 0.0

    @property
    def paid(self) -> bool:
        return self.tier == ModelTier.CLOUD_PAID or self.input_usd_per_million > 0 or self.output_usd_per_million > 0


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
    allow_cloud: bool = False
    max_spend_usd: float = 0.0
    tool_failures_before_escalation: int = 2
    generation_failures_before_escalation: int = 1
    max_escalations: int = 3

    def __post_init__(self) -> None:
        if self.max_spend_usd < 0:
            raise ValueError("max_spend_usd must be >= 0")
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
    input_tokens: int = 0
    output_tokens: int = 0
    estimated_spend_usd: float = 0.0
    decisions: list[RouteDecision] = field(default_factory=list)


class ModelRouter:
    """Local-first model router with bounded, evidence-driven escalation.

    Cloud candidates are ineligible unless policy explicitly allows cloud use.
    Paid candidates are additionally constrained by the run's hard spend budget.
    The router never de-escalates within a run, avoiding oscillation.
    """

    def __init__(
        self,
        candidates: Sequence[ModelCandidate],
        *,
        policy: RouterPolicy | None = None,
    ) -> None:
        self.policy = policy or RouterPolicy()
        self._candidates = [candidate for candidate in candidates if self._eligible(candidate)]
        if not self._candidates:
            raise ValueError("model router has no eligible candidates")
        self._index = 0
        self._consecutive_tool_failures = 0
        self._consecutive_generation_failures = 0
        self.stats = RouterStats()
        self._record_decision("initial route")

    def _eligible(self, candidate: ModelCandidate) -> bool:
        if self.policy.local_only and not candidate.local:
            return False
        if not candidate.local and not self.policy.allow_cloud:
            return False
        if candidate.paid and (self.policy.max_spend_usd <= 0 or candidate.request_cost_ceiling_usd <= 0):
            return False
        if candidate.paid and candidate.request_cost_ceiling_usd > self.policy.max_spend_usd:
            return False
        return True

    @property
    def current(self) -> ModelCandidate:
        return self._candidates[self._index]

    @property
    def candidates(self) -> tuple[ModelCandidate, ...]:
        return tuple(self._candidates)

    def reset_run(self) -> None:
        """Start a new request at the cheapest eligible route and reset run spend."""
        self._index = 0
        self._consecutive_tool_failures = 0
        self._consecutive_generation_failures = 0
        self.stats = RouterStats()
        self._record_decision("new run")

    async def generate(self, messages: list[Message], tools: list[dict]) -> ModelResponse:
        while True:
            candidate = self.current
            self._assert_budget_for(candidate)
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
            self._record_usage(candidate, response)
            return response

    def _assert_budget_for(self, candidate: ModelCandidate) -> None:
        if not candidate.paid:
            return
        remaining = self.policy.max_spend_usd - self.stats.estimated_spend_usd
        if candidate.request_cost_ceiling_usd > remaining:
            raise RuntimeError(
                f"paid route {candidate.name} exceeds remaining spend budget "
                f"(${remaining:.4f} remaining; ${candidate.request_cost_ceiling_usd:.4f} ceiling)"
            )

    def _record_usage(self, candidate: ModelCandidate, response: ModelResponse) -> None:
        usage = response.usage
        self.stats.input_tokens += usage.input_tokens
        self.stats.output_tokens += usage.output_tokens
        cost = (
            usage.input_tokens * candidate.input_usd_per_million
            + usage.output_tokens * candidate.output_usd_per_million
        ) / 1_000_000
        self.stats.estimated_spend_usd += cost

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
        next_index = self._index + 1
        if next_index >= len(self._candidates):
            return False
        next_candidate = self._candidates[next_index]
        if next_candidate.paid:
            remaining = self.policy.max_spend_usd - self.stats.estimated_spend_usd
            if next_candidate.request_cost_ceiling_usd > remaining:
                return False
        self._index = next_index
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
