//! Snippets extensions contribute (`[[contributes.snippets]]`, extensions
//! record §9), offered as completions that expand through the snippet
//! engine ([`fe_editor::snippet`]), with the user's own
//! ([`crate::user_snippets`]), which are offered alike ([`offers`]): their
//! detail "User snippet", one for each of a snippet's prefixes, and sorted
//! before an extension's that matches as well.
//!
//! - **Set** by the host for every window ([`set_extension_snippets`]): one
//!   with no prefix, no body or a body that doesn't parse
//!   ([`snippet_body_error`]) is logged and dropped. A body of tabstops
//!   alone (`$1$0`) is kept.
//! - **Offered** by each editor's completion provider (`lsp.rs`), after all
//!   its server's items ([`SNIPPET_GROUP`]), however well those match, and
//!   whether or not a server runs for the buffer: each
//!   snippet for the buffer's language, or for every language if it names
//!   none, whose prefix the word before the caret matches as the menu
//!   matches it (its chars in order, case aside), when asked for, or again
//!   for an incomplete list. A server's trigger character asks for them
//!   only if it's a word char, such as `_`, which the word then holds; a
//!   punctuation one (`.`, `:`, `>`) asks for none ([`offers_snippets`]).
//!   Typing narrows them with the server's. Its item is a snippet
//!   (kind "snippet", detail the extension's name) replacing that word, or
//!   more of what's before the caret if that starts the prefix (`#reg` for
//!   `#region`), so accepting it inserts the body indented to the row, and
//!   Tab walks its tabstops. It's worked out from the editor as it is when
//!   the provider's answer is first polled, so text typed at the caret
//!   since the request doesn't lose it; an edit before the caret does.
//! - **Built once**: each snippet's offers ([`Offer`]), one for each of
//!   its prefixes, are made when the snippets are set or read, sharing one
//!   copy of its text, unexpanded. A request filters them by language and
//!   word before copying any body into an item, and copies at most
//!   [`SNIPPET_PAYLOAD_LIMIT`] of text in all, the list incomplete if that
//!   left any out, so typing narrows by asking again; it expands bodies
//!   for documentation only for items it offers, up to
//!   [`SNIPPET_DOCUMENTATION_LIMIT`].

use std::{ops::Range, rc::Rc};

use fe_buffer::{Bias, BufferId, BufferSnapshot, Version};
use fe_editor::{
    completion::{CompletionItem, CompletionList, CompletionTrigger},
    snippet::{MAX_DEPTH, Snippet},
};
use gpui::{App, Global, SharedString, WeakEntity};
use kiln_l10n::tr;

use crate::Workbench;

/// A snippet an extension contributes, as the host gives it to
/// [`set_extension_snippets`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionSnippet {
    pub extension_id: String,
    /// The name people see.
    pub extension_name: String,
    /// What's typed to offer it.
    pub prefix: String,
    /// In the LSP snippet grammar, lines joined with `\n`.
    pub body: String,
    pub description: Option<String>,
    /// The languages, by fastedit's names, it's offered in: every one if
    /// none.
    pub languages: Vec<String>,
}

/// What [`set_extension_snippets`] last set, and their offers, built once.
#[derive(Default)]
struct ExtensionSnippets {
    snippets: Vec<ExtensionSnippet>,
    offers: Rc<[Offer]>,
}

impl Global for ExtensionSnippets {}

/// The completion group of snippet items: after a language server's, 0.
pub(crate) const SNIPPET_GROUP: u8 = 1;

/// Whether a completion request asked by `trigger` offers snippets: one
/// asked for, or asked again for an incomplete list, does, and one a
/// server's trigger character asks does only if that's a word char (`_`),
/// the end of the word before the caret, which snippets' prefixes are
/// matched against. After a punctuation one (`.`, `::`, `->`) the word is
/// empty, so it offers none.
pub(crate) fn offers_snippets(trigger: &CompletionTrigger) -> bool {
    match trigger {
        CompletionTrigger::Invoked | CompletionTrigger::Incomplete => true,
        CompletionTrigger::Character(typed) => (typed.chars().next_back()).is_some_and(is_word),
    }
}

