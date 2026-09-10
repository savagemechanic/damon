from __future__ import annotations

import sqlite3
from pathlib import Path


SCHEMA = """
CREATE TABLE IF NOT EXISTS chats(id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY, chat_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, chat_id TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS scripts(id TEXT PRIMARY KEY, run_id TEXT NOT NULL, turn INTEGER NOT NULL, path TEXT NOT NULL, source_hash TEXT NOT NULL, model TEXT NOT NULL, created_at TEXT NOT NULL, exit_code INTEGER, duration REAL, reusable_status TEXT NOT NULL DEFAULT 'none');
CREATE TABLE IF NOT EXISTS tools(id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL, path TEXT NOT NULL UNIQUE, category TEXT NOT NULL, source_hash TEXT NOT NULL, originating_run TEXT NOT NULL, originating_model TEXT NOT NULL, created_at TEXT NOT NULL, executions INTEGER NOT NULL DEFAULT 0, successes INTEGER NOT NULL DEFAULT 0, failures INTEGER NOT NULL DEFAULT 0, last_used TEXT);
CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, type TEXT NOT NULL, json TEXT NOT NULL, created_at TEXT NOT NULL);
"""


class DamonStore:
    def __init__(self, path: Path):
        path.parent.mkdir(parents=True, exist_ok=True)
        self.connection = sqlite3.connect(path)
        self.connection.row_factory = sqlite3.Row
        self.connection.executescript(SCHEMA)

    def add_event(self, event) -> None:
        self.connection.execute(
            "INSERT INTO events(run_id,type,json,created_at) VALUES(?,?,?,?)",
            (event.run_id, event.type, event.to_json(), event.timestamp),
        )
        self.connection.commit()

    def list_events(self, run_id: str):
        return self.connection.execute("SELECT * FROM events WHERE run_id=? ORDER BY id", (run_id,)).fetchall()
