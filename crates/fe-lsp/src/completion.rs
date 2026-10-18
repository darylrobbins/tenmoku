//! `textDocument/completion` and `completionItem/resolve`
//! (docs/decisions/lsp-client.md §7, "Completion"): what a server offers
//! at an offset, as edits in bytes of the version asked about.
//!
//! - **Edits.** An item's `textEdit` (an insert/replace edit's `insert`
//!   range, though fe-lsp doesn't advertise those) and its
//!   `additionalTextEdits` are decoded against the text the server had at
//!   the request's version. An item with neither inserts its `insertText`,
//!   or else its `label`, over the word before the offset (letters, digits
//!   and `_`), as the protocol leaves to the client. Snippets aren't
//!   advertised (`snippetSupport: false`), but a server may send them anyway:
//!   an item whose `insertTextFormat` is a snippet is inserted as its plain
//!   text, without tabstops, until snippet support lands. Other text is
//!   inserted as it is.
//! - **Resolve.** Only `documentation`, `detail` and `additionalTextEdits`
//!   are resolved (`resolveSupport`), only from a server with a
//!   `resolveProvider`, and only by the session that gave the item, while
//!   it has that opening of the document. Resolved `additionalTextEdits`
//!   (a TypeScript auto-import) replace the listed ones, decoded against
//!   the text of the request's version; a reply without them keeps the
//!   listed ones. The main edit never changes.
//! - **Failures.** A completion past `lsp.timeouts.completion` (3 s) is no
//!   completions, and a resolve past `lsp.timeouts.resolve` (2 s) is the
//!   item as it was: §3.5's "the menu closes". Both cancel the request.
//!   Other failures are errors.

use std::ops::Range;
use std::sync::Arc;

use fe_buffer::{BufferId, Version};
use fe_text::{Bias, Text};
use futures::FutureExt;
use lsp_types::request::{Completion, ResolveCompletionItem};
use lsp_types::{
    CompletionContext, CompletionItemKind, CompletionParams, CompletionResponse,
    CompletionTextEdit, CompletionTriggerKind, Documentation, InsertTextFormat, MarkupContent,
};

use crate::document::Admitted;
use crate::encoding::{PositionEncoding, from_lsp};
use crate::event::{LspError, ServerId};
use crate::identity::{IncarnationId, SessionId};
use crate::manager::LspManager;

/// Why completions are asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionTrigger {
    /// By the user, as with `ctrl-space`.
    Invoked,
    /// By typing one of the server's trigger characters.
    Character(String),
    /// By typing more while the last list was incomplete.
    Incomplete,
}

/// An edit in bytes of the version the request was made on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionEdit {
    pub range: Range<usize>,
    pub text: String,
}

/// One completion a server offers.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct CompletionItem {
    pub label: String,
    /// `labelDetails`: shown right after the label, such as a signature.
    pub label_detail: Option<String>,
    /// `labelDetails`: shown after that, such as a module path.
    pub label_description: Option<String>,
    pub kind: Option<CompletionItemKind>,
    pub detail: Option<String>,
    /// Markdown or plain text.
    pub documentation: Option<String>,
    /// What the typed word is matched against, if not the label.
    pub filter_text: Option<String>,
    /// What the items are ordered by, if not their labels.
    pub sort_text: Option<String>,
    /// What accepting it does at the completion's offset.
    pub edit: CompletionEdit,
    /// What else it does elsewhere, such as an import.
    pub additional_edits: Vec<CompletionEdit>,
    /// The server's item, to resolve, and the server, session and
    /// document it came from: only that session can resolve it.
    raw: Arc<lsp_types::CompletionItem>,
    server: ServerId,
    from: (BufferId, (SessionId, IncarnationId)),
    /// What resolved edits are decoded against.
    decoded: Decoded,
}

/// The text of the request's version and the session's encoding: what an
/// item's edits are decoded against, at list time and when resolved.
#[derive(Clone)]
struct Decoded {
    text: Text,
    encoding: PositionEncoding,
}

