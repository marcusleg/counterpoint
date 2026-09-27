//! Renders the Markdown of chat replies as Pango markup for GTK labels.

use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

/// Converts `markdown` to Pango markup. Supports paragraphs, headings, emphasis, strong,
/// strikethrough, inline code, code blocks, lists, block quotes and rules. Links whose
/// destination starts with `http://`, `https://` or `mailto:` (case-insensitive) become
/// `<a href="…">`, URL escaped; other links render their text only. Raw HTML and everything else
/// appear as escaped text. `&`, `<`, `>`, `'` and `"` are always escaped. The result always has
/// balanced tags.
pub fn to_pango(markdown: &str) -> String {
    let mut writer = Writer::default();
    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
        writer.event(event);
    }
    writer.out
}

/// Escapes text for use in Pango markup, including attribute values.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\'' => escaped.push_str("&#39;"),
            '"' => escaped.push_str("&quot;"),
            c => escaped.push(c),
        }
    }
    escaped
}

struct Writer {
    out: String,
    /// One entry per open list: the next item number, or `None` for bullet lists.
    lists: Vec<Option<u64>>,
    quote_depth: usize,
    /// Text of the code or HTML block being read, emitted when the block ends.
    raw_block: Option<String>,
    /// True where a block may start without a separator: at the beginning and after a list
    /// marker or quote opening.
    at_block_start: bool,
    /// One entry per open link: whether its destination was clickable, so the matching `</a>`
    /// is only emitted where an `<a>` was.
    links: Vec<bool>,
}

impl Default for Writer {
    fn default() -> Self {
        Self {
            out: String::new(),
            lists: Vec::new(),
            quote_depth: 0,
            raw_block: None,
            at_block_start: true,
            links: Vec::new(),
        }
    }
}

impl Writer {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) | Event::Html(text) => match &mut self.raw_block {
                Some(raw) => raw.push_str(&text),
                None => self.text(&text),
            },
            Event::Code(code) => {
                self.out.push_str("<tt>");
                self.text(&code);
                self.out.push_str("</tt>");
            }
            Event::InlineHtml(html) => self.text(&html),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.newline(),
            Event::Rule => {
                self.separate();
                self.text("———");
            }
            // Footnotes, task lists and math are not enabled in the parser options.
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.separate(),
            Tag::Heading { level, .. } => {
                self.separate();
                self.out.push_str(match level {
                    HeadingLevel::H1 => "<span weight=\"bold\" size=\"x-large\">",
                    HeadingLevel::H2 => "<span weight=\"bold\" size=\"large\">",
                    HeadingLevel::H3 => "<span weight=\"bold\" size=\"larger\">",
                    _ => "<span weight=\"bold\">",
                });
            }
            Tag::BlockQuote(_) => {
                self.separate();
                self.quote_depth += 1;
                self.out.push_str(QUOTE_INDENT);
                self.out.push_str("<span alpha=\"70%\">");
            }
            Tag::CodeBlock(_) | Tag::HtmlBlock => {
                self.separate();
                self.raw_block = Some(String::new());
            }
            Tag::List(first_number) => {
                self.separate();
                self.lists.push(first_number);
            }
            Tag::Item => {
                if !self.at_block_start {
                    self.out.push('\n');
                    self.out.push_str(&self.indent(self.lists.len() - 1));
                }
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    _ => "• ".to_string(),
                };
                self.out.push_str(&marker);
                self.at_block_start = true;
            }
            Tag::Emphasis => self.out.push_str("<i>"),
            Tag::Strong => self.out.push_str("<b>"),
            Tag::Strikethrough => self.out.push_str("<s>"),
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                // An email autolink's `dest_url` is the bare address with no scheme.
                let href = if link_type == LinkType::Email {
                    format!("mailto:{dest_url}")
                } else {
                    dest_url.into_string()
                };
                let clickable = is_clickable(&href);
                if clickable {
                    self.out.push_str("<a href=\"");
                    self.out.push_str(&escape(&href));
                    self.out.push_str("\">");
                }
                self.links.push(clickable);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => self.out.push_str("</span>"),
            TagEnd::BlockQuote(_) => {
                self.out.push_str("</span>");
                self.quote_depth -= 1;
            }
            TagEnd::CodeBlock => {
                let code = self.raw_block.take().unwrap_or_default();
                self.out.push_str("<tt>");
                self.lines(code.trim_end_matches('\n'));
                self.out.push_str("</tt>");
            }
            TagEnd::HtmlBlock => {
                let html = self.raw_block.take().unwrap_or_default();
                self.lines(html.trim_end_matches('\n'));
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Emphasis => self.out.push_str("</i>"),
            TagEnd::Strong => self.out.push_str("</b>"),
            TagEnd::Strikethrough => self.out.push_str("</s>"),
            TagEnd::Link if self.links.pop().unwrap_or(false) => self.out.push_str("</a>"),
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        self.out.push_str(&escape(text));
        self.at_block_start = false;
    }

    /// Writes multi-line text, indenting each line to the current nesting.
    fn lines(&mut self, text: &str) {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.newline();
            }
            self.text(line);
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
        self.out.push_str(&self.indent(self.lists.len()));
    }

    /// Starts a new block: a blank line between top-level blocks, a line break inside lists.
    fn separate(&mut self) {
        if !self.at_block_start {
            self.out
                .push_str(if self.lists.is_empty() { "\n\n" } else { "\n" });
            self.out.push_str(&self.indent(self.lists.len()));
        }
        self.at_block_start = true;
    }

    fn indent(&self, list_levels: usize) -> String {
        QUOTE_INDENT.repeat(self.quote_depth) + &LIST_INDENT.repeat(list_levels)
    }
}

