import json

import pytest

from damon.providers.zen import ModelInfo, ZenProvider, parse_sse


def test_request_serialization_is_model_aware():
    model = ModelInfo("model-a", "Model A", ("low", "high"))
    body = ZenProvider.request_body([{"role": "user", "content": "hi"}], model, "high")
    assert body == {"model": "model-a", "stream": True, "messages": [{"role": "user", "content": "hi"}], "reasoning_effort": "high"}


def test_unsupported_thinking_option_is_not_sent():
    with pytest.raises(ValueError):
        ZenProvider.request_body([], ModelInfo("plain", "Plain"), "low")
    assert "reasoning_effort" not in ZenProvider.request_body([], ModelInfo("plain", "Plain"), None)


def test_sse_parser_separates_reasoning_and_text():
    lines = [
        b'data: {"choices":[{"delta":{"reasoning_content":"think"}}]}\n',
        b'data: {"choices":[{"delta":{"content":"answer"}}]}\n', b'data: [DONE]\n',
    ]
    assert list(parse_sse(lines)) == [("reasoning", "think"), ("text", "answer")]


def test_sse_parser_rejects_bad_json():
    with pytest.raises(ValueError):
        list(parse_sse([b"data: nope\n"]))
