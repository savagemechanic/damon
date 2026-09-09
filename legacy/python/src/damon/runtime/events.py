from __future__ import annotations

import asyncio
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any


@dataclass(slots=True)
class Event:
    type: str
    data: Any
    created_at: str

    @classmethod
    def make(cls, type: str, data: Any) -> "Event":
        return cls(type=type, data=data, created_at=datetime.now(timezone.utc).isoformat())


class EventBus:
    def __init__(self) -> None:
        self._queue: asyncio.Queue[Event] = asyncio.Queue()

    async def emit(self, event: Event) -> None:
        await self._queue.put(event)

    async def next(self) -> Event:
        return await self._queue.get()
