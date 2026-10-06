//! The Markdown source editor: a GtkSourceView that holds the exact file text.

use std::cell::Cell;

use gtk::gdk;
use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::text_diff;
use crate::zoom;

const ZOOM_CLASS_PREFIX: &str = "counterpoint-zoom-";

thread_local! {
    /// Whether the zoom style sheet has been registered on the display. It has one rule per
    /// zoom level (`.counterpoint-zoom-110 { font-size: 110%; }`), so every editor picks its
    /// level by class and none needs a style provider of its own.
    static ZOOM_STYLE_LOADED: Cell<bool> = const { Cell::new(false) };
}

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
        load_zoom_style();
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
        view.upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Label("Document")]);
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
    /// first and the last differing character. The new text is selected and scrolled into view,
    /// so the change is visible even when it happened off screen.
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
        let inserted_end = start;
        let inserted_start = self.buffer.iter_at_offset(offset(span.start));
        self.select(&inserted_start, &inserted_end);
    }

    /// Selects the text from `start` to `end` and scrolls it into view.
    pub fn select(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        self.buffer.select_range(start, end);
        self.view.scroll_mark_onscreen(&self.buffer.get_insert());
    }

    /// Scales the editor's font to `percent` of its default size; `percent` must be one of the
    /// levels in `zoom`.
    pub fn set_zoom(&self, percent: u32) {
        for class in self.view.css_classes() {
            if class.starts_with(ZOOM_CLASS_PREFIX) {
                self.view.remove_css_class(&class);
            }
        }
        self.view
            .add_css_class(&format!("{ZOOM_CLASS_PREFIX}{percent}"));
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

fn load_zoom_style() {
    if ZOOM_STYLE_LOADED.replace(true) {
        return;
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let css: String = (zoom::MIN..=zoom::MAX)
        .step_by(zoom::STEP as usize)
        .map(|percent| format!(".{ZOOM_CLASS_PREFIX}{percent} {{ font-size: {percent}%; }}\n"))
        .collect();
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
