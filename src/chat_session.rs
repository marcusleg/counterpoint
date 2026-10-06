//! Which saved chat the conversation in the chat pane is, and when it is saved to the chat
//! history. Kept apart from the widgets so that the rules for what is stored, when and under
//! which id can be tested without a display or the user's real history file.

use std::path::{Path, PathBuf};

use crate::chat::Conversation;
use crate::chat_history::{self, ChatHistory, SavedChat};

/// What a change to the session means for the user interface.
#[must_use]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Each chat id the conversation took on, in order: `None` for a conversation not saved
    /// yet. Whoever remembers the chat shown is told every one of them.
    pub chat_changes: Vec<Option<u64>>,
    /// Set when the history could not be saved and the user has not been told so since the
    /// last save that worked.
    pub report_save_failure: bool,
}

pub struct ChatSession {
    /// The saved chats, loaded once and saved after every change to the conversation.
    history: ChatHistory,
    /// Where the history is saved. `None` when the file exists but could not be read: chats are
    /// then kept for this session only, so the file is never overwritten.
    file: Option<PathBuf>,
    /// Set once the user was told that the history could not be saved, until a save succeeds.
    save_failed: bool,
    /// The open document's file, under which the conversation is saved; `None` until the
    /// document has been saved.
    document: Option<PathBuf>,
    /// The conversation's id among the document's saved chats, once it has been saved.
    chat_id: Option<u64>,
}

impl ChatSession {
    /// Loads the history from its file in the user's data directory.
    pub fn load() -> Self {
        match chat_history::history_path() {
            Ok(file) => Self::load_from(file),
            Err(_) => Self::new(ChatHistory::default(), None),
        }
    }

    /// Loads the history from `file`, where it is saved from then on unless it cannot be read.
    pub fn load_from(file: PathBuf) -> Self {
        match ChatHistory::load_from(&file) {
            Ok(history) => Self::new(history, Some(file)),
            Err(_) => Self::new(ChatHistory::default(), None),
        }
    }

    fn new(history: ChatHistory, file: Option<PathBuf>) -> Self {
        Self {
            history,
            file,
            save_failed: false,
            document: None,
            chat_id: None,
        }
    }

    /// The conversation's id among the open document's saved chats, once it has been saved.
    pub fn chat_id(&self) -> Option<u64> {
        self.chat_id
    }

    /// The open document's saved chats, newest first; none for an untitled document.
    pub fn chats(&self) -> &[SavedChat] {
        self.document
            .as_deref()
            .map_or(&[], |document| self.history.chats(document))
    }

    /// Starts `conversation` afresh about the document just opened from `path`.
    pub fn open_document(&mut self, path: &Path, conversation: &mut Conversation) -> Outcome {
        conversation.reset();
        let mut outcome = Outcome::default();
        self.set_document(Some(path), &mut outcome);
        outcome
    }

    /// Files `conversation` under `path` once the document has been saved there. Saved under
    /// another name, the document keeps its old chats and the conversation goes on as a new
    /// chat of the new file. `None` when the document was saved where it already was, which
    /// changes nothing.
    pub fn document_saved(
        &mut self,
        path: &Path,
        conversation: &Conversation,
        now: i64,
    ) -> Option<Outcome> {
        if self.document.as_deref() == Some(path) {
            return None;
        }
        let mut outcome = Outcome::default();
        self.set_document(Some(path), &mut outcome);
        self.store(conversation, now, &mut outcome);
        Some(outcome)
    }

    /// Keeps the conversation for a new, untitled document, but stops saving it as a chat
    /// about the previous one.
    pub fn close_document(&mut self) -> Outcome {
        let mut outcome = Outcome::default();
        self.set_document(None, &mut outcome);
        outcome
    }

    /// Forgets `conversation` so far; what is said next becomes a new chat.
    pub fn new_conversation(&mut self, conversation: &mut Conversation) -> Outcome {
        conversation.reset();
        let mut outcome = Outcome::default();
        self.set_chat_id(None, &mut outcome);
        outcome
    }

    /// Continues the saved chat `id` about the open document in `conversation`, if it still has
    /// one by that id and no reply is awaited.
    pub fn switch_to_chat(&mut self, id: u64, conversation: &mut Conversation) -> Outcome {
        let mut outcome = Outcome::default();
        if self.chat_id == Some(id) || conversation.is_busy() {
            return outcome;
        }
        let Some(document) = self.document.as_deref() else {
            return outcome;
        };
        let Some(chat) = self.history.chat(document, id) else {
            return outcome;
        };
        conversation.restore(chat.entries.clone());
        self.set_chat_id(Some(id), &mut outcome);
        outcome
    }

