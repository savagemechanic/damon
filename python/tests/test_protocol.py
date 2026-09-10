import pytest

from damon.protocol import FinalAnswer, PythonAction, parse_action


def test_parses_one_python_action():
    assert parse_action("ACTION: python\n\n```python\nprint('ok')\n```") == PythonAction("print('ok')")


def test_parses_finish_action():
    assert parse_action("ACTION: finish\n\nDone.") == FinalAnswer("Done.")


@pytest.mark.parametrize("value", [
    "```python\nprint(1)\n```", "ACTION: python\n```python\nprint(1)",
    "ACTION: python\n```python\na=1\n```\n```python\nb=2\n```",
    "ACTION: finish\n", "ACTION: python\nwords\n```python\na=1\n```",
])
def test_rejects_malformed_or_ambiguous_responses(value):
    with pytest.raises(ValueError):
        parse_action(value)