/// A char of the word before the caret, as the menu takes it: a letter, a
/// digit or `_`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Has every window's completions offer `snippets`, in place of those set
/// before: one with no prefix, no body or a body that doesn't parse
/// ([`snippet_body_error`]) is logged and dropped.
pub fn set_extension_snippets(cx: &mut App, snippets: Vec<ExtensionSnippet>) {
    let snippets = (snippets.into_iter())
        .filter(|snippet| {
            let error = if snippet.prefix.trim().is_empty() {
                Some(tr!("workbench-extensions-snippet-no-prefix").into())
            } else if snippet.body.is_empty() {
                Some(tr!("workbench-extensions-snippet-no-body").into())
            } else {
                snippet_body_error(&snippet.body)
            };
            if let Some(error) = &error {
                log::warn!(
                    "extension {}'s snippet {:?} is never offered: {error}",
                    snippet.extension_id,
                    snippet.prefix
                );
            }
            error.is_none()
        })
        .collect::<Vec<_>>();
    let offers = (snippets.iter())
        .flat_map(|snippet| {
            let text = SnippetText::new(&snippet.body, snippet.description.as_deref());
            let (languages, name) = (&snippet.languages, &snippet.extension_name);
            Offer::each_prefix([&snippet.prefix], text, languages, name, false)
        })
        .collect();
    cx.set_global(ExtensionSnippets { snippets, offers });
}

/// Why `body` isn't a snippet in the LSP snippet grammar, which the engine
/// ([`Snippet::parse`]) would insert some of as literal text: a `${` never
/// closed, naming no tabstop or variable, or with a choice or transform
/// left open, or placeholders nested past [`MAX_DEPTH`]. `None` if it is
/// one. A `$` followed by anything else is literal text in the grammar.
pub fn snippet_body_error(body: &str) -> Option<String> {
    let mut check = BodyCheck {
        bytes: body.as_bytes(),
        pos: 0,
    };
    check.sequence(0).err()
}

