# GTK4 + libadwaita Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Counterpoint's Qt 6 + QML user interface with GTK 4, libadwaita and GtkSourceView 5 (gtk-rs), keeping every feature, and remove Qt entirely.

**Architecture:** The crate becomes a library plus a thin binary. UI-independent logic stays in pure, unit-tested modules (`chat`, `config`, `document`, `llm`, `prompt`, `proposal`, plus new `text_diff`, `chat_markup`, `model_requests`). The GTK UI lives in `src/ui/` as plain Rust structs that build their widgets in code (no GObject subclassing, no `.ui` files, no `build.rs`). Blocking LLM calls run on `gio::spawn_blocking` and are awaited on the main loop with `glib::spawn_future_local`. GTK buffer behaviour is checked by a `harness = false` integration test that runs on the main thread, headless via Broadway.

**Tech Stack:** Rust 1.92+, gtk4 0.11 (`v4_18`), libadwaita 0.9 (`v1_8`), sourceview5 0.11 (`v5_12`), pulldown-cmark 0.13, reqwest (blocking), serde; Python 3 for dev tooling.

**Spec:** `docs/superpowers/specs/2026-09-27-gtk4-migration-design.md`

## Global Constraints

- Application ID: `de.marcusleg.Counterpoint`, set in code only; no `.desktop` file, metainfo or Flatpak.
- Minimum libraries: GTK 4.18, libadwaita 1.8, GtkSourceView 5.12. `rust-version = "1.92"`.
- Crates: `gtk4 0.11` (renamed `gtk`, feature `v4_18`), `libadwaita 0.9` (renamed `adw`, feature `v1_8`), `sourceview5 0.11` (feature `v5_12`), `pulldown-cmark 0.13` with `default-features = false`.
- Pure modules never import `gtk`, `adw`, `glib`, `gio` or `sourceview5`.
- No `#[allow(...)]` to silence warnings. `cargo fmt --check` clean. `cargo build` shows no warnings from crate code.
- The repository is public: tests, fixtures and docs use generic example text only — no personal articles, drafts, brand-voice material, local paths or employer details.
- Byte checks use Python (`open(p, 'rb').read().count(...)`), never `grep -P` (ugrep on this machine gives false negatives). Editing tools have silently turned backslash-u escape sequences into literal invisible characters (this happened while preparing this plan). Never write special characters or their escapes through an editing tool: the fixtures come only from `tests/fixtures/generate.py`, which is plain ASCII and builds them with `chr(0x...)`. Before each commit, scan changed files for unexpected invisible characters (Step 5 of Task 10 shows how).
- GTK code is run only headless (`dev/headless.sh …`); never open a window on the user's desktop without asking.
- Work on branch `gtk4-migration`. Commit with the repository's configured git identity; every commit message ends with the trailer line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` after a blank line. Do not push.

## File Map

| Path | Status | Responsibility |
|---|---|---|
| `Cargo.toml` | modify | GTK dependencies, `rust-version`, `[[test]] gtk_editor` with `harness = false` |
| `build.rs`, `cpp/`, `qml/`, `src/bridge/`, `dev/check_editor_core.cpp` | delete | Qt UI |
| `src/lib.rs` | create | module declarations |
| `src/main.rs` | replace | calls `counterpoint::ui::run()` |
| `src/chat.rs` | modify | `entries()` public; QML serialization removed |
| `src/config.rs`, `src/llm.rs` | modify | "Options… in the main menu" wording |
| `src/document.rs` | modify | `from_disk` / `to_disk` replace `uses_crlf` / `to_crlf` |
| `src/text_diff.rs` | create | minimal single-span edit in char offsets |
| `src/model_requests.rs` | create | model-list refetch dedupe, generation tickets, status summary |
| `src/chat_markup.rs` | create | Markdown → Pango markup |
| `src/ui/mod.rs` | create | application, actions, accelerators, CSS, shortcuts dialog |
| `src/ui/editor.rs` | create | `EditorView` (GtkSourceView wrapper) |
| `src/ui/chat_pane.rs` | create | `ChatPane` |
| `src/ui/options_dialog.rs` | create | `OptionsDialog` |
| `src/ui/window.rs` | create | `MainWindow` (layout, header bar, file handling, unsaved guard) |
| `dev/headless.sh` | create | private Broadway display for tests and smoke runs |
| `tests/gtk_editor.rs` | create | GTK buffer checks (round trip, undo, apply, markup) |
| `tests/fixtures/generate.py`, `tests/fixtures/*.md` | create | round-trip fixtures, byte-verified |
| `dev/mock_llm_server.py` | modify | docstring wording |
| `README.md` | replace | GTK requirements, usage, checks, limitations |

---

### Task 1: Remove Qt and switch to a GTK library + binary skeleton

**Files:**
- Delete: `build.rs`, `cpp/`, `qml/`, `src/bridge/`, `dev/check_editor_core.cpp`
- Modify: `Cargo.toml`, `src/chat.rs`, `src/config.rs`, `src/llm.rs`
- Create: `src/lib.rs`, `src/ui/mod.rs`
- Replace: `src/main.rs`

**Interfaces:**
- Produces: library crate `counterpoint` with `pub mod chat, config, document, llm, prompt, proposal, ui`; `counterpoint::ui::run() -> gtk::glib::ExitCode`; `counterpoint::ui::APP_ID`; `Conversation::entries(&self) -> &[Entry]` (public in all builds); `Entry` and `ProposalState` no longer derive `Serialize`.

- [ ] **Step 1: Delete the Qt parts**

```bash
git rm -rq build.rs cpp qml src/bridge dev/check_editor_core.cpp
```

- [ ] **Step 2: Replace `Cargo.toml`**

```toml
[package]
name = "counterpoint"
version = "0.1.0"
edition = "2021"
rust-version = "1.92"
description = "Markdown editor with an LLM sparring partner and ghostwriter"
license = "MIT"
publish = false

[dependencies]
adw = { package = "libadwaita", version = "0.9", features = ["v1_8"] }
gtk = { package = "gtk4", version = "0.11", features = ["v4_18"] }
pulldown-cmark = { version = "0.13", default-features = false }
reqwest = { version = "0.13", features = ["blocking", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sourceview5 = { version = "0.11", features = ["v5_12"] }

[dev-dependencies]
mockito = "1"
tempfile = "3"
```

(`pulldown-cmark` and `sourceview5` are used from Tasks 5 and 6 on; unused dependencies do not warn.)

- [ ] **Step 3: Update the wording tests first (they must fail)**

In `src/config.rs`, test `require_model_points_to_the_options_dialog`, change the assertion to:

```rust
        assert!(error.contains("Options… in the main menu"), "{error}");
```

In `src/llm.rs`, test `missing_model_fails_without_sending_a_request`, change the assertion to:

```rust
        assert!(error.to_string().contains("Options… in the main menu"));
```

- [ ] **Step 4: Create `src/lib.rs`, the skeleton UI and the new `src/main.rs`**

`src/lib.rs`:

```rust
pub mod chat;
pub mod config;
pub mod document;
pub mod llm;
pub mod prompt;
pub mod proposal;
pub mod ui;
```

`src/ui/mod.rs` (skeleton; Task 9 replaces it):

```rust
//! The GTK user interface.

use adw::prelude::*;
use gtk::glib;

pub const APP_ID: &str = "de.marcusleg.Counterpoint";

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        adw::ApplicationWindow::builder()
            .application(app)
            .title("Counterpoint")
            .default_width(1400)
            .default_height(850)
            .build()
            .present();
    });
    app.run()
}
```

`src/main.rs` (replaces the whole file, including the Qt portal-theme code and its test):

`src/main.rs`:

````rust
fn main() -> gtk::glib::ExitCode {
    counterpoint::ui::run()
}
````

- [ ] **Step 5: Remove the QML serialization from `src/chat.rs`**

