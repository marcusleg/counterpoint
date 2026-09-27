# Migrate Counterpoint from Qt/QML to GTK4 + libadwaita

Status: approved design, 2026-09-27.

## Goal

Replace the Qt 6 + QML user interface (cxx-qt 0.10 plus a small C++ layer) with GTK4, libadwaita
and GtkSourceView 5 through the gtk-rs bindings, keeping every existing feature. The target
desktop is GNOME on Fedora (Wayland). Qt is removed entirely.

## Decisions

| Topic | Decision |
|---|---|
| Menus | `AdwHeaderBar` with a primary (☰) menu only; no menu bar, no header-bar buttons for file actions |
| Application ID | `de.marcusleg.Counterpoint`, set in code only; no `.desktop` file, metainfo or Flatpak for now |
| Minimum libraries | GTK 4.18, libadwaita 1.8 (for `AdwShortcutsDialog`), GtkSourceView 5.12 |
| Crates | `gtk4 0.11` (feature `v4_18`), `libadwaita 0.9` (`v1_8`), `sourceview5 0.11` (`v5_12`), `pulldown-cmark 0.13` (default features off) |
| Rust | `rust-version = "1.92"` (required by gtk4 0.11) |
| UI code style | Widgets built in plain Rust inside small structs; no GObject subclassing, no `.ui`/Blueprint files, no `build.rs` |
| Chat Markdown | pulldown-cmark → Pango markup shown in selectable, wrapping `GtkLabel`s |
| Threading | `gio::spawn_blocking` for blocking work, awaited in `glib::spawn_future_local` on the main loop |

## Architecture

The crate becomes a library plus a binary. `src/main.rs` only calls `counterpoint::ui::run()`;
`src/lib.rs` declares the modules. The split lets a `harness = false` integration test drive GTK
on its main thread (GTK must run on the thread that initialised it; the default test harness runs
each test on its own thread).

### Pure modules (no GTK, unit-tested)

| Module | Change |
|---|---|
| `config.rs` | `require_model` error now points to "Options… in the main menu" |
| `llm.rs`, `prompt.rs`, `proposal.rs` | unchanged |
| `chat.rs` | `entries()` becomes public for all builds; `entries_json()` and its serialization test are removed, along with the `Serialize` derives that only served QML |
| `document.rs` | adds `from_disk(text) -> (String, bool)`: replaces every `\r\n` with `\n` and reports whether any was found; adds `to_disk(text, crlf) -> String`: replaces any `\r\n` (e.g. pasted) with `\n`, then converts `\n` to `\r\n` if `crlf` |
| `text_diff.rs` (new) | `minimal_edit(old, new) -> Option<Span>` where `Span { start, end, replacement }`; `start`/`end` are offsets in Unicode scalar values (chars) into `old`; returns `None` when equal |
| `chat_markup.rs` (new) | `to_pango(markdown) -> String`, converts Markdown to Pango markup with its own escaping |
| `model_requests.rs` (new) | `ModelRequests` tracks the last requested (base URL, API key) pair and a generation counter: `begin(base_url, api_key, force) -> Option<Ticket>` (None when unchanged and not forced) and `is_current(&Ticket)` |

### UI modules (`src/ui/`)

- `mod.rs`: builds the `adw::Application` (`de.marcusleg.Counterpoint`), registers actions and
  accelerators, opens the window.
- `window.rs`: `MainWindow`. Layout, file actions, title and modified marker, the unsaved-changes
  guard, error alerts, and the glue between editor and chat.
- `editor.rs`: `EditorView` wrapping `sourceview5::View` and `sourceview5::Buffer`.
- `chat_pane.rs`: `ChatPane`. Selection chip, message list, proposal cards, input, busy state.
- `options_dialog.rs`: `OptionsDialog`.

State shared between callbacks lives in `Rc<RefCell<…>>` (e.g. the `Conversation`, the
`ModelRequests`, the current file path and CRLF flag). Borrows are never held across an `await`
or across a call that can re-enter a signal handler.

### Removed

`build.rs`, `cpp/`, `qml/`, `src/bridge/`, `dev/check_editor_core.cpp`, the GNOME portal-theme
selection in `main.rs` and its test, and the `cxx`, `cxx-qt`, `cxx-qt-lib` and `cxx-qt-build`
dependencies. `reqwest`, `serde` and `serde_json` stay; `mockito` and `tempfile` stay as dev
dependencies.

## Window

`AdwApplicationWindow` (default size 1400×850) → `AdwToolbarView` with an `AdwHeaderBar` on top
→ horizontal `GtkPaned`:

- Start: the editor in a `GtkScrolledWindow`. Word-char wrapping, 24 px left/right and 16 px
  top/bottom margins, the default (proportional) font, markdown language.
