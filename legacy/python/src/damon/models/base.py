from __future__ import annotations

from typing import Protocol

from damon.core.types import Message, ModelResponse


class Model(Protocol):
    async def generate(
        self,
        messages: list[Message],
        tools: list[dict],
    ) -> ModelResponse: ...


class FeedbackAwareModel(Protocol):
    """Optional model capability for deterministic execution feedback."""

    def record_tool_result(self, *, ok: bool) -> None: ...
