from __future__ import annotations

import asyncio
import json
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any

from damon.core.types import Message, ModelResponse, ModelUsage, ToolCall


class ProviderError(RuntimeError):
    pass


@dataclass(slots=True)
class OpenAICompatibleModel:
    """Dependency-free adapter for OpenAI-compatible chat/completions providers."""

    model: str
    endpoint: str
    api_key: str
    timeout: float = 120.0
    headers: dict[str, str] = field(default_factory=dict)

    async def generate(self, messages: list[Message], tools: list[dict]) -> ModelResponse:
        return await asyncio.to_thread(self._generate_sync, messages, tools)

    def _generate_sync(self, messages: list[Message], tools: list[dict]) -> ModelResponse:
        normalized_messages: list[dict[str, Any]] = []
        for message in messages:
            item = message.as_dict()
            item.pop("tool_name", None)
            if item.get("tool_calls"):
                calls = []
                for call in item["tool_calls"]:
                    function = dict(call.get("function") or {})
                    arguments = function.get("arguments", {})
                    if not isinstance(arguments, str):
                        function["arguments"] = json.dumps(arguments)
                    calls.append({**call, "function": function})
                item["tool_calls"] = calls
            normalized_messages.append(item)

        payload: dict[str, Any] = {
            "model": self.model,
            "messages": normalized_messages,
            "stream": False,
        }
        if tools:
            payload["tools"] = tools
            payload["tool_choice"] = "auto"
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
            **self.headers,
        }
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps(payload).encode(),
            headers=headers,
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                data = json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
            raise ProviderError(f"provider request failed: {exc}") from exc

        try:
            message = data["choices"][0]["message"]
        except (KeyError, IndexError, TypeError) as exc:
            raise ProviderError("provider response missing choices[0].message") from exc

        calls: list[ToolCall] = []
        for call in message.get("tool_calls") or []:
            function = call.get("function") or {}
            arguments = function.get("arguments", {})
            if isinstance(arguments, str):
                try:
                    arguments = json.loads(arguments)
                except json.JSONDecodeError as exc:
                    raise ProviderError("provider returned invalid tool arguments JSON") from exc
            calls.append(
                ToolCall(
                    id=str(call.get("id", "")),
                    name=str(function.get("name", "")),
                    arguments=arguments or {},
                )
            )

        usage = data.get("usage") or {}
        return ModelResponse(
            content=message.get("content") or "",
            tool_calls=calls,
            usage=ModelUsage(
                input_tokens=int(usage.get("prompt_tokens", 0) or 0),
                output_tokens=int(usage.get("completion_tokens", 0) or 0),
            ),
        )
