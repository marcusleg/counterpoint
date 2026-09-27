//! Parses Ghostwriting replies into edits and applies them to a document.

use std::fmt;

use serde::Serialize;

pub const ORIGINAL_TAG: &str = "original";
pub const REPLACEMENT_TAG: &str = "replacement";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Edit {
    pub original: String,
    pub replacement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedReply {
    pub explanation: String,
    pub edits: Vec<Edit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    UnterminatedFence {
        tag: String,
    },
    /// `edit` is the 1-based number of the edit whose `original` block is unpaired.
    OriginalWithoutReplacement {
        edit: usize,
    },
    ReplacementWithoutOriginal,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnterminatedFence { tag } => write!(f, "a `{tag}` block is not closed"),
            ParseError::OriginalWithoutReplacement { edit } => write!(
                f,
                "edit {edit} has an `original` block without a `replacement` block directly after it"
            ),
            ParseError::ReplacementWithoutOriginal => {
                write!(f, "a `replacement` block has no `original` block before it")
            }
        }
    }
}

impl std::error::Error for ParseError {}

struct FencedBlock {
    first_line: usize,
    last_line: usize,
    tag: String,
    content: String,
}

/// Returns the fence character, fence length and first info-string word of an opening fence.
fn opening_fence(line: &str) -> Option<(char, usize, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let fence_char = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let fence_len = rest.chars().take_while(|c| *c == fence_char).count();
    if fence_len < 3 {
        return None;
    }
    // Fence characters are ASCII, so the character count equals the byte offset.
    let info = rest[fence_len..].trim();
    if fence_char == '`' && info.contains('`') {
        return None;
    }
    let tag = info.split_whitespace().next().unwrap_or("").to_string();
    Some((fence_char, fence_len, tag))
}

fn is_closing_fence(line: &str, fence_char: char, fence_len: usize) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let trimmed = line.trim();
    indent <= 3 && trimmed.chars().count() >= fence_len && trimmed.chars().all(|c| c == fence_char)
}

fn fenced_blocks(lines: &[&str]) -> Result<Vec<FencedBlock>, ParseError> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((fence_char, fence_len, tag)) = opening_fence(lines[i]) else {
            i += 1;
            continue;
        };
        let close =
            (i + 1..lines.len()).find(|&j| is_closing_fence(lines[j], fence_char, fence_len));
        match close {
            Some(j) => {
                blocks.push(FencedBlock {
                    first_line: i,
                    last_line: j,
                    tag,
                    content: lines[i + 1..j].join("\n"),
                });
                i = j + 1;
            }
            None if tag == ORIGINAL_TAG || tag == REPLACEMENT_TAG => {
                return Err(ParseError::UnterminatedFence { tag });
            }
            // An unterminated ordinary block runs to the end of the reply.
            None => break,
        }
    }
    Ok(blocks)
}

/// Splits a Ghostwriting reply into its explanation and `original`/`replacement` edit pairs.
pub fn parse(reply: &str) -> Result<ParsedReply, ParseError> {
    let lines: Vec<&str> = reply.lines().collect();
    let blocks = fenced_blocks(&lines)?;
    let mut edits = Vec::new();
    let mut removed = vec![false; lines.len()];

    let mut k = 0;
    while k < blocks.len() {
        let block = &blocks[k];
        if block.tag == REPLACEMENT_TAG {
            return Err(ParseError::ReplacementWithoutOriginal);
        }
        if block.tag != ORIGINAL_TAG {
            k += 1;
            continue;
        }
        let replacement = blocks.get(k + 1).filter(|next| {
            next.tag == REPLACEMENT_TAG
                && lines[block.last_line + 1..next.first_line]
                    .iter()
                    .all(|line| line.trim().is_empty())
        });
        let Some(replacement) = replacement else {
            return Err(ParseError::OriginalWithoutReplacement {
                edit: edits.len() + 1,
            });
        };
        edits.push(Edit {
            original: block.content.clone(),
            replacement: replacement.content.clone(),
        });
        removed[block.first_line..=replacement.last_line].fill(true);
        k += 2;
    }

    let kept: Vec<&str> = lines
        .iter()
        .zip(&removed)
        .filter(|(_, removed)| !**removed)
        .map(|(line, _)| *line)
        .collect();
    Ok(ParsedReply {
        explanation: collapse_blank_lines(&kept).trim().to_string(),
        edits,
    })
}

