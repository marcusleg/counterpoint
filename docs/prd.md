# Counterpoint: Product Requirements

Derived from the features implemented as of version 0.1.0.

## Purpose

Counterpoint is a GNOME desktop Markdown editor for writing blog articles and social media posts
with an LLM at hand. The writer stays the author: the LLM either critiques the text or proposes
changes that the writer reviews and applies explicitly.

## Users

Individual writers who draft in Markdown and want feedback or editing help from a model they
choose, including a local one (Ollama, llama.cpp) or a hosted OpenAI-compatible provider.

## Goals

- Edit Markdown source directly without the editor altering the file.
- Get critique on a passage or the whole document without leaving the editor.
- Accept LLM edits only after reviewing them, and undo them in one step.
- Work with any OpenAI-compatible endpoint; no account or vendor lock-in.
- Send nothing beyond what the chat needs.

## Non-goals

- Rendered preview or WYSIWYG editing.
- Multiple documents or tabs in one window.
- Publishing to a platform.

## Requirements

### Editor

- R1. Edit Markdown source with syntax highlighting (headings, emphasis, code, links, front
  matter, HTML comments) via GtkSourceView.
- R2. Opening and saving an unchanged file leaves it byte for byte identical. A leading byte
  order mark is kept; a file containing any CRLF is saved with CRLF, otherwise with LF.
- R3. Loading a file leaves nothing to undo.
- R4. Open files from the command line, a file manager, drag and drop onto the editor, or
  **Open…** in the main menu. Only the first command-line file is opened.
- R5. New, Open, Save and Save As. Before discarding unsaved changes, ask the writer. Before
  overwriting a file that changed on disk since it was opened, ask the writer. Report failed
  reads and writes.
- R6. Zoom the editor text in, out and back to default; remember the zoom level.
- R7. Follow GNOME's light or dark style.
- R30. **Open Recent** in the main menu lists the ten files most recently opened or saved,
  newest first, by file name, adding the folder when two share a name; long names and folders
  are shortened so the menu stays narrow. Choosing one opens it through the unsaved-changes
  check; a file that can no longer be read is reported and dropped from the list. The item is
  always shown, with an empty list when there are no recent files.
- R31. **Find** (Ctrl+F, or **Find…** in the main menu) opens a find bar above the editor; a
  selection within one line becomes the search text. Matching ignores case. While the bar is
  open, every match is highlighted and the bar shows which match is selected and how many there
  are, or that there are none. Typing selects the first match from the selection onward; Enter,
  Ctrl+G or the down button selects the next match, and Shift+Enter, Ctrl+Shift+G or the up
  button the previous one, wrapping around the document. Ctrl+G and Ctrl+Shift+G also work
  from the editor. Escape closes the bar and returns to the editor, leaving the match selected.
- R33. Started without a file to open, Counterpoint reopens the file that was open when it was
  last used and continues the chat that was shown with it (R32). If that was an untitled
  document, it starts with a new one; if the file can no longer be read, it starts with a new
  document without reporting it, and the file leaves **Open Recent**. Started with a file, it
  opens that file instead.
- R34. The window opens at the size it had when it was last closed, and maximized if it was;
  un-maximizing it then returns it to its earlier size. Where it opens is left to the desktop.
- R35. **Undo** and **Redo** buttons at the start of the header bar undo and redo changes to
  the document, as Ctrl+Z and Ctrl+Shift+Z do in the editor, and leave the focus where it was.
  Each is greyed out while there is nothing to undo or redo.

### Chat

- R8. A chat pane beside the editor, toggled with F9. Dragging the divider between them resizes
  the chat; the editor takes up the rest of the window. The chat's width is remembered, also
  while it is hidden.
- R9. The highlighted text is the focus of a message; with nothing highlighted, the focus is the
  whole document. The pane shows the current selection.
- R10. **Sparring mode:** the LLM reads the document and critiques it, focusing on thinking
  (assumptions, vague claims, weak reasoning) rather than rewriting. It cannot change the
  document.
- R11. **Ghostwriting mode:** the LLM proposes at most one change per reply, expressed as one or
  more edits (verbatim `original` text plus `replacement`), preserving the writer's voice and
  inventing no facts.
