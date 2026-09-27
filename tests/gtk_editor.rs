//! GTK checks for the editor buffer and chat markup. GTK must run on the thread that initialised
//! it, so this test has its own `main` (`harness = false`) and runs every check in turn.
//! It needs a display; `dev/headless.sh cargo test` provides a private one and sets
//! `COUNTERPOINT_REQUIRE_DISPLAY` so a missing display fails loudly instead of skipping silently.

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
        if std::env::var_os("COUNTERPOINT_REQUIRE_DISPLAY").is_some() {
            println!("FAIL: no display although COUNTERPOINT_REQUIRE_DISPLAY is set");
            return ExitCode::FAILURE;
        }
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