impl Decoded {
    fn range(&self, range: lsp_types::Range) -> Range<usize> {
        let start = from_lsp(&self.text, range.start, self.encoding);
        let end = from_lsp(&self.text, range.end, self.encoding);
        start.min(end)..end.max(start)
    }

    fn edits(&self, edits: &[lsp_types::TextEdit]) -> Vec<CompletionEdit> {
        (edits.iter())
            .map(|edit| CompletionEdit {
                range: self.range(edit.range),
                text: edit.new_text.clone(),
            })
            .collect()
    }
}

impl std::fmt::Debug for Decoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoded")
            .field("len", &self.text.len())
            .field("encoding", &self.encoding)
            .finish()
    }
}

impl PartialEq for Decoded {
    fn eq(&self, other: &Self) -> bool {
        self.encoding == other.encoding && self.text.len() == other.text.len()
    }
}

/// What a server offers at an offset.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Completions {
    pub items: Vec<CompletionItem>,
    /// Typing more should ask again, as the list isn't all there is.
    pub is_incomplete: bool,
}

impl LspManager {
    /// What the server of open buffer `buffer` offers at byte `offset` of
    /// its `version`: sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server offers nothing, or doesn't answer within
    /// `lsp.timeouts.completion` (which also cancels the request).
    /// Dropping the future cancels it too. It doesn't borrow the manager.
    pub fn completion(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        trigger: CompletionTrigger,
    ) -> impl Future<Output = Result<Option<Completions>, LspError>> + Send + 'static + use<> {
        let server = self.server_of(buffer);
        let (trigger_kind, trigger_character) = match trigger {
            CompletionTrigger::Invoked => (CompletionTriggerKind::INVOKED, None),
            CompletionTrigger::Character(c) => (CompletionTriggerKind::TRIGGER_CHARACTER, Some(c)),
            CompletionTrigger::Incomplete => (
                CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS,
                None,
            ),
        };
        let reply =
            self.document_request::<Completion>(buffer, version, move |at| CompletionParams {
                text_document_position: at.position(offset),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: Some(CompletionContext {
                    trigger_kind,
                    trigger_character,
                }),
            });
        // Boxed, as its type names the manager's borrow.
        reply.boxed().map(move |answer| match (answer, server) {
            (Ok((reply, admitted)), Some(server)) => Ok(reply
                .result
                .map(|response| convert(response, &admitted, offset, server))),
            (Ok(_), None) | (Err(LspError::Timeout), _) => Ok(None),
            (Err(err), _) => Err(err),
        })
    }

    /// `item` with the `documentation`, `detail` and `additionalTextEdits`
    /// its server resolves.
    ///
    /// The item as it is if its server can't resolve, or doesn't answer
    /// within `lsp.timeouts.resolve` (which also cancels the request).
    pub fn resolve(
        &self,
        item: CompletionItem,
    ) -> impl Future<Output = Result<CompletionItem, LspError>> + Send + 'static + use<> {
        let resolves = self.capabilities(item.server).is_some_and(|capabilities| {
            (capabilities.completion_provider)
                .is_some_and(|provider| provider.resolve_provider == Some(true))
        });
        // Sent only while the session that gave the item has its document
        // open as it was then: data from an old session, or about an
        // earlier opening of the document, means nothing now.
        let (buffer, identity) = item.from;
        let reply = resolves.then(|| {
            self.request_if::<ResolveCompletionItem>(
                item.server,
                (*item.raw).clone(),
                move |opened| {
                    (opened.documents.iter())
                        .any(|document| document.buffer == buffer && document.identity == identity)
                },
            )
            .boxed()
        });
        async move {
            let Some(reply) = reply else {
                return Ok(item);
            };
            match reply.await {
                Ok(None) => Ok(item),
                Ok(Some(reply)) => {
                    let resolved = reply.result;
                    let mut item = item;
                    if let Some(documentation) = resolved.documentation.clone() {
                        item.documentation = Some(documentation_text(documentation));
                    }
                    if resolved.detail.is_some() {
                        item.detail = resolved.detail.clone();
                    }
                    if let Some(edits) = &resolved.additional_text_edits {
                        item.additional_edits = item.decoded.edits(edits);
                    }
                    item.raw = Arc::new(resolved);
                    Ok(item)
                }
                Err(LspError::Timeout) => Ok(item),
                Err(err) => Err(err),
            }
        }
    }

    /// The characters that should ask the server of open buffer `buffer`
    /// for completions when typed: its `triggerCharacters`, once it has
    /// initialized.
    pub fn trigger_characters(&self, buffer: BufferId) -> Vec<String> {
        (self.server_of(buffer))
            .and_then(|server| self.capabilities(server)?.completion_provider)
            .and_then(|provider| provider.trigger_characters)
            .unwrap_or_default()
    }
}

