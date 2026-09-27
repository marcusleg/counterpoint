#!/usr/bin/env python3
"""Writes the round-trip fixtures byte for byte and checks the special characters are there.

This file is plain ASCII: the special characters are built with chr() so that no editor or
tool can silently turn them into other characters. Run from the repository root:
python3 tests/fixtures/generate.py
"""

from pathlib import Path

HERE = Path(__file__).parent

NBSP = chr(0x00A0)  # no-break space
NNBSP = chr(0x202F)  # narrow no-break space
BOM = chr(0xFEFF)  # byte order mark
LINE_SEPARATOR = chr(0x2028)

CRLF = (
    "# Release notes\r\n"
    "\r\n"
    "A paragraph written on Windows.\r\n"
    "It keeps its CRLF line endings.\r\n"
    "\r\n"
    "- first item\r\n"
    "- second item\r\n"
)

NBSP_TEXT = (
    "# Prices\n"
    "\n"
    "The ticket costs 20" + NBSP + "EUR, and the trip takes 3" + NBSP + "hours.\n"
    "French typography puts a narrow space before colons" + NNBSP + ": like this" + NNBSP + "!\n"
    "Line with two trailing spaces for a hard break  \n"
    "next line.\n"
)

FRONT_MATTER = (
    BOM + "---\n"
    "title: \"An example post\"\n"
    "tags: [writing, example]\n"
    "draft: true\n"
    "---\n"
    "\n"
    "<!-- A note to self that must survive saving. -->\n"
    "\n"
    "# An example post\n"
    "\n"
    "Text with a trailing tab\t\n"
    "and a line" + LINE_SEPARATOR + "separator inside a paragraph.\n"
    "\n"
    "<!--\n"
    "A multi-line comment\n"
    "-->\n"
    "\n"
    "```python\n"
    "print(\"code stays as it is\")   \n"
    "```\n"
    "\n"
    "No newline at the end."
)

FIXTURES = {
    "crlf.md": (CRLF, {b"\r\n": 7}),
    "nbsp.md": (NBSP_TEXT, {NBSP.encode(): 2, NNBSP.encode(): 2, b"  \n": 1}),
    "front-matter.md": (
        FRONT_MATTER,
        {BOM.encode(): 1, LINE_SEPARATOR.encode(): 1, b"<!--": 2, b"\t\n": 1, b"\r": 0},
    ),
}

for name, (text, expected_counts) in FIXTURES.items():
    path = HERE / name
    path.write_bytes(text.encode("utf-8"))
    data = open(path, "rb").read()
    for needle, count in expected_counts.items():
        actual = data.count(needle)
        assert actual == count, f"{name}: {needle!r} occurs {actual} times, expected {count}"
    print(f"{name}: {len(data)} bytes OK")
