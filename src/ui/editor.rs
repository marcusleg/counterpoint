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
