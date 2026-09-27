# Counterpoint

A Markdown editor for LLM-assisted writing of blog articles and social media posts, written in
Rust with Qt 6 (via [cxx-qt](https://github.com/KDAB/cxx-qt)).

You edit the Markdown source directly, styled in place: headings are larger, bold and italic
text are styled, code uses a fixed-pitch font, and link URLs, front matter and HTML comments are
muted. Files are saved exactly as you edit them, so opening and saving a file without changes
leaves it byte-for-byte identical.

The window follows the desktop's light or dark colour scheme. On GNOME, Counterpoint uses Qt's XDG
desktop portal integration for this (set `QT_QPA_PLATFORMTHEME` yourself to override).

Highlight text, then chat with an LLM about it in one of two modes:

- **Sparring:** the LLM reads the document and critiques it. It cannot change the document.
- **Ghostwriting:** the LLM proposes a change as one or more edits. Review the before/after
  preview and click **Apply** or **Reject**. An applied proposal is a single undo step (Ctrl+Z).

## Requirements

- Rust 1.89 or newer
- Qt 6.7 or newer with QtQuick development files and `qmake6` on `PATH`
  (Fedora: `sudo dnf install qt6-qtbase-devel qt6-qtdeclarative-devel`)
- An OpenAI-compatible chat completions endpoint (for example Ollama, llama.cpp, or a hosted provider)

## Configuration

Open **Tools > Options…** to connect to an OpenAI-compatible endpoint:

- **Base URL**, for example `http://localhost:11434/v1` for Ollama (the default).
- **API key**, optional; sent as a bearer token.
- **Model**, picked from the endpoint's model list (`GET /models`, loaded when the dialog opens
  or on **Refresh**) or typed in.

Settings are stored in `~/.config/counterpoint/settings.json` (or under `$XDG_CONFIG_HOME`),
readable only by you. Changes apply to the next chat message.

## Build and run

```sh
cargo run --release
```

Run the tests with `cargo test`.

To try the editor without a real model, start the mock server in another terminal, run the
editor, and in **Tools > Options…** set the base URL to `http://127.0.0.1:8765/v1` and pick the
model `mock`:

```sh
python3 dev/mock_llm_server.py
cargo run
```

## Keyboard

| Keys             | Action                       |
|------------------|------------------------------|
| Ctrl+O           | Open                         |
| Ctrl+S           | Save                         |
| Ctrl+Shift+S     | Save As                      |
| Ctrl+Q           | Quit                         |
| Ctrl+Z           | Undo (an applied proposal is one step) |
| Ctrl+Shift+Z     | Redo                         |
| Enter            | Send chat message            |
| Shift+Enter      | New line in the chat input   |

## Development checks

The C++ editor core (exact plain-text round-trip, undoable replace, Markdown styling) has a
headless check program. From the repository root:

```sh
mkdir -p target && g++ -std=c++17 -fPIC -Icpp dev/check_editor_core.cpp cpp/plaintext.cpp cpp/markdown_highlighter.cpp \
    $(pkg-config --cflags --libs Qt6Gui) -o target/check_editor_core
git ls-files -z -- '*.md' | QT_QPA_PLATFORM=offscreen xargs -0 target/check_editor_core
```

It prints `OK: 0 failure(s), N file(s) round-tripped` when every check passes. Pass any other
Markdown files as arguments to check that they round-trip unchanged too.

## Known limitations

- Styling is line-based and approximate: it covers common Markdown syntax (ATX headings,
  emphasis, code, links, quotes, lists, front matter, HTML comments) but not every CommonMark
  edge case, and it does not render images or tables.
- Line endings: a file that contains any CRLF (`\r\n`) is saved entirely with CRLF, otherwise
  with LF. Mixed line endings, a lone `\r`, and literal U+2028/U+2029 characters are normalised
  accordingly.
- Responses are not streamed; the chat shows a busy indicator until the full reply arrives.
- Chat history is not persisted and is sent in full with every request.
- An applied proposal stays marked as applied after you undo it in the editor, and cannot be
  applied again; ask for a new proposal instead.

## License

MIT, see [LICENSE](LICENSE).
