from __future__ import annotations

import argparse
import json
from pathlib import Path
import signal
import socketserver
import threading
from typing import Callable


class RequestHandler(socketserver.StreamRequestHandler):
    def handle(self) -> None:
        for raw in self.rfile:
            try:
                request = json.loads(raw)
                response = self.server.dispatch(request)  # type: ignore[attr-defined]
            except Exception as exc:
                response = {"type": "error", "message": f"{type(exc).__name__}: {exc}"}
            self.wfile.write(json.dumps(response, separators=(",", ":")).encode() + b"\n")
            self.wfile.flush()


class _UnixServer(socketserver.ThreadingUnixStreamServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, path: str, dispatch: Callable[[dict], dict]):
        self.dispatch = dispatch
        super().__init__(path, RequestHandler)


class DamonServer:
    def __init__(self, socket_path: Path, dispatch: Callable[[dict], dict] | None = None):
        self.socket_path = socket_path
        self.dispatch = dispatch or self._dispatch
        self._server: _UnixServer | None = None

    @staticmethod
    def _dispatch(request: dict) -> dict:
        if request.get("type") == "ping":
            return {"type": "pong", "protocol": 1}
        raise ValueError("unknown request type")

    def start(self) -> None:
        self.socket_path.parent.mkdir(parents=True, exist_ok=True)
        if self.socket_path.exists():
            self.socket_path.unlink()
        self._server = _UnixServer(str(self.socket_path), self.dispatch)

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


def main() -> None:
    parser = argparse.ArgumentParser(description="Damon local daemon")
    parser.add_argument("--socket", type=Path, default=Path.home() / ".damon/damon.sock")
    args = parser.parse_args()
    server = DamonServer(args.socket)
    server.start()
    signal.signal(signal.SIGTERM, lambda *_: threading.Thread(target=server.close).start())
    server.serve_forever()


if __name__ == "__main__":
    main()
