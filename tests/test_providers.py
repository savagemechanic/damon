import json

from damon.core.types import Message
from damon.models.openai_compatible import OpenAICompatibleModel
from damon.models.providers import opencode_zen, openrouter


class FakeResponse:
    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False


def test_openai_compatible_parses_tools_and_usage(monkeypatch):
    payload = {
        "choices": [{
            "message": {
                "content": "",
                "tool_calls": [{
                    "id": "call-1",
                    "function": {"name": "read_file", "arguments": '{"path":"README.md"}'},
                }],
            }
        }],
        "usage": {"prompt_tokens": 100, "completion_tokens": 20},
    }

    class Response(FakeResponse):
        def read(self):
            return json.dumps(payload).encode()

    monkeypatch.setattr("urllib.request.urlopen", lambda request, timeout: Response())
    model = OpenAICompatibleModel("example", "https://example.test/v1/chat/completions", "secret")
    response = model._generate_sync([Message("user", "hi")], [])
    assert response.tool_calls[0].name == "read_file"
    assert response.tool_calls[0].arguments == {"path": "README.md"}
    assert response.usage.input_tokens == 100
    assert response.usage.output_tokens == 20


def test_provider_helpers_use_expected_endpoints():
    zen = opencode_zen("mimo-v2.5-free", api_key="x")
    router = openrouter("openai/gpt-5.6", api_key="x")
    assert zen.endpoint == "https://opencode.ai/zen/v1/chat/completions"
    assert router.endpoint == "https://openrouter.ai/api/v1/chat/completions"