/// [`snippet_body_error`]'s parser, over bytes: the grammar's syntax is
/// ASCII, and anything else is text.
struct BodyCheck<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl BodyCheck<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Takes an escape's backslash and the char it escapes, if it's one
    /// of `escaped`, or the backslash alone.
    fn escape(&mut self, escaped: &[u8]) {
        self.pos += 1;
        if self.peek().is_some_and(|b| escaped.contains(&b)) {
            self.pos += 1;
        }
    }

    /// Text, tabstops, placeholders and variables up to the end, or up to
    /// the `}` closing the placeholder they're `depth` deep in, not taken.
    fn sequence(&mut self, depth: usize) -> Result<(), String> {
        while let Some(b) = self.peek() {
            match b {
                b'\\' => self.escape(b"$}\\"),
                b'}' if depth > 0 => return Ok(()),
                b'$' => {
                    self.pos += 1;
                    let construct = (self.peek())
                        .is_some_and(|b| b == b'{' || b == b'_' || b.is_ascii_alphanumeric());
                    if construct && depth >= MAX_DEPTH {
                        return Err(tr!(
                            "workbench-extensions-snippet-too-deep",
                            depth = MAX_DEPTH.to_string()
                        )
                        .into());
                    }
                    match self.peek() {
                        Some(b'{') => {
                            self.pos += 1;
                            self.brace(depth)?;
                        }
                        Some(b'0'..=b'9') => self.int()?,
                        Some(b) if b == b'_' || b.is_ascii_alphabetic() => self.var(),
                        _ => {}
                    }
                }
                _ => self.pos += 1,
            }
        }
        match depth {
            0 => Ok(()),
            _ => Err(tr!("workbench-extensions-snippet-unclosed").into()),
        }
    }

    /// A tabstop's number, which the engine keeps as a `u32`.
    fn int(&mut self) -> Result<(), String> {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        let digits = &self.bytes[start..self.pos];
        match std::str::from_utf8(digits)
            .ok()
            .and_then(|n| n.parse::<u32>().ok())
        {
            Some(_) => Ok(()),
            None => Err(tr!(
                "workbench-extensions-snippet-tabstop-too-large",
                at = start.to_string()
            )
            .into()),
        }
    }

    fn var(&mut self) {
        while self
            .peek()
            .is_some_and(|b| b == b'_' || b.is_ascii_alphanumeric())
        {
            self.pos += 1;
        }
    }

    /// What follows a `${`, `depth` deep, up to and with its `}`.
    fn brace(&mut self, depth: usize) -> Result<(), String> {
        let at = self.pos - 2;
        let number = match self.peek() {
            Some(b'0'..=b'9') => {
                self.int()?;
                true
            }
            Some(b) if b == b'_' || b.is_ascii_alphabetic() => {
                self.var();
                false
            }
            _ => {
                return Err(tr!(
                    "workbench-extensions-snippet-names-nothing",
                    at = at.to_string()
                )
                .into());
            }
        };
        match self.peek() {
            Some(b'}') => {}
            Some(b':') => {
                self.pos += 1;
                self.sequence(depth + 1)?;
            }
            Some(b'|') if number => self.choice(at)?,
            Some(b'/') => self.transform(at)?,
            None => {
                return Err(tr!(
                    "workbench-extensions-snippet-brace-unclosed",
                    at = at.to_string()
                )
                .into());
            }
            Some(_) => {
                return Err(tr!(
                    "workbench-extensions-snippet-brace-unmatched",
                    at = at.to_string()
                )
                .into());
            }
        }
        self.pos += 1;
        Ok(())
    }

    /// A choice's options from its first `|` to its closing `|`, before the
    /// `}`.
    fn choice(&mut self, at: usize) -> Result<(), String> {
        self.pos += 1;
        loop {
            match self.peek() {
                None => {
                    return Err(tr!(
                        "workbench-extensions-snippet-choice-unclosed",
                        at = at.to_string()
                    )
                    .into());
                }
                Some(b'\\') => self.escape(b"$}\\,|"),
                Some(b'|') => {
                    self.pos += 1;
                    return match self.peek() {
                        Some(b'}') => Ok(()),
                        _ => Err(tr!(
                            "workbench-extensions-snippet-choice-unmatched",
                            at = at.to_string()
                        )
                        .into()),
                    };
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// A transform's regex, format and options, from its first `/` to the
    /// `}`.
    fn transform(&mut self, at: usize) -> Result<(), String> {
        let open = || {
            tr!(
                "workbench-extensions-snippet-transform-unclosed",
                at = at.to_string()
            )
            .into()
        };
        for format in [false, true] {
            self.pos += 1;
            loop {
                match self.peek() {
                    None => return Err(open()),
                    // Any char.
                    Some(b'\\') => self.pos += 2,
                    Some(b'/') => break,
                    // A format's `${1:/upcase}` holds a `/`.
                    Some(b'$') if format && self.bytes.get(self.pos + 1) == Some(&b'{') => {
                        let close = self.bytes[self.pos..].iter().position(|&b| b == b'}');
                        let Some(close) = close else {
                            return Err(open());
                        };
                        self.pos += close + 1;
                    }
                    Some(_) => self.pos += 1,
                }
            }
        }
        self.pos += 1;
        while self.peek().is_some_and(|b| b != b'}') {
            self.pos += 1;
        }
        match self.peek() {
            Some(_) => Ok(()),
            None => Err(open()),
        }
    }
}

/// The snippets extensions contribute, as [`set_extension_snippets`] kept
/// them.
pub fn extension_snippets(cx: &App) -> Vec<ExtensionSnippet> {
    cx.try_global::<ExtensionSnippets>()
        .map(|snippets| snippets.snippets.clone())
        .unwrap_or_default()
}

/// A snippet as completions offer it, under one of its prefixes: the
/// user's ([`crate::user_snippets`]) or an extension's. Built once when
/// the snippets are read or set, its text shared by all of a snippet's
/// prefixes, so asking for completions copies no body until an item is
/// made of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) prefix: String,
    pub(crate) text: Rc<SnippetText>,
    /// The languages it's offered in: every one if none.
    pub(crate) languages: Rc<[String]>,
    /// Whose it is: the extension's name, or [`user_snippet`].
    pub(crate) detail: Rc<str>,
    /// Whether it's the user's, which comes before an extension's that
    /// matches as well.
    pub(crate) user: bool,
}

/// A snippet's text, shared by the offers of its prefixes. What it
/// expands to isn't kept: an item's documentation is rendered only once
/// it matches ([`items`]), as a few bytes of mirrors can expand to
/// megabytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SnippetText {
    pub(crate) body: String,
    pub(crate) description: Option<String>,
}

impl SnippetText {
    pub(crate) fn new(body: &str, description: Option<&str>) -> Rc<Self> {
        Rc::new(SnippetText {
            body: body.to_owned(),
            description: description.map(str::to_owned),
        })
    }
}

/// The detail of a user snippet without a description: its completion item's,
/// and its row in Insert Snippet.
pub(crate) fn user_snippet() -> SharedString {
    tr!("workbench-extensions-user-snippet")
}

/// The most text one request's snippet items carry in their bodies and
/// descriptions: past it, further matches aren't offered, the user's first
/// in line, and the list is incomplete, so typing asks again. However many
/// prefixes a large snippet has, a request copies no more than this.
pub(crate) const SNIPPET_PAYLOAD_LIMIT: usize = 4 << 20;

/// The most expanded text one request's snippet items show as their
/// documentation. Bodies are expanded, one item at a time, only while
/// the expansions so far are within it; an item past it, and every item
/// after, shows none. One expansion is itself bounded by the engine
/// ([`fe_editor::snippet::MAX_MIRROR_BYTES`]).
pub(crate) const SNIPPET_DOCUMENTATION_LIMIT: usize = 1 << 20;

impl Offer {
    /// The offers of a snippet with `text` under each of `prefixes`, all
    /// sharing its text, languages and detail.
    pub(crate) fn each_prefix<'a>(
        prefixes: impl IntoIterator<Item = &'a String>,
        text: Rc<SnippetText>,
        languages: &[String],
        detail: &str,
        user: bool,
    ) -> impl Iterator<Item = Offer> {
        let languages: Rc<[String]> = languages.into();
        let detail: Rc<str> = detail.into();
        (prefixes.into_iter()).map(move |prefix| Offer {
            prefix: prefix.clone(),
            text: text.clone(),
            languages: languages.clone(),
            detail: detail.clone(),
            user,
        })
    }

    /// Whether it's offered in a buffer of `language` (`None`: plain text).
    pub(crate) fn is_for(&self, language: Option<&str>) -> bool {
        self.languages.is_empty()
            || language.is_some_and(|language| {
                (self.languages.iter()).any(|name| name.eq_ignore_ascii_case(language))
            })
    }
}

