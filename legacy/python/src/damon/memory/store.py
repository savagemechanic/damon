from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4


@dataclass(slots=True)
class Job:
    id: str
    request: str
    status: str
    result: str | None
    created_at: str
    updated_at: str


class JobStore:
    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.path = path
        self._init()

    def connect(self) -> sqlite3.Connection:
        connection = sqlite3.connect(self.path)
        connection.row_factory = sqlite3.Row
        return connection

    def _init(self) -> None:
        with self.connect() as db:
            db.execute("""
                CREATE TABLE IF NOT EXISTS jobs (
                    id TEXT PRIMARY KEY,
                    request TEXT NOT NULL,
                    status TEXT NOT NULL,
                    result TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                )
            """)

    def create(self, request: str) -> Job:
        now = datetime.now(timezone.utc).isoformat()
        job = Job(str(uuid4()), request, "queued", None, now, now)
        with self.connect() as db:
            db.execute(
                "INSERT INTO jobs VALUES (?, ?, ?, ?, ?, ?)",
                (job.id, job.request, job.status, job.result, job.created_at, job.updated_at),
            )
        return job

    def update(self, job_id: str, *, status: str, result: str | None = None) -> None:
        now = datetime.now(timezone.utc).isoformat()
        with self.connect() as db:
            db.execute(
                "UPDATE jobs SET status = ?, result = ?, updated_at = ? WHERE id = ?",
                (status, result, now, job_id),
            )

    def get(self, job_id: str) -> Job | None:
        with self.connect() as db:
            row = db.execute("SELECT * FROM jobs WHERE id = ?", (job_id,)).fetchone()
        return Job(**dict(row)) if row else None