/// A reply as [`Completions`], its edits in bytes of the version asked
/// about.
fn convert(
    response: CompletionResponse,
    at: &Admitted,
    offset: usize,
    server: ServerId,
) -> Completions {
    let (items, is_incomplete) = match response {
        CompletionResponse::Array(items) => (items, false),
        CompletionResponse::List(list) => (list.items, list.is_incomplete),
    };
    let text = &at.document.sent.text;
    let offset = text.clip_offset(offset, Bias::Left);
    let word = word_before(text, offset)..offset;
    let decoded = Decoded {
        text: text.clone(),
        encoding: at.encoding,
    };
    let range = |range: lsp_types::Range| decoded.range(range);
    let items = (items.into_iter())
        .map(|item| {
            let snippet = item.insert_text_format == Some(InsertTextFormat::SNIPPET);
            let plain = |source: &str| {
                if snippet {
                    plain_snippet(source)
                } else {
                    source.to_string()
                }
            };
            let edit = match &item.text_edit {
                Some(CompletionTextEdit::Edit(edit)) => CompletionEdit {
                    range: range(edit.range),
                    text: plain(&edit.new_text),
                },
                Some(CompletionTextEdit::InsertAndReplace(edit)) => CompletionEdit {
                    range: range(edit.insert),
                    text: plain(&edit.new_text),
                },
                None => CompletionEdit {
                    range: word.clone(),
                    text: plain(item.insert_text.as_deref().unwrap_or(&item.label)),
                },
            };
            let additional_edits =
                decoded.edits(item.additional_text_edits.as_deref().unwrap_or(&[]));
            let details = item.label_details.clone().unwrap_or_default();
            CompletionItem {
                label: item.label.clone(),
                label_detail: details.detail,
                label_description: details.description,
                kind: item.kind,
                detail: item.detail.clone(),
                documentation: item.documentation.clone().map(documentation_text),
                filter_text: item.filter_text.clone(),
                sort_text: item.sort_text.clone(),
                edit,
                additional_edits,
                raw: Arc::new(item),
                server,
                from: (at.document.buffer, at.document.identity),
                decoded: decoded.clone(),
            }
        })
        .collect();
    Completions {
        items,
        is_incomplete,
    }
}

/// The text a snippet expands to, without tabstops, by the LSP snippet
/// grammar: tabstops and variables vanish, placeholders and variable
/// defaults give their text, a choice its first option, and `\$`, `\}`
/// and `\\` their second character. Malformed syntax stays as written.
///
/// Stopgap: T857 replaces it, handing the source to fe-editor's snippet
/// engine instead once `snippetSupport` is on.
fn plain_snippet(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    expand_snippet(&chars, &mut 0, 0, &mut out);
    out
}

/// How deep placeholders may nest before the rest stays literal, so a
/// server can't overflow the stack.
const MAX_SNIPPET_DEPTH: usize = 32;

