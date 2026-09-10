from damon.ipc.service import DamonService
from damon.providers.zen import ModelInfo
import threading


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
    messages = []
    service.dispatch({"type": "messages", "chat_id": chat_id}, messages.append)
    assert [message["role"] for message in messages[0]["messages"]] == ["user", "assistant"]
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


def test_configured_key_can_be_cleared_from_daemon_memory(tmp_path):
    service = DamonService(tmp_path / ".damon")
    service.dispatch({"type": "configure", "api_key": "secret"}, lambda _: None)
    output = []
    service.dispatch({"type": "clear_api_key"}, output.append)
    assert service.api_key is None
    assert output == [{"type": "api_key_cleared"}]


def test_raw_jsonl_can_be_disabled_without_disabling_sqlite_audit(tmp_path):
    provider = Provider("test-secret")
    service = DamonService(tmp_path / ".damon", provider_factory=lambda key: provider)
    service.api_key = "test-secret"
    service.catalog.cache_path.parent.mkdir(parents=True, exist_ok=True)
    service.catalog.cache_path.write_text('{"models":[{"id":"test-model","name":"Test Model","reasoning_efforts":["low"]}]}')
    output = []
    service.dispatch({
        "type": "run", "message": "inspect", "model": "test-model", "thinking_effort": "low",
        "working_directory": str(tmp_path), "raw_event_logging": False,
    }, output.append)
    run_id = output[0]["run_id"]
    assert service.store.list_events(run_id)
    assert not (tmp_path / ".damon/runs" / run_id / "events.jsonl").exists()


class SlowProvider:
    def __init__(self, _key):
        pass

    def stream_chat(self, messages, model, thinking_effort=None):
        yield "text", "ACTION: python\n```python\nimport time\ntime.sleep(30)\n```"


def test_service_cancel_terminates_active_execution(tmp_path):
    service = DamonService(tmp_path / ".damon", provider_factory=SlowProvider)
    service.api_key = "secret"
    service.catalog.cache_path.parent.mkdir(parents=True, exist_ok=True)
    service.catalog.cache_path.write_text('{"models":[{"id":"m","name":"M"}]}')
    execution_started = threading.Event()
    output = []

    def emit(event):
        output.append(event)
        if event.get("type") == "ExecutionStarted":
            execution_started.set()

    worker = threading.Thread(target=lambda: service.dispatch({
        "type": "run", "message": "wait", "model": "m", "working_directory": str(tmp_path),
    }, emit))
    worker.start()
    assert execution_started.wait(2)
    cancelled = []
    service.dispatch({"type": "cancel"}, cancelled.append)
    worker.join(3)
    assert not worker.is_alive()
    assert cancelled == [{"type": "cancelled", "count": 1}]
    finished = next(event for event in output if event.get("type") == "ExecutionFinished")
    assert finished["payload"]["cancelled"] is True
