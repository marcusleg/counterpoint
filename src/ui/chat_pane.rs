//! The chat pane: chat list, selection chip, message list with proposal cards, input and busy
//! state.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use adw::prelude::*;
use gtk::{gdk, glib, pango};

use crate::chat::{Conversation, Entry, ProposalState};
use crate::chat_history::{self, ChatHistory};
use crate::chat_markup;
use crate::config::Config;
use crate::llm::{self, LlmError};
use crate::prompt::{self, Mode};
use crate::proposal::Edit;
use crate::ui::editor::EditorView;
use crate::ui::run_blocking;

const NO_SELECTION: &str = "No selection — whole document";
/// The longest selection preview; the label ellipsizes anyway, and laying out a whole selected
/// document on every cursor move would be wasted work.
const SELECTION_PREVIEW_CHARS: usize = 120;
/// The longest first message naming a chat in the chat list.
const CHAT_TITLE_CHARS: usize = 60;
const NEW_CHAT: &str = "New Conversation";

pub struct ChatPane {
    root: gtk::Box,
    editor: EditorView,
    toasts: adw::ToastOverlay,
    conversation: RefCell<Conversation>,
    mode: Cell<Mode>,
    /// The saved chats, loaded once and saved after every change to the conversation.
    history: RefCell<ChatHistory>,
    /// Set when the history file exists but could not be read: chats are then kept for this
    /// session only, so the file is never overwritten.
    history_unreadable: bool,
    /// Set once the user was told that the history could not be saved, until a save succeeds.
    history_save_failed: Cell<bool>,
    /// The open document's file, under which the conversation is saved; `None` until the
    /// document has been saved.
    document: RefCell<Option<PathBuf>>,
    /// The conversation's id among the document's saved chats, once it has been saved.
    chat_id: Cell<Option<u64>>,
    chat_list: gtk::DropDown,
    chat_labels: gtk::StringList,
    /// The chat behind each item of the chat list; `None` for a conversation not saved yet.
    chat_ids: RefCell<Vec<Option<u64>>>,
    /// Set while the chat list is rebuilt, so its selection changes are not taken for the user's.
    updating_chat_list: Cell<bool>,
    config_banner: adw::Banner,
    empty_state: adw::StatusPage,
    /// Shows the empty state or the message list.
    stack: gtk::Stack,
    messages: gtk::ScrolledWindow,
    selection_label: gtk::Label,
    list: gtk::ListBox,
    /// The entries currently shown, one per list row.
    rendered: RefCell<Vec<Entry>>,
    /// Set when rows were appended; the list scrolls to the end once its size is known.
    scroll_to_end: Cell<bool>,
    input: gtk::TextView,
    placeholder: gtk::Label,
    send_button: gtk::Button,
    stop_button: gtk::Button,
    spinner: adw::Spinner,
    busy_label: gtk::Label,
}

impl ChatPane {
    pub fn new(editor: EditorView, toasts: adw::ToastOverlay) -> Rc<Self> {
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

        let config_banner = adw::Banner::builder()
            .title("No model configured")
            .button_label("Preferences")
            .build();
        config_banner.connect_button_clicked(|banner| {
            let _ = banner.activate_action("win.preferences", None);
        });

        let chat_labels = gtk::StringList::new(&[]);
        let chat_list = gtk::DropDown::builder()
            .model(&chat_labels)
            .factory(&chat_list_factory())
            .tooltip_text("Earlier Chats About This Document")
            .build();
        chat_list.update_property(&[gtk::accessible::Property::Label("Chat")]);

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
        list.update_property(&[gtk::accessible::Property::Label("Conversation")]);
        let messages = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        let empty_state = adw::StatusPage::builder()
            .icon_name("chat-message-new-symbolic")
            .vexpand(true)
            .css_classes(["compact"])
            .build();
        let stack = gtk::Stack::builder().vexpand(true).build();
        stack.add_named(&empty_state, Some("empty"));
        stack.add_named(&messages, Some("messages"));

        let input = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(8)
            .right_margin(8)
            .build();
        input.update_property(&[gtk::accessible::Property::Label("Message to the LLM")]);
        input.update_relation(&[gtk::accessible::Relation::DescribedBy(&[
            selection_label.upcast_ref()
        ])]);
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
        // Ellipsized, so that waiting for a reply does not make the chat pane wider.
        let busy_label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["dim-label"])
            .build();
        let send_button = gtk::Button::builder()
            .label("_Send")
            .use_underline(true)
            .sensitive(false)
            .css_classes(["suggested-action"])
            .build();
        let stop_button = gtk::Button::builder()
            .label("_Stop")
            .use_underline(true)
            .visible(false)
            .tooltip_text("Abandon the request and keep the message")
            .css_classes(["destructive-action"])
            .build();
        let new_conversation = gtk::Button::builder()
            .label("New _Conversation")
            .use_underline(true)
            .tooltip_text("Forget the conversation so far")
            .build();
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bottom.append(&new_conversation);
        bottom.append(&spinner);
        bottom.append(&busy_label);
        bottom.append(&stop_button);
        bottom.append(&send_button);

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        root.append(&chat_list);
        root.append(&mode);
        root.append(&config_banner);
        root.append(&stack);
        root.append(&selection_label);
        root.append(&input_scroller);
        root.append(&bottom);

