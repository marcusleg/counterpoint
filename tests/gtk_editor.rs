//! GTK checks for the editor buffer and chat markup. GTK must run on the thread that initialised
//! it, so this test has its own `main` (`harness = false`) and runs every check in turn.
//! It needs a display; `dev/headless.sh cargo test` provides a private one and sets
//! `COUNTERPOINT_REQUIRE_DISPLAY` so a missing display fails loudly instead of skipping silently.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use common::Checks;
use counterpoint::chat_markup;
use counterpoint::document;
use counterpoint::ui::editor::EditorView;
use gtk::prelude::*;

fn main() -> ExitCode {
    if let Err(code) = common::init_or_skip() {
        return code;
    }

    let mut checks = Checks::default();
    let files = round_trip_files();
    checks.check(
        files.iter().any(|p| p.ends_with("README.md")),
        "git ls-files found the repository's Markdown files",
    );
    for path in files {
        round_trip(&mut checks, &path);
    }
    loading_is_not_undoable(&mut checks);
    apply_is_one_undo_step(&mut checks);
    apply_leaves_text_outside_the_span_alone(&mut checks);
    apply_counts_characters(&mut checks);
    zoom_changes_the_editor_font_size(&mut checks);
    zoom_is_independent_per_editor(&mut checks);
    chat_markup_is_valid_for_labels(&mut checks);
    hostile_markup_is_shown_as_text(&mut checks);

    checks.finish()
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
        checks.fail(&format!("{what}: cannot read the file"));
        return;
    };
    let Ok(contents) = String::from_utf8(bytes.clone()) else {
        checks.fail(&format!("{what}: not UTF-8"));
        return;
    };
    let editor = EditorView::new();
    let (text, format) = document::from_disk(&contents);
    editor.load(&text);
    checks.check(
        !editor.text().starts_with('\u{FEFF}'),
        &format!("{what}: no byte order mark reaches the editor"),
    );
    let saved = document::to_disk(&editor.text(), format);
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
        editor.selection_text() == "Rewrite it all",
        "the applied change (without the unchanged trailing period) is selected",
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

/// The pixel height of the editor's first line, once its style and layout are up to date. The
/// zoom CSS is registered on the display, so the view must be realized and drawn (via
/// `gtk::test_widget_wait_for_draw`, which pumps the main loop until a frame is rendered) for a
/// style change to take effect; neither plain `MainContext` iteration nor
/// `pango_context().font_description()` picks up a relative `font-size` set through CSS under
/// Broadway, so this measures the rendered line height instead.
fn first_line_height(window: &gtk::Window, editor: &EditorView) -> i32 {
    gtk::test_widget_wait_for_draw(window);
    let (_, height) = editor.widget().line_yrange(&editor.buffer().start_iter());
    height
}

fn zoom_changes_the_editor_font_size(checks: &mut Checks) {
    let editor = EditorView::new();
    editor.load("One line of example text.");
    let window = gtk::Window::new();
    window.set_child(Some(editor.widget()));
    window.present();

    editor.set_zoom(100);
    let height_100 = first_line_height(&window, &editor);

    editor.set_zoom(200);
    let height_200 = first_line_height(&window, &editor);

    checks.check(
        height_200 > height_100,
        &format!(
            "zooming to 200% increases the editor's line height ({height_100} -> {height_200})"
        ),
    );
    let zoom_classes: Vec<_> = editor
        .widget()
        .css_classes()
        .into_iter()
        .filter(|c| c.starts_with("counterpoint-zoom-"))
        .collect();
    checks.check(
        zoom_classes.len() == 1 && zoom_classes[0] == "counterpoint-zoom-200",
        &format!("the editor carries exactly its current zoom class, got {zoom_classes:?}"),
    );
    window.destroy();
}

/// Zoom is a per-editor style class, so zooming one editor must never affect another's line
/// height. Checked in both directions, and that the zoomed editor's own height actually changes.
fn zoom_is_independent_per_editor(checks: &mut Checks) {
    let first = EditorView::new();
    first.load("One line of example text.");
    let second = EditorView::new();
    second.load("One line of example text.");

    let window = gtk::Window::new();
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.append(first.widget());
    container.append(second.widget());
    window.set_child(Some(&container));
    window.present();

    first.set_zoom(100);
    second.set_zoom(100);
    let first_100 = first_line_height(&window, &first);
    let second_100 = first_line_height(&window, &second);

    first.set_zoom(200);
    checks.check(
        first_line_height(&window, &first) > first_100,
        "zooming the first editor changes its own line height",
    );
    checks.check(
        first_line_height(&window, &second) == second_100,
        "zooming the first editor leaves the second editor's line height unchanged",
    );
    first.set_zoom(100);

    second.set_zoom(200);
    checks.check(
        first_line_height(&window, &second) > second_100,
        "zooming the second editor changes its own line height",
    );
    checks.check(
        first_line_height(&window, &first) == first_100,
        "zooming the second editor leaves the first editor's line height unchanged",
    );

    window.destroy();
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

/// Markup-looking text from the model must come out as that literal text, never as markup.
/// (Markdown itself decodes entities: `&#0;` becomes U+FFFD and `&amp;lt;` becomes `&lt;`.)
fn hostile_markup_is_shown_as_text(checks: &mut Checks) {
    for (hostile, shown) in [
        (
            "<span foreground=\"red\" size=\"9999999\">x</span>",
            "<span foreground=\"red\" size=\"9999999\">x</span>",
        ),
        (
            "</s></span><a href=\"javascript:1\">y</a> &#0; &amp;lt;",
            "</s></span><a href=\"javascript:1\">y</a> \u{FFFD} &lt;",
        ),
    ] {
        let label = gtk::Label::new(None);
        label.set_markup(&chat_markup::to_pango(hostile));
        checks.check(
            label.text() == shown,
            &format!(
                "the reply {hostile:?} is shown literally, got {:?}",
                label.text()
            ),
        );
    }
    let original = "</s><b>x";
    let label = gtk::Label::new(None);
    label.set_markup(&format!("<s>{}</s>", chat_markup::escape(original)));
    checks.check(
        label.text() == original,
        "a proposal's original text is escaped inside the strike-through",
    );
}
