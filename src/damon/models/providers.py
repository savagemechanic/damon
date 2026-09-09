from __future__ import annotations

import os

from damon.models.openai_compatible import OpenAICompatibleModel


def opencode_zen(model: str, *, api_key: str | None = None) -> OpenAICompatibleModel:
    key = api_key or os.getenv("OPENCODE_API_KEY") or os.getenv("OPENCODE_ZEN_API_KEY")
    if not key:
        raise ValueError("OpenCode Zen requires OPENCODE_API_KEY or OPENCODE_ZEN_API_KEY")
    return OpenAICompatibleModel(
        model=model,
        endpoint="https://opencode.ai/zen/v1/chat/completions",
        api_key=key,
    )


def openrouter(model: str, *, api_key: str | None = None) -> OpenAICompatibleModel:
    key = api_key or os.getenv("OPENROUTER_API_KEY")
    if not key:
        raise ValueError("OpenRouter requires OPENROUTER_API_KEY")
    return OpenAICompatibleModel(
        model=model,
        endpoint="https://openrouter.ai/api/v1/chat/completions",
        api_key=key,
        headers={"X-Title": "Damon"},
    )


def openai_chat(model: str, *, api_key: str | None = None) -> OpenAICompatibleModel:
    key = api_key or os.getenv("OPENAI_API_KEY")
    if not key:
        raise ValueError("OpenAI requires OPENAI_API_KEY")
    return OpenAICompatibleModel(
        model=model,
        endpoint="https://api.openai.com/v1/chat/completions",
        api_key=key,
    )