1. Delete the line `use serde::Serialize;` and the blank line after it.
2. Replace

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalState {
```

with

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalState {
```

3. Replace

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Entry {
```

with

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
```

4. Remove the `#[cfg(test)]` line directly above `pub fn entries(&self) -> &[Entry] {`.
5. Delete the method `pub fn entries_json(&self) -> String { … }` (and the blank line before it).
6. Delete the test `fn entries_serialize_for_qml() { … }` with its `#[test]` attribute and the blank line before it.

- [ ] **Step 6: Run the tests to see the wording tests fail**

Run: `cargo test --lib 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: 2 failures, `config::tests::require_model_points_to_the_options_dialog` and `llm::tests::missing_model_fails_without_sending_a_request`.

- [ ] **Step 7: Change the message in `src/config.rs`**

Replace the body of `require_model` with:

```rust
    pub fn require_model(&self) -> Result<&str, String> {
        self.model.as_deref().ok_or_else(|| {
            "No model configured. Choose one under Options… in the main menu.".to_string()
        })
    }
```

- [ ] **Step 8: Verify**

Run: `cargo fmt && cargo fmt --check && cargo build 2>&1 | grep -c '^warning'`
Expected: `0`

Run: `cargo test 2>&1 | grep "test result"`
Expected: first line `test result: ok. 82 passed; 0 failed` (84 before, minus the QML and portal-theme tests).

Run: `grep -rn "cxx\|Qt\|qml" Cargo.toml src || echo clean`
Expected: `clean`

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "Remove the Qt UI and start a GTK library + binary skeleton

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: CRLF handling for the editor (`document::from_disk` / `to_disk`)

**Files:**
- Modify: `src/document.rs`

**Interfaces:**
- Produces: `document::from_disk(text: &str) -> (String, bool)` — text with every `\r\n` turned into `\n`, and whether any `\r\n` was found; `document::to_disk(text: &str, crlf: bool) -> String` — first `\r\n` → `\n`, then, if `crlf`, `\n` → `\r\n`. Removes `uses_crlf` and `to_crlf`.

- [ ] **Step 1: Replace the two CRLF tests with the new ones (failing)**

In the `tests` module of `src/document.rs`, replace the tests `detects_crlf` and `converts_lf_to_crlf` with:

````rust
    #[test]
    fn lf_text_passes_through_unchanged() {
        assert_eq!(from_disk("a\nb\n"), ("a\nb\n".to_string(), false));
        assert_eq!(to_disk("a\nb\n", false), "a\nb\n");
    }

    #[test]
    fn crlf_text_is_edited_as_lf_and_saved_as_crlf() {
        assert_eq!(from_disk("a\r\nb\r\n"), ("a\nb\n".to_string(), true));
        assert_eq!(to_disk("a\nb\n", true), "a\r\nb\r\n");
    }

    #[test]
    fn any_crlf_marks_the_file_as_crlf() {
        assert_eq!(from_disk("a\r\nb\n"), ("a\nb\n".to_string(), true));
    }

    #[test]
    fn pasted_crlf_is_normalised_on_save() {
        assert_eq!(to_disk("a\r\nb\n", true), "a\r\nb\r\n");
        assert_eq!(to_disk("a\r\nb\n", false), "a\nb\n");
    }

    #[test]
    fn lone_carriage_return_is_kept() {
        assert_eq!(from_disk("a\rb\r\n"), ("a\rb\n".to_string(), true));
        assert_eq!(to_disk("a\rb\n", true), "a\rb\r\n");
        assert_eq!(to_disk("a\rb\n", false), "a\rb\n");
    }
````

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --lib document 2>&1 | grep -E "error\[|cannot find"`
Expected: compile errors `cannot find function 'from_disk'` / `'to_disk'`.

- [ ] **Step 3: Implement**

Replace the functions `uses_crlf` and `to_crlf` (with their doc comments) by:

````rust
/// Prepares file contents for the editor: returns the text with every CRLF line ending turned
/// into LF, and whether there was any CRLF, so a save can restore it.
pub fn from_disk(text: &str) -> (String, bool) {
    if text.contains("\r\n") {
        (text.replace("\r\n", "\n"), true)
    } else {
        (text.to_string(), false)
    }
}

/// Prepares editor text for saving: CRLF (e.g. pasted) becomes LF, then, if `crlf`, every LF
/// becomes CRLF. A lone `\r` is kept.
pub fn to_disk(text: &str, crlf: bool) -> String {
    let text = text.replace("\r\n", "\n");
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}
````

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo test 2>&1 | grep "test result" | head -1`
Expected: `test result: ok. 85 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/document.rs
git commit -m "Normalise CRLF on load and restore it on save

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Minimal-span diff (`text_diff`)

**Files:**
- Create: `src/text_diff.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces: `text_diff::Span { pub start: usize, pub end: usize, pub replacement: String }` (offsets in `char`s into the old text); `text_diff::minimal_edit(old: &str, new: &str) -> Option<Span>` (`None` when equal; ties keep the longest common prefix).

- [ ] **Step 1: Register the module**

Add `pub mod text_diff;` to `src/lib.rs` (keep the list alphabetical).

- [ ] **Step 2: Write the tests with a stub (failing)**

Create `src/text_diff.rs` with the doc comment, the `Span` struct, a stub `pub fn minimal_edit(_old: &str, _new: &str) -> Option<Span> { None }` and the complete `tests` module from Step 4's file.

Run: `cargo test --lib text_diff 2>&1 | grep "test result"`
Expected: `test result: FAILED. 1 passed; 6 failed` (only `equal_texts_need_no_edit` passes).

- [ ] **Step 3: Implement** — replace the stub so the file reads exactly as in Step 4.

- [ ] **Step 4: Final `src/text_diff.rs`**

`src/text_diff.rs`:

````rust
//! Smallest single edit that turns one text into another.

/// Replace the characters `start..end` of the old text with `replacement`. Offsets count Unicode
/// scalar values (`char`s), which is what GTK text buffers use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// Returns the span between the first and the last differing character, or `None` if the texts
/// are equal. Where the change is ambiguous (e.g. `"aaa"` to `"aaaa"`), the common prefix is kept
/// as long as possible.
pub fn minimal_edit(old: &str, new: &str) -> Option<Span> {
    if old == new {
        return None;
    }
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let prefix = old_chars
        .iter()
        .zip(&new_chars)
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = old_chars.len().min(new_chars.len()) - prefix;
    let suffix = old_chars
        .iter()
        .rev()
        .zip(new_chars.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    Some(Span {
        start: prefix,
        end: old_chars.len() - suffix,
        replacement: new_chars[prefix..new_chars.len() - suffix].iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, span: &Span) -> String {
        let chars: Vec<char> = old.chars().collect();
        let mut result: String = chars[..span.start].iter().collect();
        result.push_str(&span.replacement);
        result.extend(&chars[span.end..]);
        result
    }

    fn check(old: &str, new: &str, start: usize, end: usize, replacement: &str) {
        let span = minimal_edit(old, new).expect("texts differ");
        assert_eq!(
            span,
            Span {
                start,
                end,
                replacement: replacement.to_string()
            },
            "{old:?} -> {new:?}"
        );
        assert_eq!(apply(old, &span), new);
    }

    #[test]
    fn equal_texts_need_no_edit() {
        assert_eq!(minimal_edit("", ""), None);
        assert_eq!(minimal_edit("Same text.", "Same text."), None);
    }

    #[test]
    fn change_in_the_middle() {
        check("The old sentence.", "The new sentence.", 4, 7, "new");
    }

    #[test]
    fn change_at_the_start_and_end() {
        check("Old start.", "New start.", 0, 3, "New");
        check("Ends here.", "Ends there!", 5, 10, "there!");
    }

    #[test]
    fn pure_insertion_and_deletion() {
        check("ab", "aXb", 1, 1, "X");
        check("aXb", "ab", 1, 2, "");
        check("", "new", 0, 0, "new");
        check("gone", "", 0, 4, "");
    }

    #[test]
    fn repeated_characters_keep_the_longest_prefix() {
        check("aaa", "aaaa", 3, 3, "a");
        check("aaaa", "aaa", 3, 4, "");
    }

    #[test]
    fn offsets_count_characters_not_bytes() {
        check("Grüße, Welt", "Grüße, Erde", 7, 11, "Erde");
        check("🙂 a 🙂", "🙂 b 🙂", 2, 3, "b");
        check("日本語のテキスト", "日本語の文章", 4, 8, "文章");
    }

    #[test]
    fn whole_text_replacement() {
        check("abc", "xyz", 0, 3, "xyz");
    }
}
````

- [ ] **Step 5: Verify**

Run: `cargo fmt && cargo test 2>&1 | grep "test result" | head -1`
Expected: `test result: ok. 92 passed; 0 failed`

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/text_diff.rs
git commit -m "Add a minimal-span text diff counted in characters

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Model-list request bookkeeping (`model_requests`)

**Files:**
- Create: `src/model_requests.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces: `model_requests::Ticket` (`Copy`); `ModelRequests::default()`; `ModelRequests::begin(&mut self, base_url: &str, api_key: &str, force: bool) -> Option<Ticket>` (`None` if the pair equals the last requested pair and `!force`); `ModelRequests::is_current(&self, ticket: Ticket) -> bool`; `model_requests::summary(count: usize) -> String` ("The endpoint lists no models." / "1 model available." / "n models available.").

- [ ] **Step 1: Register the module** — add `pub mod model_requests;` to `src/lib.rs`.

- [ ] **Step 2: Write the tests with stubs (failing)**

Create `src/model_requests.rs` with the types, stub bodies (`begin` returns `None`, `is_current` returns `false`, `summary` returns `String::new()`) and the complete `tests` module from Step 4's file.

Run: `cargo test --lib model_requests 2>&1 | grep "test result"`
Expected: `test result: FAILED. 0 passed; 5 failed`

- [ ] **Step 3: Implement** — make the file read exactly as in Step 4.

- [ ] **Step 4: Final `src/model_requests.rs`**

`src/model_requests.rs`:

````rust
//! Bookkeeping for model-list requests: skips refetching for unchanged endpoint settings and
//! identifies the latest request, so only its result is shown.

/// Identifies one model-list request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket(u64);

#[derive(Debug, Default)]
pub struct ModelRequests {
    generation: u64,
    last_requested: Option<(String, String)>,
}

impl ModelRequests {
    /// Starts a request for `(base_url, api_key)`. Returns `None` if the same pair was requested
    /// last and `force` is false.
    pub fn begin(&mut self, base_url: &str, api_key: &str, force: bool) -> Option<Ticket> {
        let pair = (base_url.to_string(), api_key.to_string());
        if !force && self.last_requested.as_ref() == Some(&pair) {
            return None;
        }
        self.last_requested = Some(pair);
        self.generation += 1;
        Some(Ticket(self.generation))
    }

    /// True if `ticket` belongs to the most recent request.
    pub fn is_current(&self, ticket: Ticket) -> bool {
        ticket.0 == self.generation
    }
}

/// Status line for a successfully loaded model list.
pub fn summary(count: usize) -> String {
    match count {
        0 => "The endpoint lists no models.".to_string(),
        1 => "1 model available.".to_string(),
        n => format!("{n} models available."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_request_always_starts() {
        let mut requests = ModelRequests::default();
        assert!(requests.begin("http://a/v1", "", false).is_some());
    }

    #[test]
    fn unchanged_settings_do_not_refetch_unless_forced() {
        let mut requests = ModelRequests::default();
        requests.begin("http://a/v1", "key", false).unwrap();
        assert_eq!(requests.begin("http://a/v1", "key", false), None);
        assert!(requests.begin("http://a/v1", "key", true).is_some());
    }

    #[test]
    fn changed_url_or_key_refetches() {
        let mut requests = ModelRequests::default();
        requests.begin("http://a/v1", "key", false).unwrap();
        assert!(requests.begin("http://b/v1", "key", false).is_some());
        assert!(requests.begin("http://b/v1", "other", false).is_some());
    }

    #[test]
    fn only_the_latest_ticket_is_current() {
        let mut requests = ModelRequests::default();
        let first = requests.begin("http://a/v1", "", false).unwrap();
        assert!(requests.is_current(first));
        let second = requests.begin("http://a/v1", "", true).unwrap();
        assert!(!requests.is_current(first));
        assert!(requests.is_current(second));
    }

    #[test]
    fn summary_counts_models() {
        assert_eq!(summary(0), "The endpoint lists no models.");
        assert_eq!(summary(1), "1 model available.");
        assert_eq!(summary(3), "3 models available.");
    }
}
````

- [ ] **Step 5: Verify**

Run: `cargo fmt && cargo test 2>&1 | grep "test result" | head -1`
Expected: `test result: ok. 97 passed; 0 failed`

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/model_requests.rs
git commit -m "Add bookkeeping for model-list requests

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Chat Markdown → Pango markup (`chat_markup`)

**Files:**
- Create: `src/chat_markup.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces: `chat_markup::to_pango(markdown: &str) -> String`; `chat_markup::escape(text: &str) -> String` (escapes `& < > ' "` as `&amp; &lt; &gt; &#39; &quot;`).

Notes for the implementer: pulldown-cmark 0.13 emits `Event::Start(Tag)` / `Event::End(TagEnd)`; HTML blocks arrive as `Tag::HtmlBlock` containing one `Event::Html` per line (each ending in `\n`), so they are buffered like code blocks. Only `Options::ENABLE_STRIKETHROUGH` is enabled, so footnotes, task lists, tables and math never occur (tables arrive as plain paragraphs). Every opening markup tag written in `start` has its closing tag in `end`, which keeps the output balanced.

- [ ] **Step 1: Register the module** — add `pub mod chat_markup;` to `src/lib.rs`.

- [ ] **Step 2: Write the tests with stubs (failing)**

Create `src/chat_markup.rs` containing `pub fn to_pango(_markdown: &str) -> String { String::new() }`, `pub fn escape(_text: &str) -> String { String::new() }` and the complete `tests` module from Step 4's file.

Run: `cargo test --lib chat_markup 2>&1 | grep "test result"`
Expected: `test result: FAILED.` with most tests failing (only `unusual_input_stays_balanced` can pass on empty output).

- [ ] **Step 3: Implement** — make the file read exactly as in Step 4.

- [ ] **Step 4: Final `src/chat_markup.rs`**

`src/chat_markup.rs`:

````rust
//! Renders the Markdown of chat replies as Pango markup for GTK labels.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Converts `markdown` to Pango markup. Supports paragraphs, headings, emphasis, strong,
/// strikethrough, inline code, code blocks, lists, block quotes, links and rules; raw HTML and
/// everything else appear as escaped text. The result always has balanced tags.
pub fn to_pango(markdown: &str) -> String {
    let mut writer = Writer::default();
    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
        writer.event(event);
    }
    writer.out
}

/// Escapes text for use in Pango markup, including attribute values.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\'' => escaped.push_str("&#39;"),
            '"' => escaped.push_str("&quot;"),
            c => escaped.push(c),
        }
    }
    escaped
}

