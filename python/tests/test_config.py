import json

from damon.config.settings import Settings


def test_config_round_trip_never_persists_api_key(tmp_path):
    path = tmp_path / "config.json"
    path.write_text(json.dumps({"model": "m", "api_key": "secret"}))
    settings = Settings.load(path)
    settings.save(path)
    assert settings.model == "m"
    assert "secret" not in path.read_text() and "api_key" not in path.read_text()