fn collapse_blank_lines(lines: &[&str]) -> String {
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && out.last().is_some_and(|last| last.trim().is_empty()) {
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyError {
    EmptyOriginal { edit: usize },
    NotFound { edit: usize },
    Ambiguous { edit: usize, occurrences: usize },
    Overlapping { edit: usize, other: usize },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplyError::EmptyOriginal { edit } => {
                write!(f, "Edit {edit}: the original text is empty.")
            }
            ApplyError::NotFound { edit } => write!(
                f,
                "Edit {edit}: the original text was not found in the document. \
                 It may have changed since the request."
            ),
            ApplyError::Ambiguous { edit, occurrences } => write!(
                f,
                "Edit {edit}: the original text occurs {occurrences} times in the document, \
                 so it is unclear which one to change."
            ),
            ApplyError::Overlapping { edit, other } => {
                write!(f, "Edit {edit} overlaps with edit {other}.")
            }
        }
    }
}

impl std::error::Error for ApplyError {}

/// One character of whitespace-normalized text and the byte range it covers in the source.
struct Token {
    ch: char,
    start: usize,
    end: usize,
}

/// Collapses every run of whitespace into a single `' '` token.
fn normalize(text: &str) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    for (start, ch) in text.char_indices() {
        let end = start + ch.len_utf8();
        if ch.is_whitespace() {
            if let Some(last) = tokens.last_mut().filter(|last| last.ch == ' ') {
                last.end = end;
                continue;
            }
            tokens.push(Token {
                ch: ' ',
                start,
                end,
            });
        } else {
            tokens.push(Token { ch, start, end });
        }
    }
    tokens
}

/// Byte ranges in the source of every occurrence of `needle` in `haystack`.
fn occurrences(haystack: &[Token], needle: &[char]) -> Vec<(usize, usize)> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    (0..=haystack.len() - needle.len())
        .filter(|&i| {
            haystack[i..i + needle.len()]
                .iter()
                .zip(needle)
                .all(|(token, ch)| token.ch == *ch)
        })
        .map(|i| (haystack[i].start, haystack[i + needle.len() - 1].end))
        .collect()
}

