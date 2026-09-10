import json
import io
from unittest.mock import patch

import pytest

from damon.providers.zen import ModelInfo, ZenProvider, parse_sse


class Response:
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return None

    def __iter__(self):
        return iter([b"data: [DONE]\n"])


class JSONResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


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


def test_requests_match_current_opencode_identity_envelope():
    provider = ZenProvider("secret", "https://example.test/v1", session_id="session-1", project_id="project-1")
    with patch("damon.providers.zen.urlopen", return_value=Response()) as send:
        list(provider.stream_chat([], ModelInfo("model-a", "Model A")))

    request = send.call_args.args[0]
    headers = {key.lower(): value for key, value in request.header_items()}
    assert request.full_url == "https://example.test/v1/chat/completions"
    assert headers["authorization"] == "Bearer secret"
    assert headers["x-opencode-session"] == "session-1"
    assert headers["x-opencode-client"] == "damon"
    assert headers["x-opencode-project"] == "project-1"
    assert headers["x-opencode-request"]
    assert headers["user-agent"] == "damon/0.1.0"


def test_model_catalogue_is_enriched_from_current_models_metadata_without_leaking_key():
    zen = JSONResponse(json.dumps({"data": [{"id": "model-a", "object": "model", "owned_by": "opencode"}]}).encode())
    metadata = JSONResponse(json.dumps({"opencode": {"models": {"model-a": {
        "name": "Model A", "reasoning_options": [{"type": "effort", "values": ["low", "high"]}]
    }}}}).encode())
    provider = ZenProvider("secret", "https://zen.test/v1", metadata_url="https://models.test/api.json")
    with patch("damon.providers.zen.urlopen", side_effect=[zen, metadata]) as send:
        assert provider.list_models() == [ModelInfo("model-a", "Model A", ("low", "high"))]

    metadata_headers = {key.lower(): value for key, value in send.call_args_list[1].args[0].header_items()}
    assert "authorization" not in metadata_headers
