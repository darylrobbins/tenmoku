//! A paragraph's inline events as tokens with their source ranges, the
//! autolink literals found in that source, and the [`Inline`] tree made of
//! them.
//!
//! cmark-gfm finds autolink literals as it reads a paragraph's source, so a
//! URL takes in what would otherwise be markup: `https://a.com/*b*` is one
//! link, and `www.a.com/b&amp;` ends before the entity. pulldown-cmark has
//! already parsed the markup by then, so [`autolinks`] finds links in the
//! source the tokens span, and turns the tokens a link covers back into its
//! text. A delimiter whose pair a link took is shown as written.

use std::ops::Range;

use pulldown_cmark::CowStr;

use crate::{Inline, autolink};

pub(crate) struct Tok<'a> {
    pub(crate) token: Token<'a>,
    /// For an open tag, its opening delimiter (`*`, `[`); for a close, its
    /// closing one (`*`, `](dest)`). For the rest, the event's.
    pub(crate) range: Range<usize>,
}

pub(crate) enum Token<'a> {
    Text(CowStr<'a>),
    Code(CowStr<'a>),
    Html(CowStr<'a>),
    SoftBreak,
    HardBreak,
    Open(Open<'a>),
    Close,
    /// Shown as written: a delimiter whose pair a link took.
    Literal(&'a str),
    AutoLink {
        dest: String,
        text: String,
    },
    /// Taken into a link.
    Gone,
}

pub(crate) enum Open<'a> {
    Emphasis,
    Strong,
    Strikethrough,
    Link {
        dest: String,
        title: CowStr<'a>,
    },
    Image {
        dest: CowStr<'a>,
        title: CowStr<'a>,
    },
    /// A tag of an extension that is off: its content shows without it.
    Other,
}

impl Token<'_> {
    fn is_break(&self) -> bool {
        matches!(self, Token::SoftBreak | Token::HardBreak)
    }
}

/// `tokens` with GitHub's autolink literals, outside links and images: URLs
/// found in the source they span, then e-mail addresses in the text.
pub(crate) fn autolinks<'a>(source: &'a str, tokens: Vec<Tok<'a>>) -> Vec<Tok<'a>> {
    emails(urls(source, tokens))
}

/// URL and `www.` literals, starting in text as written and ending by the
/// end of their line.
fn urls<'a>(source: &'a str, mut tokens: Vec<Tok<'a>>) -> Vec<Tok<'a>> {
    let close_of = matching_closes(&tokens);
    let line_ends = line_ends(&tokens);
    // Where each token that starts a line starts, before any is taken.
    let line_starts: Vec<_> = (0..tokens.len())
        .map(|ix| (ix == 0 || tokens[ix - 1].token.is_break()).then_some(tokens[ix].range.start))
        .collect();
    let mut out: Vec<Tok<'a>> = Vec::with_capacity(tokens.len());
    // `out`'s open tags around the token, innermost last.
    let mut open: Vec<usize> = Vec::new();
    let mut in_link = 0;
    let mut ix = 0;
    while ix < tokens.len() {
        let range = tokens[ix].range.clone();
        match &tokens[ix].token {
            Token::Open(tag) => {
                in_link += usize::from(matches!(tag, Open::Link { .. } | Open::Image { .. }));
                open.push(out.len());
            }
            Token::Close => {
                let tag = open.pop().map(|open| &out[open].token);
                if let Some(Token::Open(Open::Link { .. } | Open::Image { .. })) = tag {
                    in_link -= 1;
                }
            }
            Token::Gone => {
                ix += 1;
                continue;
            }
            // Text as written, which a link can start in.
            Token::Text(text) if in_link == 0 && **text == source[range.clone()] => {
                let line_end = line_ends[ix];
                let found = source[range.clone()].char_indices().find_map(|(at, _)| {
                    let start = range.start + at;
                    let before = match line_starts[ix] {
                        Some(line_start) if line_start == start => None,
                        _ => source[..start].chars().next_back(),
                    };
                    let (len, prefix) = autolink::url(&source[start..line_end], before)?;
                    Some((start..start + len, prefix))
                });
                if let Some((link, prefix)) = found {
                    if range.start < link.start {
                        out.push(Tok {
                            token: Token::Text(source[range.start..link.start].into()),
                            range: range.start..link.start,
                        });
                    }
                    let text = take_into_link(source, &mut tokens, &close_of, ix, &link, || {
                        // A tag around the link that closes inside it: its
                        // opening delimiter is text now.
                        if let Some(opened) = open.pop() {
                            out[opened].token = Token::Literal(&source[out[opened].range.clone()]);
                        }
                    });
                    out.push(Tok {
                        token: Token::AutoLink {
                            dest: format!("{prefix}{text}"),
                            text,
                        },
                        range: link,
                    });
                    // The rest of the token the link ends in, if any, is next.
                    while ix < tokens.len() && matches!(tokens[ix].token, Token::Gone) {
                        ix += 1;
                    }
                    continue;
                }
            }
            _ => {}
        }
        let gone = Tok {
            token: Token::Gone,
            range,
        };
        out.push(std::mem::replace(&mut tokens[ix], gone));
        ix += 1;
    }
    out
}

/// Takes the tokens from `ix` that start before `link` ends into it, and
/// returns its text: the source, with entities and escapes decoded. The
/// rest of a token it ends in is left as text, and a tag it opens but
/// doesn't close keeps its closing delimiter as text. `closes_outer` is
/// called for each tag it closes that opened before it.
fn take_into_link<'a>(
    source: &'a str,
    tokens: &mut [Tok<'a>],
    close_of: &[Option<usize>],
    ix: usize,
    link: &Range<usize>,
    mut closes_outer: impl FnMut(),
) -> String {
    let mut text = String::new();
    let mut cursor = link.start;
    let mut opened = Vec::new();
    let mut at = ix;
    while at < tokens.len() && tokens[at].range.start < link.end {
        let range = tokens[at].range.clone();
        // An escape's backslash is in no event: the escaped character
        // starts the next one.
        let escaped = at > ix
            && tokens[at - 1].range.end + 1 == range.start
            && source[..range.start].ends_with('\\');
        if escaped && cursor < range.start {
            text.push_str(&source[cursor..range.start - 1]);
            cursor = range.start;
        }
        match &tokens[at].token {
            Token::Open(_) => opened.push(at),
            Token::Close => {
                if opened.pop().is_none() {
                    closes_outer();
                }
            }
            // An entity or escape: shown decoded.
            Token::Text(decoded)
                if link.start <= range.start
                    && range.end <= link.end
                    && **decoded != source[range.clone()] =>
            {
                text.push_str(&source[cursor..range.start]);
                text.push_str(decoded);
                cursor = range.end;
            }
            _ => {}
        }
        // What's left of a token the link ends in, a tag's delimiter
        // too, as in `](z "title")`, shows as written.
        if link.end < range.end {
            tokens[at] = Tok {
                token: Token::Text(source[link.end..range.end].into()),
                range: link.end..range.end,
            };
            break;
        }
        tokens[at].token = Token::Gone;
        at += 1;
    }
    text.push_str(&source[cursor..link.end]);
    for open in opened {
        if let Some(close) = close_of[open] {
            tokens[close].token = Token::Literal(&source[tokens[close].range.clone()]);
        }
    }
    text
}

/// E-mail addresses in each run of text, as cmark-gfm finds them once a
/// paragraph's text is decoded and joined: `a\+b@c.com` and `a&#64;c.com`
/// are addresses too.
fn emails<'a>(tokens: Vec<Tok<'a>>) -> Vec<Tok<'a>> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut run: Vec<Tok<'a>> = Vec::new();
    // For each open tag, whether it's a link or image, and how many are.
    let mut links: Vec<bool> = Vec::new();
    let mut in_link = 0;
    for tok in tokens {
        match &tok.token {
            Token::Text(_) | Token::Literal(_) if in_link == 0 => {
                run.push(tok);
                continue;
            }
            Token::Open(tag) => {
                let link = matches!(tag, Open::Link { .. } | Open::Image { .. });
                in_link += usize::from(link);
                links.push(link);
            }
            Token::Close => in_link -= usize::from(links.pop() == Some(true)),
            _ => {}
        }
        link_emails(std::mem::take(&mut run), &mut out);
        out.push(tok);
    }
    link_emails(run, &mut out);
    out
}

