//! The find bar above the editor: a search entry that highlights every match in the document,
//! selects the current one and counts them.

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sourceview5::prelude::*;

use crate::ui::editor::EditorView;

pub struct FindBar {
    bar: gtk::SearchBar,
    entry: gtk::SearchEntry,
    count_label: gtk::Label,
    previous_button: gtk::Button,
    next_button: gtk::Button,
    editor: EditorView,
    /// Finds the matches, case-insensitively and wrapping around the end of the document, and
    /// highlights them while the bar is open.
    context: sourceview5::SearchContext,
}

impl FindBar {
    pub fn new(editor: EditorView) -> Rc<Self> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find")
            .width_chars(30)
            .build();
        let count_label = gtk::Label::builder()
            .width_chars(10)
            .xalign(0.0)
            .css_classes(["dim-label", "numeric"])
            .build();
        let previous_button = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Previous Match")
            .sensitive(false)
            .build();
        let next_button = gtk::Button::builder()
            .icon_name("go-down-symbolic")
            .tooltip_text("Next Match")
            .sensitive(false)
            .build();
        let buttons = gtk::Box::builder().css_classes(["linked"]).build();
        buttons.append(&previous_button);
        buttons.append(&next_button);
        let content = gtk::Box::builder().spacing(6).build();
        content.append(&entry);
        content.append(&count_label);
        content.append(&buttons);

        let bar = gtk::SearchBar::builder()
            .child(&content)
            .show_close_button(true)
            .build();
        bar.connect_entry(&entry);

        let settings = sourceview5::SearchSettings::builder()
            .wrap_around(true)
            .build();
        let context = sourceview5::SearchContext::new(editor.buffer(), Some(&settings));
        context.set_highlight(false);

        let this = Rc::new(Self {
            bar,
            entry,
            count_label,
            previous_button,
            next_button,
            editor,
            context,
        });
        this.connect_signals();
        this
    }

    pub fn widget(&self) -> &gtk::SearchBar {
        &self.bar
    }

    /// Shows the bar and focuses its entry. A selection within one line becomes the search
    /// text; otherwise the previous search text is kept, selected so typing replaces it.
    pub fn open(&self) {
        let selection = self.editor.selection_text();
        if !selection.is_empty() && !selection.contains('\n') {
            self.entry.set_text(&selection);
        }
        self.bar.set_search_mode(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        self.update_count();
    }

    /// Selects the next match after the selection or cursor, wrapping around at the end.
    pub fn find_next(&self) {
        let (_, from) = self.selection_or_cursor();
        if let Some((start, end, _)) = self.context.forward(&from) {
            self.select_match(&start, &end);
        }
    }

    /// Selects the match before the selection or cursor, wrapping around at the start.
    pub fn find_previous(&self) {
        let (from, _) = self.selection_or_cursor();
        if let Some((start, end, _)) = self.context.backward(&from) {
            self.select_match(&start, &end);
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        // While typing, the selection moves to the first match at or after where it starts, so
        // a longer search text keeps the match it extends.
        self.entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |entry| {
                let text = entry.text();
                let has_text = !text.is_empty();
                this.context
                    .settings()
                    .set_search_text(has_text.then_some(text.as_str()));
                this.previous_button.set_sensitive(has_text);
                this.next_button.set_sensitive(has_text);
                let (from, _) = this.selection_or_cursor();
                if let Some((start, end, _)) = this.context.forward(&from) {
                    this.select_match(&start, &end);
                }
                this.update_count();
            }
        ));
        self.entry.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.find_next()
        ));
        self.entry.connect_next_match(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.find_next()
        ));
        self.entry.connect_previous_match(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.find_previous()
        ));
        // Shift+Enter goes back, as in other GNOME editors. The entry would treat it as Enter.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let enter = matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter);
                if enter && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                    this.find_previous();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        self.entry.add_controller(keys);
        self.previous_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.find_previous()
        ));
        self.next_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.find_next()
        ));

        // Matches are highlighted only while the bar is open. Closing it returns to the editor
        // with the current match still selected.
        self.bar.connect_search_mode_enabled_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |bar| {
                let open = bar.is_search_mode();
                this.context.set_highlight(open);
                if !open {
                    this.editor.widget().grab_focus();
                }
            }
        ));
        // The context counts matches in the background, and again after every edit.
        self.context.connect_occurrences_count_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.update_count()
        ));
        self.editor
            .buffer()
            .connect_cursor_position_notify(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_| this.update_count()
            ));
    }

    /// The bounds of the selection, or the cursor position twice.
    fn selection_or_cursor(&self) -> (gtk::TextIter, gtk::TextIter) {
        let buffer = self.editor.buffer();
        buffer.selection_bounds().unwrap_or_else(|| {
            let cursor = buffer.iter_at_mark(&buffer.get_insert());
            (cursor, cursor)
        })
    }

    fn select_match(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        self.editor.select(start, end);
        self.update_count();
    }

    /// Shows how many matches there are and which one is selected, and marks the entry as an
    /// error when nothing matches.
    fn update_count(&self) {
        if self.entry.text().is_empty() {
            self.count_label.set_label("");
            self.entry.remove_css_class("error");
            return;
        }
        let count = self.context.occurrences_count();
        let position = self
            .editor
            .buffer()
            .selection_bounds()
            .map(|(start, end)| self.context.occurrence_position(&start, &end))
            .unwrap_or(0);
        self.count_label.set_label(&count_label(count, position));
        if count == 0 {
            self.entry.add_css_class("error");
        } else {
            self.entry.remove_css_class("error");
        }
    }
}

/// The match count for `count` matches with the `position`th one selected, both as
/// GtkSourceView reports them: a negative count while it is still counting, and position 0 when
/// the selection is not a match.
fn count_label(count: i32, position: i32) -> String {
    match (count, position) {
        (..0, _) => String::new(),
        (0, _) => "No matches".to_string(),
        (_, 1..) => format!("{position} of {count}"),
        (1, _) => "1 match".to_string(),
        _ => format!("{count} matches"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_label_names_the_selected_match() {
        assert_eq!(count_label(5, 2), "2 of 5");
    }

    #[test]
    fn count_label_counts_without_a_selected_match() {
        assert_eq!(count_label(5, 0), "5 matches");
        assert_eq!(count_label(1, 0), "1 match");
        assert_eq!(count_label(0, 0), "No matches");
    }

    #[test]
    fn count_label_is_empty_while_counting() {
        assert_eq!(count_label(-1, -1), "");
    }
}