struct Writer {
    out: String,
    /// One entry per open list: the next item number, or `None` for bullet lists.
    lists: Vec<Option<u64>>,
    quote_depth: usize,
    /// Text of the code or HTML block being read, emitted when the block ends.
    raw_block: Option<String>,
    /// True where a block may start without a separator: at the beginning and after a list
    /// marker or quote opening.
    at_block_start: bool,
}

impl Default for Writer {
    fn default() -> Self {
        Self {
            out: String::new(),
            lists: Vec::new(),
            quote_depth: 0,
            raw_block: None,
            at_block_start: true,
        }
    }
}

impl Writer {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) | Event::Html(text) => match &mut self.raw_block {
                Some(raw) => raw.push_str(&text),
                None => self.text(&text),
            },
            Event::Code(code) => {
                self.out.push_str("<tt>");
                self.text(&code);
                self.out.push_str("</tt>");
            }
            Event::InlineHtml(html) => self.text(&html),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.newline(),
            Event::Rule => {
                self.separate();
                self.text("———");
            }
            // Footnotes, task lists and math are not enabled in the parser options.
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.separate(),
            Tag::Heading { level, .. } => {
                self.separate();
                self.out.push_str(match level {
                    HeadingLevel::H1 => "<span weight=\"bold\" size=\"x-large\">",
                    HeadingLevel::H2 => "<span weight=\"bold\" size=\"large\">",
                    HeadingLevel::H3 => "<span weight=\"bold\" size=\"larger\">",
                    _ => "<span weight=\"bold\">",
                });
            }
            Tag::BlockQuote(_) => {
                self.separate();
                self.quote_depth += 1;
                self.out.push_str(QUOTE_INDENT);
                self.out.push_str("<span alpha=\"70%\">");
            }
            Tag::CodeBlock(_) | Tag::HtmlBlock => {
                self.separate();
                self.raw_block = Some(String::new());
            }
            Tag::List(first_number) => {
                self.separate();
                self.lists.push(first_number);
            }
            Tag::Item => {
                if !self.at_block_start {
                    self.out.push('\n');
                    self.out.push_str(&self.indent(self.lists.len() - 1));
                }
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    _ => "• ".to_string(),
                };
                self.out.push_str(&marker);
                self.at_block_start = true;
            }
            Tag::Emphasis => self.out.push_str("<i>"),
            Tag::Strong => self.out.push_str("<b>"),
            Tag::Strikethrough => self.out.push_str("<s>"),
            Tag::Link { dest_url, .. } => {
                self.out.push_str("<a href=\"");
                self.out.push_str(&escape(&dest_url));
                self.out.push_str("\">");
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => self.out.push_str("</span>"),
            TagEnd::BlockQuote(_) => {
                self.out.push_str("</span>");
                self.quote_depth -= 1;
            }
            TagEnd::CodeBlock => {
                let code = self.raw_block.take().unwrap_or_default();
                self.out.push_str("<tt>");
                self.lines(code.trim_end_matches('\n'));
                self.out.push_str("</tt>");
            }
            TagEnd::HtmlBlock => {
                let html = self.raw_block.take().unwrap_or_default();
                self.lines(html.trim_end_matches('\n'));
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Emphasis => self.out.push_str("</i>"),
            TagEnd::Strong => self.out.push_str("</b>"),
            TagEnd::Strikethrough => self.out.push_str("</s>"),
            TagEnd::Link => self.out.push_str("</a>"),
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        self.out.push_str(&escape(text));
        self.at_block_start = false;
    }

    /// Writes multi-line text, indenting each line to the current nesting.
    fn lines(&mut self, text: &str) {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.newline();
            }
            self.text(line);
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
        self.out.push_str(&self.indent(self.lists.len()));
    }

    /// Starts a new block: a blank line between top-level blocks, a line break inside lists.
    fn separate(&mut self) {
        if !self.at_block_start {
            self.out
                .push_str(if self.lists.is_empty() { "\n\n" } else { "\n" });
            self.out.push_str(&self.indent(self.lists.len()));
        }
        self.at_block_start = true;
    }

    fn indent(&self, list_levels: usize) -> String {
        QUOTE_INDENT.repeat(self.quote_depth) + &LIST_INDENT.repeat(list_levels)
    }
}

const QUOTE_INDENT: &str = "    ";
const LIST_INDENT: &str = "   ";

#[cfg(test)]
mod tests {
    use super::*;

    /// Panics unless every opening tag in `markup` is closed in order.
    fn assert_balanced(markup: &str) {
        let mut open: Vec<&str> = Vec::new();
        let mut rest = markup;
        while let Some(start) = rest.find('<') {
            let end = start + rest[start..].find('>').expect("unclosed tag bracket");
            let tag = &rest[start + 1..end];
            if let Some(name) = tag.strip_prefix('/') {
                assert_eq!(open.pop(), Some(name), "in {markup:?}");
            } else {
                open.push(tag.split(' ').next().unwrap());
            }
            rest = &rest[end + 1..];
        }
        assert!(open.is_empty(), "unclosed {open:?} in {markup:?}");
    }

    fn check(markdown: &str, expected: &str) {
        let markup = to_pango(markdown);
        assert_eq!(markup, expected, "for {markdown:?}");
        assert_balanced(&markup);
    }

    #[test]
    fn paragraphs_are_separated_by_a_blank_line() {
        check("One\ntwo.\n\nThree.", "One two.\n\nThree.");
    }

    #[test]
    fn inline_styles() {
        check(
            "*it* **bold** ~~gone~~ `x < y`",
            "<i>it</i> <b>bold</b> <s>gone</s> <tt>x &lt; y</tt>",
        );
    }

    #[test]
    fn headings_are_bold_and_larger() {
        check(
            "# One\n\n## Two\n\n### Three\n\n#### Four",
            "<span weight=\"bold\" size=\"x-large\">One</span>\n\n\
             <span weight=\"bold\" size=\"large\">Two</span>\n\n\
             <span weight=\"bold\" size=\"larger\">Three</span>\n\n\
             <span weight=\"bold\">Four</span>",
        );
    }

    #[test]
    fn code_blocks_keep_their_lines() {
        check(
            "Before:\n\n```rust\nlet a = 1 < 2;\n  indented\n```\n\nAfter.",
            "Before:\n\n<tt>let a = 1 &lt; 2;\n  indented</tt>\n\nAfter.",
        );
    }

    #[test]
    fn bullet_and_numbered_lists() {
        check("- one\n- two", "• one\n• two");
        check("3. three\n4. four", "3. three\n4. four");
    }

    #[test]
    fn nested_lists_are_indented() {
        check(
            "Intro:\n\n- outer\n  - inner\n- next",
            "Intro:\n\n• outer\n   • inner\n• next",
        );
    }

    #[test]
    fn loose_list_items_keep_their_paragraphs_together() {
        check("- one\n\n  more\n\n- two", "• one\n   more\n• two");
    }

    #[test]
    fn block_quotes_are_indented_and_dimmed() {
        check(
            "> Quoted\n> text.\n\nAfter.",
            "    <span alpha=\"70%\">Quoted text.</span>\n\nAfter.",
        );
    }

    #[test]
    fn links_escape_their_url() {
        check(
            "[site](https://example.com/?a=1&b=\"2\")",
            "<a href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\">site</a>",
        );
    }

    #[test]
    fn raw_html_is_shown_as_text() {
        check(
            "<b>x</b> & <!-- note -->",
            "&lt;b&gt;x&lt;/b&gt; &amp; &lt;!-- note --&gt;",
        );
        check("<div>\nblock\n</div>", "&lt;div&gt;\nblock\n&lt;/div&gt;");
    }

    #[test]
    fn rules_and_hard_breaks() {
        check("a\n\n---\n\nb", "a\n\n———\n\nb");
        check("line  \nbreak", "line\nbreak");
    }

    #[test]
    fn images_show_their_alt_text() {
        check("![a chart](chart.png)", "a chart");
    }

    #[test]
    fn apostrophes_and_quotes_are_escaped() {
        check("It's \"fine\"", "It&#39;s &quot;fine&quot;");
    }

    #[test]
    fn unusual_input_stays_balanced() {
        for markdown in [
            "",
            "**unclosed",
            "> - quoted list\n>   - nested\n>\n> ```\n> code\n> ```",
            "1. a\n\n   > quote in item\n\n2. b",
            "| a | b |\n|---|---|\n| 1 | 2 |",
            "[link with `code` and **bold**](x)",
            "# Heading with [link](y) and *em*",
        ] {
            assert_balanced(&to_pango(markdown));
        }
    }
}
````

- [ ] **Step 5: Verify**

Run: `cargo fmt && cargo test 2>&1 | grep "test result" | head -1`
Expected: `test result: ok. 111 passed; 0 failed`

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/chat_markup.rs
git commit -m "Render chat Markdown as Pango markup

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Editor view, headless runner and GTK buffer checks

**Files:**
- Create: `dev/headless.sh`, `tests/fixtures/generate.py`, `tests/fixtures/crlf.md`, `tests/fixtures/nbsp.md`, `tests/fixtures/front-matter.md` (the three `.md` files are generated), `tests/gtk_editor.rs`, `src/ui/editor.rs`
- Modify: `Cargo.toml`, `src/ui/mod.rs`

**Interfaces:**
- Consumes: `document::from_disk`, `document::to_disk` (Task 2), `text_diff::minimal_edit` (Task 3), `chat_markup::to_pango` (Task 5).
- Produces: `ui::editor::EditorView` (`Clone`): `new() -> Self`, `widget(&self) -> &sourceview5::View`, `buffer(&self) -> &sourceview5::Buffer`, `load(&self, text: &str)` (no undo step, unmodified, cursor at start), `text(&self) -> String` (exact contents), `selection_text(&self) -> String` (empty if none), `apply_markdown(&self, markdown: &str)` (one undo step over the minimal span), `set_dark(&self, dark: bool)` (Adwaita / Adwaita-dark scheme). `dev/headless.sh <command…>`.

- [ ] **Step 1: Create `dev/headless.sh`** and make it executable (`chmod +x dev/headless.sh`)

`dev/headless.sh`:

````sh
#!/bin/sh
# Runs a command against a private GTK Broadway display, so no window reaches the desktop.
# Usage: dev/headless.sh cargo test
set -eu

