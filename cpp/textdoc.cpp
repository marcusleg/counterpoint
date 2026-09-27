#include "textdoc.h"

#include "markdown_highlighter.h"
#include "plaintext.h"

QString textdocText(const QQuickTextDocument& doc)
{
  return plaintext::text(*doc.textDocument());
}

void textdocLoadText(QQuickTextDocument& doc, const QString& text)
{
  plaintext::load(*doc.textDocument(), text);
  doc.setModified(false);
}

QString textdocRangeText(const QQuickTextDocument& doc, ::std::int32_t start, ::std::int32_t end)
{
  return plaintext::rangeText(*doc.textDocument(), start, end);
}

void textdocReplaceTextUndoable(QQuickTextDocument& doc, const QString& text)
{
  plaintext::replaceUndoable(*doc.textDocument(), text);
}

void textdocSetModified(QQuickTextDocument& doc, bool modified)
{
  doc.setModified(modified);
}

void textdocAttachHighlighter(QQuickTextDocument& doc)
{
  MarkdownHighlighter::attach(*doc.textDocument());
}
