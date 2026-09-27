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
    },
}

/// Identifies an in-flight request so its reply can be matched to the conversation that sent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestTicket {
    generation: u64,
    mode: Mode,
    user_input: String,
}

#[derive(Debug, Default)]
pub struct Conversation {
    entries: Vec<Entry>,
    history: Vec<ChatMessage>,
    /// Incremented on every reset; replies from older generations are discarded.
    generation: u64,
    busy: bool,
}

impl Conversation {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn history(&self) -> &[ChatMessage] {
        &self.history
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
            Err(message) => self.entries.push(Entry::Error { text: message }),
            Ok(reply) => {
                self.history.push(ChatMessage::user(ticket.user_input));
                self.history.push(ChatMessage::assistant(reply.clone()));
                self.entries.push(entry_for_reply(ticket.mode, reply));
            }
        }
        true
    }

    /// Starts over in a fresh context.
    pub fn reset(&mut self) {
        self.entries.clear();
        self.history.clear();
        self.generation += 1;
        self.busy = false;
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
                self.entries.push(Entry::Error { text: text.clone() });
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

fn entry_for_reply(mode: Mode, reply: String) -> Entry {
    if mode == Mode::Sparring {
        return Entry::Assistant { text: reply };
    }
    match proposal::parse(&reply) {
        Ok(parsed) if parsed.edits.is_empty() => Entry::Assistant { text: reply },
        Ok(parsed) => Entry::Proposal {
            explanation: parsed.explanation,
            edits: parsed.edits,
            state: ProposalState::Pending,
        },
        Err(error) => Entry::Error {
            text: format!("Could not read the proposed edits ({error}). The reply was:\n\n{reply}"),
        },
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
            }
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
    fn malformed_edits_become_an_error_entry_with_the_raw_reply() {
        let reply = "```original\nOld text.\n```";
        let conversation = conversation_with_reply(Mode::Ghostwriting, reply);
        match &conversation.entries()[1] {
            Entry::Error { text } => {
                assert!(text.contains("Could not read the proposed edits"), "{text}");
                assert!(text.contains(reply), "{text}");
            }
            other => panic!("expected error entry, got {other:?}"),
        }
    }

    #[test]
    fn successful_reply_extends_history() {
        let conversation = conversation_with_reply(Mode::Sparring, "Answer");
        assert_eq!(
            conversation.history(),
            &[
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
            &[
                ChatMessage::user("New"),
                ChatMessage::assistant("new reply")
            ]
        );
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
