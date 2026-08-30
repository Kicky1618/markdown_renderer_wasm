#!/usr/bin/env python3
"""Tiny same-origin OpenAI-compatible SSE fixture for browser smoke tests."""
import argparse
import json
import mimetypes
import pathlib
import socketserver
from http.server import BaseHTTPRequestHandler

WEBAPP = pathlib.Path(__file__).resolve().parents[2]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        pass

    def headers_out(self, status, content_type, length=None):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Cache-Control", "no-store")
        if length is not None:
            self.send_header("Content-Length", str(length))
        self.end_headers()

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        relative = path.lstrip("/") or "index.html"
        target = (WEBAPP / relative).resolve()
        if target.is_dir():
            target = target / "index.html"
        if not str(target).startswith(str(WEBAPP.resolve())) or not target.is_file():
            self.headers_out(404, "text/plain; charset=utf-8", 9)
            self.wfile.write(b"not found")
            return
        data = target.read_bytes()
        content_type = mimetypes.guess_type(target.name)[0] or "application/octet-stream"
        self.headers_out(200, content_type, len(data))
        self.wfile.write(data)

    def do_POST(self):
        if self.path.split("?", 1)[0] != "/v1/chat/completions":
            self.headers_out(404, "text/plain; charset=utf-8", 9)
            self.wfile.write(b"not found")
            return
        length = int(self.headers.get("content-length", "0"))
        try:
            body = json.loads(self.rfile.read(length))
        except Exception:
            body = None

        messages = body.get("messages", []) if isinstance(body, dict) else []
        system = messages[0].get("content", "") if len(messages) > 0 else ""
        user = messages[1].get("content", "") if len(messages) > 1 else ""
        common = (
            isinstance(body, dict)
            and body.get("stream") is True
            and body.get("model") == "fixture-model"
            and "Streamdown" in system
            and "Never emit JavaScript" in system
        )
        interaction = "Current local UI state" in user
        if interaction:
            valid = (
                common
                and "Use the current state to append one compact recommendation card" in user
                and '"temperature":42' in user
                and "Current generated UI components" in user
                and '"type":"slider"' in user
                and '"id":"temp"' in user
                and '"id":"ask-model"' in user
                and "password" not in user.lower()
                and "api_token" not in user.lower()
            )
            chunks = [
                "\n## Model continuation\n\n",
                ":::llm ui type=state\ntemperature=58\nmode=exact\napi_token=server-must-not-overwrite\n:::\n\n",
                ":::llm ui type=metric id=interaction-result\nlabel=State-aware continuation\n",
                "value={{temperature}}\nunit=°C\n",
                ":::\n",
            ]
        else:
            valid = common and user == "Build the POST smoke dashboard"
            chunks = [
                "# POST LLM smoke\n\n",
                ":::llm ui type=metric id=post-remote\nlabel=Proxy generated\n",
                "value=POST\nunit=SSE\n",
                ":::\n",
            ]
        if not valid:
            payload = json.dumps({"error": "bad request payload"}).encode()
            self.headers_out(400, "application/json", len(payload))
            self.wfile.write(payload)
            return

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        for text in chunks:
            event = "data: " + json.dumps({"choices": [{"delta": {"content": text}}]}) + "\n\n"
            self.wfile.write(event.encode())
            self.wfile.flush()
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()
        self.close_connection = True


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    with Server(("127.0.0.1", args.port), Handler) as httpd:
        httpd.serve_forever()


if __name__ == "__main__":
    main()