/// Pushes a run of text tokens, with the e-mail addresses in their text
/// made links.
fn link_emails<'a>(run: Vec<Tok<'a>>, out: &mut Vec<Tok<'a>>) {
    if !run.iter().any(|tok| text_of(tok).contains('@')) {
        out.extend(run);
        return;
    }
    let range =
        run.first().map_or(0, |tok| tok.range.start)..run.last().map_or(0, |tok| tok.range.end);
    let text: String = run.iter().map(text_of).collect();
    let mut plain = 0;
    let mut ix = 0;
    while ix < text.len() {
        let before = text[..ix].chars().next_back();
        match autolink::email(&text[ix..], before) {
            Some(len) => {
                if plain < ix {
                    out.push(Tok {
                        token: Token::Text(text[plain..ix].to_owned().into()),
                        range: range.clone(),
                    });
                }
                let address = text[ix..ix + len].to_owned();
                out.push(Tok {
                    token: Token::AutoLink {
                        dest: format!("mailto:{address}"),
                        text: address,
                    },
                    range: range.clone(),
                });
                ix += len;
                plain = ix;
            }
            None => ix += text[ix..].chars().next().map_or(1, char::len_utf8),
        }
    }
    if plain < text.len() {
        out.push(Tok {
            token: Token::Text(text[plain..].to_owned().into()),
            range,
        });
    }
}