    /// Saves `conversation` as a chat about the open document, unless it is empty, waiting for
    /// a reply, or the document has no file yet. A new chat is dated `now`, in seconds since
    /// the Unix epoch.
    pub fn persist(&mut self, conversation: &Conversation, now: i64) -> Outcome {
        let mut outcome = Outcome::default();
        self.store(conversation, now, &mut outcome);
        outcome
    }

    fn set_document(&mut self, path: Option<&Path>, outcome: &mut Outcome) {
        self.document = path.map(Path::to_path_buf);
        self.set_chat_id(None, outcome);
    }

    fn set_chat_id(&mut self, id: Option<u64>, outcome: &mut Outcome) {
        if self.chat_id != id {
            self.chat_id = id;
            outcome.chat_changes.push(id);
        }
    }

    /// Saving is a convenience, so a failure is only reported once until a save works again.
    fn store(&mut self, conversation: &Conversation, now: i64, outcome: &mut Outcome) {
        if conversation.is_busy() || conversation.entries().is_empty() {
            return;
        }
        let Some(document) = self.document.clone() else {
            return;
        };
        let entries = conversation.entries().to_vec();
        let id = self.history.store(&document, self.chat_id, now, entries);
        self.set_chat_id(id, outcome);
        let saved = self
            .file
            .as_deref()
            .is_some_and(|file| self.history.save_to(file).is_ok());
        if saved {
            self.save_failed = false;
        } else if !self.save_failed {
            self.save_failed = true;
            outcome.report_save_failure = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::chat::Entry;
    use crate::prompt::Mode;

    fn user(text: &str) -> Entry {
        Entry::User {
            text: text.to_string(),
        }
    }

    fn conversation(messages: &[&str]) -> Conversation {
        let mut conversation = Conversation::default();
        conversation.restore(messages.iter().map(|text| user(text)).collect());
        conversation
    }

    fn saved_chats(file: &Path, document: &str) -> Vec<SavedChat> {
        ChatHistory::load_from(file)
            .unwrap()
            .chats(Path::new(document))
            .to_vec()
    }

    #[test]
    fn a_history_file_that_cannot_be_read_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chats.json");
        fs::write(&file, "not json").unwrap();
        let mut session = ChatSession::load_from(file.clone());
        let _ = session.open_document(Path::new("/post.md"), &mut Conversation::default());

        let outcome = session.persist(&conversation(&["Thoughts?"]), 100);
        assert_eq!(outcome.chat_changes, [Some(1)]);
        assert!(outcome.report_save_failure);
        assert_eq!(fs::read_to_string(&file).unwrap(), "not json");
        assert_eq!(
            session.chats().len(),
            1,
            "the chat is kept for this session"
        );
    }

    #[test]
    fn an_untitled_documents_conversation_becomes_a_chat_once_the_document_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chats.json");
        let mut session = ChatSession::load_from(file.clone());
        let talk = conversation(&["Thoughts?"]);

        assert_eq!(session.persist(&talk, 100), Outcome::default());
        assert!(session.chats().is_empty());
        assert!(!file.exists());

        let outcome = session.document_saved(Path::new("/post.md"), &talk, 200);
        assert_eq!(outcome.unwrap().chat_changes, [Some(1)]);
        let saved = saved_chats(&file, "/post.md");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].started, 200);
        assert_eq!(saved[0].entries, talk.entries());
    }

    #[test]
    fn after_save_as_the_conversation_goes_on_as_a_new_chat_of_the_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chats.json");
        let mut session = ChatSession::load_from(file.clone());
        let _ = session.open_document(Path::new("/old.md"), &mut Conversation::default());
        let _ = session.persist(&conversation(&["Thoughts?"]), 100);

        let talk = conversation(&["Thoughts?", "More?"]);
        let outcome = session.document_saved(Path::new("/new.md"), &talk, 200);
        assert_eq!(
            outcome.unwrap().chat_changes,
            [None, Some(1)],
            "the conversation leaves the old file's chat before it becomes the new file's"
        );
        let old = saved_chats(&file, "/old.md");
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].entries, [user("Thoughts?")]);
        assert_eq!(saved_chats(&file, "/new.md")[0].entries, talk.entries());
    }

    #[test]
    fn saving_the_document_under_its_own_name_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = ChatSession::load_from(dir.path().join("chats.json"));
        let _ = session.open_document(Path::new("/post.md"), &mut Conversation::default());
        let _ = session.persist(&conversation(&["Thoughts?"]), 100);

        let talk = conversation(&["Thoughts?", "More?"]);
        assert_eq!(
            session.document_saved(Path::new("/post.md"), &talk, 200),
            None
        );
        assert_eq!(session.chats()[0].entries, [user("Thoughts?")]);
    }

    #[test]
    fn empty_or_busy_conversations_are_not_saved() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chats.json");
        let mut session = ChatSession::load_from(file.clone());
        let mut talk = Conversation::default();
        let _ = session.open_document(Path::new("/post.md"), &mut talk);

        assert_eq!(session.persist(&talk, 100), Outcome::default());
        talk.begin_request(Mode::Sparring, "Thoughts?").unwrap();
        assert_eq!(session.persist(&talk, 100), Outcome::default());
        assert_eq!(
            session.document_saved(Path::new("/other.md"), &talk, 100),
            Some(Outcome::default())
        );
        assert!(session.chats().is_empty());
        assert!(!file.exists());
    }

    #[test]
    fn every_change_to_the_conversation_is_saved_to_the_same_chat() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chats.json");
        let mut session = ChatSession::load_from(file.clone());
        let _ = session.open_document(Path::new("/post.md"), &mut Conversation::default());

        assert_eq!(
            session.persist(&conversation(&["One"]), 100).chat_changes,
            [Some(1)]
        );
        let outcome = session.persist(&conversation(&["One", "Two"]), 200);
        assert_eq!(outcome, Outcome::default(), "the chat id stays the same");
        let saved = saved_chats(&file, "/post.md");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].entries, [user("One"), user("Two")]);
        assert_eq!(saved[0].started, 100);
    }

    #[test]
    fn a_failed_save_is_reported_once_until_a_save_works_again() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("counterpoint");
        let mut session = ChatSession::load_from(blocker.join("chats.json"));
        let _ = session.open_document(Path::new("/post.md"), &mut Conversation::default());
        // A file where the history's folder should be makes every save fail until it is gone.
        fs::write(&blocker, "").unwrap();

        assert!(
            session
                .persist(&conversation(&["One"]), 100)
                .report_save_failure
        );
        assert!(
            !session
                .persist(&conversation(&["Two"]), 100)
                .report_save_failure
        );
        fs::remove_file(&blocker).unwrap();
        assert!(
            !session
                .persist(&conversation(&["Three"]), 100)
                .report_save_failure
        );
        fs::remove_dir_all(&blocker).unwrap();
        fs::write(&blocker, "").unwrap();
        assert!(
            session
                .persist(&conversation(&["Four"]), 100)
                .report_save_failure
        );
    }

    #[test]
    fn switching_to_a_saved_chat_restores_its_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = ChatSession::load_from(dir.path().join("chats.json"));
        let mut talk = Conversation::default();
        let _ = session.open_document(Path::new("/post.md"), &mut talk);
        let _ = session.persist(&conversation(&["Earlier"]), 100);
        let _ = session.new_conversation(&mut talk);

        let outcome = session.switch_to_chat(1, &mut talk);
        assert_eq!(outcome.chat_changes, [Some(1)]);
        assert_eq!(talk.entries(), [user("Earlier")]);
        assert_eq!(session.chat_id(), Some(1));
        assert_eq!(
            session.switch_to_chat(1, &mut talk),
            Outcome::default(),
            "the chat shown already"
        );
    }

    #[test]
    fn switching_chats_is_ignored_while_a_reply_is_awaited_or_for_an_unknown_chat() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = ChatSession::load_from(dir.path().join("chats.json"));
        let mut talk = Conversation::default();
        let _ = session.open_document(Path::new("/post.md"), &mut talk);
        let _ = session.persist(&conversation(&["Earlier"]), 100);
        let _ = session.new_conversation(&mut talk);

        assert_eq!(session.switch_to_chat(2, &mut talk), Outcome::default());
        talk.begin_request(Mode::Sparring, "Now").unwrap();
        assert_eq!(session.switch_to_chat(1, &mut talk), Outcome::default());
        assert_eq!(talk.entries(), [user("Now")]);
        assert_eq!(session.chat_id(), None);
    }

    #[test]
    fn leaving_a_saved_chat_is_reported_but_leaving_an_unsaved_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = ChatSession::load_from(dir.path().join("chats.json"));
        let mut talk = Conversation::default();
        let _ = session.open_document(Path::new("/post.md"), &mut talk);
        assert_eq!(session.close_document(), Outcome::default());

        let _ = session.open_document(Path::new("/post.md"), &mut talk);
        let _ = session.persist(&conversation(&["One"]), 100);
        assert_eq!(session.new_conversation(&mut talk).chat_changes, [None]);

        let _ = session.switch_to_chat(1, &mut talk);
        assert_eq!(
            session
                .open_document(Path::new("/other.md"), &mut talk)
                .chat_changes,
            [None]
        );
        assert!(talk.entries().is_empty(), "opening a file starts afresh");

        let _ = session.open_document(Path::new("/post.md"), &mut talk);
        let _ = session.switch_to_chat(1, &mut talk);
        assert_eq!(session.close_document().chat_changes, [None]);
        assert_eq!(
            talk.entries(),
            [user("One")],
            "closing keeps the conversation"
        );
        assert!(session.chats().is_empty());
    }
}
