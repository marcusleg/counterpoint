//! The rows of the chat pane's message list: the writer's messages, replies, errors and
//! proposal cards.

use adw::prelude::*;
use gtk::pango;

use crate::chat::{Entry, ProposalState};
use crate::chat_markup;
use crate::proposal::Edit;

/// The row showing `entry` in the conversation. A pending proposal gets buttons that call
/// `on_apply` and `on_reject`.
pub fn row(
    entry: &Entry,
    on_apply: impl Fn() + 'static,
    on_reject: impl Fn() + 'static,
) -> gtk::ListBoxRow {
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
            proposal_card(explanation, edits, *state, on_apply, on_reject).upcast(),
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
    explanation: &str,
    edits: &[Edit],
    state: ProposalState,
    on_apply: impl Fn() + 'static,
    on_reject: impl Fn() + 'static,
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
            apply.connect_clicked(move |_| on_apply());
            let reject = gtk::Button::builder()
                .label("_Reject")
                .use_underline(true)
                .build();
            reject.connect_clicked(move |_| on_reject());
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