/// A text token's text.
fn text_of<'t>(tok: &'t Tok<'_>) -> &'t str {
    match &tok.token {
        Token::Text(text) => text,
        Token::Literal(text) => text,
        _ => "",
    }
}

/// For each token, where its line's content ends: at the next break, or
/// the end of the tokens.
fn line_ends(tokens: &[Tok<'_>]) -> Vec<usize> {
    let mut end = tokens.last().map_or(0, |tok| tok.range.end);
    let mut ends = vec![0; tokens.len()];
    for (ix, tok) in tokens.iter().enumerate().rev() {
        if tok.token.is_break() {
            end = tok.range.start;
        }
        ends[ix] = end;
    }
    ends
}

/// For each open tag, the index of its close.
fn matching_closes(tokens: &[Tok<'_>]) -> Vec<Option<usize>> {
    let mut close_of = vec![None; tokens.len()];
    let mut open = Vec::new();
    for (ix, tok) in tokens.iter().enumerate() {
        match tok.token {
            Token::Open(_) => open.push(ix),
            Token::Close => {
                if let Some(opened) = open.pop() {
                    close_of[opened] = Some(ix);
                }
            }
            _ => {}
        }
    }
    close_of
}

/// The tree `tokens` make. Tags nested deeper than `max_depth` are left
/// out, their content kept, so that no document nests without bound.
pub(crate) fn build(tokens: Vec<Tok<'_>>, max_depth: usize) -> Vec<Inline> {
    let mut root = Vec::new();
    // The tags open around the next token, and their content so far.
    let mut stack: Vec<(Open<'_>, Vec<Inline>)> = Vec::new();
    // For each open tag, whether it's on `stack` or left out.
    let mut kept: Vec<bool> = Vec::new();
    for tok in tokens {
        let content = match stack.last_mut() {
            Some((_, content)) => content,
            None => &mut root,
        };
        let inline = match tok.token {
            Token::Open(tag) => {
                let keep = !matches!(tag, Open::Other) && stack.len() < max_depth;
                kept.push(keep);
                if keep {
                    stack.push((tag, Vec::new()));
                }
                continue;
            }
            Token::Close => {
                if kept.pop() != Some(true) {
                    continue;
                }
                let (tag, inner) = stack.pop().expect("kept tags are on the stack");
                let inline = close(tag, inner);
                let content = match stack.last_mut() {
                    Some((_, content)) => content,
                    None => &mut root,
                };
                content.push(inline);
                continue;
            }
            Token::Text(text) => {
                push_text(content, &text);
                continue;
            }
            Token::Literal(text) => {
                push_text(content, text);
                continue;
            }
            Token::Code(code) => Inline::Code(code.into_string()),
            Token::Html(html) => Inline::Html(html.into_string()),
            Token::SoftBreak => Inline::SoftBreak,
            Token::HardBreak => Inline::HardBreak,
            Token::AutoLink { dest, text } => Inline::Link {
                dest,
                title: String::new(),
                content: vec![Inline::Text(text)],
            },
            Token::Gone => continue,
        };
        content.push(inline);
    }
    // Tags the events left open, which pulldown-cmark doesn't do.
    while let Some((tag, inner)) = stack.pop() {
        let inline = close(tag, inner);
        match stack.last_mut() {
            Some((_, content)) => content.push(inline),
            None => root.push(inline),
        }
    }
    root
}

fn close(tag: Open<'_>, content: Vec<Inline>) -> Inline {
    match tag {
        Open::Emphasis => Inline::Emphasis(content),
        Open::Strong => Inline::Strong(content),
        Open::Strikethrough => Inline::Strikethrough(content),
        Open::Link { dest, title } => Inline::Link {
            dest,
            title: title.into_string(),
            content,
        },
        Open::Image { dest, title } => Inline::Image {
            dest: dest.into_string(),
            title: title.into_string(),
            alt: Inline::plain_text(&content),
        },
        Open::Other => unreachable!("other tags are left out"),
    }
}

/// Pushes text, joined to the text before it.
fn push_text(content: &mut Vec<Inline>, text: &str) {
    if text.is_empty() {
        return;
    }
    match content.last_mut() {
        Some(Inline::Text(last)) => last.push_str(text),
        _ => content.push(Inline::Text(text.to_owned())),
    }
}
