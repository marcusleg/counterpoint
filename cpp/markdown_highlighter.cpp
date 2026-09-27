#include "markdown_highlighter.h"

#include <QtCore/QRegularExpression>
#include <QtGui/QColor>
#include <QtGui/QFontDatabase>
#include <QtGui/QTextDocument>

#include <initializer_list>

namespace {

const QString kObjectName = QStringLiteral("markdownHighlighter");
const std::array<qreal, 6> kHeadingScale = { 1.6, 1.4, 1.25, 1.1, 1.0, 1.0 };

// Returns '`' or '~' when `text` is a fenced code block delimiter, otherwise a null QChar.
QChar fenceChar(const QString& text)
{
  static const QRegularExpression fence(QStringLiteral("^ {0,3}(`{3,}|~{3,})"));
  const QRegularExpressionMatch match = fence.match(text);
  return match.hasMatch() ? match.captured(1).at(0) : QChar();
}

} // namespace

MarkdownHighlighter::MarkdownHighlighter(QTextDocument* document)
  : QSyntaxHighlighter(document)
{
  setObjectName(kObjectName);

  const qreal basePointSize = document->defaultFont().pointSizeF();
  for (std::size_t level = 0; level < m_headings.size(); ++level) {
    m_headings[level].setFontWeight(QFont::Bold);
    if (basePointSize > 0)
      m_headings[level].setFontPointSize(basePointSize * kHeadingScale[level]);
  }
  m_bold.setFontWeight(QFont::Bold);
  m_italic.setFontItalic(true);
  m_code.setFontFamilies(QFontDatabase::systemFont(QFontDatabase::FixedFont).families());
  m_marker.setForeground(QColor(0x3d, 0x7f, 0xd6));
  m_muted.setForeground(QColor(0x88, 0x88, 0x88));
  m_comment = m_muted;
  m_comment.setFontItalic(true);
}

void MarkdownHighlighter::attach(QTextDocument& document)
{
  if (document.findChild<QSyntaxHighlighter*>(kObjectName) == nullptr)
    new MarkdownHighlighter(&document);
}

void MarkdownHighlighter::highlightBlock(const QString& text)
{
  const int previous = previousBlockState();
  const qsizetype length = text.size();

  // A line that starts inside an HTML comment: heading/fence/list detection does not apply.
  if (previous == HtmlComment) {
    if (!text.contains(QLatin1String("-->"))) {
      setFormat(0, int(length), m_comment);
      setCurrentBlockState(HtmlComment);
      return;
    }
    setCurrentBlockState(Normal);
    highlightInline(text);
    if (highlightComments(text, true))
      setCurrentBlockState(HtmlComment);
    return;
  }

  // YAML front matter: a first line of '---' up to the next '---' or '...' line.
  if (currentBlock().blockNumber() == 0 && text == QLatin1String("---")) {
    setFormat(0, int(length), m_muted);
    setCurrentBlockState(FrontMatter);
    return;
  }
  if (previous == FrontMatter) {
    setFormat(0, int(length), m_muted);
    const bool closes = text == QLatin1String("---") || text == QLatin1String("...");
    setCurrentBlockState(closes ? Normal : FrontMatter);
    return;
  }

  // Fenced code blocks.
  const QChar fence = fenceChar(text);
  if (previous == BacktickFence || previous == TildeFence) {
    setFormat(0, int(length), m_code);
    const QChar open = previous == BacktickFence ? QChar(u'`') : QChar(u'~');
    setCurrentBlockState(fence == open ? Normal : previous);
    return;
  }
  if (!fence.isNull()) {
    setFormat(0, int(length), m_code);
    setCurrentBlockState(fence == u'`' ? BacktickFence : TildeFence);
    return;
  }

  setCurrentBlockState(Normal);

  static const QRegularExpression heading(QStringLiteral("^ {0,3}(#{1,6})(\\s|$)"));
  static const QRegularExpression quote(QStringLiteral("^ {0,3}>+"));
  static const QRegularExpression listMarker(QStringLiteral("^\\s*([-*+]|\\d+[.)])\\s"));

  const QRegularExpressionMatch headingMatch = heading.match(text);
  const QRegularExpressionMatch quoteMatch = quote.match(text);
  const QRegularExpressionMatch listMatch = listMarker.match(text);
  if (headingMatch.hasMatch()) {
    setFormat(0, int(length), m_headings[std::size_t(headingMatch.capturedLength(1) - 1)]);
    mergeFormat(headingMatch.capturedStart(1), headingMatch.capturedLength(1), m_marker);
  } else if (quoteMatch.hasMatch()) {
    mergeFormat(quoteMatch.capturedStart(), quoteMatch.capturedLength(), m_marker);
  } else if (listMatch.hasMatch()) {
    mergeFormat(listMatch.capturedStart(1), listMatch.capturedLength(1), m_marker);
  }

  highlightInline(text);
  if (highlightComments(text, previous == HtmlComment))
    setCurrentBlockState(HtmlComment);
}