number="${HEADLESS_DISPLAY:-5}"
display=":$number"
# Display :N listens on broadway<N+1>.socket.
socket="${XDG_RUNTIME_DIR:-/tmp}/broadway$((number + 1)).socket"

# Bind the daemon's web viewer to loopback only.
gtk4-broadwayd --address 127.0.0.1 "$display" >/dev/null 2>&1 &
daemon=$!
trap 'kill "$daemon" 2>/dev/null || true' EXIT INT TERM

tries=0
until [ -S "$socket" ]; do
    tries=$((tries + 1))
    if [ "$tries" -gt 50 ]; then
        echo "gtk4-broadwayd did not start on $display" >&2
        exit 1
    fi
    sleep 0.1
done

GDK_BACKEND=broadway BROADWAY_DISPLAY="$display" "$@"
````

Note: for display `:N`, `gtk4-broadwayd` listens on `broadway<N+1>.socket`; `--address 127.0.0.1` keeps its web viewer off the network (by default it listens on all interfaces).

- [ ] **Step 2: Create `tests/fixtures/generate.py` and generate the fixtures**

The script must stay pure ASCII; check with `python3 -c "d=open('tests/fixtures/generate.py','rb').read(); print(all(b < 128 for b in d))"` → `True`.

`tests/fixtures/generate.py`:

````python
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
````

Run: `python3 tests/fixtures/generate.py`
Expected:
```
crlf.md: 116 bytes OK
nbsp.md: 195 bytes OK
front-matter.md: 314 bytes OK
```

- [ ] **Step 3: Register the integration test in `Cargo.toml`** — append:

```toml

[[test]]
name = "gtk_editor"
harness = false
```

- [ ] **Step 4: Write `tests/gtk_editor.rs` (fails to compile until the editor exists)**

`tests/gtk_editor.rs`:

````rust
//! GTK checks for the editor buffer and chat markup. GTK must run on the thread that initialised
//! it, so this test has its own `main` (`harness = false`) and runs every check in turn.
//! It needs a display; `dev/headless.sh cargo test` provides a private one.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use counterpoint::chat_markup;
use counterpoint::document;
use counterpoint::ui::editor::EditorView;
use gtk::prelude::*;

#[derive(Default)]
struct Checks {
    failures: usize,
    passed: usize,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str) {
        if ok {
            self.passed += 1;
        } else {
            self.failures += 1;
            println!("FAIL: {what}");
        }
    }
}

fn main() -> ExitCode {
    if gtk::init().is_err() {
        println!("SKIPPED: no display");
        return ExitCode::SUCCESS;
    }
    sourceview5::init();

    let mut checks = Checks::default();
    for path in round_trip_files() {
        round_trip(&mut checks, &path);
    }
    loading_is_not_undoable(&mut checks);
    apply_is_one_undo_step(&mut checks);
    apply_leaves_text_outside_the_span_alone(&mut checks);
    apply_counts_characters(&mut checks);
    chat_markup_is_valid_for_labels(&mut checks);

    println!("{} passed, {} failed", checks.passed, checks.failures);
    if checks.failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Every git-tracked Markdown file, the fixtures, and any colon-separated paths in
/// `COUNTERPOINT_ROUNDTRIP_FILES` (for private files that must stay out of the repository).
fn round_trip_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<PathBuf> = Command::new("git")
        .args(["ls-files", "-z", "--", "*.md"])
        .current_dir(root)
        .output()
        .map(|output| {
            output
                .stdout
                .split(|&byte| byte == 0)
                .filter(|name| !name.is_empty())
                .map(|name| root.join(String::from_utf8_lossy(name).as_ref()))
                .collect()
        })
        .unwrap_or_default();
    for name in ["crlf.md", "nbsp.md", "front-matter.md"] {
        let path = root.join("tests/fixtures").join(name);
        if !files.contains(&path) {
            files.push(path);
        }
    }
    if let Some(extra) = std::env::var_os("COUNTERPOINT_ROUNDTRIP_FILES") {
        files.extend(std::env::split_paths(&extra).filter(|p| !p.as_os_str().is_empty()));
    }
    files
}

fn round_trip(checks: &mut Checks, path: &Path) {
    let what = format!("round trip of {}", path.display());
    let Ok(bytes) = std::fs::read(path) else {
        checks.check(false, &format!("{what}: cannot read the file"));
        return;
    };
    let Ok(contents) = String::from_utf8(bytes.clone()) else {
        checks.check(false, &format!("{what}: not UTF-8"));
        return;
    };
    let editor = EditorView::new();
    let (text, crlf) = document::from_disk(&contents);
    editor.load(&text);
    let saved = document::to_disk(&editor.text(), crlf);
    checks.check(saved.as_bytes() == bytes.as_slice(), &what);
}

fn loading_is_not_undoable(checks: &mut Checks) {
    let editor = EditorView::new();
    editor.load("# Title\n\nFirst version.\n");
    let buffer = editor.buffer();
    checks.check(!buffer.can_undo(), "load leaves nothing to undo");
    checks.check(!buffer.is_modified(), "load leaves the buffer unmodified");

    buffer.insert_at_cursor("typed ");
    checks.check(buffer.is_modified(), "typing marks the buffer modified");
    editor.load("# Title\n\nSecond version.\n");
    checks.check(!buffer.can_undo(), "loading again drops earlier undo steps");
    checks.check(
        editor.text() == "# Title\n\nSecond version.\n",
        "load replaces the whole text",
    );
}

fn apply_is_one_undo_step(checks: &mut Checks) {
    let old = "# Title\n\nThe old sentence.\n\nA second paragraph.\n";
    let new = "# Title\n\nThe new sentence, rewritten.\n\nA second paragraph, too.\n";
    let editor = EditorView::new();
    editor.load(old);
    editor.apply_markdown(new);
    let buffer = editor.buffer();
    checks.check(editor.text() == new, "apply sets the new text");
    checks.check(buffer.is_modified(), "apply marks the buffer modified");
    buffer.undo();
    checks.check(editor.text() == old, "one undo restores the old text");
    checks.check(!buffer.can_undo(), "apply added exactly one undo step");
    buffer.redo();
    checks.check(editor.text() == new, "one redo restores the new text");
}

fn apply_leaves_text_outside_the_span_alone(checks: &mut Checks) {
    let editor = EditorView::new();
    editor.load("Keep this. Change that. Keep the end.");
    let buffer = editor.buffer();
    buffer.place_cursor(&buffer.iter_at_offset(4));
    let before = buffer.create_mark(None, &buffer.iter_at_offset(2), true);
    let after = buffer.create_mark(None, &buffer.iter_at_offset(24), true);
    editor.apply_markdown("Keep this. Rewrite it all. Keep the end.");
    checks.check(
        buffer.iter_at_mark(&buffer.get_insert()).offset() == 4,
        "the cursor before the change stays put",
    );
    checks.check(
        buffer.iter_at_mark(&before).offset() == 2,
        "a mark before the change keeps its offset",
    );
    let after_iter = buffer.iter_at_mark(&after);
    checks.check(
        buffer.text(&after_iter, &buffer.end_iter(), true) == "Keep the end.",
        "a mark after the change still points at the same text",
    );
}

fn apply_counts_characters(checks: &mut Checks) {
    let editor = EditorView::new();
    editor.load("🙂 Grüße aus 日本, Welt!\n");
    editor.apply_markdown("🙂 Grüße aus 日本, Erde!\n");
    checks.check(
        editor.text() == "🙂 Grüße aus 日本, Erde!\n",
        "apply after multi-byte characters changes the right span",
    );
}

fn chat_markup_is_valid_for_labels(checks: &mut Checks) {
    let samples = [
        "Plain *emphasis*, **strong**, ~~struck~~ and `code < 1`.",
        "# Heading\n\n## Sub-heading\n\nText & more <text>.",
        "- one\n- two\n  - nested\n\n1. first\n2. second",
        "> A quote with **bold**\n> and a [link](https://example.com/?a=1&b=2).",
        "```rust\nfn main() { println!(\"<hi>\"); }\n```",
        "<div>raw html</div>\n\nIt's \"quoted\".\n\n---\n\n![alt text](image.png)",
    ];
    for markdown in samples {
        let label = gtk::Label::new(None);
        label.set_markup(&chat_markup::to_pango(markdown));
        checks.check(
            !label.text().is_empty(),
            &format!("GTK accepts the markup for {markdown:?}"),
        );
    }
}
````

Run: `cargo test --test gtk_editor 2>&1 | grep -E "error\[" | head -3`
Expected: `error[E0432]: unresolved import` for `counterpoint::ui::editor`.

- [ ] **Step 5: Create `src/ui/editor.rs`**

`src/ui/editor.rs`:

````rust
//! The Markdown source editor: a GtkSourceView that holds the exact file text.

use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::text_diff;