/// Expands `s` from `*at` into `out`, up to the end or, when `depth` is
/// above 0, the `}` that closes a placeholder (consumed, and true). False
/// if nested and unclosed, or malformed: the rest from the failing `$` is
/// then kept literally.
fn expand_snippet(s: &[char], at: &mut usize, depth: usize, out: &mut String) -> bool {
    let nested = depth > 0;
    while let Some(&c) = s.get(*at) {
        match c {
            '\\' => match s.get(*at + 1) {
                Some(&next @ ('$' | '}' | '\\')) => {
                    out.push(next);
                    *at += 2;
                }
                _ => {
                    out.push('\\');
                    *at += 1;
                }
            },
            '}' if nested => {
                *at += 1;
                return true;
            }
            '$' => {
                let mut expanded = String::new();
                let mut next = *at;
                if expand_dollar(s, &mut next, depth, &mut expanded) {
                    out.push_str(&expanded);
                    *at = next;
                } else {
                    out.extend(&s[*at..]);
                    *at = s.len();
                    return false;
                }
            }
            c => {
                out.push(c);
                *at += 1;
            }
        }
    }
    !nested
}

/// Expands the tabstop, placeholder, choice or variable at `s[*at]`
/// (a `$`). False if malformed.
fn expand_dollar(s: &[char], at: &mut usize, depth: usize, out: &mut String) -> bool {
    if depth >= MAX_SNIPPET_DEPTH {
        return false;
    }
    let digits = |at: &mut usize| {
        let start = *at;
        while s.get(*at).is_some_and(char::is_ascii_digit) {
            *at += 1;
        }
        *at > start
    };
    let name = |at: &mut usize| {
        let start = *at;
        if !s
            .get(*at)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
        {
            return false;
        }
        while s
            .get(*at)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            *at += 1;
        }
        *at > start
    };
    *at += 1;
    if s.get(*at) != Some(&'{') {
        return digits(at) || name(at);
    }
    *at += 1;
    let is_tabstop = digits(at);
    if !is_tabstop && !name(at) {
        return false;
    }
    match s.get(*at) {
        Some('}') => {
            *at += 1;
            true
        }
        Some(':') => {
            *at += 1;
            expand_snippet(s, at, depth + 1, out)
        }
        Some('|') if is_tabstop => {
            *at += 1;
            let mut first = String::new();
            let mut in_first = true;
            while let Some(&c) = s.get(*at) {
                match c {
                    '\\' if matches!(s.get(*at + 1), Some('\\' | ',' | '|')) => {
                        if in_first {
                            first.push(s[*at + 1]);
                        }
                        *at += 2;
                    }
                    '|' if s.get(*at + 1) == Some(&'}') => {
                        *at += 2;
                        out.push_str(&first);
                        return true;
                    }
                    ',' => {
                        in_first = false;
                        *at += 1;
                    }
                    c => {
                        if in_first {
                            first.push(c);
                        }
                        *at += 1;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// The start of the word (letters, digits and `_`) that ends at `offset`.
fn word_before(text: &Text, offset: usize) -> usize {
    let row_start = text.row_start(text.offset_to_point(offset).row);
    let before = text.slice_to_string(row_start..offset);
    let word = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last();
    word.map_or(offset, |(ix, _)| row_start + ix)
}

fn documentation_text(documentation: Documentation) -> String {
    match documentation {
        Documentation::String(text)
        | Documentation::MarkupContent(MarkupContent { value: text, .. }) => text,
    }
}

/// A short name for `kind`, as a menu shows it.
pub fn kind_name(kind: CompletionItemKind) -> &'static str {
    const NAMES: [&str; 25] = [
        "text",
        "method",
        "function",
        "constructor",
        "field",
        "variable",
        "class",
        "interface",
        "module",
        "property",
        "unit",
        "value",
        "enum",
        "keyword",
        "snippet",
        "color",
        "file",
        "reference",
        "folder",
        "enum member",
        "constant",
        "struct",
        "event",
        "operator",
        "type parameter",
    ];
    // Kinds are numbered from 1.
    let number: i32 = serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_i64())
        .and_then(|number| i32::try_from(number).ok())
        .unwrap_or(0);
    (number.checked_sub(1))
        .and_then(|ix| NAMES.get(usize::try_from(ix).ok()?))
        .copied()
        .unwrap_or("")
}
