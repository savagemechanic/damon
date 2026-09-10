from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import signal
import socketserver
import threading
from typing import Callable

from .service import DamonService


class RequestHandler(socketserver.StreamRequestHandler):
    def send_response(self, response: dict) -> None:
        self.wfile.write(json.dumps(response, separators=(",", ":")).encode() + b"\n")
        self.wfile.flush()

    def handle(self) -> None:
        for raw in self.rfile:
            try:
                request = json.loads(raw)
                self.server.dispatch(request, self.send_response)  # type: ignore[attr-defined]
            except Exception as exc:
                self.send_response({"type": "error", "message": f"{type(exc).__name__}: {exc}"})


class _UnixServer(socketserver.ThreadingUnixStreamServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, path: str, dispatch: Callable[[dict, Callable[[dict], None]], None]):
        self.dispatch = dispatch
        super().__init__(path, RequestHandler)


class DamonServer:
    def __init__(self, socket_path: Path, dispatch: Callable[[dict, Callable[[dict], None]], None] | None = None):
        self.socket_path = socket_path
        self.dispatch = dispatch or self._dispatch
        self._server: _UnixServer | None = None

    @staticmethod
    def _dispatch(request: dict, emit: Callable[[dict], None]) -> None:
        if request.get("type") == "ping":
            emit({"type": "pong", "protocol": 1})
            return
        raise ValueError("unknown request type")

    def start(self) -> None:
        self.socket_path.parent.mkdir(parents=True, exist_ok=True)
        if self.socket_path.exists():
            self.socket_path.unlink()
        self._server = _UnixServer(str(self.socket_path), self.dispatch)
        os.chmod(self.socket_path, 0o600)

    def serve_forever(self) -> None:
        if self._server is None:
            self.start()
        assert self._server
        try:
            self._server.serve_forever()
        finally:
            self.close()

    def close(self) -> None:
        if self._server:
            self._server.server_close()
            self._server = None
        if self.socket_path.exists():
            self.socket_path.unlink()

    def shutdown(self) -> None:
        if self._server:
            self._server.shutdown()


def main() -> None:
    parser = argparse.ArgumentParser(description="Damon local daemon")
    parser.add_argument("--socket", type=Path, default=Path.home() / ".damon/damon.sock")
    parser.add_argument("--home", type=Path, default=Path.home() / ".damon")
    args = parser.parse_args()
    server = DamonServer(args.socket, DamonService(args.home).dispatch)
    server.start()
    signal.signal(signal.SIGTERM, lambda *_: threading.Thread(target=server.shutdown).start())
    server.serve_forever()


if __name__ == "__main__":
    main()