/// Cheap to clone; clones share the same view and buffer.
#[derive(Clone)]
pub struct EditorView {
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorView {
    pub fn new() -> Self {
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_language(
            sourceview5::LanguageManager::default()
                .language("markdown")
                .as_ref(),
        );
        buffer.set_highlight_syntax(true);
        let view = sourceview5::View::with_buffer(&buffer);
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_left_margin(24);
        view.set_right_margin(24);
        view.set_top_margin(16);
        view.set_bottom_margin(16);
        Self { view, buffer }
    }

    pub fn widget(&self) -> &sourceview5::View {
        &self.view
    }

    pub fn buffer(&self) -> &sourceview5::Buffer {
        &self.buffer
    }

    /// Replaces the text without an undo step and marks the buffer unmodified.
    pub fn load(&self, text: &str) {
        self.buffer.begin_irreversible_action();
        self.buffer.set_text(text);
        self.buffer.end_irreversible_action();
        self.buffer.set_modified(false);
        self.buffer.place_cursor(&self.buffer.start_iter());
    }

    /// The exact buffer contents.
    pub fn text(&self) -> String {
        let (start, end) = self.buffer.bounds();
        self.buffer.text(&start, &end, true).into()
    }

    /// The selected text, or an empty string.
    pub fn selection_text(&self) -> String {
        self.buffer
            .selection_bounds()
            .map(|(start, end)| self.buffer.text(&start, &end, true).into())
            .unwrap_or_default()
    }

    /// Changes the text to `markdown` as a single undo step, replacing only the span between the
    /// first and the last differing character.
    pub fn apply_markdown(&self, markdown: &str) {
        let Some(span) = text_diff::minimal_edit(&self.text(), markdown) else {
            return;
        };
        let offset = |chars: usize| i32::try_from(chars).expect("document fits in i32 offsets");
        let mut start = self.buffer.iter_at_offset(offset(span.start));
        let mut end = self.buffer.iter_at_offset(offset(span.end));
        self.buffer.begin_user_action();
        self.buffer.delete(&mut start, &mut end);
        self.buffer.insert(&mut start, &span.replacement);
        self.buffer.end_user_action();
    }

    /// Uses the Adwaita style scheme matching `dark`.
    pub fn set_dark(&self, dark: bool) {
        let name = if dark { "Adwaita-dark" } else { "Adwaita" };
        self.buffer.set_style_scheme(
            sourceview5::StyleSchemeManager::default()
                .scheme(name)
                .as_ref(),
        );
    }
}
````

Add `pub mod editor;` to `src/ui/mod.rs` directly below the `//! The GTK user interface.` line (followed by a blank line).

- [ ] **Step 6: Verify headless**

Run: `cargo fmt && dev/headless.sh cargo test 2>&1 | grep -E "test result|passed|FAIL|SKIPPED"`
Expected: `test result: ok. 111 passed; 0 failed`, and a line `N passed, 0 failed` from `gtk_editor` where N = 20 + number of round-tripped files (with this branch: README.md, the spec, the plan and the three fixtures → at least 25). No `FAIL:` and no `SKIPPED` lines.

Sanity check that the round trip can fail: temporarily change `round_trip` to skip `document::to_disk` (`let saved = editor.text();`), rerun, expect `FAIL: round trip of …/crlf.md`; then revert.

Run: `cargo build 2>&1 | grep -c '^warning'`
Expected: `0`

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml dev/headless.sh tests src/ui
git commit -m "Add the GtkSourceView editor with headless buffer checks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Chat pane

**Files:**
- Create: `src/ui/chat_pane.rs`
- Modify: `src/ui/mod.rs`

**Interfaces:**
- Consumes: `EditorView` (`text`, `selection_text`, `apply_markdown`, `buffer`), `chat::{Conversation, Entry, ProposalState}` (`entries`, `history`, `is_busy`, `begin_request`, `finish_request`, `reset`, `apply_proposal`, `reject_proposal`), `prompt::build_messages`, `prompt::Mode`, `config::Config::load`, `llm::complete`, `llm::LlmError::Config`, `chat_markup::{to_pango, escape}`, `proposal::Edit`.
- Produces: `ui::chat_pane::ChatPane`: `new(editor: EditorView) -> Rc<ChatPane>`, `widget(&self) -> &gtk::Box`, `set_mode(&self, mode: Mode)`. CSS classes used by Task 9's stylesheet: rows `chat-user`, `chat-assistant`, `chat-error`, `chat-proposal`; labels `edit-original`, `edit-replacement`.

Behaviour notes: layout top to bottom is message list, selection chip, input, bottom row (New conversation, spinner + "Waiting for the LLM…", Send). `render()` diffs the conversation against the rendered entries (clear if shorter, replace changed rows in place, append new rows and scroll to the end once the adjustment reports its new size). Enter/Ctrl+Enter send from a capture-phase key controller; Shift+Enter falls through to the text view. The LLM call runs in `gio::spawn_blocking`; the result is applied in `glib::spawn_future_local` through a `Weak` reference; `Conversation`'s generation check drops replies from before *New conversation*. `RefCell` borrows are never held across calls into GTK that can emit signals.

- [ ] **Step 1: Create `src/ui/chat_pane.rs`**

`src/ui/chat_pane.rs`:

````rust
//! The chat pane: selection chip, message list with proposal cards, input and busy state.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};

use crate::chat::{Conversation, Entry, ProposalState};
use crate::chat_markup;
use crate::config::Config;
use crate::llm::{self, LlmError};
use crate::prompt::{self, Mode};
use crate::proposal::Edit;
use crate::ui::editor::EditorView;

const NO_SELECTION: &str = "No selection — whole document";
const WORKER_FAILED: &str = "The request failed unexpectedly.";

pub struct ChatPane {
    root: gtk::Box,
    editor: EditorView,
    conversation: RefCell<Conversation>,
    mode: Cell<Mode>,
    selection_label: gtk::Label,
    list: gtk::ListBox,
    /// The entries currently shown, one per list row.
    rendered: RefCell<Vec<Entry>>,
    /// Set when rows were appended; the list scrolls to the end once its size is known.
    scroll_to_end: Cell<bool>,
    input: gtk::TextView,
    placeholder: gtk::Label,
    send_button: gtk::Button,
    spinner: adw::Spinner,
    busy_label: gtk::Label,
}

impl ChatPane {
    pub fn new(editor: EditorView) -> Rc<Self> {
        let selection_label = gtk::Label::builder()
            .label(NO_SELECTION)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["dim-label"])
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .valign(gtk::Align::Start)
            .css_classes(["boxed-list-separate"])
            .build();
        let messages = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let input = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(8)
            .right_margin(8)
            .build();
        let placeholder = gtk::Label::builder()
            .xalign(0.0)
            .yalign(0.0)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .margin_top(8)
            .margin_start(8)
            .margin_end(8)
            .can_target(false)
            .css_classes(["dim-label"])
            .build();
        let input_overlay = gtk::Overlay::builder().child(&input).build();
        input_overlay.add_overlay(&placeholder);
        let input_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .min_content_height(64)
            .max_content_height(160)
            .child(&input_overlay)
            .css_classes(["card"])
            .build();

        let spinner = adw::Spinner::builder().visible(false).build();
        let busy_label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["dim-label"])
            .build();
        let send_button = gtk::Button::builder()
            .label("Send")
            .sensitive(false)
            .css_classes(["suggested-action"])
            .build();
        let new_conversation = gtk::Button::with_label("New conversation");
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bottom.append(&new_conversation);
        bottom.append(&spinner);
        bottom.append(&busy_label);
        bottom.append(&send_button);

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        root.append(&messages);
        root.append(&selection_label);
        root.append(&input_scroller);
        root.append(&bottom);

        let pane = Rc::new(Self {
            root,
            editor,
            conversation: RefCell::new(Conversation::default()),
            mode: Cell::new(Mode::Sparring),
            selection_label,
            list,
            rendered: RefCell::new(Vec::new()),
            scroll_to_end: Cell::new(false),
            input,
            placeholder,
            send_button,
            spinner,
            busy_label,
        });
        pane.update_placeholder();

        new_conversation.connect_clicked(glib::clone!(
            #[weak]
            pane,
            move |_| {
                pane.conversation.borrow_mut().reset();
                pane.render();
            }
        ));
        pane.send_button.connect_clicked(glib::clone!(
            #[weak]
            pane,
            move |_| pane.send()
        ));
        pane.input.buffer().connect_changed(glib::clone!(
            #[weak]
            pane,
            move |_| {
                pane.update_placeholder();
                pane.update_send_button();
            }
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            pane,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, state| {
                let enter = matches!(
                    key,
                    gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
                );
                if !enter || state.contains(gdk::ModifierType::SHIFT_MASK) {
                    // Shift+Enter falls through to the text view, which inserts a newline.
                    return glib::Propagation::Proceed;
                }
                pane.send();
                glib::Propagation::Stop
            }
        ));
        pane.input.add_controller(keys);

        messages.vadjustment().connect_changed(glib::clone!(
            #[weak]
            pane,
            move |adjustment| {
                if pane.scroll_to_end.take() {
                    adjustment.set_value(adjustment.upper() - adjustment.page_size());
                }
            }
        ));

        let buffer = pane.editor.buffer().clone();
        buffer.connect_has_selection_notify(glib::clone!(
            #[weak]
            pane,
            move |_| pane.update_selection()
        ));
        buffer.connect_mark_set(glib::clone!(
            #[weak]
            pane,
            move |buffer, _, mark| {
                if *mark == buffer.get_insert() || *mark == buffer.selection_bound() {
                    pane.update_selection();
                }
            }
        ));
        pane
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_mode(&self, mode: Mode) {
        self.mode.set(mode);
        self.update_placeholder();
    }

    fn send(self: &Rc<Self>) {
        let buffer = self.input.buffer();
        let (start, end) = buffer.bounds();
        let text = buffer.text(&start, &end, false);
        let user_input = text.trim();
        if user_input.is_empty() || self.conversation.borrow().is_busy() {
            return;
        }
        let mode = self.mode.get();
        let selection = self.editor.selection_text();
        let messages = prompt::build_messages(
            mode,
            &self.editor.text(),
            Some(&selection),
            self.conversation.borrow().history(),
            user_input,
        );
        let Some(ticket) = self
            .conversation
            .borrow_mut()
            .begin_request(mode, user_input)
        else {
            return;
        };
        buffer.set_text("");
        self.render();

        let reply = gio::spawn_blocking(move || {
            Config::load()
                .map_err(LlmError::Config)
                .and_then(|config| llm::complete(&config, &messages))
                .map_err(|e| e.to_string())
        });
        let pane = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = reply
                .await
                .unwrap_or_else(|_| Err(WORKER_FAILED.to_string()));
            let Some(pane) = pane.upgrade() else {
                return;
            };
            let current = pane
                .conversation
                .borrow_mut()
                .finish_request(ticket, result);
            if current {
                pane.render();
            }
        });
    }

    fn apply(self: &Rc<Self>, index: usize) {
        let result = self
            .conversation
            .borrow_mut()
            .apply_proposal(index, &self.editor.text());
        if let Ok(markdown) = result {
            self.editor.apply_markdown(&markdown);
        }
        self.render();
    }

    fn reject(self: &Rc<Self>, index: usize) {
        self.conversation.borrow_mut().reject_proposal(index);
        self.render();
    }

    /// Brings the message list and busy state in line with the conversation.
    fn render(self: &Rc<Self>) {
        let entries = self.conversation.borrow().entries().to_vec();
        let mut rendered = self.rendered.borrow_mut();
        if entries.len() < rendered.len() {
            self.list.remove_all();
            rendered.clear();
        }
        for (index, entry) in entries.iter().enumerate() {
            match rendered.get(index) {
                Some(shown) if shown == entry => {}
                Some(_) => {
                    let position = i32::try_from(index).expect("row index fits in i32");
                    if let Some(row) = self.list.row_at_index(position) {
                        self.list.remove(&row);
                    }
                    self.list.insert(&self.row(index, entry), position);
                    rendered[index] = entry.clone();
                }
                None => {
                    self.list.append(&self.row(index, entry));
                    rendered.push(entry.clone());
                    self.scroll_to_end.set(true);
                }
            }
        }
        let busy = self.conversation.borrow().is_busy();
        self.spinner.set_visible(busy);
        self.busy_label
            .set_text(if busy { "Waiting for the LLM…" } else { "" });
        self.update_send_button();
    }

    fn row(self: &Rc<Self>, index: usize, entry: &Entry) -> gtk::ListBoxRow {
        let (child, class): (gtk::Widget, &str) = match entry {
            Entry::User { text } => (text_label(text).upcast(), "chat-user"),
            Entry::Assistant { text } => (
                markup_label(&chat_markup::to_pango(text)).upcast(),
                "chat-assistant",
            ),
            Entry::Error { text } => {
                let label = text_label(text);
                label.add_css_class("error");
                (label.upcast(), "chat-error")
            }
            Entry::Proposal {
                explanation,
                edits,
                state,
            } => (
                self.proposal_card(index, explanation, edits, *state)
                    .upcast(),
                "chat-proposal",
            ),
        };
        child.set_margin_top(10);
        child.set_margin_bottom(10);
        child.set_margin_start(12);
        child.set_margin_end(12);
        gtk::ListBoxRow::builder()
            .activatable(false)
            .selectable(false)
            .css_classes([class])
            .child(&child)
            .build()
    }

    fn proposal_card(
        self: &Rc<Self>,
        index: usize,
        explanation: &str,
        edits: &[Edit],
        state: ProposalState,
    ) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.append(
            &gtk::Label::builder()
                .label("Proposed change")
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
        if !explanation.is_empty() {
            card.append(&markup_label(&chat_markup::to_pango(explanation)));
        }
        for (number, edit) in edits.iter().enumerate() {
            if edits.len() > 1 {
                let heading = text_label(&format!("Edit {} of {}", number + 1, edits.len()));
                heading.add_css_class("dim-label");
                card.append(&heading);
            }
            let original = markup_label(&format!("<s>{}</s>", chat_markup::escape(&edit.original)));
            original.add_css_class("edit-original");
            card.append(&original);
            let replacement = if edit.replacement.is_empty() {
                markup_label("<i>(delete)</i>")
            } else {
                text_label(&edit.replacement)
            };
            replacement.add_css_class("edit-replacement");
            card.append(&replacement);
        }
        match state {
            ProposalState::Pending => {
                let apply = gtk::Button::builder()
                    .label("Apply")
                    .css_classes(["suggested-action"])
                    .build();
                apply.connect_clicked(glib::clone!(
                    #[weak(rename_to = pane)]
                    self,
                    move |_| pane.apply(index)
                ));
                let reject = gtk::Button::with_label("Reject");
                reject.connect_clicked(glib::clone!(
                    #[weak(rename_to = pane)]
                    self,
                    move |_| pane.reject(index)
                ));
                let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                buttons.append(&apply);
                buttons.append(&reject);
                card.append(&buttons);
            }
            ProposalState::Applied | ProposalState::Rejected => {
                let done = text_label(if state == ProposalState::Applied {
                    "✓ Applied"
                } else {
                    "✗ Rejected"
                });
                done.add_css_class("dim-label");
                card.append(&done);
            }
        }
        card
    }

    fn update_selection(&self) {
        let selection = self.editor.selection_text();
        let text = if selection.is_empty() {
            NO_SELECTION.to_string()
        } else {
            let collapsed: Vec<&str> = selection.split_whitespace().collect();
            format!("Selection: “{}”", collapsed.join(" "))
        };
        self.selection_label.set_text(&text);
    }

    fn update_placeholder(&self) {
        let hint = match self.mode.get() {
            Mode::Sparring => "Ask about the text…",
            Mode::Ghostwriting => "Ask for a change…",
        };
        self.placeholder.set_text(&format!(
            "{hint} (Enter to send, Shift+Enter for a new line)"
        ));
        self.placeholder
            .set_visible(self.input.buffer().char_count() == 0);
    }

    fn update_send_button(&self) {
        let buffer = self.input.buffer();
        let (start, end) = buffer.bounds();
        let blank = buffer.text(&start, &end, false).trim().is_empty();
        self.send_button
            .set_sensitive(!blank && !self.conversation.borrow().is_busy());
    }
}

