import json

from damon.events import DamonEvent
from damon.execution.store import ScriptStore
from damon.state.sqlite import DamonStore
from damon.tools.library import ToolLibrary


def test_event_round_trip_and_sqlite_persistence(tmp_path):
    event = DamonEvent("RunStarted", "run-1", {"ok": True})
    assert DamonEvent.from_json(event.to_json()) == event
    store = DamonStore(tmp_path / "damon.sqlite3")
    store.add_event(event)
    assert store.list_events("run-1")[0]["type"] == "RunStarted"


def test_scripts_are_saved_before_use_with_provenance(tmp_path):
    store = ScriptStore(tmp_path)
    path, metadata = store.save("run-1", 1, "print('hello')", "zen-model")
    assert path.read_text() == "print('hello')"
    assert metadata["source_hash"]
    assert json.loads((path.parent / "metadata.json").read_text())["scripts"][0]["model"] == "zen-model"


def test_promoted_tools_remain_readable_python(tmp_path):
    database = DamonStore(tmp_path / "damon.sqlite3")
    script, _ = ScriptStore(tmp_path).save("run-1", 1, "print('hello')", "model")
    library = ToolLibrary(tmp_path, database.connection)
    tool = library.promote(script, "Say Hello", "prints hello", "system", "run-1", "model")
    assert (tmp_path / "tools/system/say-hello.py").read_text() == "print('hello')"
    assert library.list()[0]["id"] == tool["id"]