const QUOTE_INDENT: &str = "    ";
const LIST_INDENT: &str = "   ";

/// True if `dest` starts with `http://`, `https://` or `mailto:`, ASCII case-insensitively.
fn is_clickable(dest: &str) -> bool {
    const SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];
    SCHEMES.iter().any(|scheme| {
        dest.get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Panics unless every opening tag in `markup` is closed in order.
    fn assert_balanced(markup: &str) {
        let mut open: Vec<&str> = Vec::new();
        let mut rest = markup;
        while let Some(start) = rest.find('<') {
            let end = start + rest[start..].find('>').expect("unclosed tag bracket");
            let tag = &rest[start + 1..end];
            if let Some(name) = tag.strip_prefix('/') {
                assert_eq!(open.pop(), Some(name), "in {markup:?}");
            } else {
                open.push(tag.split(' ').next().unwrap());
            }
            rest = &rest[end + 1..];
        }
        assert!(open.is_empty(), "unclosed {open:?} in {markup:?}");
    }

    fn check(markdown: &str, expected: &str) {
        let markup = to_pango(markdown);
        assert_eq!(markup, expected, "for {markdown:?}");
        assert_balanced(&markup);
    }

    #[test]
    fn paragraphs_are_separated_by_a_blank_line() {
        check("One\ntwo.\n\nThree.", "One two.\n\nThree.");
    }

    #[test]
    fn inline_styles() {
        check(
            "*it* **bold** ~~gone~~ `x < y`",
            "<i>it</i> <b>bold</b> <s>gone</s> <tt>x &lt; y</tt>",
        );
    }

    #[test]
    fn headings_are_bold_and_larger() {
        check(
            "# One\n\n## Two\n\n### Three\n\n#### Four",
            "<span weight=\"bold\" size=\"x-large\">One</span>\n\n\
             <span weight=\"bold\" size=\"large\">Two</span>\n\n\
             <span weight=\"bold\" size=\"larger\">Three</span>\n\n\
             <span weight=\"bold\">Four</span>",
        );
    }

    #[test]
    fn code_blocks_keep_their_lines() {
        check(
            "Before:\n\n```rust\nlet a = 1 < 2;\n  indented\n```\n\nAfter.",
            "Before:\n\n<tt>let a = 1 &lt; 2;\n  indented</tt>\n\nAfter.",
        );
    }

    #[test]
    fn bullet_and_numbered_lists() {
        check("- one\n- two", "• one\n• two");
        check("3. three\n4. four", "3. three\n4. four");
    }

    #[test]
    fn nested_lists_are_indented() {
        check(
            "Intro:\n\n- outer\n  - inner\n- next",
            "Intro:\n\n• outer\n   • inner\n• next",
        );
    }

    #[test]
    fn loose_list_items_keep_their_paragraphs_together() {
        check("- one\n\n  more\n\n- two", "• one\n   more\n• two");
    }

    #[test]
    fn block_quotes_are_indented_and_dimmed() {
        check(
            "> Quoted\n> text.\n\nAfter.",
            "    <span alpha=\"70%\">Quoted text.</span>\n\nAfter.",
        );
    }

    #[test]
    fn links_escape_their_url() {
        check(
            "[site](https://example.com/?a=1&b=\"2\")",
            "<a href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\">site</a>",
        );
    }

    #[test]
    fn only_web_and_mail_links_become_clickable() {
        check(
            "[site](https://example.com)",
            "<a href=\"https://example.com\">site</a>",
        );
        check(
            "[mail](mailto:user@example.com)",
            "<a href=\"mailto:user@example.com\">mail</a>",
        );
        check("[passwd](file:///etc/passwd)", "passwd");
        check("[notes](notes.md)", "notes");
    }

    #[test]
    fn email_autolinks_become_mailto() {
        check(
            "<user@example.com>",
            "<a href=\"mailto:user@example.com\">user@example.com</a>",
        );
    }

    #[test]
    fn raw_html_is_shown_as_text() {
        check(
            "<b>x</b> & <!-- note -->",
            "&lt;b&gt;x&lt;/b&gt; &amp; &lt;!-- note --&gt;",
        );
        check("<div>\nblock\n</div>", "&lt;div&gt;\nblock\n&lt;/div&gt;");
    }

    #[test]
    fn rules_and_hard_breaks() {
        check("a\n\n---\n\nb", "a\n\n———\n\nb");
        check("line  \nbreak", "line\nbreak");
    }

    #[test]
    fn images_show_their_alt_text() {
        check("![a chart](chart.png)", "a chart");
    }

    #[test]
    fn apostrophes_and_quotes_are_escaped() {
        check("It's \"fine\"", "It&#39;s &quot;fine&quot;");
    }

    #[test]
    fn unusual_input_stays_balanced() {
        for markdown in [
            "",
            "**unclosed",
            "> - quoted list\n>   - nested\n>\n> ```\n> code\n> ```",
            "1. a\n\n   > quote in item\n\n2. b",
            "| a | b |\n|---|---|\n| 1 | 2 |",
            "[link with `code` and **bold**](x)",
            "# Heading with [link](y) and *em*",
        ] {
            assert_balanced(&to_pango(markdown));
        }
    }
}