        let (history, history_unreadable) = match ChatHistory::load() {
            Ok(history) => (history, false),
            Err(_) => (ChatHistory::default(), true),
        };
        let pane = Rc::new(Self {
            root,
            editor,
            toasts,
            conversation: RefCell::new(Conversation::default()),
            mode: Cell::new(Mode::Sparring),
            history: RefCell::new(history),
            history_unreadable,
            history_save_failed: Cell::new(false),
            document: RefCell::new(None),
            chat_id: Cell::new(None),
            chat_list,
            chat_labels,
            chat_ids: RefCell::new(Vec::new()),
            updating_chat_list: Cell::new(false),
            config_banner,
            empty_state,
            stack,
            messages,
            selection_label,
            list,
            rendered: RefCell::new(Vec::new()),
            scroll_to_end: Cell::new(false),
            input,
            placeholder,
            send_button,
            stop_button,
            spinner,
            busy_label,
        });
        pane.set_mode(Mode::Sparring);
        pane.refresh_config();
        pane.render();

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
                pane.chat_id.set(None);
                pane.render();
            }
        ));
        pane.chat_list.connect_selected_notify(glib::clone!(
            #[weak]
            pane,
            move |list| {
                if pane.updating_chat_list.get() {
                    return;
                }
                let id = pane
                    .chat_ids
                    .borrow()
                    .get(list.selected() as usize)
                    .copied()
                    .flatten();
                if let Some(id) = id {
                    pane.switch_to_chat(id);
                }
            }
        ));
        pane.send_button.connect_clicked(glib::clone!(
            #[weak]
            pane,
            move |_| pane.send()
        ));
        pane.stop_button.connect_clicked(glib::clone!(
            #[weak]
            pane,
            move |_| pane.cancel()
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
            move |controller, key, _, state| {
                let enter = matches!(
                    key,
                    gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
                );
                if !enter || state.contains(gdk::ModifierType::SHIFT_MASK) {
                    // Shift+Enter falls through to the text view, which inserts a newline.
                    return glib::Propagation::Proceed;
                }
                // An input method that is composing text gets Enter first, to commit the
                // composition; only an Enter it does not use sends the message.
                let composing = controller
                    .current_event()
                    .is_some_and(|event| pane.input.im_context_filter_keypress(&event));
                if !composing {
                    pane.send();
                }
                glib::Propagation::Stop
            }
        ));
        pane.input.add_controller(keys);

        pane.messages.vadjustment().connect_changed(glib::clone!(
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

    /// Shows or hides the banner asking for a model, after the settings may have changed.
    pub fn refresh_config(&self) {
        let (revealed, title) = match Config::load() {
            Ok(config) if config.model.is_some() => (false, "No model configured"),
            Ok(_) => (true, "No model configured"),
            Err(_) => (true, "The settings could not be read"),
        };
        self.config_banner.set_title(title);
        self.config_banner.set_revealed(revealed);
    }

    /// Starts a fresh conversation about the document just opened from `path`. Its earlier
    /// chats are in the chat list.
    pub fn open_document(self: &Rc<Self>, path: &Path) {
        self.conversation.borrow_mut().reset();
        self.set_document(Some(path));
        self.render();
    }

    /// Files the conversation under `path` once the document has been saved there. Saved under
    /// another name, the document keeps its old chats and the conversation goes on as a new chat
    /// of the new file.
    pub fn document_saved(self: &Rc<Self>, path: &Path) {
        if self.document.borrow().as_deref() == Some(path) {
            return;
        }
        self.set_document(Some(path));
        self.persist();
        self.render();
    }

    /// Keeps the conversation for a new, untitled document, but stops saving it as a chat
    /// about the previous one.
    pub fn close_document(self: &Rc<Self>) {
        self.set_document(None);
        self.render();
    }

    fn set_document(&self, path: Option<&Path>) {
        *self.document.borrow_mut() = path.map(Path::to_path_buf);
        self.chat_id.set(None);
    }

    /// Continues the saved chat `id` about the open document.
    fn switch_to_chat(self: &Rc<Self>, id: u64) {
        if self.chat_id.get() == Some(id) || self.conversation.borrow().is_busy() {
            return;
        }
        let entries = {
            let history = self.history.borrow();
            let document = self.document.borrow();
            document
                .as_deref()
                .and_then(|document| history.chat(document, id))
                .map(|chat| chat.entries.clone())
        };
        let Some(entries) = entries else {
            return;
        };
        self.conversation.borrow_mut().restore(entries);
        self.chat_id.set(Some(id));
        self.render();
    }

    /// Saves the conversation as a chat about the open document, unless it is empty, waiting
    /// for a reply, or the document has no file yet. Saving is a convenience, so a failure shows
    /// a toast, once until a save works again.
    fn persist(&self) {
        let entries = {
            let conversation = self.conversation.borrow();
            if conversation.is_busy() || conversation.entries().is_empty() {
                return;
            }
            conversation.entries().to_vec()
        };
        let Some(document) = self.document.borrow().clone() else {
            return;
        };
        let started = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64);
        let mut history = self.history.borrow_mut();
        let id = history.store(&document, self.chat_id.get(), started, entries);
        self.chat_id.set(id);
        let saved = !self.history_unreadable && history.save().is_ok();
        drop(history);
        if saved {
            self.history_save_failed.set(false);
        } else if !self.history_save_failed.replace(true) {
            self.toasts
                .add_toast(adw::Toast::new("Could not save the chat history"));
        }
    }

    fn set_mode(&self, mode: Mode) {
        self.mode.set(mode);
        let (title, description) = match mode {
            Mode::Sparring => (
                "Sparring",
                "The LLM reads the document and critiques it, but cannot change it. Highlight a \
                 passage in the editor to focus on it, or leave nothing selected to discuss the \
                 whole document.",
            ),
            Mode::Ghostwriting => (
                "Ghostwriting",
                "The LLM proposes a change that you can apply or reject. Highlight the passage \
                 to change, or leave nothing selected for the whole document.",
            ),
        };
        self.empty_state.set_title(title);
        self.empty_state.set_description(Some(description));
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
            &self.conversation.borrow().history(),
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

        let pane = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = run_blocking(move || {
                Config::load()
                    .map_err(LlmError::Config)
                    .and_then(|config| llm::complete(&config, &messages))
                    .map_err(|e| e.to_string())
            })
            .await;
            let Some(pane) = pane.upgrade() else {
                return;
            };
            let current = pane
                .conversation
                .borrow_mut()
                .finish_request(ticket, result);
            if current {
                pane.persist();
                pane.render();
            }
        });
    }

    /// Abandons the in-flight request and puts the message back into the input.
    fn cancel(self: &Rc<Self>) {
        let message = self.conversation.borrow_mut().cancel_request();
        if let Some(message) = message {
            self.input.buffer().set_text(&message);
        }
        self.render();
    }

    fn apply(self: &Rc<Self>, index: usize) {
        let result = self
            .conversation
            .borrow_mut()
            .apply_proposal(index, &self.editor.text());
        if let Ok(markdown) = result {
            self.editor.apply_markdown(&markdown);
            let toast = adw::Toast::builder()
                .title("Change applied")
                .button_label("Undo")
                .build();
            toast.connect_button_clicked(glib::clone!(
                #[weak(rename_to = pane)]
                self,
                move |_| pane.editor.buffer().undo()
            ));
            self.toasts.add_toast(toast);
        }
        self.persist();
        self.render();
    }

    fn reject(self: &Rc<Self>, index: usize) {
        self.conversation.borrow_mut().reject_proposal(index);
        self.persist();
        self.render();
    }

    /// Brings the message list and busy state in line with the conversation. Rows are only
    /// rebuilt for entries that changed; new rows scroll the list to the end unless the user
    /// has scrolled up to read something, except for their own message.
    fn render(self: &Rc<Self>) {
        let mut rendered = self.rendered.take();
        {
            let conversation = self.conversation.borrow();
            let entries = conversation.entries();
            let adjustment = self.messages.vadjustment();
            let at_bottom = adjustment.value() + adjustment.page_size() >= adjustment.upper() - 1.0;
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
                        if at_bottom || matches!(entry, Entry::User { .. }) {
                            self.scroll_to_end.set(true);
                        }
                    }
                }
            }
            self.stack.set_visible_child_name(if entries.is_empty() {
                "empty"
            } else {
                "messages"
            });
        }
        self.rendered.replace(rendered);
        let busy = self.conversation.borrow().is_busy();
        self.spinner.set_visible(busy);
        self.busy_label
            .set_text(if busy { "Waiting for the LLM…" } else { "" });
        self.stop_button.set_visible(busy);
        self.send_button.set_visible(!busy);
        self.update_send_button();
        self.update_chat_list(busy);
    }

    /// Rebuilds the chat list: the open document's saved chats, newest first, after the
    /// conversation if it is not saved (yet). The conversation is selected; the list can only be
    /// opened when there is another chat to choose and no reply is awaited.
    fn update_chat_list(&self, busy: bool) {
        let current = self.chat_id.get();
        let mut ids = Vec::new();
        let mut labels = Vec::new();
        {
            let history = self.history.borrow();
            let document = self.document.borrow();
            let saved = document
                .as_deref()
                .map_or(&[][..], |document| history.chats(document));
            if !current.is_some_and(|id| saved.iter().any(|chat| chat.id == id)) {
                let conversation = self.conversation.borrow();
                ids.push(None);
                labels.push(chat_label(
                    chat_history::first_message(conversation.entries()),
                    None,
                ));
            }
            for chat in saved {
                ids.push(Some(chat.id));
                labels.push(chat_label(chat.first_message(), Some(chat.started)));
            }
        }
        let selected = ids
            .iter()
            .position(|&id| id.is_none() || id == current)
            .unwrap_or(0);
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.updating_chat_list.set(true);
        self.chat_labels
            .splice(0, self.chat_labels.n_items(), &labels);
        self.chat_list
            .set_selected(u32::try_from(selected).expect("chat index fits in u32"));
        self.updating_chat_list.set(false);
        self.chat_list.set_sensitive(!busy && ids.len() > 1);
        self.chat_ids.replace(ids);
    }

    fn row(self: &Rc<Self>, index: usize, entry: &Entry) -> gtk::ListBoxRow {
        let (child, class): (gtk::Widget, &str) = match entry {
            Entry::User { text } => (text_label(text).upcast(), "chat-user"),
            Entry::Assistant { text } => (markdown_label(text).upcast(), "chat-assistant"),
            Entry::Error { text } => {
                let label = text_label(text);
                label.add_css_class("error");
                (label.upcast(), "chat-error")
            }
            Entry::Proposal {
                explanation,
                edits,
                state,
                ..
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
                .label("Proposed Change")
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
        if !explanation.is_empty() {
            card.append(&markdown_label(explanation));
        }
        for (number, edit) in edits.iter().enumerate() {
            if edits.len() > 1 {
                let heading = text_label(&format!("Edit {} of {}", number + 1, edits.len()));
                heading.add_css_class("dim-label");
                card.append(&heading);
            }
            // A blank original only occurs when writing into an empty document.
            if !edit.original.trim().is_empty() {
                let original = markup_label(
                    &format!("<s>{}</s>", chat_markup::escape(&edit.original)),
                    &edit.original,
                );
                original.add_css_class("edit-original");
                card.append(&original);
            }
            let replacement = if edit.replacement.is_empty() {
                markup_label("<i>(delete)</i>", "(delete)")
            } else {
                text_label(&edit.replacement)
            };
            replacement.add_css_class("edit-replacement");
            card.append(&replacement);
        }
        match state {
            ProposalState::Pending => {
                let apply = gtk::Button::builder()
                    .label("_Apply")
                    .use_underline(true)
                    .css_classes(["suggested-action"])
                    .build();
                apply.connect_clicked(glib::clone!(
                    #[weak(rename_to = pane)]
                    self,
                    move |_| pane.apply(index)
                ));
                let reject = gtk::Button::builder()
                    .label("_Reject")
                    .use_underline(true)
                    .build();
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
            format!(
                "Selection: “{}”",
                preview(&selection, SELECTION_PREVIEW_CHARS)
            )
        };
        self.selection_label.set_text(&text);
    }

    fn update_placeholder(&self) {
        let hint = match self.mode.get() {
            Mode::Sparring => "Ask about the text…",
            Mode::Ghostwriting => "Ask for a change…",
        };
        let placeholder = format!("{hint} (Enter to send, Shift+Enter for a new line)");
        self.placeholder.set_text(&placeholder);
        self.placeholder
            .set_visible(self.input.buffer().char_count() == 0);
        self.input
            .update_property(&[gtk::accessible::Property::Placeholder(&placeholder)]);
    }

    fn update_send_button(&self) {
        let buffer = self.input.buffer();
        let (start, end) = buffer.bounds();
        let blank = buffer.text(&start, &end, false).trim().is_empty();
        self.send_button
            .set_sensitive(!blank && !self.conversation.borrow().is_busy());
    }
}

/// A chat list entry: the chat's first message, after the day and time it started if it has been
/// saved, so that an ellipsis cuts off the message rather than the date.
fn chat_label(first_message: Option<&str>, started: Option<i64>) -> String {
    let title = first_message
        .map(|message| preview(message, CHAT_TITLE_CHARS))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| NEW_CHAT.to_string());
    let date = started
        .and_then(|seconds| glib::DateTime::from_unix_local(seconds).ok())
        .and_then(|date| date.format("%-d %b, %H:%M").ok());
    match date {
        Some(date) => format!("{date} · {title}"),
        None => title,
    }
}

/// Shows each chat list entry in one line, ellipsized so a long message cannot widen the pane.
fn chat_list_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .max_width_chars(40)
            .build();
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let (Some(label), Some(string)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) {
            label.set_text(&string.string());
        }
    });
    factory
}

