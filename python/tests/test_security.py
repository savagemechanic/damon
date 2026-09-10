from damon.security import redact


def test_secret_redaction_is_recursive():
    value = {"api_key": "one", "nested": [{"Authorization": "two"}], "safe": "ok"}
    assert redact(value) == {"api_key": "[REDACTED]", "nested": [{"Authorization": "[REDACTED]"}], "safe": "ok"}
