from typing import Iterable, Protocol


class Provider(Protocol):
    def stream_chat(self, messages: list[dict], model: str, thinking_effort: str | None = None) -> Iterable[tuple[str, str]]: ...