- R12. A proposal shows a before/after preview with **Apply** and **Reject**. Apply fails with an
  explanation when an original is empty, not found, found more than once or overlaps another
  edit. Matches must not start or end inside a word. In an empty document, an empty original
  stands for the whole document, so the LLM can write a first draft.
- R13. An applied change is selected in the editor and is a single undo step, undoable from a
  toast, with Ctrl+Z or with the **Undo** button.
- R14. Every request includes the current document, so the model always sees the latest text,
  plus the conversation so far. Failed, cancelled and in-flight turns are left out of the
  history.
- R15. **Stop** abandons a running request and returns the message to the input. Late replies to
  a stopped or reset conversation are discarded.
- R16. **New conversation** starts an empty chat; the previous one stays in the chat list
  (R32). Starting a new document keeps the conversation, but no longer saves it as a chat about
  the previous file.
- R17. Chat replies render a safe Markdown subset; model output never becomes live markup.
- R18. Enter sends, Shift+Enter inserts a new line; input method composition is committed rather
  than sent.
- R19. When no model is configured, the pane says so and points to Preferences.
- R32. Conversations are saved as chats about the open file, after every reply, Apply and
  Reject, keeping the twenty newest per file. Opening a file starts a new conversation; a
  dropdown at the top of the chat pane lists that file's earlier chats, newest first, by start
  date and first message, and choosing one continues it, pending proposals included. The list
  is greyed out while a reply is awaited or when there is no other chat to choose. A document
  that has never been saved has no chats until it is: then the conversation becomes its first
  chat, as it does a new chat of the new file after **Save As**, while the old file keeps its
  chats. A history file that cannot be read is left untouched and chats are kept only until the
  app closes; a failed save shows a notice.

### LLM endpoint

- R20. Configure base URL (default `http://localhost:11434/v1`), optional API key (sent as a
  bearer token) and model in a Preferences dialog. Changes apply to the next message.
- R21. Fetch the model list from `GET /models` when the dialog opens, on refresh, and when the
  URL or key field is left; the model can also be typed in.
- R22. Reject base URLs that are not `http(s)://` or that contain a query, fragment or
  credentials. Warn when an API key would travel over plain HTTP to another machine.
- R23. Requests time out after ten minutes (model list: 30 seconds; connect: 10 seconds);
  responses are capped at 8 MiB. HTTP and parse errors are shown in the chat.

### Privacy and storage

- R24. A chat message sends only the document text, the highlighted passage and the
  conversation. File names, paths and settings are never sent.
- R25. The system prompt tells the model to treat the document and highlight as material, not
  instructions.
- R26. Settings live in `$XDG_CONFIG_HOME/counterpoint/settings.json`; the last folder, recent
  files, open file and chat, zoom level, chat width and window size in
  `$XDG_STATE_HOME/counterpoint/state.json`; saved chats, with the paths of the files they are
  about, in `$XDG_DATA_HOME/counterpoint/chats.json`. All three files are readable only by the
  user. The chats never leave the computer except as the conversation of the chat being
  continued (R24).
  In the Flatpak, these directories are under `~/.var/app/de.marcusleg.Counterpoint/`.

### Keyboard

- R27. Shortcuts for New, Open, Save, Save As, Find, Find Next, Find Previous, chat pane,
  Preferences, shortcuts help, Quit, Undo, Redo, zoom and sending messages, as listed in the
  README.

### Distribution

- R28. Every GitHub release for a version tag carries an x86_64 Flatpak bundle on the GNOME
  runtime, limited to network, display and GPU access with files going through the portals,
  and an RPM for the current Fedora release and the one before it.
- R29. Both packages install a desktop entry that offers to open Markdown files, AppStream
  metainfo and an icon, all under the app ID `de.marcusleg.Counterpoint`.

## Quality requirements

- Q1. `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test` pass in CI.
- Q2. GTK tests run headless on a private Broadway display and fail on GLib criticals.
- Q3. Every Markdown file in the repository and the fixtures (CRLF, front matter,
  non-breaking spaces) round-trips byte for byte.
- Q4. A mock OpenAI-compatible server (`dev/mock_llm_server.py`) covers success and failure
  paths and is pinned to the proposal format by a test.

## Platform

- Linux with GTK 4.18+, libadwaita 1.8+, GtkSourceView 5.12+; Rust 1.92+.
