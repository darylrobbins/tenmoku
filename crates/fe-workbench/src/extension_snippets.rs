//! Snippets extensions contribute (`[[contributes.snippets]]`, extensions
//! record §9), offered as completions that expand through the snippet
//! engine ([`fe_editor::snippet`]).
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

use std::ops::Range;

use fe_buffer::{Bias, BufferId, BufferSnapshot, Version};
use fe_editor::{
    completion::{CompletionItem, CompletionTrigger},
    snippet::{MAX_DEPTH, Snippet},
};
use gpui::{App, Global, WeakEntity};

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

/// What [`set_extension_snippets`] last set.
#[derive(Default)]
struct ExtensionSnippets(Vec<ExtensionSnippet>);

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
                Some("it has no prefix".to_owned())
            } else if snippet.body.is_empty() {
                Some("it has no body".to_owned())
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
        .collect();
    cx.set_global(ExtensionSnippets(snippets));
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
                        return Err(format!("its placeholders nest more than {MAX_DEPTH} deep"));
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
            _ => Err("a `${` is never closed".to_owned()),
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
            None => Err(format!("the tabstop number at byte {start} is too large")),
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
                return Err(format!(
                    "the `${{` at byte {at} names no tabstop or variable"
                ));
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
            None => return Err(format!("the `${{` at byte {at} is never closed")),
            Some(_) => return Err(format!("the `${{` at byte {at} isn't closed by a `}}`")),
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
                None => return Err(format!("the choice at byte {at} is never closed")),
                Some(b'\\') => self.escape(b"$}\\,|"),
                Some(b'|') => {
                    self.pos += 1;
                    return match self.peek() {
                        Some(b'}') => Ok(()),
                        _ => Err(format!("the choice at byte {at} isn't closed by `|}}`")),
                    };
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// A transform's regex, format and options, from its first `/` to the
    /// `}`.
    fn transform(&mut self, at: usize) -> Result<(), String> {
        let open = || format!("the transform at byte {at} is never closed");
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
        .map(|snippets| snippets.0.clone())
        .unwrap_or_default()
}

/// The snippet items to offer at byte `offset` of `buffer` at its
/// `version`, in that version's offsets, as one of `workbench`'s editors
/// shows it now: none if none shows that version or a later one, or if an
/// edit since reached before `offset`, as typing at the caret doesn't.
pub(crate) fn snippet_completions(
    cx: &App,
    workbench: &WeakEntity<Workbench>,
    buffer: BufferId,
    version: Version,
    offset: usize,
) -> Vec<CompletionItem> {
    let snippets = match cx.try_global::<ExtensionSnippets>() {
        Some(snippets) if !snippets.0.is_empty() => &snippets.0,
        _ => return Vec::new(),
    };
    let Some(workbench) = workbench.upgrade() else {
        return Vec::new();
    };
    let editors = workbench.read(cx).open_editors(cx);
    let view = (editors.iter().map(|editor| editor.read(cx))).find(|view| {
        let snapshot = view.model().snapshot();
        snapshot.id() == buffer
            && snapshot.version() >= version
            && snapshot.oldest_version() <= version
    });
    let Some(view) = view else {
        return Vec::new();
    };
    let snapshot = view.model().snapshot();
    // Then `offset`, and the text before it, are as they were.
    let untouched = (snapshot.edits_since(version))
        .flat_map(|batch| batch.iter())
        .all(|edit| edit.old.start >= offset);
    if !untouched {
        return Vec::new();
    }
    let language = view.syntax().map(|syntax| syntax.language().name());
    items(snippets, language, snapshot, offset)
}

/// `snippets`' items for a buffer in `language` (`None`: plain text) at
/// byte `offset` of `snapshot`.
fn items(
    snippets: &[ExtensionSnippet],
    language: Option<&str>,
    snapshot: &BufferSnapshot,
    offset: usize,
) -> Vec<CompletionItem> {
    let text = snapshot.text();
    let offset = text.clip_offset(offset, Bias::Left);
    let row_start = text.row_start(text.offset_to_point(offset).row);
    let before = text.slice_to_string(row_start..offset);
    let word_len: usize = (before.chars().rev())
        .take_while(|&c| is_word(c))
        .map(char::len_utf8)
        .sum();
    let word = &before[before.len() - word_len..];
    (snippets.iter())
        .filter(|snippet| {
            snippet.languages.is_empty()
                || language.is_some_and(|language| {
                    (snippet.languages.iter()).any(|name| name.eq_ignore_ascii_case(language))
                })
        })
        .filter(|snippet| matches(word, &snippet.prefix))
        .map(|snippet| {
            let replaced = replaced(&before, word_len, &snippet.prefix);
            item(snippet, language, offset - replaced..offset)
        })
        .collect()
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

/// `snippet`'s item, replacing `range`, its documentation the text it
/// expands to, if any.
fn item(snippet: &ExtensionSnippet, language: Option<&str>, range: Range<usize>) -> CompletionItem {
    let expanded = Snippet::parse(&snippet.body);
    let expanded = expanded.text();
    // Longer than any run of backticks in it.
    let mut fence = 3;
    let mut run = 0;
    for c in expanded.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        fence = fence.max(run + 1);
    }
    let fence = "`".repeat(fence);
    let documentation = (!expanded.is_empty()).then(|| {
        format!(
            "{fence}{}\n{expanded}\n{fence}",
            language.unwrap_or_default()
        )
    });
    CompletionItem {
        label: snippet.prefix.clone(),
        label_detail: (snippet.description.as_ref()).map(|description| format!(" {description}")),
        kind: Some("snippet".to_owned()),
        detail: Some(snippet.extension_name.clone()),
        documentation,
        group: SNIPPET_GROUP,
        edit: (range, snippet.body.clone()),
        snippet: true,
        ..CompletionItem::default()
    }
}