fn text_label(text: &str) -> gtk::Label {
    let label = base_label();
    label.set_text(text);
    label
}

fn markup_label(markup: &str) -> gtk::Label {
    let label = base_label();
    label.set_markup(markup);
    label
}

fn base_label() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .selectable(true)
        .build()
}
````

- [ ] **Step 2: Register the module** — add `pub mod chat_pane;` to `src/ui/mod.rs` above `pub mod editor;`.

- [ ] **Step 3: Verify**

Run: `cargo fmt && cargo fmt --check && cargo build 2>&1 | grep -c '^warning'`
Expected: `0`

Run: `cargo clippy --all-targets 2>&1 | grep -c '^warning'`
Expected: `0`

Run: `dev/headless.sh cargo test 2>&1 | grep -E "test result|passed|FAIL"`
Expected: all green as after Task 6. (The pane is exercised end to end in Task 9's smoke run and the desktop checklist.)

- [ ] **Step 4: Commit**

```bash
git add src/ui
git commit -m "Add the chat pane with proposal cards

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Options dialog

**Files:**
- Create: `src/ui/options_dialog.rs`
- Modify: `src/ui/mod.rs`

**Interfaces:**
- Consumes: `config::{self, Config}` (`load`, `from_fields`, `save`, `settings_path`, `Default`), `llm::list_models`, `model_requests::{self, ModelRequests}` (`begin`, `is_current`, `summary`).
- Produces: `ui::options_dialog::OptionsDialog::present(parent: &impl IsA<gtk::Widget>)`.

Behaviour notes: the dialog state lives in an `Rc<OptionsDialog>`; widget callbacks hold `Weak` references and a `RefCell<Option<Rc<…>>>` moved into the `closed` handler keeps it alive until the dialog closes (then it is taken and dropped, freeing the dialog). On open: `Config::load()`; on error the fields show the defaults, the error label shows the message and Save reads "Overwrite". The model list is fetched on open and on Refresh (forced), and on Enter / focus-leave of the base URL and API key rows only if the pair changed. Only the latest ticket's result is shown.

- [ ] **Step 1: Create `src/ui/options_dialog.rs`**

`src/ui/options_dialog.rs`:

````rust
//! The Options dialog: LLM endpoint settings and the endpoint's model list.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::config::{self, Config};
use crate::llm;
use crate::model_requests::{self, ModelRequests};

const WORKER_FAILED: &str = "The request failed unexpectedly.";

pub struct OptionsDialog {
    dialog: adw::Dialog,
    base_url: adw::EntryRow,
    api_key: adw::PasswordEntryRow,
    model: adw::EntryRow,
    models: gtk::ListBox,
    models_button: gtk::MenuButton,
    refresh: gtk::Button,
    spinner: adw::Spinner,
    status: gtk::Label,
    requests: RefCell<ModelRequests>,
}

impl OptionsDialog {
    /// Shows the dialog over `parent` with the saved settings and a freshly loaded model list.
    pub fn present(parent: &impl IsA<gtk::Widget>) {
        let base_url = adw::EntryRow::builder()
            .title("Base URL (OpenAI-compatible)")
            .build();
        let api_key = adw::PasswordEntryRow::builder()
            .title("API key (optional)")
            .build();
        let model = adw::EntryRow::builder().title("Model").build();

        let models = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["navigation-sidebar"])
            .build();
        let models_popover = gtk::Popover::builder()
            .child(
                &gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .propagate_natural_height(true)
                    .max_content_height(300)
                    .child(&models)
                    .build(),
            )
            .build();
        let models_button = gtk::MenuButton::builder()
            .icon_name("pan-down-symbolic")
            .tooltip_text("Available models")
            .valign(gtk::Align::Center)
            .sensitive(false)
            .popover(&models_popover)
            .css_classes(["flat"])
            .build();
        let refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh the model list")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        model.add_suffix(&models_button);
        model.add_suffix(&refresh);

        let group = adw::PreferencesGroup::new();
        group.add(&base_url);
        group.add(&api_key);
        group.add(&model);

        let spinner = adw::Spinner::builder().visible(false).build();
        let status = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .hexpand(true)
            .css_classes(["dim-label"])
            .build();
        let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        status_row.append(&spinner);
        status_row.append(&status);

        let load_error = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["error"])
            .build();
        let path = config::settings_path()
            .map(|path| format!("Stored in {} (readable only by you).", path.display()))
            .unwrap_or_else(|message| message);
        let path_label = gtk::Label::builder()
            .label(path)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label", "caption"])
            .build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        content.append(&group);
        content.append(&status_row);
        content.append(&load_error);
        content.append(&path_label);

        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::builder()
            .label("Save")
            .css_classes(["suggested-action"])
            .build();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&save);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&content));

        let dialog = adw::Dialog::builder()
            .title("Options")
            .content_width(560)
            .child(&toolbar)
            .build();

        let this = Rc::new(Self {
            dialog,
            base_url,
            api_key,
            model,
            models,
            models_button,
            refresh,
            spinner,
            status,
            requests: RefCell::new(ModelRequests::default()),
        });

        match Config::load() {
            Ok(config) => this.show_config(&config),
            Err(message) => {
                this.show_config(&Config::default());
                load_error.set_text(&message);
                load_error.set_visible(true);
                save.set_label("Overwrite");
            }
        }

        cancel.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            this.dialog,
            move |_| {
                dialog.close();
            }
        ));
        save.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| this.save()
        ));
        this.refresh.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| this.fetch_models(true)
        ));
        this.models.connect_row_activated(glib::clone!(
            #[weak]
            this,
            move |_, row| {
                if let Some(label) = row.child().and_downcast::<gtk::Label>() {
                    this.model.set_text(&label.text());
                }
                this.models_button.popdown();
            }
        ));
        for row in [
            this.base_url.upcast_ref::<adw::EntryRow>(),
            this.api_key.upcast_ref(),
        ] {
            row.connect_entry_activated(glib::clone!(
                #[weak]
                this,
                move |_| this.fetch_models(false)
            ));
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(
                #[weak]
                this,
                move |_| this.fetch_models(false)
            ));
            row.add_controller(focus);
        }

        // The dialog's widgets only hold weak references; keep the state alive until it closes.
        let keep_alive = RefCell::new(Some(this.clone()));
        this.dialog.connect_closed(move |_| {
            keep_alive.take();
        });

        this.dialog.present(Some(parent));
        this.fetch_models(true);
    }

    fn show_config(&self, config: &Config) {
        self.base_url.set_text(&config.base_url);
        self.api_key
            .set_text(config.api_key.as_deref().unwrap_or(""));
        self.model.set_text(config.model.as_deref().unwrap_or(""));
    }

    fn fetch_models(self: &Rc<Self>, force: bool) {
        let base_url = self.base_url.text();
        let api_key = self.api_key.text();
        let Some(ticket) = self.requests.borrow_mut().begin(&base_url, &api_key, force) else {
            return;
        };
        self.spinner.set_visible(true);
        self.refresh.set_sensitive(false);
        self.status.set_text("Loading models…");

        let config = Config::from_fields(&base_url, &api_key, "");
        let models =
            gio::spawn_blocking(move || llm::list_models(&config).map_err(|e| e.to_string()));
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = models
                .await
                .unwrap_or_else(|_| Err(WORKER_FAILED.to_string()));
            let Some(this) = this.upgrade() else {
                return;
            };
            if this.requests.borrow().is_current(ticket) {
                this.show_models(result);
            }
        });
    }

    fn show_models(&self, result: Result<Vec<String>, String>) {
        self.spinner.set_visible(false);
        self.refresh.set_sensitive(true);
        self.models.remove_all();
        let models = match result {
            Ok(models) => {
                self.status.set_text(&model_requests::summary(models.len()));
                models
            }
            Err(message) => {
                self.status.set_text(&message);
                Vec::new()
            }
        };
        for id in &models {
            self.models
                .append(&gtk::Label::builder().label(id).xalign(0.0).build());
        }
        self.models_button.set_sensitive(!models.is_empty());
    }

    fn save(&self) {
        let config = Config::from_fields(
            &self.base_url.text(),
            &self.api_key.text(),
            &self.model.text(),
        );
        match config.save() {
            Ok(()) => {
                self.dialog.close();
            }
            Err(message) => self.status.set_text(&message),
        }
    }
}
````

- [ ] **Step 2: Register the module** — add `pub mod options_dialog;` to `src/ui/mod.rs` below `pub mod editor;`.

- [ ] **Step 3: Verify**

Run: `cargo fmt && cargo fmt --check && cargo build 2>&1 | grep -c '^warning'` → `0`
Run: `cargo clippy --all-targets 2>&1 | grep -c '^warning'` → `0`

- [ ] **Step 4: Commit**

```bash
git add src/ui
git commit -m "Add the Options dialog with the endpoint's model list

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Main window, application actions and headless smoke run

**Files:**
- Create: `src/ui/window.rs`
- Replace: `src/ui/mod.rs`

**Interfaces:**
- Consumes: `EditorView`, `ChatPane::{new, widget, set_mode}`, `OptionsDialog::present`, `document::{read_file, from_disk, to_disk, write_file}`, `prompt::Mode`.
- Produces: `ui::window::MainWindow::{new(app: &adw::Application) -> Rc<MainWindow>, present(&self)}`; actions `win.open`, `win.save`, `win.save-as`, `win.options`, `app.shortcuts`, `app.quit` with accelerators Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+comma, Ctrl+question, Ctrl+Q.

Behaviour notes: the only strong reference to `MainWindow` is held by the `close-request` handler; GTK destroys it with the window. Open, close and Quit share `confirm_discard()`, an `AdwAlertDialog` (Cancel / Discard destructive / Save suggested; Save continues only if saving succeeded, including via Save As). Quit closes the active window so it passes the same guard. The title shows "• " while modified; the subtitle is the folder. The editor's style scheme follows `adw::StyleManager`'s `dark` property. The application stylesheet tints user and error rows and the proposal edits using libadwaita's CSS variables.

- [ ] **Step 1: Create `src/ui/window.rs`**

`src/ui/window.rs`:

````rust
//! The main window: header bar, editor, chat pane and file handling.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::document;
use crate::prompt::Mode;
use crate::ui::chat_pane::ChatPane;
use crate::ui::editor::EditorView;
use crate::ui::options_dialog::OptionsDialog;

pub struct MainWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    editor: EditorView,
    /// The open file, or `None` for a new document.
    path: RefCell<Option<PathBuf>>,
    /// True if the open file uses CRLF line endings, so saving restores them.
    crlf: Cell<bool>,
    /// Set once the user has saved or discarded changes, so the window may close.
    close_confirmed: Cell<bool>,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let editor = EditorView::new();
        let editor_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .width_request(300)
            .child(editor.widget())
            .build();
        let chat = ChatPane::new(editor.clone());
        chat.widget().set_width_request(280);

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&editor_scroller)
            .end_child(chat.widget())
            .resize_start_child(true)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(960)
            .build();

        let mode = adw::ToggleGroup::new();
        mode.add(
            adw::Toggle::builder()
                .name("sparring")
                .label("Sparring")
                .tooltip("The LLM can read the document but not change it")
                .build(),
        );
        mode.add(
            adw::Toggle::builder()
                .name("ghostwriting")
                .label("Ghostwriting")
                .tooltip("The LLM can propose changes that you apply or reject")
                .build(),
        );
        mode.set_active_name(Some("sparring"));
        mode.connect_active_name_notify(glib::clone!(
            #[weak]
            chat,
            move |group| {
                chat.set_mode(match group.active_name().as_deref() {
                    Some("ghostwriting") => Mode::Ghostwriting,
                    _ => Mode::Sparring,
                });
            }
        ));

        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main Menu")
            .menu_model(&primary_menu())
            .primary(true)
            .build();

        let title = adw::WindowTitle::new("", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_end(&menu_button);
        header.pack_end(&mode);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&paned));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1400)
            .default_height(850)
            .content(&toolbar)
            .build();

        let this = Rc::new(Self {
            window,
            title,
            editor,
            path: RefCell::new(None),
            crlf: Cell::new(false),
            close_confirmed: Cell::new(false),
        });
        this.update_title();
        this.follow_dark_mode();
        this.add_actions();

        this.editor.buffer().connect_modified_changed(glib::clone!(
            #[weak]
            this,
            move |_| this.update_title()
        ));
        // Holds the only strong reference; GTK drops it when the window is destroyed.
        this.window.connect_close_request(glib::clone!(
            #[strong]
            this,
            move |_| this.on_close_request()
        ));
        this
    }

    pub fn present(&self) {
        self.window.present();
        self.editor.widget().grab_focus();
    }

    fn add_actions(self: &Rc<Self>) {
        let open = gio::ActionEntry::builder("open")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    glib::spawn_future_local(async move { this.open().await });
                }
            ))
            .build();
        let save = gio::ActionEntry::builder("save")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    glib::spawn_future_local(async move {
                        this.save().await;
                    });
                }
            ))
            .build();
        let save_as = gio::ActionEntry::builder("save-as")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    glib::spawn_future_local(async move {
                        this.save_as().await;
                    });
                }
            ))
            .build();
        let options = gio::ActionEntry::builder("options")
            .activate(|window: &adw::ApplicationWindow, _, _| OptionsDialog::present(window))
            .build();
        self.window
            .add_action_entries([open, save, save_as, options]);
    }

    fn on_close_request(self: &Rc<Self>) -> glib::Propagation {
        if self.close_confirmed.get() || !self.editor.buffer().is_modified() {
            return glib::Propagation::Proceed;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            if this.confirm_discard().await {
                this.close_confirmed.set(true);
                this.window.close();
            }
        });
        glib::Propagation::Stop
    }

    /// Asks what to do with unsaved changes. Returns true if the caller may go on: there were no
    /// changes, the user discarded them, or they were saved.
    async fn confirm_discard(self: &Rc<Self>) -> bool {
        if !self.editor.buffer().is_modified() {
            return true;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Save changes?")
            .body("The document has unsaved changes. Save them first?")
            .default_response("save")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[
            ("cancel", "_Cancel"),
            ("discard", "_Discard"),
            ("save", "_Save"),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        match dialog.choose_future(Some(&self.window)).await.as_str() {
            "discard" => true,
            "save" => self.save().await,
            _ => false,
        }
    }

    async fn open(self: &Rc<Self>) {
        if !self.confirm_discard().await {
            return;
        }
        let dialog = file_dialog("Open Markdown File");
        let Ok(file) = dialog.open_future(Some(&self.window)).await else {
            return;
        };
        let Some(path) = file.path() else {
            return;
        };
        match document::read_file(&path) {
            Ok(contents) => {
                let (text, crlf) = document::from_disk(&contents);
                self.crlf.set(crlf);
                self.editor.load(&text);
                self.set_path(path);
            }
            Err(message) => self.show_error(&message),
        }
    }

    /// Saves to the open file, or asks for a file name first. Returns true once saved.
    async fn save(self: &Rc<Self>) -> bool {
        let path = self.path.borrow().clone();
        match path {
            Some(path) => self.write_to(path),
            None => self.save_as().await,
        }
    }

    async fn save_as(self: &Rc<Self>) -> bool {
        let dialog = file_dialog("Save Markdown File");
        match self.path.borrow().as_ref() {
            Some(path) => dialog.set_initial_file(Some(&gio::File::for_path(path))),
            None => dialog.set_initial_name(Some("Untitled.md")),
        }
        let Ok(file) = dialog.save_future(Some(&self.window)).await else {
            return false;
        };
        match file.path() {
            Some(path) => self.write_to(path),
            None => false,
        }
    }

    fn write_to(&self, path: PathBuf) -> bool {
        let contents = document::to_disk(&self.editor.text(), self.crlf.get());
        match document::write_file(&path, &contents) {
            Ok(()) => {
                self.editor.buffer().set_modified(false);
                self.set_path(path);
                true
            }
            Err(message) => {
                self.show_error(&message);
                false
            }
        }
    }

    fn set_path(&self, path: PathBuf) {
        *self.path.borrow_mut() = Some(path);
        self.update_title();
    }

    fn update_title(&self) {
        let path = self.path.borrow();
        let name = path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        let folder = path
            .as_deref()
            .and_then(Path::parent)
            .map(|folder| folder.display().to_string())
            .unwrap_or_default();
        let marker = if self.editor.buffer().is_modified() {
            "• "
        } else {
            ""
        };
        self.title.set_title(&format!("{marker}{name}"));
        self.title.set_subtitle(&folder);
        self.window
            .set_title(Some(&format!("{marker}{name} — Counterpoint")));
    }

    fn follow_dark_mode(self: &Rc<Self>) {
        let style = adw::StyleManager::default();
        self.editor.set_dark(style.is_dark());
        style.connect_dark_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |style| this.editor.set_dark(style.is_dark())
        ));
    }

    fn show_error(&self, message: &str) {
        let dialog = adw::AlertDialog::new(Some("Error"), Some(message));
        dialog.add_response("ok", "_OK");
        dialog.present(Some(&self.window));
    }
}