void MarkdownHighlighter::highlightInline(const QString& text)
{
  static const QRegularExpression bold(QStringLiteral("(\\*\\*|__)(?=\\S)(.+?)(?<=\\S)\\1"));
  static const QRegularExpression italicStar(
    QStringLiteral("(?<![*\\w])\\*(?=[^\\s*])([^*]+?)(?<=[^\\s*])\\*(?![*\\w])"));
  static const QRegularExpression italicUnderscore(
    QStringLiteral("(?<![_\\w])_(?=[^\\s_])([^_]+?)(?<=[^\\s_])_(?![_\\w])"));
  static const QRegularExpression link(QStringLiteral("\\[[^\\]]+\\](\\([^)\\s]+(?:\\s+\"[^\"]*\")?\\))"));
  static const QRegularExpression codeSpan(QStringLiteral("`[^`]+`"));

  for (auto it = bold.globalMatch(text); it.hasNext();) {
    const QRegularExpressionMatch match = it.next();
    mergeFormat(match.capturedStart(), match.capturedLength(), m_bold);
  }
  for (const QRegularExpression* italic : { &italicStar, &italicUnderscore }) {
    for (auto it = italic->globalMatch(text); it.hasNext();) {
      const QRegularExpressionMatch match = it.next();
      mergeFormat(match.capturedStart(), match.capturedLength(), m_italic);
    }
  }
  for (auto it = link.globalMatch(text); it.hasNext();) {
    const QRegularExpressionMatch match = it.next();
    mergeFormat(match.capturedStart(1), match.capturedLength(1), m_muted);
  }
  for (auto it = codeSpan.globalMatch(text); it.hasNext();) {
    const QRegularExpressionMatch match = it.next();
    mergeFormat(match.capturedStart(), match.capturedLength(), m_code);
  }
}

bool MarkdownHighlighter::highlightComments(const QString& text, bool startsInComment)
{
  qsizetype start = startsInComment ? 0 : text.indexOf(QLatin1String("<!--"));
  while (start >= 0) {
    const qsizetype searchFrom = startsInComment ? 0 : start + 4;
    const qsizetype close = text.indexOf(QLatin1String("-->"), searchFrom);
    if (close < 0) {
      setFormat(int(start), int(text.size() - start), m_comment);
      return true;
    }
    setFormat(int(start), int(close + 3 - start), m_comment);
    startsInComment = false;
    start = text.indexOf(QLatin1String("<!--"), close + 3);
  }
  return false;
}

void MarkdownHighlighter::mergeFormat(qsizetype start, qsizetype length, const QTextCharFormat& format)
{
  for (qsizetype i = start; i < start + length; ++i) {
    QTextCharFormat merged = this->format(int(i));
    merged.merge(format);
    setFormat(int(i), 1, merged);
  }
}
