from __future__ import annotations

from dataclasses import asdict, dataclass
import json
from pathlib import Path
import sys


DEFAULT_PROMPT = """You are the reasoning engine inside Damon. Interact with the local computer by returning either ACTION: python with exactly one fenced Python program, or ACTION: finish with the final answer. Damon executes generated code and returns real results. Inspect before modifying. Prefer minimal deterministic standard-library Python. Never claim an action succeeded without evidence. Finish only when sufficient evidence exists."""


@dataclass
class Settings:
    model: str = ""
    thinking_effort: str | None = None
    system_prompt: str = DEFAULT_PROMPT
    python_executable: str = "python3"
    tools_directory: str = ""
    max_turns: int = 8
    execution_timeout: float = 60.0
    reasoning_visibility: bool = True
    raw_event_logging: bool = True
    save_generated_scripts: bool = True

    @classmethod
    def load(cls, path: Path) -> "Settings":
        if not path.exists():
            return cls()
        data = json.loads(path.read_text())
        data.pop("api_key", None)
        return cls(**data)

    def save(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(asdict(self), indent=2) + "\n")
