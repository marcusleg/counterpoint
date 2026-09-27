#!/usr/bin/env python3
"""Canned OpenAI-compatible endpoint for smoke-testing Counterpoint.

Sparring requests get a plain-text reply. Ghostwriting requests get a proposal that
upper-cases the first two unique lines of the document, so multi-edit proposals
can be tried out without a real model.

Usage: python3 dev/mock_llm_server.py, then set Options… in the main menu to base URL
http://127.0.0.1:8765/v1 and model "mock".
"""

import json
import re
from http.server import BaseHTTPRequestHandler, HTTPServer

PORT = 8765


def document_from(system_prompt: str) -> str:
    match = re.search(r"<document>\n(.*?)\n</document>", system_prompt, re.DOTALL)
    return match.group(1) if match else ""


def ghostwriting_reply(document: str) -> str:
    lines = [line.strip() for line in document.splitlines()]
    candidates = [l for l in lines if len(l) > 10 and document.count(l) == 1][:2]
    if not candidates:
        return "The document is too short for a mock proposal."
    edits = "\n\n".join(
        f"```original\n{line}\n```\n```replacement\n{line.upper()}\n```" for line in candidates
    )
    return f"Mock proposal: shout the first {len(candidates)} line(s).\n\n{edits}"


class Handler(BaseHTTPRequestHandler):
    def send_json(self, payload):
        body = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if not self.path.endswith("/models"):
            self.send_error(404)
            return
        self.send_json({"object": "list", "data": [{"id": "mock", "object": "model"}]})

    def do_POST(self):
        if not self.path.endswith("/chat/completions"):
            self.send_error(404)
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        messages = body["messages"]
        system, last_user = messages[0]["content"], messages[-1]["content"]
        if system.startswith("You are a ghostwriter"):
            content = ghostwriting_reply(document_from(system))
        else:
            content = f"Mock sparring reply to: *{last_user}* ({len(messages)} messages in context)"
        self.send_json({"choices": [{"message": {"role": "assistant", "content": content}}]})


if __name__ == "__main__":
    print(f"Mock LLM listening on http://127.0.0.1:{PORT}/v1")
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