/// Applies all edits or none. Each `original` must match exactly once, ignoring differences in
/// whitespace, and no two edits may overlap.
pub fn apply(document_md: &str, edits: &[Edit]) -> Result<String, ApplyError> {
    let haystack = normalize(document_md);
    let mut spans: Vec<(usize, usize, usize)> = Vec::with_capacity(edits.len());

    for (index, edit) in edits.iter().enumerate() {
        let number = index + 1;
        let needle: Vec<char> = normalize(edit.original.trim())
            .iter()
            .map(|t| t.ch)
            .collect();
        if needle.is_empty() {
            return Err(ApplyError::EmptyOriginal { edit: number });
        }
        match occurrences(&haystack, &needle).as_slice() {
            [] => return Err(ApplyError::NotFound { edit: number }),
            [(start, end)] => spans.push((*start, *end, index)),
            found => {
                return Err(ApplyError::Ambiguous {
                    edit: number,
                    occurrences: found.len(),
                })
            }
        }
    }

    spans.sort_by_key(|(start, _, _)| *start);
    for pair in spans.windows(2) {
        let (_, previous_end, previous) = pair[0];
        let (next_start, _, next) = pair[1];
        if next_start < previous_end {
            return Err(ApplyError::Overlapping {
                edit: previous.max(next) + 1,
                other: previous.min(next) + 1,
            });
        }
    }

    let mut result = String::with_capacity(document_md.len());
    let mut cursor = 0;
    for (start, end, index) in spans {
        result.push_str(&document_md[cursor..start]);
        result.push_str(&edits[index].replacement);
        cursor = end;
    }
    result.push_str(&document_md[cursor..]);
    Ok(result)
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    fn edit(original: &str, replacement: &str) -> Edit {
        Edit {
            original: original.to_string(),
            replacement: replacement.to_string(),
        }
    }

    #[test]
    fn reply_without_blocks_has_no_edits() {
        let parsed = parse("Looks good to me.").unwrap();
        assert_eq!(parsed.explanation, "Looks good to me.");
        assert!(parsed.edits.is_empty());
    }

    #[test]
    fn single_edit() {
        let reply =
            "I tightened the intro.\n\n```original\nOld text.\n```\n```replacement\nNew text.\n```";
        let parsed = parse(reply).unwrap();
        assert_eq!(parsed.explanation, "I tightened the intro.");
        assert_eq!(parsed.edits, vec![edit("Old text.", "New text.")]);
    }

    #[test]
    fn multiple_edits_with_text_between() {
        let reply = "Two changes.\n\n```original\nA\n```\n```replacement\nB\n```\n\nAnd also:\n\n```original\nC\n```\n\n```replacement\nD\n```\n";
        let parsed = parse(reply).unwrap();
        assert_eq!(parsed.explanation, "Two changes.\n\nAnd also:");
        assert_eq!(parsed.edits, vec![edit("A", "B"), edit("C", "D")]);
    }

    #[test]
    fn tilde_and_longer_fences() {
        let reply = "~~~original\nkeep ``` inside\n~~~\n````replacement\nnew\n````";
        let parsed = parse(reply).unwrap();
        assert_eq!(parsed.edits, vec![edit("keep ``` inside", "new")]);
    }

    #[test]
    fn multiline_content_is_preserved() {
        let reply = "```original\nLine one\n\nLine two\n```\n```replacement\nMerged line\n```";
        let parsed = parse(reply).unwrap();
        assert_eq!(
            parsed.edits,
            vec![edit("Line one\n\nLine two", "Merged line")]
        );
    }

    #[test]
    fn empty_replacement_deletes() {
        let reply = "```original\nDelete me.\n```\n```replacement\n```";
        let parsed = parse(reply).unwrap();
        assert_eq!(parsed.edits, vec![edit("Delete me.", "")]);
    }

    #[test]
    fn unrelated_code_blocks_stay_in_the_explanation() {
        let reply =
            "Example:\n\n```rust\nfn main() {}\n```\n\n```original\nx\n```\n```replacement\ny\n```";
        let parsed = parse(reply).unwrap();
        assert_eq!(parsed.explanation, "Example:\n\n```rust\nfn main() {}\n```");
        assert_eq!(parsed.edits, vec![edit("x", "y")]);
    }

    #[test]
    fn original_without_replacement_is_an_error() {
        let reply = "```original\nA\n```\n```replacement\nB\n```\n```original\nC\n```";
        assert_eq!(
            parse(reply),
            Err(ParseError::OriginalWithoutReplacement { edit: 2 })
        );
    }

    #[test]
    fn text_between_original_and_replacement_is_an_error() {
        let reply = "```original\na\n```\nthen\n```replacement\nb\n```";
        assert_eq!(
            parse(reply),
            Err(ParseError::OriginalWithoutReplacement { edit: 1 })
        );
    }

    #[test]
    fn replacement_without_original_is_an_error() {
        let reply = "Here:\n```replacement\nb\n```";
        assert_eq!(parse(reply), Err(ParseError::ReplacementWithoutOriginal));
    }

    #[test]
    fn unterminated_edit_fence_is_an_error() {
        assert_eq!(
            parse("```original\nabc"),
            Err(ParseError::UnterminatedFence {
                tag: "original".to_string()
            })
        );
    }

    #[test]
    fn unterminated_ordinary_fence_is_part_of_the_explanation() {
        let parsed = parse("See:\n```\ncode").unwrap();
        assert_eq!(parsed.explanation, "See:\n```\ncode");
        assert!(parsed.edits.is_empty());
    }

    #[test]
    fn parse_errors_have_readable_messages() {
        assert!(ParseError::OriginalWithoutReplacement { edit: 2 }
            .to_string()
            .contains("edit 2"));
    }
}

