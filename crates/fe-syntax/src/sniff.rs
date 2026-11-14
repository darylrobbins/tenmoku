//! Telling XML and JSON by how a file starts, for files nothing else names.

use std::path::Path;

use crate::language::built_in_name;

/// How much of the text the sniff reads.
const SNIFF_BYTES: usize = 512;

/// The language a file's content says it is, when its path has no extension
/// at all (`Pipfile`, `bin/config`) or there is no path: XML for an element,
/// after any comments, whose closing tag or a `/>` is in the first
/// [`SNIFF_BYTES`] of `text_start`, but not `<html`; JSON for `{` then `"` or
/// `}`, or `[` then `{`, `"`, a digit, `-` or `]`, past whitespace. Only
/// languages compiled in count. When unsure, `None`: a file with an
/// extension, even an unknown one, is never sniffed.
pub(crate) fn language_name_by_content(
    path: Option<&Path>,
    text_start: &str,
) -> Option<&'static str> {
    if path.is_some_and(|path| path.extension().is_some()) {
        return None;
    }
    let head = head(text_start);
    let text = head.trim_start_matches(|c: char| c == '\u{feff}' || c.is_whitespace());
    let language = if is_json(text) {
        "json"
    } else if is_xml(text) {
        "xml"
    } else {
        return None;
    };
    built_in_name(language)
}

/// The first [`SNIFF_BYTES`] of `text`, cut back to a character boundary.
fn head(text: &str) -> &str {
    let mut end = text.len().min(SNIFF_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn is_json_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn is_json(text: &str) -> bool {
    let (open, rest) = match text.as_bytes().first() {
        Some(b'{') => (b'{', &text[1..]),
        Some(b'[') => (b'[', &text[1..]),
        _ => return false,
    };
    let Some(next) = rest.trim_start_matches(is_json_space).chars().next() else {
        return false;
    };
    match open {
        b'{' => matches!(next, '"' | '}'),
        _ => matches!(next, '{' | '"' | '-' | ']') || next.is_ascii_digit(),
    }
}

/// An element after any comments, with its closing tag or a `/>` in `head`.
fn is_xml(head: &str) -> bool {
    let mut text = head;
    while let Some(comment) = text.strip_prefix("<!--") {
        let Some(end) = comment.find("-->") else {
            return false;
        };
        text = comment[end + 3..].trim_start();
    }
    let Some(element) = text.strip_prefix('<') else {
        return false;
    };
    let name_len = element
        .find(|c: char| !is_name_char(c))
        .unwrap_or(element.len());
    let name = &element[..name_len];
    if !name.starts_with(|c: char| c.is_alphabetic() || c == '_')
        || name.eq_ignore_ascii_case("html")
        || name_len == element.len()
    {
        return false;
    }
    let after = &element[name_len..];
    after.contains("/>")
        || after.match_indices("</").any(|(at, close)| {
            let rest = &after[at + close.len()..];
            rest.strip_prefix(name)
                .is_some_and(|rest| rest.trim_start().starts_with('>'))
        })
}

/// A character of an XML name after its first.
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_is_cut_at_a_character_boundary() {
        let text = format!("{}é", "a".repeat(SNIFF_BYTES - 1));
        assert_eq!(head(&text).len(), SNIFF_BYTES - 1);
        assert_eq!(head("ab"), "ab");
    }

    #[test]
    fn xml_needs_a_closing_tag_or_an_empty_element() {
        assert!(is_xml("<a>x</a>"));
        assert!(is_xml("<a><b/></a"));
        assert!(is_xml("<!-- c --> <!--d--><a:b>x</a:b >"));
        assert!(!is_xml("<a>x</ab>"));
        assert!(!is_xml("<a>x"));
        assert!(!is_xml("<!-- c <a/>"));
        assert!(!is_xml("<1a/>"));
        assert!(!is_xml("<HTML><br/></HTML>"));
        assert!(!is_xml("<?xml version=\"1.0\"?><a/>"));
    }

    #[test]
    fn json_is_judged_by_its_first_two_tokens() {
        for text in [
            "{\"a\": 1}",
            "{ }",
            "{\n}",
            "[1]",
            "[ -1]",
            "[]",
            "[{",
            "[\"a\"",
        ] {
            assert!(is_json(text), "{text:?}");
        }
        for text in ["{", "{ a: 1}", "[section]", "[[bin]]", "[ ", "x{}"] {
            assert!(!is_json(text), "{text:?}");
        }
    }
}
