//! Inline HTML phrasing tags made into Markdown inlines, as VS Code's hover
//! and preview render them: rust-analyzer's hovers carry rustdoc's
//! `<code>` around intra-doc links.
//!
//! A tag is mapped only when it's one of a few phrasing tags, written with
//! no attributes, and its pair is in the same Markdown tag as it is. Any
//! other HTML is left as it is.

use crate::inline::{Open, Tok, Token};

/// The phrasing tags mapped.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Name {
    Code,
    Kbd,
    B,
    Strong,
    I,
    Em,
    S,
    Del,
    Sub,
    Sup,
}

impl Name {
    fn from(name: &str) -> Option<Name> {
        const NAMES: [(&str, Name); 10] = [
            ("code", Name::Code),
            ("kbd", Name::Kbd),
            ("b", Name::B),
            ("strong", Name::Strong),
            ("i", Name::I),
            ("em", Name::Em),
            ("s", Name::S),
            ("del", Name::Del),
            ("sub", Name::Sub),
            ("sup", Name::Sup),
        ];
        let (_, name) = NAMES
            .iter()
            .find(|(tag, _)| tag.eq_ignore_ascii_case(name))?;
        Some(*name)
    }

    fn open(self) -> Open<'static> {
        match self {
            Name::Code | Name::Kbd => Open::Code,
            Name::B | Name::Strong => Open::Strong,
            Name::I | Name::Em => Open::Emphasis,
            Name::S | Name::Del => Open::Strikethrough,
            // No inline of their own: their content shows without them.
            Name::Sub | Name::Sup => Open::Other,
        }
    }
}

/// An HTML tag of a name and maybe attributes, mapped or not.
struct Tag<'h> {
    name: &'h str,
    closing: bool,
    /// Whether it has attributes; a closing tag never does.
    attributes: bool,
    /// `<x/>`, which opens nothing.
    self_closing: bool,
}

impl Tag<'_> {
    /// The tag it maps to, if it opens or closes one.
    fn mapped(&self) -> Option<Name> {
        Name::from(self.name).filter(|_| !self.attributes)
    }

    /// `<br>`, `<br/>` or `<br />`.
    fn is_break(&self) -> bool {
        self.name.eq_ignore_ascii_case("br") && !self.closing && !self.attributes
    }

    /// Whether it opens an element that a closing tag ends: not `<x/>` or
    /// a void element such as `<img>`.
    fn opens(&self) -> bool {
        const VOID: [&str; 14] = [
            "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
            "source", "track", "wbr",
        ];
        !self.closing
            && !self.self_closing
            && !VOID.iter().any(|void| void.eq_ignore_ascii_case(self.name))
    }
}

/// The tag `html` is, if it's an opening or closing tag rather than a
/// comment, declaration or the like.
fn tag(html: &str) -> Option<Tag<'_>> {
    let inner = html.strip_prefix('<')?.strip_suffix('>')?;
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(inner) => (true, inner),
        None => (false, inner),
    };
    let end =
        (inner.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))).unwrap_or(inner.len());
    let (name, rest) = inner.split_at(end);
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let rest = rest.trim_end();
    let (self_closing, rest) = match rest.strip_suffix('/') {
        Some(rest) if !closing => (true, rest),
        _ => (false, rest),
    };
    if closing && !rest.is_empty() {
        return None;
    }
    Some(Tag {
        name,
        closing,
        attributes: !rest.trim().is_empty(),
        self_closing,
    })
}

/// Whether `line`, an HTML block's first, is a tag [`tags`] may map, so
/// the block may be the paragraph it would be without its tags.
pub(crate) fn starts_phrasing(line: &str) -> bool {
    tag(line.trim()).is_some_and(|tag| tag.is_break() || tag.mapped().is_some())
}

/// `tokens` with each mapped tag that has its pair made the Markdown tag
/// it means, and each `<br>` a hard break. A closing tag closes the
/// innermost tag open, of any name or attributes and in any Markdown tag,
/// when it's of the same name, and pairs with it when both are in the same
/// Markdown tag. A line ending just inside a pair's tags is
/// moved outside them, or dropped where the text outside already ends in a
/// space, as HTML collapses it.
pub(crate) fn tags(mut tokens: Vec<Tok<'_>>) -> Vec<Tok<'_>> {
    let mut breaks = Vec::new();
    let mut pairs = Vec::new();
    {
        // The Markdown tags open around the token, innermost last, each by
        // a number of its own; the paragraph is 0.
        let mut scopes = vec![0];
        let mut scopes_seen = 0;
        // The HTML tags opened and not yet closed, innermost last, each
        // with the Markdown tag it's in: one that closes leaves them open.
        let mut open: Vec<(usize, Tag<'_>, usize)> = Vec::new();
        for (ix, tok) in tokens.iter().enumerate() {
            let html = match &tok.token {
                Token::Open(_) => {
                    scopes_seen += 1;
                    scopes.push(scopes_seen);
                    continue;
                }
                Token::Close => {
                    if scopes.len() > 1 {
                        scopes.pop();
                    }
                    continue;
                }
                Token::Html(html) => html,
                _ => continue,
            };
            let Some(tag) = tag(html) else {
                continue;
            };
            let scope = *scopes.last().expect("the paragraph's");
            if tag.is_break() {
                breaks.push(ix);
            } else if tag.opens() {
                open.push((ix, tag, scope));
            } else if tag.closing
                && let Some((_, opened, _)) = open.last()
                && opened.name.eq_ignore_ascii_case(tag.name)
            {
                let (start, opened, opened_in) = open.pop().expect("an open tag");
                if let Some(name) = opened.mapped()
                    && opened_in == scope
                {
                    pairs.push((start, ix, name));
                }
            }
        }
    }
    for ix in breaks {
        tokens[ix].token = Token::HardBreak;
    }
    for (start, end, name) in pairs {
        tokens[start].token = Token::Open(name.open());
        tokens[end].token = Token::Close;
        break_out(&mut tokens, start, start + 1, start.checked_sub(1));
        break_out(&mut tokens, end, end - 1, Some(end + 1));
    }
    tokens
}

/// Moves a soft break at `inside`, next to the tag at `tag`, to the tag's
/// other side, or drops it where the token at `outside` (none at the
/// paragraph's edge) is a break or has a space next to the tag.
fn break_out(tokens: &mut [Tok<'_>], tag: usize, inside: usize, outside: Option<usize>) {
    if !matches!(tokens[inside].token, Token::SoftBreak) {
        return;
    }
    let before = outside.is_some_and(|outside| outside < tag);
    let spaced = |text: &str| match before {
        true => text.ends_with([' ', '\t']),
        false => text.starts_with([' ', '\t']),
    };
    let collapses = match outside.and_then(|outside| tokens.get(outside)) {
        None => true,
        Some(tok) => match &tok.token {
            Token::SoftBreak | Token::HardBreak | Token::Gone => true,
            Token::Text(text) => spaced(text),
            Token::Literal(text) => spaced(text),
            _ => false,
        },
    };
    match collapses {
        true => tokens[inside].token = Token::Gone,
        false => tokens.swap(tag, inside),
    }
}
