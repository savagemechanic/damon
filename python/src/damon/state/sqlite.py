from __future__ import annotations

import sqlite3
import threading
from pathlib import Path
from datetime import datetime, timezone
import uuid


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
        self.connection = sqlite3.connect(path, check_same_thread=False)
        self.connection.row_factory = sqlite3.Row
        self.connection.executescript(SCHEMA)
        self.lock = threading.RLock()

    def add_event(self, event) -> None:
        with self.lock:
            self.connection.execute(
                "INSERT INTO events(run_id,type,json,created_at) VALUES(?,?,?,?)",
                (event.run_id, event.type, event.to_json(), event.timestamp),
            )
            self.connection.commit()

    def list_events(self, run_id: str):
        with self.lock:
            return self.connection.execute("SELECT * FROM events WHERE run_id=? ORDER BY id", (run_id,)).fetchall()

    def create_chat(self, title: str) -> str:
        chat_id = str(uuid.uuid4())
        now = datetime.now(timezone.utc).isoformat()
        with self.lock:
            self.connection.execute("INSERT INTO chats(id,title,created_at) VALUES(?,?,?)", (chat_id, title[:120], now))
            self.connection.commit()
        return chat_id

    def add_message(self, chat_id: str, role: str, content: str) -> None:
        now = datetime.now(timezone.utc).isoformat()
        with self.lock:
            self.connection.execute("INSERT INTO messages(chat_id,role,content,created_at) VALUES(?,?,?,?)", (chat_id, role, content, now))
            self.connection.commit()

    def start_run(self, run_id: str, chat_id: str) -> None:
        now = datetime.now(timezone.utc).isoformat()
        with self.lock:
            self.connection.execute("INSERT INTO runs(id,chat_id,status,created_at) VALUES(?,?,?,?)", (run_id, chat_id, "running", now))
            self.connection.commit()

    def finish_run(self, run_id: str, status: str) -> None:
        with self.lock:
            self.connection.execute("UPDATE runs SET status=? WHERE id=?", (status, run_id))
            self.connection.commit()

    def list_chats(self) -> list[dict]:
        with self.lock:
            rows = self.connection.execute("SELECT * FROM chats ORDER BY created_at DESC").fetchall()
        return [dict(row) for row in rows]

    def chat_messages(self, chat_id: str) -> list[dict]:
        with self.lock:
            rows = self.connection.execute("SELECT role,content,created_at FROM messages WHERE chat_id=? ORDER BY id", (chat_id,)).fetchall()
        return [dict(row) for row in rows]

    def add_script(self, run_id: str, payload: dict) -> None:
        with self.lock:
            self.connection.execute(
                "INSERT INTO scripts(id,run_id,turn,path,source_hash,model,created_at,reusable_status) VALUES(?,?,?,?,?,?,?,?)",
                (payload["script_id"], run_id, payload["turn"], payload["path"], payload["source_hash"],
                 payload["model"], payload["created_at"], payload["reusable_status"]),
            )
            self.connection.commit()

    def finish_script(self, script_id: str, exit_code: int | None, duration: float) -> None:
        with self.lock:
            self.connection.execute("UPDATE scripts SET exit_code=?,duration=? WHERE id=?", (exit_code, duration, script_id))
            self.connection.commit()

    def list_scripts(self, run_id: str) -> list[dict]:
        with self.lock:
            rows = self.connection.execute("SELECT * FROM scripts WHERE run_id=? ORDER BY turn", (run_id,)).fetchall()
        return [dict(row) for row in rows]
