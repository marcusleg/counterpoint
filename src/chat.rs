//! Conversation state behind the chat pane: displayed entries, LLM history and request lifecycle.

use crate::llm::ChatMessage;
use crate::prompt::Mode;
use crate::proposal::{self, Edit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalState {
    Pending,
    Applied,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    User {
        text: String,
    },
    Assistant {
        text: String,
    },
    Error {
        text: String,
    },
    Proposal {
        explanation: String,
        edits: Vec<Edit>,
        state: ProposalState,
        /// The model's reply as received, which is what it sees as its own turn in the history.
        reply: String,
    },
}

/// Identifies an in-flight request so its reply can be matched to the conversation that sent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestTicket {
    generation: u64,
    mode: Mode,
    user_input: String,
}

/// The entries are the only state: the history sent to the model is derived from them, so
/// persisting the conversation means persisting the entries.
#[derive(Debug, Default)]
pub struct Conversation {
    entries: Vec<Entry>,
    /// Incremented on every reset or cancellation; replies from older generations are discarded.
    generation: u64,
    busy: bool,
}

impl Conversation {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The completed turns as the model sees them: every user message that got a reply,
    /// followed by that reply. Failed, cancelled and in-flight requests are left out.
    pub fn history(&self) -> Vec<ChatMessage> {
        let mut history = Vec::with_capacity(self.entries.len());
        for (index, entry) in self.entries.iter().enumerate() {
            match entry {
                Entry::User { text } => {
                    let answered = matches!(
                        self.entries.get(index + 1),
                        Some(Entry::Assistant { .. } | Entry::Proposal { .. })
                    );
                    if answered {
                        history.push(ChatMessage::user(text.clone()));
                    }
                }
                Entry::Assistant { text } => history.push(ChatMessage::assistant(text.clone())),
                Entry::Proposal { reply, .. } => {
                    history.push(ChatMessage::assistant(reply.clone()))
                }
                Entry::Error { .. } => {}
            }
        }
        history
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Records the user's message and marks the conversation busy. Returns `None` while a
    /// request is already in flight.
    pub fn begin_request(&mut self, mode: Mode, user_input: &str) -> Option<RequestTicket> {
        if self.busy {
            return None;
        }
        self.busy = true;
        self.entries.push(Entry::User {
            text: user_input.to_string(),
        });
        Some(RequestTicket {
            generation: self.generation,
            mode,
            user_input: user_input.to_string(),
        })
    }

    /// Records the outcome of a request. Returns `false` if the reply belongs to a conversation
    /// that has since been reset, in which case nothing changes.
    pub fn finish_request(
        &mut self,
        ticket: RequestTicket,
        result: Result<String, String>,
    ) -> bool {
        if ticket.generation != self.generation {
            return false;
        }
        self.busy = false;
        match result {
            Err(message) => self.push_error(message),
            Ok(reply) => self.entries.extend(entries_for_reply(ticket.mode, reply)),
        }
        true
    }

    /// Abandons the in-flight request: its reply is discarded when it arrives. Returns the
    /// user's message, removed from the conversation so it can go back into the input.
    pub fn cancel_request(&mut self) -> Option<String> {
        if !self.busy {
            return None;
        }
        self.busy = false;
        self.generation += 1;
        match self.entries.pop() {
            Some(Entry::User { text }) => Some(text),
            Some(other) => {
                self.entries.push(other);
                None
            }
            None => None,
        }
    }

    /// Starts over in a fresh context.
    pub fn reset(&mut self) {
        self.entries.clear();
        self.generation += 1;
        self.busy = false;
    }

    /// Adds an error entry, unless the same error is already the last entry: clicking Apply
    /// repeatedly on a stale proposal should not fill the conversation with copies.
    fn push_error(&mut self, text: String) {
        if matches!(self.entries.last(), Some(Entry::Error { text: last }) if *last == text) {
            return;
        }
        self.entries.push(Entry::Error { text });
    }

    /// Applies a pending proposal to `document_md` and returns the new Markdown. On failure the
    /// proposal stays pending and an error entry explains why.
    pub fn apply_proposal(&mut self, index: usize, document_md: &str) -> Result<String, String> {
        let result = match self.entries.get(index) {
            Some(Entry::Proposal {
                edits,
                state: ProposalState::Pending,
                ..
            }) => proposal::apply(document_md, edits).map_err(|e| e.to_string()),
            _ => return Err("This proposal can no longer be applied.".to_string()),
        };
        match result {
            Ok(markdown) => {
                if let Some(Entry::Proposal { state, .. }) = self.entries.get_mut(index) {
                    *state = ProposalState::Applied;
                }
                Ok(markdown)
            }
            Err(message) => {
                let text = format!("Could not apply the proposal. {message}");
                self.push_error(text.clone());
                Err(text)
            }
        }
    }

    pub fn reject_proposal(&mut self, index: usize) -> bool {
        match self.entries.get_mut(index) {
            Some(Entry::Proposal { state, .. }) if *state == ProposalState::Pending => {
                *state = ProposalState::Rejected;
                true
            }
            _ => false,
        }
    }
}

/// The entries a reply turns into: one message or proposal, or, for a Ghostwriting reply whose
/// edits cannot be read, the reply as a message followed by a short error, so the reply stays
/// readable and the user can ask the model to fix its formatting.
fn entries_for_reply(mode: Mode, reply: String) -> Vec<Entry> {
    if mode == Mode::Sparring {
        return vec![Entry::Assistant { text: reply }];
    }
    match proposal::parse(&reply) {
        Ok(parsed) if parsed.edits.is_empty() => vec![Entry::Assistant { text: reply }],
        Ok(parsed) => vec![Entry::Proposal {
            explanation: parsed.explanation,
            edits: parsed.edits,
            state: ProposalState::Pending,
            reply,
        }],
        Err(error) => vec![
            Entry::Assistant { text: reply },
            Entry::Error {
                text: format!(
                    "Could not read the proposed edits: {error}. Ask for the change again."
                ),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GHOST_REPLY: &str =
        "Shorter.\n\n```original\nOld text.\n```\n```replacement\nNew text.\n```";

    fn conversation_with_reply(mode: Mode, reply: &str) -> Conversation {
        let mut conversation = Conversation::default();
        let ticket = conversation.begin_request(mode, "Please help").unwrap();
        assert!(conversation.finish_request(ticket, Ok(reply.to_string())));
        conversation
    }

    #[test]
    fn begin_request_adds_user_entry_and_sets_busy() {
        let mut conversation = Conversation::default();
        assert!(conversation
            .begin_request(Mode::Sparring, "Hello")
            .is_some());
        assert!(conversation.is_busy());
        assert_eq!(
            conversation.entries(),
            &[Entry::User {
                text: "Hello".to_string()
            }]
        );
    }

    #[test]
    fn begin_request_while_busy_is_ignored() {
        let mut conversation = Conversation::default();
        conversation.begin_request(Mode::Sparring, "One").unwrap();
        assert!(conversation.begin_request(Mode::Sparring, "Two").is_none());
        assert_eq!(conversation.entries().len(), 1);
    }

    #[test]
    fn sparring_reply_is_plain_text_even_with_edit_blocks() {
        let conversation = conversation_with_reply(Mode::Sparring, GHOST_REPLY);
        assert!(!conversation.is_busy());
        assert_eq!(
            conversation.entries()[1],
            Entry::Assistant {
                text: GHOST_REPLY.to_string()
            }
        );
    }

    #[test]
    fn ghostwriting_reply_with_edits_becomes_pending_proposal() {
        let conversation = conversation_with_reply(Mode::Ghostwriting, GHOST_REPLY);
        assert_eq!(
            conversation.entries()[1],
            Entry::Proposal {
                explanation: "Shorter.".to_string(),
                edits: vec![Edit {
                    original: "Old text.".to_string(),
                    replacement: "New text.".to_string()
                }],
                state: ProposalState::Pending,
                reply: GHOST_REPLY.to_string(),
            }
        );
        assert_eq!(
            conversation.history(),
            vec![
                ChatMessage::user("Please help"),
                ChatMessage::assistant(GHOST_REPLY)
            ]
        );
    }

    #[test]
    fn ghostwriting_reply_without_edits_is_plain_text() {
        let conversation = conversation_with_reply(Mode::Ghostwriting, "No change needed.");
        assert_eq!(
            conversation.entries()[1],
            Entry::Assistant {
                text: "No change needed.".to_string()
            }
        );
    }

    #[test]
    fn malformed_edits_show_the_reply_followed_by_an_error() {
        let reply = "```original\nOld text.\n```";
        let conversation = conversation_with_reply(Mode::Ghostwriting, reply);
        assert_eq!(
            conversation.entries()[1],
            Entry::Assistant {
                text: reply.to_string()
            }
        );
        match &conversation.entries()[2] {
            Entry::Error { text } => {
                assert!(text.contains("Could not read the proposed edits"), "{text}");
                assert!(text.contains("without a `replacement` block"), "{text}");
            }
            other => panic!("expected error entry, got {other:?}"),
        }
        // The reply stays in the history, so "fix your formatting" has something to refer to.
        assert_eq!(conversation.history().len(), 2);
    }

    #[test]
    fn successful_reply_extends_history() {
        let conversation = conversation_with_reply(Mode::Sparring, "Answer");
        assert_eq!(
            conversation.history(),
            vec![
                ChatMessage::user("Please help"),
                ChatMessage::assistant("Answer")
            ]
        );
    }

    #[test]
    fn failed_request_shows_error_and_leaves_history_unchanged() {
        let mut conversation = Conversation::default();
        let ticket = conversation.begin_request(Mode::Sparring, "Hello").unwrap();
        assert!(conversation.finish_request(ticket, Err("timeout".to_string())));
        assert!(!conversation.is_busy());
        assert!(conversation.history().is_empty());
        assert_eq!(
            conversation.entries()[1],
            Entry::Error {
                text: "timeout".to_string()
            }
        );
    }

    #[test]
    fn reset_clears_everything_and_discards_stale_replies() {
        let mut conversation = Conversation::default();
        let ticket = conversation.begin_request(Mode::Sparring, "Hello").unwrap();
        conversation.reset();
        assert!(!conversation.is_busy());
        assert!(conversation.entries().is_empty());

        assert!(!conversation.finish_request(ticket, Ok("late".to_string())));
        assert!(conversation.entries().is_empty());
        assert!(conversation.history().is_empty());
    }

    #[test]
    fn new_request_after_reset_is_not_affected_by_old_reply() {
        let mut conversation = Conversation::default();
        let old = conversation.begin_request(Mode::Sparring, "Old").unwrap();
        conversation.reset();
        let new = conversation.begin_request(Mode::Sparring, "New").unwrap();

        assert!(!conversation.finish_request(old, Ok("old reply".to_string())));
        assert!(conversation.is_busy());
        assert!(conversation.finish_request(new, Ok("new reply".to_string())));
        assert_eq!(
            conversation.history(),
            vec![
                ChatMessage::user("New"),
                ChatMessage::assistant("new reply")
            ]
        );
    }

    #[test]
    fn cancel_removes_the_pending_message_and_discards_the_late_reply() {
        let mut conversation = conversation_with_reply(Mode::Sparring, "Answer");
        let ticket = conversation
            .begin_request(Mode::Sparring, "Never mind")
            .unwrap();
        assert_eq!(
            conversation.cancel_request(),
            Some("Never mind".to_string())
        );
        assert!(!conversation.is_busy());
        assert_eq!(conversation.entries().len(), 2);

        assert!(!conversation.finish_request(ticket, Ok("late".to_string())));
        assert_eq!(conversation.entries().len(), 2);
        assert_eq!(conversation.history().len(), 2);
        assert_eq!(conversation.cancel_request(), None);
    }

    #[test]
    fn history_skips_failed_turns_and_errors() {
        let mut conversation = Conversation::default();
        let ticket = conversation.begin_request(Mode::Sparring, "One").unwrap();
        conversation.finish_request(ticket, Err("down".to_string()));
        let ticket = conversation.begin_request(Mode::Sparring, "Two").unwrap();
        conversation.finish_request(ticket, Ok("Reply".to_string()));
        let ticket = conversation.begin_request(Mode::Sparring, "Three").unwrap();
        assert_eq!(
            conversation.history(),
            vec![ChatMessage::user("Two"), ChatMessage::assistant("Reply")]
        );
        conversation.finish_request(ticket, Ok("More".to_string()));
        assert_eq!(conversation.history().len(), 4);
    }

    #[test]
    fn repeated_failed_apply_adds_one_error() {
        let mut conversation = conversation_with_reply(Mode::Ghostwriting, GHOST_REPLY);
        assert!(conversation.apply_proposal(1, "Other.").is_err());
        assert!(conversation.apply_proposal(1, "Other.").is_err());
        assert_eq!(conversation.entries().len(), 3);
        assert!(matches!(conversation.entries()[2], Entry::Error { .. }));
    }

    #[test]
    fn apply_proposal_marks_it_applied_and_returns_new_markdown() {
        let mut conversation = conversation_with_reply(Mode::Ghostwriting, GHOST_REPLY);
        let result = conversation.apply_proposal(1, "# T\n\nOld text.\n");
        assert_eq!(result, Ok("# T\n\nNew text.\n".to_string()));
        assert!(matches!(
            conversation.entries()[1],
            Entry::Proposal {
                state: ProposalState::Applied,
                ..
            }
        ));
    }

    #[test]
    fn failed_apply_keeps_proposal_pending_and_adds_error() {
        let mut conversation = conversation_with_reply(Mode::Ghostwriting, GHOST_REPLY);
        let result = conversation.apply_proposal(1, "# T\n\nSomething else.\n");
        assert!(result.is_err());
        assert!(matches!(
            conversation.entries()[1],
            Entry::Proposal {
                state: ProposalState::Pending,
                ..
            }
        ));
        match conversation.entries().last().unwrap() {
            Entry::Error { text } => assert!(text.contains("Edit 1"), "{text}"),
            other => panic!("expected error entry, got {other:?}"),
        }
    }

    #[test]
    fn proposal_cannot_be_applied_twice_or_after_rejection() {
        let mut conversation = conversation_with_reply(Mode::Ghostwriting, GHOST_REPLY);
        conversation.apply_proposal(1, "Old text.").unwrap();
        assert!(conversation.apply_proposal(1, "Old text.").is_err());

        let mut conversation = conversation_with_reply(Mode::Ghostwriting, GHOST_REPLY);
        assert!(conversation.reject_proposal(1));
        assert!(matches!(
            conversation.entries()[1],
            Entry::Proposal {
                state: ProposalState::Rejected,
                ..
            }
        ));
        assert!(conversation.apply_proposal(1, "Old text.").is_err());
        assert!(!conversation.reject_proposal(1));
    }

    #[test]
    fn apply_on_a_non_proposal_entry_fails() {
        let mut conversation = conversation_with_reply(Mode::Sparring, "Answer");
        assert!(conversation.apply_proposal(0, "x").is_err());
        assert!(conversation.apply_proposal(99, "x").is_err());
    }
}