fn primary_menu() -> gio::Menu {
    let file = gio::Menu::new();
    file.append(Some("_Open…"), Some("win.open"));
    file.append(Some("_Save"), Some("win.save"));
    file.append(Some("Save _As…"), Some("win.save-as"));
    let tools = gio::Menu::new();
    tools.append(Some("_Options…"), Some("win.options"));
    tools.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
    let quit = gio::Menu::new();
    quit.append(Some("_Quit"), Some("app.quit"));
    let menu = gio::Menu::new();
    menu.append_section(None, &file);
    menu.append_section(None, &tools);
    menu.append_section(None, &quit);
    menu
}

fn file_dialog(title: &str) -> gtk::FileDialog {
    let markdown = gtk::FileFilter::new();
    markdown.set_name(Some("Markdown files"));
    markdown.add_pattern("*.md");
    markdown.add_pattern("*.markdown");
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&markdown);
    filters.append(&all);
    gtk::FileDialog::builder()
        .title(title)
        .modal(true)
        .filters(&filters)
        .default_filter(&markdown)
        .build()
}
````

- [ ] **Step 2: Replace `src/ui/mod.rs`**

`src/ui/mod.rs`:

````rust
//! The GTK user interface.

pub mod chat_pane;
pub mod editor;
pub mod options_dialog;
pub mod window;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use window::MainWindow;

pub const APP_ID: &str = "de.marcusleg.Counterpoint";

const STYLE: &str = "
.boxed-list-separate > row.chat-user { background-color: alpha(var(--accent-bg-color), 0.12); }
.boxed-list-separate > row.chat-error { background-color: alpha(var(--error-bg-color), 0.12); }
label.edit-original,
label.edit-replacement { border-radius: 6px; padding: 6px 8px; }
label.edit-original { background-color: alpha(var(--error-bg-color), 0.15); }
label.edit-replacement { background-color: alpha(var(--success-bg-color), 0.15); }
";