/// Extensions' snippets' offers, built when they were set: one each.
pub(crate) fn extension_offers(cx: &App) -> Rc<[Offer]> {
    cx.try_global::<ExtensionSnippets>()
        .map_or_else(|| Rc::from([]), |snippets| snippets.offers.clone())
}

/// Every snippet to offer, as built when read or set: the user's
/// ([`crate::user_snippets`], read again if their files changed), one for
/// each of a snippet's prefixes, then extensions'.
pub(crate) fn offers(cx: &mut App) -> [Rc<[Offer]>; 2] {
    [crate::user_snippets::user_offers(cx), extension_offers(cx)]
}

/// The snippet items to offer at byte `offset` of `buffer` at its
/// `version`, in that version's offsets, as one of `workbench`'s editors
/// shows it now: none if none shows that version or a later one, or if an
/// edit since reached before `offset`, as typing at the caret doesn't.
/// Incomplete if matches were left out ([`SNIPPET_PAYLOAD_LIMIT`]).
pub(crate) fn snippet_completions(
    cx: &mut App,
    workbench: &WeakEntity<Workbench>,
    buffer: BufferId,
    version: Version,
    offset: usize,
) -> CompletionList {
    let snippets = offers(cx);
    if snippets.iter().all(|offers| offers.is_empty()) {
        return CompletionList::default();
    }
    let Some(workbench) = workbench.upgrade() else {
        return CompletionList::default();
    };
    let editors = workbench.read(cx).open_editors(cx);
    let view = (editors.iter().map(|editor| editor.read(cx))).find(|view| {
        let snapshot = view.model().snapshot();
        snapshot.id() == buffer
            && snapshot.version() >= version
            && snapshot.oldest_version() <= version
    });
    let Some(view) = view else {
        return CompletionList::default();
    };
    let snapshot = view.model().snapshot();
    // Then `offset`, and the text before it, are as they were.
    let untouched = (snapshot.edits_since(version))
        .flat_map(|batch| batch.iter())
        .all(|edit| edit.old.start >= offset);
    if !untouched {
        return CompletionList::default();
    }
    let language = view.syntax().map(|syntax| syntax.language().name());
    items(
        snippets.iter().flat_map(|offers| offers.iter()),
        language,
        snapshot,
        offset,
    )
}

