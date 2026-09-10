from __future__ import annotations

from dataclasses import dataclass
import re


@dataclass(frozen=True)
class PythonAction:
    source: str


@dataclass(frozen=True)
class FinalAnswer:
    text: str


Action = PythonAction | FinalAnswer
_ACTION = re.compile(r"^\s*ACTION:\s*(python|finish)\s*$", re.IGNORECASE | re.MULTILINE)
_PYTHON = re.compile(r"```python\s*\n(?P<source>.*?)\n```", re.IGNORECASE | re.DOTALL)


def parse_action(text: str) -> Action:
    markers = list(_ACTION.finditer(text))
    if len(markers) != 1:
        raise ValueError("response must contain exactly one ACTION marker")
    kind = markers[0].group(1).lower()
    body = text[markers[0].end():].strip()
    if kind == "finish":
        if not body:
            raise ValueError("finish action requires an answer")
        if _PYTHON.search(body):
            raise ValueError("finish action cannot contain Python")
        return FinalAnswer(body)
    blocks = list(_PYTHON.finditer(body))
    if len(blocks) != 1:
        raise ValueError("python action requires exactly one complete Python block")
    remainder = (body[:blocks[0].start()] + body[blocks[0].end():]).strip()
    if remainder:
        raise ValueError("python action contains ambiguous extra content")
    source = blocks[0].group("source")
    if not source.strip():
        raise ValueError("python action cannot be empty")
    return PythonAction(source)
