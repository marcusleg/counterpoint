# Counterpoint

A Markdown editor for LLM-assisted writing of blog articles and social media posts, written in
Rust with GTK 4 and libadwaita.

You edit the Markdown source directly, styled in place by GtkSourceView: headings, emphasis,
code, links, front matter and HTML comments are highlighted. Files are saved exactly as you edit
them, so opening and saving a file without changes leaves it byte-for-byte identical. The window
follows GNOME's light or dark style.

Highlight text, then chat with an LLM about it in one of two modes:

- **Sparring:** the LLM reads the document and critiques it. It cannot change the document.
- **Ghostwriting:** the LLM proposes a change as one or more edits. Review the before/after
  preview and click **Apply** or **Reject**. An applied change is selected in the editor and can
  be undone from the toast or with Ctrl+Z; it is a single undo step.

The chat pane can be hidden (F9) to write in peace, and collapses over the editor in a narrow
window. **Stop** abandons a request that takes too long and gives you the message back.

Files open from the command line (`counterpoint post.md`), from a file manager, by dragging
them onto the editor, or with **Open** in the header bar. If a file changes on disk while it is
open, saving asks before overwriting those changes.

## Requirements

- Rust 1.92 or newer
- GTK 4.18 or newer, libadwaita 1.8 or newer and GtkSourceView 5.12 or newer, with development
  files (Fedora: `sudo dnf install gtk4-devel libadwaita-devel gtksourceview5-devel`)
- An OpenAI-compatible chat completions endpoint (for example Ollama, llama.cpp, or a hosted provider)

## Configuration

Open **Preferences** in the main menu (☰) to connect to an OpenAI-compatible endpoint:

- **Base URL**, for example `http://localhost:11434/v1` for Ollama (the default).
- **API key**, optional; sent as a bearer token.
- **Model**, picked from the endpoint's model list (`GET /models`, loaded when the dialog opens
  or on refresh) or typed in.

The base URL must be `http://` or `https://` without a query, fragment or user name; secrets
belong in the API key field. The dialog warns when the key would travel over plain HTTP to
another machine. The model list is also refreshed when you leave the URL or key field.

Settings are stored in `~/.config/counterpoint/settings.json` (or under `$XDG_CONFIG_HOME`),
readable only by you. Changes apply to the next chat message.

The folder of the last file you opened or saved and the editor zoom level are remembered in
`~/.local/state/counterpoint/state.json` (or under `$XDG_STATE_HOME`), also readable only by
you.

## Privacy

Every chat message sends the whole document text (front matter and HTML comments included),
the highlighted passage and the conversation so far to the configured endpoint. With a hosted
provider, that is a third party. Nothing else is sent: not the file name, path or any settings.

The model is told to treat the document as the writer's material rather than as instructions,
but a document you did not write can still contain text that steers the model. Read a proposal
before applying it, as you would anyway.

## Build and run

```sh
cargo run --release
```

To try the editor without a real model, start the mock server in another terminal, run the
editor, and in **Preferences** set the base URL to `http://127.0.0.1:8765/v1` and pick a model:

```sh
python3 dev/mock_llm_server.py
cargo run
```

`mock` answers Sparring with plain text and Ghostwriting with a proposal that shouts the first
two long lines. The other models exercise failure paths, for example `mock-stale` (an edit that
no longer matches), `mock-error-500`, `mock-malformed` and `mock-slow` (a five-second reply to
try **Stop**); the script's docstring lists them all.

## Keyboard

| Keys              | Action                                   |
|-------------------|------------------------------------------|
| Ctrl+N            | New document                             |
| Ctrl+O            | Open                                     |
| Ctrl+S            | Save                                     |
| Ctrl+Shift+S      | Save As                                  |
| F9                | Show or hide the chat pane               |
| Ctrl+,            | Preferences                              |
| Ctrl+?            | Keyboard shortcuts                       |
| Ctrl+Q            | Quit                                     |
| Ctrl+Z            | Undo (an applied proposal is one step)   |
| Ctrl+Shift+Z      | Redo                                     |
| Ctrl++, Ctrl+=    | Zoom in the editor text                  |
| Ctrl+-            | Zoom out the editor text                 |
| Ctrl+0            | Reset the editor zoom                    |
| Enter, Ctrl+Enter | Send chat message                        |
| Shift+Enter       | New line in the chat input               |

The zoom controls are in the main menu (☰).

## Development checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
dev/headless.sh cargo test
```

`dev/headless.sh` runs a command against a private GTK Broadway display (`gtk4-broadwayd`, part
of Fedora's `gtk4` package) on a private session bus (`dbus-run-session`, package
`dbus-daemon`), so no window appears on your desktop and nothing touches your settings. It
needs `XDG_RUNTIME_DIR` and `git`, and makes GLib criticals fatal so the tests catch them.

`cargo test` runs the unit tests of the pure modules plus three integration tests:

- `tests/gtk_editor.rs` checks the editor buffer: every Markdown file in the repository and in
  `tests/fixtures/` must survive loading and saving byte for byte, loading must leave nothing to
  undo, an applied proposal must be exactly one undo step, zoom must be per editor, and chat
  markup from a hostile model must render as plain text.
- `tests/gtk_window.rs` drives the real main window: the chat pane and its empty state, the
  primary menu and zoom, the unsaved-changes and overwrite dialogs, opening and saving files
  (CRLF preserved, failures reported), and a chat round trip against a mock endpoint covering
  sparring replies, proposals (apply, undo, stale, reject), HTTP errors and **Stop**.
- `tests/mock_server.rs` pins `dev/mock_llm_server.py` to the proposal format the editor parses.

Without a display the GTK tests print `SKIPPED: no display`. The same checks run in GitHub
Actions (`.github/workflows/ci.yml`) in a Fedora container.

To check that your own files round-trip unchanged without adding them to the repository, list
them, separated by colons, in `COUNTERPOINT_ROUNDTRIP_FILES`:

```sh
COUNTERPOINT_ROUNDTRIP_FILES=post.md:notes.md dev/headless.sh cargo test --test gtk_editor
```

The fixtures are generated by `python3 tests/fixtures/generate.py`, which also verifies their
bytes; CI checks that the committed fixtures match.

## Known limitations

- Styling comes from GtkSourceView's Markdown highlighting: headings are not enlarged, and not
  every CommonMark edge case is covered.
- Line endings: a file that contains any CRLF (`\r\n`) is saved entirely with CRLF, otherwise
  with LF. A leading byte order mark is kept.
- Responses are not streamed; the chat shows a busy indicator until the full reply arrives, for
  up to ten minutes.
- Chat history is not persisted and is sent in full with every request.
- An applied proposal stays marked as applied after you undo it in the editor, and cannot be
  applied again; ask for a new proposal instead.
- Chat replies render a subset of Markdown (no tables or images).
- Enter in the chat input is handed to the input method first, so a composition is committed
  rather than sent; this has not been tested with every input method.
- Save As does not add a `.md` extension automatically; the suggested name `Untitled.md` has
  one.
- Only one document is open at a time; a second file given on the command line is ignored.

## License

MIT, see [LICENSE](LICENSE).
