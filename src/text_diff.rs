//! Smallest single edit that turns one text into another.

/// Replace the characters `start..end` of the old text with `replacement`. Offsets count Unicode
/// scalar values (`char`s), which is what GTK text buffers use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// Returns the span between the first and the last differing character, or `None` if the texts
/// are equal. Where the change is ambiguous (e.g. `"aaa"` to `"aaaa"`), the common prefix is kept
/// as long as possible.
pub fn minimal_edit(old: &str, new: &str) -> Option<Span> {
    if old == new {
        return None;
    }
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let prefix = old_chars
        .iter()
        .zip(&new_chars)
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = old_chars.len().min(new_chars.len()) - prefix;
    let suffix = old_chars
        .iter()
        .rev()
        .zip(new_chars.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    Some(Span {
        start: prefix,
        end: old_chars.len() - suffix,
        replacement: new_chars[prefix..new_chars.len() - suffix].iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, span: &Span) -> String {
        let chars: Vec<char> = old.chars().collect();
        let mut result: String = chars[..span.start].iter().collect();
        result.push_str(&span.replacement);
        result.extend(&chars[span.end..]);
        result
    }

    fn check(old: &str, new: &str, start: usize, end: usize, replacement: &str) {
        let span = minimal_edit(old, new).expect("texts differ");
        assert_eq!(
            span,
            Span {
                start,
                end,
                replacement: replacement.to_string()
            },
            "{old:?} -> {new:?}"
        );
        assert_eq!(apply(old, &span), new);
    }

    #[test]
    fn equal_texts_need_no_edit() {
        assert_eq!(minimal_edit("", ""), None);
        assert_eq!(minimal_edit("Same text.", "Same text."), None);
    }

    #[test]
    fn change_in_the_middle() {
        check("The old sentence.", "The new sentence.", 4, 7, "new");
    }

    #[test]
    fn change_at_the_start_and_end() {
        check("Old start.", "New start.", 0, 3, "New");
        check("Ends here.", "Ends there!", 5, 10, "there!");
    }

    #[test]
    fn pure_insertion_and_deletion() {
        check("ab", "aXb", 1, 1, "X");
        check("aXb", "ab", 1, 2, "");
        check("", "new", 0, 0, "new");
        check("gone", "", 0, 4, "");
    }

    #[test]
    fn repeated_characters_keep_the_longest_prefix() {
        check("aaa", "aaaa", 3, 3, "a");
        check("aaaa", "aaa", 3, 4, "");
    }

    #[test]
    fn offsets_count_characters_not_bytes() {
        check("Grüße, Welt", "Grüße, Erde", 7, 11, "Erde");
        check("🙂 a 🙂", "🙂 b 🙂", 2, 3, "b");
        check("日本語のテキスト", "日本語の文章", 4, 8, "文章");
    }

    #[test]
    fn whole_text_replacement() {
        check("abc", "xyz", 0, 3, "xyz");
    }
}
