//! Earlier chats about each document, kept across restarts so the writer can return to them.
//! Chats are filed under the document's path; a document without one (not yet saved) has no
//! history. Unlike the session state, the history is the writer's own material: a file that
//! exists but cannot be read is reported and never overwritten.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::chat::Entry;
use crate::xdg;

/// How many chats are kept per document; the oldest go first.
pub const MAX_CHATS_PER_DOCUMENT: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedChat {
    /// Unique among the chats of one document.
    pub id: u64,
    /// When the chat was first saved, in seconds since the Unix epoch.
    pub started: i64,
    pub entries: Vec<Entry>,
}

impl SavedChat {
    /// The writer's first message, which names the chat in the list.
    pub fn first_message(&self) -> Option<&str> {
        first_message(&self.entries)
    }
}

/// The first user message in `entries`, if any.
pub fn first_message(entries: &[Entry]) -> Option<&str> {
    entries.iter().find_map(|entry| match entry {
        Entry::User { text } => Some(text.as_str()),
        _ => None,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatHistory {
    /// Chats by document path, newest first.
    documents: BTreeMap<String, Vec<SavedChat>>,
}

impl ChatHistory {
    /// Loads the history file; a missing file is an empty history.
    pub fn load() -> Result<Self, String> {
        Self::load_from(&history_path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self, String> {
        let error = |e: &dyn fmt::Display| {
            format!("Could not read the chat history in {}: {e}", path.display())
        };
        match fs::read_to_string(path) {
            Ok(json) => serde_json::from_str(&json).map_err(|e| error(&e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(error(&e)),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&history_path()?)
    }

    /// Writes the history atomically, readable only by the owner since it holds the
    /// conversations and names the user's files; a leftover temp file from a previous crash is
    /// replaced.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let error = |e: &dyn fmt::Display| {
            format!("Could not save the chat history to {}: {e}", path.display())
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| error(&e))?;
        }
        let mut json = serde_json::to_string_pretty(self).map_err(|e| error(&e))?;
        json.push('\n');

        let temp_path = path.with_extension("json.tmp");
        let _ = fs::remove_file(&temp_path);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = options
            .open(&temp_path)
            .and_then(|mut file| {
                file.write_all(json.as_bytes())?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&temp_path, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result.map_err(|e| error(&e))
    }

    /// The chats about `document`, newest first.
    pub fn chats(&self, document: &Path) -> &[SavedChat] {
        document
            .to_str()
            .and_then(|key| self.documents.get(key))
            .map_or(&[], Vec::as_slice)
    }

    pub fn chat(&self, document: &Path, id: u64) -> Option<&SavedChat> {
        self.chats(document).iter().find(|chat| chat.id == id)
    }

    /// Saves `entries` as the chat `id` about `document`, or as a new chat started at
    /// `started` when `id` is `None` or no longer known. A new chat comes first, and the oldest
    /// beyond `MAX_CHATS_PER_DOCUMENT` are dropped. Returns the chat's id, or `None` for a path
    /// that is not valid UTF-8, which JSON cannot hold.
    pub fn store(
        &mut self,
        document: &Path,
        id: Option<u64>,
        started: i64,
        entries: Vec<Entry>,
    ) -> Option<u64> {
        let chats = self
            .documents
            .entry(document.to_str()?.to_string())
            .or_default();
        if let Some(chat) = id.and_then(|id| chats.iter_mut().find(|chat| chat.id == id)) {
            chat.entries = entries;
            return Some(chat.id);
        }
        let id = chats
            .iter()
            .map(|chat| chat.id)
            .max()
            .map_or(1, |max| max + 1);
        chats.insert(
            0,
            SavedChat {
                id,
                started,
                entries,
            },
        );
        chats.truncate(MAX_CHATS_PER_DOCUMENT);
        Some(id)
    }
}

/// The history file: `$XDG_DATA_HOME/counterpoint/chats.json`, or
/// `~/.local/share/counterpoint/chats.json`.
pub fn history_path() -> Result<PathBuf, String> {
    xdg::user_file_from_env("XDG_DATA_HOME", &[".local", "share"], "chats.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ProposalState;
    use crate::proposal::Edit;

    fn user(text: &str) -> Entry {
        Entry::User {
            text: text.to_string(),
        }
    }

    #[test]
    fn a_new_chat_comes_first_with_a_fresh_id() {
        let mut history = ChatHistory::default();
        let doc = Path::new("/blog/post.md");
        assert_eq!(history.store(doc, None, 100, vec![user("One")]), Some(1));
        assert_eq!(history.store(doc, None, 200, vec![user("Two")]), Some(2));
        let chats = history.chats(doc);
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].first_message(), Some("Two"));
        assert_eq!(chats[0].started, 200);
        assert_eq!(chats[1].first_message(), Some("One"));
    }

    #[test]
    fn storing_a_known_chat_updates_it_in_place() {
        let mut history = ChatHistory::default();
        let doc = Path::new("/blog/post.md");
        let first = history.store(doc, None, 100, vec![user("One")]);
        history.store(doc, None, 200, vec![user("Two")]);
        let entries = vec![user("One"), user("And more")];
        assert_eq!(history.store(doc, first, 300, entries.clone()), first);
        let chats = history.chats(doc);
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[1].entries, entries);
        assert_eq!(chats[1].started, 100, "the start time stays");
    }

    #[test]
    fn an_unknown_id_starts_a_new_chat() {
        let mut history = ChatHistory::default();
        let doc = Path::new("/blog/post.md");
        assert_eq!(history.store(doc, Some(7), 100, vec![user("One")]), Some(1));
    }

    #[test]
    fn chats_are_kept_per_document() {
        let mut history = ChatHistory::default();
        history.store(Path::new("/a.md"), None, 100, vec![user("A")]);
        history.store(Path::new("/b.md"), None, 100, vec![user("B")]);
        assert_eq!(history.chats(Path::new("/a.md")).len(), 1);
        assert_eq!(
            history.chats(Path::new("/b.md"))[0].first_message(),
            Some("B")
        );
        assert!(history.chats(Path::new("/c.md")).is_empty());
    }

    #[test]
    fn only_the_newest_chats_are_kept() {
        let mut history = ChatHistory::default();
        let doc = Path::new("/post.md");
        for i in 0..MAX_CHATS_PER_DOCUMENT + 2 {
            history.store(doc, None, i as i64, vec![user(&i.to_string())]);
        }
        let chats = history.chats(doc);
        assert_eq!(chats.len(), MAX_CHATS_PER_DOCUMENT);
        assert_eq!(chats.last().unwrap().first_message(), Some("2"));
    }

    #[test]
    fn a_non_utf8_path_is_not_stored() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let mut history = ChatHistory::default();
        let doc = Path::new(OsStr::from_bytes(b"/caf\xe9.md"));
        assert_eq!(history.store(doc, None, 1, vec![user("x")]), None);
        assert!(history.chats(doc).is_empty());
    }

    #[test]
    fn save_then_load_round_trips_every_kind_of_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("counterpoint").join("chats.json");
        let mut history = ChatHistory::default();
        history.store(
            Path::new("/blog/post.md"),
            None,
            1_790_000_000,
            vec![
                user("Tighten it."),
                Entry::Proposal {
                    explanation: "Shorter.".to_string(),
                    edits: vec![Edit {
                        original: "Old.".to_string(),
                        replacement: "New.".to_string(),
                    }],
                    state: ProposalState::Applied,
                    reply: "raw reply".to_string(),
                },
                Entry::Assistant {
                    text: "*Done*".to_string(),
                },
                Entry::Error {
                    text: "timeout".to_string(),
                },
            ],
        );
        history.save_to(&path).unwrap();
        assert_eq!(ChatHistory::load_from(&path), Ok(history));
    }

    #[test]
    fn a_missing_file_is_an_empty_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chats.json");
        assert_eq!(ChatHistory::load_from(&path), Ok(ChatHistory::default()));
    }

    #[test]
    fn an_invalid_file_is_an_error_rather_than_an_empty_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chats.json");
        fs::write(&path, "not json").unwrap();
        let error = ChatHistory::load_from(&path).unwrap_err();
        assert!(error.contains("chats.json"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn history_file_is_readable_only_by_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chats.json");
        ChatHistory::default().save_to(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
