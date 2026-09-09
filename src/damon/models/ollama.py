from __future__ import annotations

import asyncio
import json
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any

from damon.core.types import Message, ModelResponse, ToolCall


class OllamaError(RuntimeError):
    pass


@dataclass(slots=True)
class OllamaModel:
    model: str = "qwen3:8b"
    base_url: str = "http://127.0.0.1:11434"
    timeout: float = 120.0

    async def generate(self, messages: list[Message], tools: list[dict]) -> ModelResponse:
        return await asyncio.to_thread(self._generate_sync, messages, tools)

    def _generate_sync(self, messages: list[Message], tools: list[dict]) -> ModelResponse:
        payload: dict[str, Any] = {
            "model": self.model,
            "messages": [m.as_dict() for m in messages],
            "stream": False,
        }
        if tools:
            payload["tools"] = tools
        request = urllib.request.Request(
            f"{self.base_url.rstrip('/')}/api/chat",
            data=json.dumps(payload).encode(),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                data = json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
            raise OllamaError(f"Ollama request failed: {exc}") from exc

        message = data.get("message", {})
        calls: list[ToolCall] = []
        for index, call in enumerate(message.get("tool_calls", []) or []):
            function = call.get("function", {})
            arguments = function.get("arguments", {})
            if isinstance(arguments, str):
                arguments = json.loads(arguments or "{}")
            calls.append(
                ToolCall(
                    id=str(call.get("id") or f"call-{index}"),
                    name=str(function.get("name", "")),
                    arguments=dict(arguments),
                )
            )
        return ModelResponse(content=str(message.get("content", "")), tool_calls=calls)
