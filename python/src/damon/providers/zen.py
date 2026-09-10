from __future__ import annotations

from dataclasses import dataclass
import json
import os
from typing import Iterable
import uuid
from urllib.error import HTTPError
from urllib.request import Request, urlopen


@dataclass(frozen=True)
class ModelInfo:
    id: str
    name: str
    reasoning_efforts: tuple[str, ...] = ()


def parse_sse(lines: Iterable[bytes]) -> Iterable[tuple[str, str]]:
    for raw in lines:
        line = raw.decode("utf-8", errors="replace").strip()
        if not line.startswith("data:"):
            continue
        data = line[5:].strip()
        if data == "[DONE]":
            return
        try:
            payload = json.loads(data)
            delta = payload["choices"][0]["delta"]
        except (ValueError, KeyError, IndexError, TypeError) as exc:
            raise ValueError("malformed Zen stream event") from exc
        reasoning = delta.get("reasoning_content") or delta.get("reasoning")
        content = delta.get("content")
        if reasoning:
            yield "reasoning", reasoning
        if content:
            yield "text", content


class ZenProvider:
    def __init__(self, api_key: str, base_url: str | None = None,
                 session_id: str | None = None, project_id: str | None = None):
        if not api_key:
            raise ValueError("Zen API key is required")
        self._api_key = api_key
        self.base_url = (base_url or os.environ.get("DAMON_ZEN_BASE_URL") or "https://opencode.ai/zen/v1").rstrip("/")
        self.session_id = session_id or str(uuid.uuid4())
        self.project_id = project_id or os.environ.get("OPENCODE_PROJECT_ID")

    def _request(self, path: str, body: dict | None = None):
        data = None if body is None else json.dumps(body).encode()
        request = Request(self.base_url + path, data=data)
        request.add_header("Authorization", f"Bearer {self._api_key}")
        request.add_header("Content-Type", "application/json")
        request.add_header("x-opencode-session", self.session_id)
        request.add_header("x-opencode-request", str(uuid.uuid4()))
        request.add_header("x-opencode-client", "damon")
        request.add_header("User-Agent", "damon/0.1.0")
        if self.project_id:
            request.add_header("x-opencode-project", self.project_id)
        try:
            return urlopen(request, timeout=30)
        except HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")[:2000]
            raise RuntimeError(f"Zen HTTP {exc.code}: {detail}") from exc

    def list_models(self) -> list[ModelInfo]:
        with self._request("/models") as response:
            payload = json.load(response)
        rows = payload.get("data", payload.get("models", []))
        result = []
        for row in rows:
            model_id = row.get("id") or row.get("name")
            if not model_id:
                continue
            efforts = row.get("reasoning_efforts") or row.get("reasoning", {}).get("efforts", [])
            result.append(ModelInfo(model_id, row.get("name", model_id), tuple(efforts)))
        return sorted(result, key=lambda item: item.id)

    @staticmethod
    def request_body(messages: list[dict], model: ModelInfo, thinking_effort: str | None) -> dict:
        body = {"model": model.id, "stream": True, "messages": messages}
        if thinking_effort is not None:
            if thinking_effort not in model.reasoning_efforts:
                raise ValueError("selected model does not support that thinking effort")
            body["reasoning_effort"] = thinking_effort
        return body

    def stream_chat(self, messages: list[dict], model: ModelInfo, thinking_effort: str | None = None):
        with self._request("/chat/completions", self.request_body(messages, model, thinking_effort)) as response:
            yield from parse_sse(response)
