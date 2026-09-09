from __future__ import annotations

from typing import Protocol

from damon.core.types import Message, ModelResponse


class Model(Protocol):
    async def generate(
        self,
        messages: list[Message],
        tools: list[dict],
    ) -> ModelResponse: ...
