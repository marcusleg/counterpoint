#pragma once

#include <QtGui/QSyntaxHighlighter>
#include <QtGui/QTextCharFormat>

#include <array>

// Styles Markdown source in place: headings, emphasis, code, links, quote and list markers,
// front matter and HTML comments. The text itself is never changed.
class MarkdownHighlighter : public QSyntaxHighlighter
{
public:
  explicit MarkdownHighlighter(QTextDocument* document);

  // Attaches a highlighter to `document` unless it already has one; the document owns it.
  static void attach(QTextDocument& document);

protected:
  void highlightBlock(const QString& text) override;

private:
  enum BlockState
  {
    Normal = 0,
    FrontMatter = 1,
    BacktickFence = 2,
    TildeFence = 3,
    HtmlComment = 4,
  };

  void highlightInline(const QString& text);
  // Styles HTML comments; returns true if a comment is still open at the end of the line.
  bool highlightComments(const QString& text, bool startsInComment);
  void mergeFormat(qsizetype start, qsizetype length, const QTextCharFormat& format);

  std::array<QTextCharFormat, 6> m_headings;
  QTextCharFormat m_bold;
  QTextCharFormat m_italic;
  QTextCharFormat m_code;
  QTextCharFormat m_marker;
  QTextCharFormat m_muted;
  QTextCharFormat m_comment;
};
