#include "plaintext.h"

#include <QtGui/QTextCursor>

#include <algorithm>

namespace plaintext {

QString text(const QTextDocument& doc)
{
  // toRawText keeps non-breaking spaces (toPlainText converts them to spaces).
  QString raw = doc.toRawText();
  raw.replace(QChar::ParagraphSeparator, u'\n');
  raw.replace(QChar::LineSeparator, u'\n');
  return raw;
}

void load(QTextDocument& doc, const QString& text)
{
  doc.setPlainText(text);
  doc.clearUndoRedoStacks();
  doc.setModified(false);
}

QString rangeText(const QTextDocument& doc, ::std::int32_t start, ::std::int32_t end)
{
  // Document positions equal indices into the raw text, whose separators map 1:1 to '\n'.
  if (end <= start)
    return {};
  return text(doc).mid(start, end - start);
}

void replaceUndoable(QTextDocument& doc, const QString& replacement)
{
  const QString current = text(doc);
  if (current == replacement)
    return;

  const qsizetype shorter = std::min(current.size(), replacement.size());
  qsizetype prefix = 0;
  while (prefix < shorter && current[prefix] == replacement[prefix])
    ++prefix;
  if (prefix > 0 && current[prefix - 1].isHighSurrogate())
    --prefix; // never split a surrogate pair

  qsizetype suffix = 0;
  while (suffix < shorter - prefix
         && current[current.size() - 1 - suffix] == replacement[replacement.size() - 1 - suffix])
    ++suffix;
  if (suffix > 0 && current[current.size() - suffix].isLowSurrogate())
    --suffix;

  QTextCursor cursor(&doc);
  cursor.beginEditBlock();
  cursor.setPosition(int(prefix));
  cursor.setPosition(int(current.size() - suffix), QTextCursor::KeepAnchor);
  cursor.removeSelectedText();
  cursor.insertText(replacement.mid(prefix, replacement.size() - prefix - suffix));
  cursor.endEditBlock();
}

} // namespace plaintext