/// `snippets`' items for a buffer in `language` (`None`: plain text) at
/// byte `offset` of `snapshot`: those for the language whose prefix the
/// word matches, in order, up to [`SNIPPET_PAYLOAD_LIMIT`], incomplete if
/// any were left out; documented up to [`SNIPPET_DOCUMENTATION_LIMIT`].
fn items<'a>(
    snippets: impl Iterator<Item = &'a Offer>,
    language: Option<&str>,
    snapshot: &BufferSnapshot,
    offset: usize,
) -> CompletionList {
    let text = snapshot.text();
    let offset = text.clip_offset(offset, Bias::Left);
    let row_start = text.row_start(text.offset_to_point(offset).row);
    let before = text.slice_to_string(row_start..offset);
    let word_len: usize = (before.chars().rev())
        .take_while(|&c| is_word(c))
        .map(char::len_utf8)
        .sum();
    let word = &before[before.len() - word_len..];
    let mut budget = SNIPPET_PAYLOAD_LIMIT;
    let mut documentation = Some(SNIPPET_DOCUMENTATION_LIMIT);
    let mut left_out = 0usize;
    let mut items = Vec::new();
    for snippet in snippets {
        if !snippet.is_for(language) || !matches(word, &snippet.prefix) {
            continue;
        }
        let payload = snippet.prefix.len()
            + snippet.text.body.len()
            + snippet.text.description.as_ref().map_or(0, String::len);
        if payload > budget {
            left_out += 1;
            continue;
        }
        budget -= payload;
        let replaced = replaced(&before, word_len, &snippet.prefix);
        let mut item = item(snippet, offset - replaced..offset);
        item.documentation = documentation.and_then(|left| {
            let expanded = Snippet::parse(&snippet.text.body);
            let expanded = expanded.text();
            match expanded.len() <= left {
                true => {
                    documentation = Some(left - expanded.len());
                    documented(expanded, language)
                }
                // None from here on, so no more is expanded.
                false => {
                    documentation = None;
                    None
                }
            }
        });
        items.push(item);
    }
    if left_out > 0 {
        log::debug!("{left_out} matching snippets left out: their text is too long to offer");
    }
    CompletionList {
        items,
        is_incomplete: left_out > 0,
    }
}

/// Whether `word`'s chars are in `prefix` in order, case aside, as the
/// menu matches it.
fn matches(word: &str, prefix: &str) -> bool {
    let mut prefix = prefix.chars().flat_map(char::to_lowercase);
    (word.chars().flat_map(char::to_lowercase)).all(|c| prefix.any(|p| p == c))
}

/// How many bytes before the caret a snippet with `prefix` replaces: the
/// word, `word_len` bytes, or more of `before`, the row up to the caret, if
/// that much starts the prefix, case aside.
fn replaced(before: &str, word_len: usize, prefix: &str) -> usize {
    let prefix = prefix.to_lowercase();
    let word_start = before.len() - word_len;
    (before.char_indices().map(|(ix, _)| ix))
        .filter(|&ix| ix < word_start && before.len() - ix <= prefix.len())
        .find(|&ix| prefix.starts_with(&before[ix..].to_lowercase()))
        .map_or(word_len, |ix| before.len() - ix)
}

/// `snippet`'s item, replacing `range`, undocumented. A user snippet's
/// sorts before an extension's, as the menu takes the sort text after how
/// well an item matches.
fn item(snippet: &Offer, range: Range<usize>) -> CompletionItem {
    CompletionItem {
        label: snippet.prefix.clone(),
        label_detail: (snippet.text.description.as_ref())
            .map(|description| format!(" {}", description)),
        kind: Some("snippet".to_owned()),
        detail: Some(snippet.detail.to_string()),
        sort_text: snippet.user.then(|| format!("\0{}", snippet.prefix)),
        group: SNIPPET_GROUP,
        edit: (range, snippet.text.body.clone()),
        snippet: true,
        ..CompletionItem::default()
    }
}

/// An item's documentation: `expanded`, the text its snippet expands to,
/// fenced as `language`'s code; none if it's empty.
fn documented(expanded: &str, language: Option<&str>) -> Option<String> {
    // Longer than any run of backticks in it.
    let mut fence = 3;
    let mut run = 0;
    for c in expanded.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        fence = fence.max(run + 1);
    }
    let fence = "`".repeat(fence);
    (!expanded.is_empty()).then(|| {
        let language = language.unwrap_or_default();
        format!("{}{}\n{}\n{}", fence, language, expanded, fence)
    })
}