/// (action, accelerators, label) for every application shortcut, in menu order.
const SHORTCUTS: &[(&str, &str, &str)] = &[
    ("win.open", "<Control>o", "Open"),
    ("win.save", "<Control>s", "Save"),
    ("win.save-as", "<Control><Shift>s", "Save As"),
    ("win.options", "<Control>comma", "Options"),
    ("app.shortcuts", "<Control>question", "Keyboard Shortcuts"),
    ("app.quit", "<Control>q", "Quit"),
];

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|app| {
        sourceview5::init();
        load_style();
        add_app_actions(app);
    });
    app.connect_activate(|app| match app.active_window() {
        Some(window) => window.present(),
        None => MainWindow::new(app).present(),
    });
    app.run()
}

fn load_style() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn add_app_actions(app: &adw::Application) {
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| {
            // Closing goes through the window's unsaved-changes check; the app exits with it.
            match app.active_window() {
                Some(window) => window.close(),
                None => app.quit(),
            }
        })
        .build();
    let shortcuts = gio::ActionEntry::builder("shortcuts")
        .activate(|app: &adw::Application, _, _| {
            shortcuts_dialog().present(app.active_window().as_ref());
        })
        .build();
    app.add_action_entries([quit, shortcuts]);
    for (action, accel, _) in SHORTCUTS {
        app.set_accels_for_action(action, &[accel]);
    }
}

fn shortcuts_dialog() -> adw::ShortcutsDialog {
    let file = adw::ShortcutsSection::new(Some("Application"));
    for (_, accel, label) in SHORTCUTS {
        file.add(adw::ShortcutsItem::new(label, accel));
    }
    let editor = adw::ShortcutsSection::new(Some("Editor"));
    editor.add(adw::ShortcutsItem::new("Undo", "<Control>z"));
    editor.add(adw::ShortcutsItem::new("Redo", "<Control><Shift>z"));
    let chat = adw::ShortcutsSection::new(Some("Chat"));
    chat.add(adw::ShortcutsItem::new("Send message", "Return"));
    chat.add(adw::ShortcutsItem::new("Send message", "<Control>Return"));
    chat.add(adw::ShortcutsItem::new("New line", "<Shift>Return"));
    let dialog = adw::ShortcutsDialog::new();
    dialog.add(file);
    dialog.add(editor);
    dialog.add(chat);
    dialog
}
````

- [ ] **Step 3: Verify build, lints and tests**

Run: `cargo fmt && cargo fmt --check && cargo build 2>&1 | grep -c '^warning'` → `0`
Run: `cargo clippy --all-targets 2>&1 | grep -c '^warning'` → `0`
Run: `dev/headless.sh cargo test 2>&1 | grep -E "test result|passed|FAIL"` → all ok, no `FAIL:`.

- [ ] **Step 4: Headless smoke run (settings isolated from the user's)**

```bash
mkdir -p target/smoke-config
XDG_CONFIG_HOME=$PWD/target/smoke-config dev/headless.sh timeout 5 target/debug/counterpoint; echo "exit $?"
```

Expected: no output other than `exit 124` (the timeout ended a healthy run). Any `Gtk-CRITICAL`, `Gtk-WARNING`, `Adwaita-WARNING` or `Theme parser error` line is a failure to fix before committing.

- [ ] **Step 5: Commit**

```bash
git add src/ui
git commit -m "Add the main window with header bar, primary menu and file handling

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Documentation, final verification and desktop checklist

**Files:**
- Replace: `README.md`
- Modify: `dev/mock_llm_server.py`

- [ ] **Step 1: Replace `README.md`**

`README.md`:

````markdown
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
  preview and click **Apply** or **Reject**. An applied proposal is a single undo step (Ctrl+Z).

## Requirements

- Rust 1.92 or newer
- GTK 4.18 or newer, libadwaita 1.8 or newer and GtkSourceView 5.12 or newer, with development
  files (Fedora: `sudo dnf install gtk4-devel libadwaita-devel gtksourceview5-devel`)
- An OpenAI-compatible chat completions endpoint (for example Ollama, llama.cpp, or a hosted provider)

## Configuration

Open **Options…** in the main menu (☰) to connect to an OpenAI-compatible endpoint:

- **Base URL**, for example `http://localhost:11434/v1` for Ollama (the default).
- **API key**, optional; sent as a bearer token.
- **Model**, picked from the endpoint's model list (`GET /models`, loaded when the dialog opens
  or on refresh) or typed in.

Settings are stored in `~/.config/counterpoint/settings.json` (or under `$XDG_CONFIG_HOME`),
readable only by you. Changes apply to the next chat message.

## Build and run

```sh
cargo run --release
```

To try the editor without a real model, start the mock server in another terminal, run the
editor, and in **Options…** set the base URL to `http://127.0.0.1:8765/v1` and pick the model
`mock`:

```sh
python3 dev/mock_llm_server.py
cargo run
```

## Keyboard

| Keys              | Action                                   |
|-------------------|------------------------------------------|
| Ctrl+O            | Open                                     |
| Ctrl+S            | Save                                     |
| Ctrl+Shift+S      | Save As                                  |
| Ctrl+,            | Options                                  |
| Ctrl+?            | Keyboard shortcuts                       |
| Ctrl+Q            | Quit                                     |
| Ctrl+Z            | Undo (an applied proposal is one step)   |
| Ctrl+Shift+Z      | Redo                                     |
| Enter, Ctrl+Enter | Send chat message                        |
| Shift+Enter       | New line in the chat input               |

## Development checks

```sh
cargo fmt --check
dev/headless.sh cargo test
```

`dev/headless.sh` runs a command against a private GTK Broadway display (`gtk4-broadwayd`, part
of Fedora's `gtk4` package), so no window appears on your desktop. Besides the unit tests,
`cargo test` runs `tests/gtk_editor.rs`, which checks the editor buffer: every Markdown file in
the repository and in `tests/fixtures/` must survive loading and saving byte for byte, loading
must leave nothing to undo, and an applied proposal must be exactly one undo step. Without a
display it prints `SKIPPED: no display`.

To check that your own files round-trip unchanged without adding them to the repository, list
them, separated by colons, in `COUNTERPOINT_ROUNDTRIP_FILES`:

```sh
COUNTERPOINT_ROUNDTRIP_FILES=post.md:notes.md dev/headless.sh cargo test --test gtk_editor
```

The fixtures are generated by `python3 tests/fixtures/generate.py`, which also verifies their
bytes.

## Known limitations

- Styling comes from GtkSourceView's Markdown highlighting: headings are not enlarged, and not
  every CommonMark edge case is covered.
- Line endings: a file that contains any CRLF (`\r\n`) is saved entirely with CRLF, otherwise
  with LF.
- Responses are not streamed; the chat shows a busy indicator until the full reply arrives.
- Chat history is not persisted and is sent in full with every request.
- An applied proposal stays marked as applied after you undo it in the editor, and cannot be
  applied again; ask for a new proposal instead.
- Chat replies render a subset of Markdown (no tables or images).

## License

MIT, see [LICENSE](LICENSE).
````

- [ ] **Step 2: Update the mock server docstring**

In `dev/mock_llm_server.py`, replace the line

```
Usage: python3 dev/mock_llm_server.py, then set Tools > Options… to base URL http://127.0.0.1:8765/v1 and model "mock".
```

with

```
Usage: python3 dev/mock_llm_server.py, then set Options… in the main menu to base URL
http://127.0.0.1:8765/v1 and model "mock".
```

- [ ] **Step 3: Check that the mock server still works**

```bash
python3 dev/mock_llm_server.py & sleep 0.5
curl -s http://127.0.0.1:8765/v1/models
curl -s -X POST http://127.0.0.1:8765/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"model":"mock","messages":[{"role":"system","content":"You are a sparring partner"},{"role":"user","content":"hi"}]}'
kill %1
```

Expected: `{"object": "list", "data": [{"id": "mock", "object": "model"}]}` and a JSON reply containing `Mock sparring reply to: *hi*`.

- [ ] **Step 4: Full verification**

```bash
cargo fmt --check && echo fmt-ok
cargo build 2>&1 | grep -c '^warning'          # 0
cargo clippy --all-targets 2>&1 | grep -c '^warning'   # 0
dev/headless.sh cargo test 2>&1 | grep -E "test result|passed|FAIL|SKIPPED"
grep -rn "Qt\|qml\|QT_\|cxx\|Tools >" README.md Cargo.toml src dev tests || echo no-qt-left
grep -rn "#\[allow" src tests || echo no-allow
```

Expected: `fmt-ok`, `0`, `0`, all test results ok with no `FAIL:`/`SKIPPED`, `no-qt-left`, `no-allow`.

- [ ] **Step 5: Privacy scan of everything that will be committed on this branch**

```bash
git add -A
git diff --cached --name-only
git diff main --name-only | xargs -r grep -nIE "/home/|/Users/|@[a-z0-9-]+\.[a-z]{2,}" -- 2>/dev/null | grep -v "noreply@anthropic.com" || echo privacy-ok
```

Expected: only intended files; `privacy-ok` (or only generic example URLs such as `example.com`). Read the fixtures and README once more for personal text.

Then check for invisible characters outside the fixtures (the fixtures contain them on purpose):

```bash
git diff main --name-only --diff-filter=AM | grep -v '^tests/fixtures/.*\.md$' | python3 -c "
import sys, unicodedata
bad = 0
for path in sys.stdin.read().split():
    for n, line in enumerate(open(path, encoding='utf-8', errors='replace').read().split(chr(10)), 1):
        for c in line:
            if unicodedata.category(c) in ('Zs', 'Cf', 'Zl', 'Zp', 'Cc') and c not in ' ' + chr(9) + chr(13):
                bad += 1
                print(path, n, hex(ord(c)))
print('invisible-ok' if bad == 0 else f'{bad} invisible characters')
"
```

Expected: `invisible-ok`.

- [ ] **Step 6: Commit**

```bash
git commit -m "Document the GTK version and update dev tooling

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 7: Hand the desktop checklist to the user** (do not run the app on their desktop yourself):

1. `python3 dev/mock_llm_server.py` in one terminal, `cargo run` in another.
2. ☰ → Options… (Ctrl+,): the model list loads; set base URL `http://127.0.0.1:8765/v1`, pick `mock`, Save. Reopen: values persist, the path line is shown. Toggle the API key's eye icon.
3. Ctrl+O a real article with front matter and HTML comments: styling looks right in light and dark (switch in GNOME Settings while the app runs); Ctrl+Z right after opening does nothing.
4. Ctrl+S without edits, then `cmp` the file with a backup copy: identical.
5. Select a sentence: the chip above the input shows it; send in Sparring with Enter; Shift+Enter adds a line; Ctrl+Enter sends.
6. Switch to Ghostwriting, ask for a change, Apply: only the changed text moves, scroll position stays; one Ctrl+Z undoes the whole proposal. Reject another one.
7. Edit, then close the window / Ctrl+Q / Ctrl+O: Save / Discard / Cancel each behave as labelled.
8. Ctrl+? shows the shortcuts; scroll bars in editor and chat are usable; the pane divider drags.