#[cfg(test)]
mod apply_tests {
    use super::*;

    fn edit(original: &str, replacement: &str) -> Edit {
        Edit {
            original: original.to_string(),
            replacement: replacement.to_string(),
        }
    }

    #[test]
    fn single_edit() {
        let doc = "# Title\n\nOld sentence here.\n";
        let result = apply(doc, &[edit("Old sentence", "New sentence")]);
        assert_eq!(result, Ok("# Title\n\nNew sentence here.\n".to_string()));
    }

    #[test]
    fn multiple_edits_in_any_order() {
        let doc = "Alpha one.\n\nBeta two.\n\nGamma three.\n";
        let result = apply(doc, &[edit("Gamma", "G"), edit("Alpha", "A")]);
        assert_eq!(result, Ok("A one.\n\nBeta two.\n\nG three.\n".to_string()));
    }

    #[test]
    fn matches_across_hard_wrapped_lines() {
        let doc = "This sentence was\nwrapped by the\nMarkdown writer.\n";
        let result = apply(doc, &[edit("sentence was wrapped by the Markdown", "line")]);
        assert_eq!(result, Ok("This line writer.\n".to_string()));
    }

    #[test]
    fn ignores_surrounding_whitespace_in_original() {
        let doc = "Alpha one.\n\nBeta two.\n";
        let result = apply(doc, &[edit("\n  Beta two.\n", "Beta 2.")]);
        assert_eq!(result, Ok("Alpha one.\n\nBeta 2.\n".to_string()));
    }

    #[test]
    fn handles_non_ascii_text() {
        let doc = "Grüße aus Köln.\n";
        let result = apply(doc, &[edit("aus Köln", "aus Berlin")]);
        assert_eq!(result, Ok("Grüße aus Berlin.\n".to_string()));
    }

    #[test]
    fn empty_replacement_deletes_text() {
        let doc = "Keep this. Drop this.\n";
        let result = apply(doc, &[edit(" Drop this.", "")]);
        assert_eq!(result, Ok("Keep this. \n".to_string()));
    }

    #[test]
    fn missing_original_is_not_found() {
        let result = apply("Some text.\n", &[edit("Other text", "x")]);
        assert_eq!(result, Err(ApplyError::NotFound { edit: 1 }));
    }

    #[test]
    fn repeated_original_is_ambiguous() {
        let result = apply("the cat and the cat", &[edit("the cat", "a dog")]);
        assert_eq!(
            result,
            Err(ApplyError::Ambiguous {
                edit: 1,
                occurrences: 2
            })
        );
    }

    #[test]
    fn overlapping_edits_are_rejected() {
        let result = apply(
            "one two three",
            &[edit("one two", "x"), edit("two three", "y")],
        );
        assert_eq!(result, Err(ApplyError::Overlapping { edit: 2, other: 1 }));
    }

    #[test]
    fn one_failing_edit_fails_the_whole_proposal() {
        let result = apply("Alpha. Beta.", &[edit("Alpha", "A"), edit("Gamma", "G")]);
        assert_eq!(result, Err(ApplyError::NotFound { edit: 2 }));
    }

    #[test]
    fn blank_original_is_rejected() {
        let result = apply("Text.", &[edit("  \n", "x")]);
        assert_eq!(result, Err(ApplyError::EmptyOriginal { edit: 1 }));
    }

    #[test]
    fn apply_errors_have_readable_messages() {
        let message = ApplyError::NotFound { edit: 3 }.to_string();
        assert!(message.starts_with("Edit 3"), "{message}");
        assert!(message.contains("not found"), "{message}");
    }
}
