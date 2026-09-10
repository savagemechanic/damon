#!/usr/bin/env python3
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        return

    def do_GET(self):
        if self.path == "/v1/models":
            self._json({"data": [{"id": "mock-model", "object": "model", "owned_by": "opencode"}]})
        elif self.path == "/models-metadata":
            self._json({"opencode": {"models": {"mock-model": {"name": "Mock Model", "reasoning_options": [{"type": "effort", "values": ["low"]}]}}}})
        else:
            self.send_error(404)

    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", "0"))))
        observed = any("EXECUTION RESULT" in message.get("content", "") for message in body["messages"])
        text = "ACTION: finish\nPackaged conversation complete." if observed else (
            "ACTION: python\n```python\nfrom pathlib import Path\n"
            "Path('damon-smoke-output.txt').write_text('packaged execution')\n"
            "print('packaged execution')\n```"
        )
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        event = {"choices": [{"delta": {"content": text}}]}
        self.wfile.write(("data: " + json.dumps(event) + "\n\ndata: [DONE]\n\n").encode())
        self.wfile.flush()

    def _json(self, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port-file", type=Path, required=True)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    args.port_file.write_text(str(server.server_port))
    server.serve_forever()
