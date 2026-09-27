#!/usr/bin/env python3
"""Canned OpenAI-compatible endpoint for smoke-testing Counterpoint.

The `model` field of the request selects the behaviour; `GET /models` advertises all of them:

  mock              Sparring requests get a plain-text reply. Ghostwriting requests get a
                    proposal that upper-cases the first two unique long lines of the document,
                    so multi-edit proposals can be tried out without a real model.
  mock-delete       One edit whose replacement is empty (deletes the first long line).
  mock-stale        One edit whose `original` does not occur in the document.
  mock-ambiguous    One edit whose `original` is a short word that occurs more than once.
  mock-overlap      Two edits whose originals overlap in the document.
  mock-noedit       A ghostwriting reply without any edit blocks.
  mock-malformed    HTTP 200 with a body that is not JSON.
  mock-null-content HTTP 200 with a JSON reply whose message content is null.
  mock-error-401    HTTP 401 with a short body.
  mock-error-500    HTTP 500 with a short body.
  mock-slow         Sleeps 5 seconds, then answers like `mock`.

The mock-* proposal variants answer with edit blocks whichever mode the request came from,
since the document is in the system prompt in both. Any other model name is answered with 404.

Requests without a Content-Length header are refused with 411, and bodies over 16 MiB with 413.

Usage: python3 dev/mock_llm_server.py, then set Preferences in the main menu to base URL
http://127.0.0.1:8765/v1 and pick a model. COUNTERPOINT_MOCK_PORT overrides the port. The server
only ever binds to 127.0.0.1.
"""

import json
import os
import re
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(os.environ.get("COUNTERPOINT_MOCK_PORT", "8765"))
MAX_BODY = 16 * 1024 * 1024
SLOW_SECONDS = 5

MODELS = [
    "mock",
    "mock-delete",
    "mock-stale",
    "mock-ambiguous",
    "mock-overlap",
    "mock-noedit",
    "mock-malformed",
    "mock-null-content",
    "mock-error-401",
    "mock-error-500",
    "mock-slow",
]


def document_from(system_prompt: str) -> str:
    match = re.search(r"<document>\n(.*?)\n</document>", system_prompt, re.DOTALL)
    return match.group(1) if match else ""


def unique_long_lines(document: str) -> list:
    """Stripped lines longer than ten characters that occur exactly once in the document."""
    lines = [line.strip() for line in document.splitlines()]
    return [line for line in lines if len(line) > 10 and document.count(line) == 1]


def edit_block(original: str, replacement: str) -> str:
    return f"```original\n{original}\n```\n```replacement\n{replacement}\n```"


def proposal(explanation: str, edits: list) -> str:
    return f"{explanation}\n\n" + "\n\n".join(edit_block(o, r) for o, r in edits)


def shout_reply(document: str) -> str:
    candidates = unique_long_lines(document)[:2]
    if not candidates:
        return "The document is too short for a mock proposal."
    return proposal(
        f"Mock proposal: shout the first {len(candidates)} line(s).",
        [(line, line.upper()) for line in candidates],
    )


def delete_reply(document: str) -> str:
    candidates = unique_long_lines(document)[:1]
    if not candidates:
        return "The document is too short for a mock proposal."
    return proposal("Mock proposal: delete the first long line.", [(candidates[0], "")])


def stale_reply(_document: str) -> str:
    return proposal(
        "Mock proposal whose original text is not in the document.",
        [("THIS TEXT IS NOT IN THE DOCUMENT", "This replaces nothing.")],
    )


def ambiguous_reply(document: str) -> str:
    # The first word that occurs at least twice as a substring, since that is how the editor
    # looks the original up; "the" as a fallback for documents where no word repeats.
    word = next((w for w in re.findall(r"[A-Za-z]+", document) if document.count(w) >= 2), "the")
    return proposal(
        f"Mock proposal whose original text ({word!r}) occurs more than once.",
        [(word, f"**{word}**")],
    )


def overlap_reply(document: str) -> str:
    candidates = unique_long_lines(document)[:1]
    if not candidates:
        return "The document is too short for a mock proposal."
    first = candidates[0]
    start = document.index(first)
    middle = start + len(first) // 2
    # The last half of the line through the end of the following non-blank line (Markdown
    # paragraphs are usually separated by a blank one), taken verbatim from the document.
    end = start + len(first)
    while True:
        newline = document.find("\n", end)
        if newline == -1:
            end = len(document)
            break
        next_newline = document.find("\n", newline + 1)
        end = len(document) if next_newline == -1 else next_newline
        if document[newline + 1 : end].strip() or end == len(document):
            break
    second = document[middle:end]
    return proposal(
        "Mock proposal with two overlapping edits.",
        [(first, first.upper()), (second, second.upper())],
    )


def noedit_reply(_document: str) -> str:
    return "Mock ghostwriting reply: no change is warranted, so there are no edit blocks."


PROPOSALS = {
    "mock-delete": delete_reply,
    "mock-stale": stale_reply,
    "mock-ambiguous": ambiguous_reply,
    "mock-overlap": overlap_reply,
    "mock-noedit": noedit_reply,
}


class Handler(BaseHTTPRequestHandler):
    error_message_format = "%(code)d %(message)s\n"
    error_content_type = "text/plain"

    def send_json(self, payload):
        self.send_raw(json.dumps(payload).encode(), "application/json")

    def send_raw(self, body: bytes, content_type: str):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if not self.path.endswith("/models"):
            self.send_error(404)
            return
        self.send_json({"object": "list", "data": [{"id": m, "object": "model"} for m in MODELS]})

    def read_body(self):
        """Returns the request body, or None after sending an error response."""
        length = self.headers.get("Content-Length")
        if length is None:
            self.send_error(411)
            return None
        try:
            length = int(length)
        except ValueError:
            self.send_error(400, "Content-Length is not a number")
            return None
        if length > MAX_BODY:
            self.close_connection = True
            self.send_error(413, "Request body exceeds 16 MiB")
            return None
        return self.rfile.read(length)

    def do_POST(self):
        if not self.path.endswith("/chat/completions"):
            self.send_error(404)
            return
        raw = self.read_body()
        if raw is None:
            return
        try:
            body = json.loads(raw)
            model = body["model"]
            messages = body["messages"]
            system, last_user = messages[0]["content"], messages[-1]["content"]
        except (ValueError, KeyError, IndexError, TypeError):
            self.send_error(400, "Request body is not a chat completion")
            return

        if model == "mock-error-401":
            self.send_error(401, "mock: invalid API key")
            return
        if model == "mock-error-500":
            self.send_error(500, "mock: internal server error")
            return
        if model == "mock-malformed":
            self.send_raw(b"not json", "text/plain")
            return
        if model == "mock-null-content":
            self.send_json({"choices": [{"message": {"role": "assistant", "content": None}}]})
            return
        if model == "mock-slow":
            time.sleep(SLOW_SECONDS)
            model = "mock"

        document = document_from(system)
        if model == "mock":
            if system.startswith("You are a ghostwriter"):
                content = shout_reply(document)
            else:
                content = (
                    f"Mock sparring reply to: *{last_user}* ({len(messages)} messages in context)"
                )
        elif model in PROPOSALS:
            content = PROPOSALS[model](document)
        else:
            self.send_error(404, f"mock: unknown model {model!r}")
            return
        self.send_json({"choices": [{"message": {"role": "assistant", "content": content}}]})


if __name__ == "__main__":
    print(f"Mock LLM listening on http://127.0.0.1:{PORT}/v1", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