/// The first `max_chars` characters of `text` with whitespace collapsed, plus an ellipsis if
/// that cut anything off.
fn preview(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(max_chars + 4);
    let mut count = 0;
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
            count += 1;
        }
        for ch in word.chars() {
            if count >= max_chars {
                out.push('…');
                return out;
            }
            out.push(ch);
            count += 1;
        }
    }
    out
}

fn text_label(text: &str) -> gtk::Label {
    let label = base_label();
    label.set_text(text);
    label
}

/// A label showing `markdown` rendered, or as plain text should Pango reject the markup.
fn markdown_label(markdown: &str) -> gtk::Label {
    markup_label(&chat_markup::to_pango(markdown), markdown)
}

/// A label showing `markup`, falling back to `text` if Pango cannot parse the markup, so that a
/// reply is never shown as an empty row.
fn markup_label(markup: &str, text: &str) -> gtk::Label {
    let label = base_label();
    match pango::parse_markup(markup, '\0') {
        Ok(_) => label.set_markup(markup),
        Err(_) => label.set_text(text),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_collapses_whitespace_and_cuts_long_text() {
        assert_eq!(
            preview("  Some\n  example   text ", 120),
            "Some example text"
        );
        let long = "word ".repeat(100);
        let shown = preview(&long, SELECTION_PREVIEW_CHARS);
        assert!(shown.ends_with('…'), "{shown}");
        assert_eq!(shown.chars().count(), SELECTION_PREVIEW_CHARS + 1);
    }

    #[test]
    fn chat_label_names_a_chat_by_its_first_message() {
        assert_eq!(chat_label(None, None), NEW_CHAT);
        assert_eq!(
            chat_label(Some("  Is the\nintro long? "), None),
            "Is the intro long?"
        );
        let label = chat_label(Some("Thoughts?"), Some(1_790_000_000));
        assert!(label.ends_with(" · Thoughts?"), "{label}");
    }
}
