//! The chat list at the top of the chat pane: the open document's saved chats, to pick one up
//! again.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{glib, pango};

use crate::chat_history::SavedChat;
use crate::ui::chat_pane::preview;

/// The longest first message naming a chat in the chat list.
const CHAT_TITLE_CHARS: usize = 60;
const NEW_CHAT: &str = "New Conversation";

/// Called with the id of the saved chat the user picked.
type ChatChosen = Box<dyn Fn(u64)>;

pub struct ChatList {
    dropdown: gtk::DropDown,
    labels: gtk::StringList,
    /// The chat behind each item; `None` for a conversation not saved yet.
    ids: RefCell<Vec<Option<u64>>>,
    /// Set while the list is rebuilt, so its selection changes are not taken for the user's.
    updating: Cell<bool>,
    chosen: RefCell<Option<ChatChosen>>,
}

impl ChatList {
    pub fn new() -> Rc<Self> {
        let labels = gtk::StringList::new(&[]);
        let dropdown = gtk::DropDown::builder()
            .model(&labels)
            .factory(&factory())
            .tooltip_text("Earlier Chats About This Document")
            .build();
        dropdown.update_property(&[gtk::accessible::Property::Label("Chat")]);
        let list = Rc::new(Self {
            dropdown,
            labels,
            ids: RefCell::new(Vec::new()),
            updating: Cell::new(false),
            chosen: RefCell::new(None),
        });
        list.dropdown.connect_selected_notify(glib::clone!(
            #[weak]
            list,
            move |dropdown| {
                if list.updating.get() {
                    return;
                }
                let id = list
                    .ids
                    .borrow()
                    .get(dropdown.selected() as usize)
                    .copied()
                    .flatten();
                if let (Some(id), Some(callback)) = (id, &*list.chosen.borrow()) {
                    callback(id);
                }
            }
        ));
        list
    }

    pub fn widget(&self) -> &gtk::DropDown {
        &self.dropdown
    }

    /// Calls `callback` with the id of a saved chat the user picks from the list.
    pub fn connect_chat_chosen(&self, callback: impl Fn(u64) + 'static) {
        self.chosen.replace(Some(Box::new(callback)));
    }

    /// Lists the open document's `saved` chats, newest first, after the conversation if it is
    /// not one of them: `current` is its chat id and `first_message` names it. The conversation
    /// is selected; the list can only be opened when there is another chat to choose and no
    /// reply is awaited.
    pub fn set_chats(
        &self,
        current: Option<u64>,
        first_message: Option<&str>,
        saved: &[SavedChat],
        busy: bool,
    ) {
        let mut ids = Vec::new();
        let mut labels = Vec::new();
        if !current.is_some_and(|id| saved.iter().any(|chat| chat.id == id)) {
            ids.push(None);
            labels.push(chat_label(first_message, None));
        }
        for chat in saved {
            ids.push(Some(chat.id));
            labels.push(chat_label(chat.first_message(), Some(chat.started)));
        }
        let selected = ids
            .iter()
            .position(|&id| id.is_none() || id == current)
            .unwrap_or(0);
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.updating.set(true);
        self.labels.splice(0, self.labels.n_items(), &labels);
        self.dropdown
            .set_selected(u32::try_from(selected).expect("chat index fits in u32"));
        self.updating.set(false);
        self.dropdown.set_sensitive(!busy && ids.len() > 1);
        self.ids.replace(ids);
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
fn factory() -> gtk::SignalListItemFactory {
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

#[cfg(test)]
mod tests {
    use super::*;

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