- End: the chat pane, default width about 440 px, resizable; both children keep a minimum width
  (editor 300 px, chat 280 px).

Header bar:

- Title: `AdwWindowTitle` with the file name ("Untitled" for a new document), prefixed with
  "• " while modified, and the file's folder as subtitle.
- End: an `AdwToggleGroup` with "Sparring" and "Ghostwriting" toggles (tooltips: "The LLM can
  read the document but not change it" / "The LLM can propose changes that you apply or
  reject"), then a `GtkMenuButton` with the primary menu.

The window title is "• post.md — Counterpoint" (marker only while modified).

Primary menu, three sections, accelerators shown next to each entry:

1. Open… · Save · Save As…
2. Options… · Keyboard Shortcuts
3. Quit

| Action | Accelerator |
|---|---|
| `win.open` | Ctrl+O |
| `win.save` | Ctrl+S |
| `win.save-as` | Ctrl+Shift+S |
| `win.options` | Ctrl+, |
| `app.shortcuts` | Ctrl+? |
| `app.quit` | Ctrl+Q |

Undo and redo are GtkSourceView's own (Ctrl+Z, Ctrl+Shift+Z). The Keyboard Shortcuts entry opens
an `AdwShortcutsDialog` built in code listing the shortcuts above plus undo/redo and the chat
input keys.

## Editor

- Holds the exact Markdown source; styling comes from GtkSourceView's `markdown` language and a
  style scheme that follows `adw::StyleManager`'s dark setting (`Adwaita` / `Adwaita-dark`),
  updated on `notify::dark`.
- `load(text)`: sets the text between `begin_irreversible_action` and `end_irreversible_action`,
  calls `set_modified(false)` and places the cursor at the start, so there is nothing to undo.
- `text()`: the exact buffer contents (`buffer.text(start, end, true)`).
- `selection_text()`: the selected text, or an empty string.
- `apply_markdown(new)`: `text_diff::minimal_edit(current, new)`; if `Some(span)`, then
  `begin_user_action` → delete `[start, end)` → insert `replacement` at `start` →
  `end_user_action`. One undo step; offsets outside the span are unchanged.
- Signals: `modified-changed` updates the title; `notify::has-selection` and `mark-set` (insert or
  selection-bound mark) update the selection chip.

## File handling

- Open: `GtkFileDialog` with filters "Markdown files" (`*.md`, `*.markdown`) and "All files".
  Reads with `document::read_file`, then `document::from_disk`; stores the path and CRLF flag,
  then `EditorView::load`.
- Save: without a path, runs Save As. Otherwise `document::to_disk(text, crlf)` →
  `document::write_file` → `set_modified(false)`.
- Save As: `GtkFileDialog::save` with the same filters and initial name "Untitled.md"; on success
  the path and title update. A new document keeps LF.
- Unsaved-changes guard: Open, the window's `close-request` and Quit share one helper,
  `confirm_discard(then)`. When the buffer is modified it shows an `AdwAlertDialog` ("Save
  changes?") with responses Cancel (close response, so Escape cancels), Discard (destructive) and Save
  (suggested and default, so Enter saves). Save runs Save (or Save As) and continues only if the save succeeded; Discard
  continues; Cancel does nothing. `close-request` returns `Propagation::Stop` while the dialog is
  pending and closes the window once the user saves or discards. While the alert (or a save it started) is pending, further close, Quit or Open requests are ignored, so only one alert is ever shown. Quit calls `window.close()`.
- File errors appear in an `AdwAlertDialog` titled "Error" with an OK response.

## Chat pane

A vertical box with 12 px margins, top to bottom:

1. Message list: `GtkScrolledWindow` → `GtkListBox` (selection mode none,
   `boxed-list-separate` style). The pane keeps a clone of each rendered `Entry`. `render()`
   compares with `conversation.entries()`: if the conversation is shorter, it clears all rows;
   rows whose entry changed are replaced in place; new entries are appended and the list scrolls
   to the bottom. Applying an older proposal therefore does not move the list.
2. Rows (all text selectable and wrapping, word-char):
   - User: plain text on an accent-tinted card.
   - Assistant: `GtkLabel` with markup from `chat_markup::to_pango`; links open with the default
     handler.
   - Error: plain text with the `error` style class on a tinted card.
   - Proposal card: "Proposed change" heading; the explanation as markup (hidden when empty);
     for each edit, "Edit n of m" (only when more than one), the original (struck through, red
     tint) and the replacement (green tint, or "(delete)" when empty); Apply (suggested) and
     Reject while pending, replaced by "✓ Applied" or "✗ Rejected" afterwards.
3. Selection chip, directly above the input it applies to (dimmed, ellipsized, single line):
   `Selection: “…”` with runs of whitespace collapsed to a space, or
   "No selection — whole document".
4. Input: `GtkTextView` (word-char wrap) in a scrolled window between 64 and about 160 px high,
   with a wrapping placeholder hint ("Ask about the text…" / "Ask for a change…", plus "(Enter to
   send, Shift+Enter for a new line)"). An `EventControllerKey` in the capture phase: Enter or
   Ctrl+Enter sends; Shift+Enter falls through to the text view, which inserts `\n`.
5. Bottom row: a "New conversation" button, an `AdwSpinner` and "Waiting for the LLM…" while
   busy, and a Send button, disabled while busy or when the input is blank.

Sending: ignored if the trimmed input is empty or the conversation is busy.
`prompt::build_messages(mode, document, selection, history, input)` →
`conversation.begin_request(mode, input)` → the input is cleared and the pane re-rendered →
`gio::spawn_blocking(Config::load + llm::complete)` awaited inside `glib::spawn_future_local` →
`conversation.finish_request(ticket, result)`; if it returns true, `render()`. A panic in the worker
becomes `Err("The request failed unexpectedly.")`. New conversation calls `conversation.reset()`
and `render()`; the generation counter drops replies that arrive later.

Apply: `conversation.apply_proposal(index, &editor.text())`. On `Ok(markdown)` the window calls
`editor.apply_markdown(&markdown)`; on `Err` the conversation has already added an error entry and
the proposal stays pending. `render()` in both cases. Reject: `conversation.reject_proposal(index)`
and `render()`.

### Chat Markdown subset

`chat_markup::to_pango` handles paragraphs, headings (bold, larger for levels 1–3), emphasis
(`<i>`), strong (`<b>`), strikethrough (`<s>`), inline code and fenced/indented code blocks
(`<tt>`), bullet and ordered lists (with "• " / "n. " prefixes and indentation for nesting),
block quotes (indented, dimmed), links whose destination starts with `http://`, `https://` or
`mailto:` (case-insensitive, ASCII) as `<a href="…">` with the URL escaped, hard and soft
breaks, and horizontal rules. A link with any other destination renders its text only, with no
`<a>` tag. Raw HTML, images (alt text), and anything else appear as escaped text. `&`, `<`, `>`,
`'` and `"` are always escaped. Output always has balanced tags.

## Options dialog

`AdwDialog` titled "Options", about 560 px wide:

- Header bar with Cancel (start) and Save (end, suggested). Save reads "Overwrite" when the
  settings file could not be read.
- `AdwPreferencesGroup`:
  - Base URL: `AdwEntryRow` (placeholder via title "Base URL (OpenAI-compatible)").
  - API key: `AdwPasswordEntryRow` (built-in show/hide toggle), title "API key (optional)".
  - Model: `AdwEntryRow` with two suffixes: a `GtkMenuButton` whose popover lists the fetched
    models (clicking one fills the entry and closes the popover; insensitive when the list is
    empty), and a Refresh button (insensitive while loading).
- Status: an `AdwSpinner` with status text ("Loading models…", "1 model available.",
  "n models available.", "The endpoint lists no models.", or the fetch error).
- Load error: the `Config::load` error in its own label with the `error` style class, hidden when
  there is none.
- Footer: "Stored in <path> (readable only by you)." (dimmed).

Behaviour:

- On open: reload the config into the fields (on error, leave fields at their previous/default
  values and show the load error), then fetch the model list (forced).
- Refresh: fetch (forced).
- Base URL or API key: on `apply`/Enter or when focus leaves the row, fetch only if the
  (URL, key) pair differs from the last request.
- Fetch: `model_requests.begin(...)` → spinner on → `gio::spawn_blocking(llm::list_models)`
  awaited in `glib::spawn_future_local` → if `is_current(ticket)`, spinner off and show the
  models or error; otherwise drop the result.
- Save: `Config::from_fields(...).save()`; on success close; on failure show the error in the
  status text and stay open.
- The chat reads the config fresh for every request, so changes apply to the next message.

## Dark mode and scroll bars

libadwaita follows GNOME's light/dark preference; the editor's style scheme follows
`adw::StyleManager`. GTK's overlay scroll bars are used as they are.

## Testing

### Unit tests (`cargo test`)

All existing tests are kept except `entries_serialize_for_qml` (QML-only) and the portal-theme
test in `main.rs`. New tests:

- `text_diff`: equal texts → `None`; changes at start, middle and end; pure insertion and
  deletion; repeated characters (`"aaa"` → `"aaaa"`); multi-byte characters (ü, emoji, CJK) with
  char offsets; whole-text replacement; applying the span to `old` yields `new`.
- `document`: `from_disk`/`to_disk` for LF, CRLF, CRLF with a pasted `\r\n`; a lone `\r` is kept.
- `chat_markup`: each supported construct; escaping in text, code, and link URLs; raw HTML
  appears as text.
- `model_requests`: unchanged pair → `None`; forced → `Some`; changed pair → `Some`; an older
  ticket is not current.
- `config`: the new `require_model` message.

### GTK integration test (`tests/gtk_editor.rs`, `harness = false`)

One `main` runs every check in turn on the main thread:

- Round trip: for each file, read bytes, `from_disk`, `EditorView::load`, `text()`, `to_disk`,
  compare with the original bytes. Files: every git-tracked `*.md`, the fixtures in
  `tests/fixtures/` — `crlf.md` (CRLF endings), `nbsp.md` (U+00A0 and U+202F), and
  `front-matter.md` (YAML front matter, HTML comments, trailing spaces, a leading BOM, U+2028) —
  and any paths listed in `COUNTERPOINT_ROUNDTRIP_FILES` (colon-separated) for local checks on
  private files that must not enter the repository. Fixtures are generated with a Python script
  and their bytes verified with `open(p, 'rb').read().count(...)`.
- After `load`: `can_undo()` is false and the buffer is not modified.
- Apply: after `apply_markdown`, one undo restores the exact old text and one redo the new text;
  a mark outside the changed span keeps its offset.
- Every `chat_markup` output in a set of sample inputs is accepted by `GtkLabel::set_markup`
  (Pango's own parser rejects GtkLabel's `<a>` links, so the check uses a label).
- If GTK cannot initialise (no display), the test prints `SKIPPED: no display` and exits
  successfully, unless `COUNTERPOINT_REQUIRE_DISPLAY` is set, in which case it fails instead.

`tests/gtk_window.rs` (also `harness = false`) checks window-level behaviour built through the
public API: `MainWindow::new` is presented inside a real `adw::Application`, and the checks run
once the window is realized, walking the widget tree to confirm the chat input enables Send, the
mode toggle switches the chat's placeholder hint, the selection chip reflects the editor's
selection, and the editor placeholder shows and hides with the editor's text.

### Headless runs

`dev/headless.sh <command…>` removes a stale `broadway6.socket` (in `$XDG_RUNTIME_DIR`, for the
default display `:5`) before starting, so a socket left by a killed daemon cannot make the wait
loop pass instantly, then starts `gtk4-broadwayd --address 127.0.0.1 :5` in the background (its
web viewer bound to loopback only, its output captured to a temp log shown on failure), waits for
its socket while checking the daemon is still running, and runs the command with
`COUNTERPOINT_REQUIRE_DISPLAY=1 GDK_BACKEND=broadway BROADWAY_DISPLAY=:5` through
`dbus-run-session` so it gets its own private D-Bus session bus and can never forward to, or be
activated by, a Counterpoint instance on the user's own session. The daemon and its socket and log
are removed afterwards. Used for `cargo test` and for a launch smoke test (start the binary, check
stderr for GTK criticals or warnings, stop it after a few seconds). No windows are opened on the
user's desktop without asking.

### Dev tooling

`dev/mock_llm_server.py` stays; its docstring points to "Options… in the main menu".

## README

Rewritten for GTK: intro; requirements (Rust 1.92+, GTK 4.18+, libadwaita 1.8+, GtkSourceView
5.12+, Fedora: `sudo dnf install gtk4-devel libadwaita-devel gtksourceview5-devel`); configuration
(menu path "Options…"); build and run; mock server; keyboard table (adds Ctrl+, and Ctrl+?, and
Ctrl+Enter in the chat); development checks (integration test, `dev/headless.sh`,
`COUNTERPOINT_ROUNDTRIP_FILES`); known limitations. All Qt notes, including
`QT_QPA_PLATFORMTHEME`, are removed.

Known limitations:

- Styling comes from GtkSourceView's markdown language: headings are not enlarged and not every
  CommonMark edge case is covered.
- A file that contains any CRLF is saved entirely with CRLF, otherwise with LF.
- Responses are not streamed.
- Chat history is not persisted and is sent in full with every request.
- An applied proposal stays marked as applied after you undo it.
- Chat Markdown supports a subset (no tables or images).

## Done when

- `cargo fmt --check` is clean; `cargo build` shows no warnings from crate code; no `#[allow]`
  added to silence warnings.
- `cargo test` passes, run through `dev/headless.sh`, including the GTK integration test (not
  skipped).
- The headless launch smoke test shows no GTK criticals or warnings from the app.
- Staged files are scanned for personal data before any push.
- The user receives a short desktop checklist to try the app.

## Process

Work happens on the branch `gtk4-migration`, committed per plan task with the repository's git
identity and the session's attribution trailer. Nothing is pushed without asking.
