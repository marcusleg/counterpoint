#pragma once

#include <QtCore/QString>
#include <QtQuick/QQuickTextDocument>

#include <cstdint>

// Bridge helpers for the editor TextArea's document, which holds Markdown source verbatim.
// The logic lives in plaintext.h; these wrappers adapt QQuickTextDocument for the Rust bridge.

QString textdocText(const QQuickTextDocument& doc);

// Replaces the content, marks the document unmodified and clears the undo history.
void textdocLoadText(QQuickTextDocument& doc, const QString& text);

// Exact source text of the character range [start, end).
QString textdocRangeText(const QQuickTextDocument& doc, ::std::int32_t start, ::std::int32_t end);

// Replaces the content as a single undo step, touching only the span that differs.
void textdocReplaceTextUndoable(QQuickTextDocument& doc, const QString& text);

void textdocSetModified(QQuickTextDocument& doc, bool modified);

// Attaches Markdown syntax styling to the document (idempotent).
void textdocAttachHighlighter(QQuickTextDocument& doc);
