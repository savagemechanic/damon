import json
from pathlib import Path
import socket
import tempfile
import threading

from damon.ipc.server import DamonServer


def test_unix_socket_ping_and_error_recovery():
    with tempfile.TemporaryDirectory(dir="/tmp") as directory:
        path = Path(directory) / "damon.sock"
        server = DamonServer(path)
        server.start()
        thread = threading.Thread(target=server._server.serve_forever, daemon=True)
        thread.start()
        try:
            with socket.socket(socket.AF_UNIX) as client:
                client.connect(str(path))
                stream = client.makefile("rwb")
                stream.write(b'{"type":"unknown"}\n{"type":"ping"}\n')
                stream.flush()
                assert json.loads(stream.readline())["type"] == "error"
                assert json.loads(stream.readline()) == {"type": "pong", "protocol": 1}
        finally:
            server.shutdown()
            server.close()
