// Headless checks for the C++ editor core (plain-text model and Markdown highlighter).
// Build and run from editor/; see "Development checks" in README.md for the commands.

#include "markdown_highlighter.h"
#include "plaintext.h"

#include <QtCore/QFile>
#include <QtGui/QGuiApplication>
#include <QtGui/QTextBlock>
#include <QtGui/QTextDocument>
#include <QtGui/QTextLayout>

#include <cstdio>

namespace {

int failures = 0;

void check(bool ok, const char* what)
{
  if (!ok) {
    ++failures;
    std::printf("FAIL: %s\n", what);
  }
}

void checkRoundTrip(const char* path)
{
  QFile file(QString::fromLocal8Bit(path));
  if (!file.open(QIODevice::ReadOnly)) {
    ++failures;
    std::printf("FAIL: cannot read %s\n", path);
    return;
  }
  const QByteArray bytes = file.readAll();
  QTextDocument doc;
  plaintext::load(doc, QString::fromUtf8(bytes));
  if (plaintext::text(doc).toUtf8() != bytes) {
    ++failures;
    std::printf("FAIL: round trip changed %s\n", path);
  }
}

void checkPlainText()
{
  const QString original = QStringLiteral("---\ntitle: Old\n---\n\n# Head\n\nBody\u00a0text.\n<!-- note -->\n");
  QTextDocument doc;
  plaintext::load(doc, original);
  check(!doc.isUndoAvailable(), "load leaves no undo step");
  check(!doc.isModified(), "load marks the document unmodified");
  check(plaintext::text(doc) == original, "text() is exact, including non-breaking spaces");
  check(plaintext::rangeText(doc, 4, 14) == QStringLiteral("title: Old"), "rangeText returns the exact span");

  const QString edited = QStringLiteral("---\ntitle: New\n---\n\n# Head\n\nBody\u00a0text, edited.\n<!-- note -->\n");
  plaintext::replaceUndoable(doc, edited);
  check(plaintext::text(doc) == edited, "replaceUndoable produces the new text");
  check(doc.isModified(), "replaceUndoable marks the document modified");
  doc.undo();
  check(plaintext::text(doc) == original, "one undo restores the original");

  plaintext::load(doc, QStringLiteral("a \U0001F600 b\n"));
  plaintext::replaceUndoable(doc, QStringLiteral("a \U0001F601 b\n"));
  check(plaintext::text(doc) == QStringLiteral("a \U0001F601 b\n"), "replacing an emoji keeps surrogate pairs intact");

  plaintext::load(doc, QStringLiteral("same\n"));
  plaintext::replaceUndoable(doc, QStringLiteral("same\n"));
  check(!doc.isUndoAvailable(), "replacing with identical text adds no undo step");
}

QTextCharFormat formatAt(const QTextDocument& doc, int blockNumber, qsizetype column)
{
  const QTextBlock block = doc.findBlockByNumber(blockNumber);
  for (const QTextLayout::FormatRange& range : block.layout()->formats()) {
    if (column >= range.start && column < range.start + range.length)
      return range.format;
  }
  return {};
}

void checkHighlighter()
{
  const QColor muted(0x88, 0x88, 0x88);
  const QColor marker(0x3d, 0x7f, 0xd6);
  const QString source = QStringLiteral(
    "---\ntitle: T\n---\n# Heading\n"
    "Some **bold**, *em* and `code` with [a link](https://example.com).\n"
    "```\nfenced\n```\n<!-- a\ncomment -->\n- item\n> quote\n");

  QTextDocument doc;
  doc.setDefaultFont(QFont(QStringLiteral("Sans"), 12));
  plaintext::load(doc, source);
  MarkdownHighlighter highlighter(&doc);
  highlighter.rehighlight();

  const QString line = doc.findBlockByNumber(4).text();
  check(formatAt(doc, 1, 0).foreground().color() == muted, "front matter is muted");
  check(formatAt(doc, 3, 3).fontWeight() == QFont::Bold, "headings are bold");
  check(formatAt(doc, 3, 3).fontPointSize() > 12, "headings are larger");
  check(formatAt(doc, 4, line.indexOf(u"bold")).fontWeight() == QFont::Bold, "**bold** is bold");
  check(formatAt(doc, 4, line.indexOf(u"*em*") + 1).fontItalic(), "*em* is italic");
  check(!formatAt(doc, 4, line.indexOf(u"`code`") + 1).fontFamilies().toStringList().isEmpty(),
        "inline code uses the fixed-pitch font");
  check(formatAt(doc, 4, line.indexOf(u"https")).foreground().color() == muted, "link URLs are muted");
  check(!formatAt(doc, 6, 0).fontFamilies().toStringList().isEmpty(), "fenced code uses the fixed-pitch font");
  check(formatAt(doc, 9, 0).fontItalic(), "multi-line HTML comments are styled");
  check(formatAt(doc, 10, 0).foreground().color() == marker, "list markers are highlighted");
  check(formatAt(doc, 11, 0).foreground().color() == marker, "quote markers are highlighted");
  check(plaintext::text(doc) == source, "highlighting leaves the text unchanged");
  check(!doc.isModified(), "highlighting does not mark the document modified");

  QTextDocument commentDoc;
  commentDoc.setDefaultFont(QFont(QStringLiteral("Sans"), 12));
  plaintext::load(commentDoc, QStringLiteral("<!-- draft\n```\nstill comment -->\nafter\n"));
  MarkdownHighlighter commentHighlighter(&commentDoc);
  commentHighlighter.rehighlight();
  check(formatAt(commentDoc, 1, 0).fontItalic()
          && formatAt(commentDoc, 3, 0).fontFamilies().toStringList().isEmpty(),
        "a fence inside an HTML comment does not start a code block");

  QTextDocument other;
  MarkdownHighlighter::attach(other);
  MarkdownHighlighter::attach(other);
  check(other.findChildren<QSyntaxHighlighter*>().size() == 1, "attach is idempotent");
}

} // namespace

int main(int argc, char** argv)
{
  QGuiApplication app(argc, argv);

  checkPlainText();
  checkHighlighter();
  for (int i = 1; i < argc; ++i)
    checkRoundTrip(argv[i]);

  std::printf("%s: %d failure(s), %d file(s) round-tripped\n", failures ? "FAILED" : "OK", failures, argc - 1);
  return failures ? 1 : 0;
}
