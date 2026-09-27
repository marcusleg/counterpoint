//! QObject that runs chat requests against the LLM and exposes the conversation to QML.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, ghostwriting)]
        #[qproperty(bool, busy)]
        #[qproperty(QString, messages_json, cxx_name = "messagesJson")]
        type ChatController = super::ChatControllerRust;

        #[qinvokable]
        fn send(
            self: Pin<&mut Self>,
            user_input: &QString,
            document_markdown: &QString,
            selection_markdown: &QString,
        );

        #[qinvokable]
        #[cxx_name = "newConversation"]
        fn new_conversation(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "applyProposal"]
        fn apply_proposal(self: Pin<&mut Self>, index: i32, document_markdown: &QString);

        #[qinvokable]
        #[cxx_name = "rejectProposal"]
        fn reject_proposal(self: Pin<&mut Self>, index: i32);

        #[qsignal]
        #[cxx_name = "proposalApplied"]
        fn proposal_applied(self: Pin<&mut Self>, markdown: QString);
    }

    impl cxx_qt::Threading for ChatController {}
}

use core::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use crate::chat::Conversation;
use crate::config::Config;
use crate::llm::{self, LlmError};
use crate::prompt::{self, Mode};

pub struct ChatControllerRust {
    ghostwriting: bool,
    busy: bool,
    messages_json: QString,
    conversation: Conversation,
}

impl Default for ChatControllerRust {
    fn default() -> Self {
        Self {
            ghostwriting: false,
            busy: false,
            messages_json: QString::from("[]"),
            conversation: Conversation::default(),
        }
    }
}

impl qobject::ChatController {
    fn send(
        mut self: Pin<&mut Self>,
        user_input: &QString,
        document_markdown: &QString,
        selection_markdown: &QString,
    ) {
        let user_input = user_input.to_string();
        if user_input.trim().is_empty() || self.rust().conversation.is_busy() {
            return;
        }
        let mode = if *self.ghostwriting() {
            Mode::Ghostwriting
        } else {
            Mode::Sparring
        };
        let selection = selection_markdown.to_string();
        let messages = prompt::build_messages(
            mode,
            &document_markdown.to_string(),
            Some(selection.as_str()),
            self.rust().conversation.history(),
            &user_input,
        );
        let Some(ticket) = self
            .as_mut()
            .rust_mut()
            .conversation
            .begin_request(mode, &user_input)
        else {
            return;
        };
        self.as_mut().sync();

        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = Config::load()
                .map_err(LlmError::Config)
                .and_then(|config| llm::complete(&config, &messages))
                .map_err(|e| e.to_string());
            // Queueing only fails once the QObject is destroyed; the reply has nowhere to go then.
            let _ = qt_thread.queue(move |mut chat| {
                if chat
                    .as_mut()
                    .rust_mut()
                    .conversation
                    .finish_request(ticket, result)
                {
                    chat.as_mut().sync();
                }
            });
        });
    }

    fn new_conversation(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().conversation.reset();
        self.sync();
    }

    fn apply_proposal(mut self: Pin<&mut Self>, index: i32, document_markdown: &QString) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let result = self
            .as_mut()
            .rust_mut()
            .conversation
            .apply_proposal(index, &document_markdown.to_string());
        self.as_mut().sync();
        if let Ok(markdown) = result {
            self.proposal_applied(QString::from(markdown.as_str()));
        }
    }

    fn reject_proposal(mut self: Pin<&mut Self>, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            self.as_mut().rust_mut().conversation.reject_proposal(index);
        }
        self.sync();
    }

    /// Publishes the conversation state through the QML-visible properties.
    fn sync(mut self: Pin<&mut Self>) {
        let busy = self.rust().conversation.is_busy();
        let json = QString::from(self.rust().conversation.entries_json().as_str());
        self.as_mut().set_busy(busy);
        self.as_mut().set_messages_json(json);
    }
}
