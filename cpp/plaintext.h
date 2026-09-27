#pragma once

#include <QtCore/QString>
#include <QtGui/QTextDocument>

#include <cstdint>

// Helpers for a QTextDocument that holds Markdown source verbatim as plain text.
namespace plaintext {

// Exact document text: non-breaking spaces preserved, block separators as '\n'.
QString text(const QTextDocument& doc);

// Replaces the content, marks the document unmodified and clears the undo/redo history.
void load(QTextDocument& doc, const QString& text);

// Exact text of the character range [start, end).
QString rangeText(const QTextDocument& doc, ::std::int32_t start, ::std::int32_t end);

// Replaces the content with `replacement` as one undo step, touching only the span that differs.
void replaceUndoable(QTextDocument& doc, const QString& replacement);

} // namespace plaintext
