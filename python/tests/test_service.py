from damon.ipc.service import DamonService
from damon.providers.zen import ModelInfo


class Provider:
    def __init__(self, key):
        assert key == "test-secret"
        self.responses = iter([
            "ACTION: python\n```python\nprint('service output')\n```",
            "ACTION: finish\nComplete.",
        ])

    def list_models(self):
        return [ModelInfo("test-model", "Test Model", ("low",))]

    def stream_chat(self, messages, model, thinking_effort=None):
        assert thinking_effort == "low"
        yield "text", next(self.responses)


def test_service_config_models_and_streamed_run(tmp_path):
    provider = Provider("test-secret")
    service = DamonService(tmp_path / ".damon", provider_factory=lambda key: provider)
    output = []
    service.dispatch({"type": "configure", "api_key": "test-secret"}, output.append)
    assert output == [{"type": "configured"}]
    assert "test-secret" not in repr(output)

    output.clear()
    service.dispatch({"type": "models"}, output.append)
    assert output[0]["source"] == "live"
    assert output[0]["models"][0]["id"] == "test-model"

    output.clear()
    service.dispatch({
        "type": "run", "message": "inspect", "model": "test-model",
        "thinking_effort": "low", "working_directory": str(tmp_path),
    }, output.append)
    assert output[0]["type"] == "RunStarted"
    assert any(event["type"] == "StdoutDelta" and event["payload"]["delta"] == "service output\n" for event in output)
    assert output[-1]["type"] == "RunFinished"
    assert len(service.store.list_chats()) == 1
    chat_id = service.store.list_chats()[0]["id"]
    assert [message["role"] for message in service.store.chat_messages(chat_id)] == ["user", "assistant"]
    run_id = output[0]["run_id"]
    assert service.store.list_events(run_id)
    assert (tmp_path / ".damon/runs" / run_id / "events.jsonl").exists()
    script_path = next(event["payload"]["path"] for event in output if event["type"] == "ScriptSaved")
    promoted = []
    service.dispatch({"type": "promote", "path": script_path, "name": "service script", "description": "test", "category": "misc", "run_id": run_id, "model": "test-model"}, promoted.append)
    assert promoted[0]["type"] == "tool_saved"
    assert service.tools.list()[0]["name"] == "service script"


def test_service_refuses_run_without_key(tmp_path):
    service = DamonService(tmp_path / ".damon")
    service.catalog.cache_path.parent.mkdir(parents=True, exist_ok=True)
    service.catalog.cache_path.write_text('{"models":[{"id":"m","name":"M"}]}')
    try:
        service.dispatch({"type": "run", "message": "x", "model": "m"}, lambda _: None)
    except RuntimeError as error:
        assert "not configured" in str(error)
    else:
        raise AssertionError("run should require a configured API key")
