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
        let mode = adw::ToggleGroup::builder()
            .homogeneous(true)
            .hexpand(true)
            .build();
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
        root.append(&mode);
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

        mode.connect_active_name_notify(glib::clone!(
            #[weak]
            pane,
            move |group| {
                pane.set_mode(match group.active_name().as_deref() {
                    Some("ghostwriting") => Mode::Ghostwriting,
                    _ => Mode::Sparring,
                });
            }
        ));

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

    fn set_mode(&self, mode: Mode) {
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
        let text = if selection.trim().is_empty() {
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
