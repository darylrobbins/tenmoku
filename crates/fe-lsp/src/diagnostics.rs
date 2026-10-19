//! `textDocument/publishDiagnostics` (docs/decisions/lsp-client.md §4.1,
//! "Diagnostics"), decoded on the server's thread (§9 Q3 (a)).
//!
//! The router hands each publication it reads to the actor through an
//! [`Inbox`], since only the actor holds the documents and their rings. For
//! the URI's current incarnation, the actor decodes a publication carrying
//! a version that's in the ring, and not older than the newest it decoded
//! for that incarnation, against the ring's text for that version, in the
//! session's encoding. Its ranges leave as byte offsets in that buffer
//! version ([`LspEvent::Diagnostics`](crate::LspEvent::Diagnostics)). It
//! drops everything else:
//!
//! - an unversioned publication: nothing says which text it was computed
//!   on, not even "before the first `didChange`" (a reopen's `didOpen` and
//!   a publication on the earlier incarnation's text cross on the wire), so
//!   [`UnversionedDiagnostics::Drop`](crate::UnversionedDiagnostics::Drop);
//! - one older than the newest decoded (out of order);
//! - one whose version isn't in the ring: older than the ring, of an
//!   earlier incarnation (whose versions are all lower, §4.1), or never sent;
//! - one for a URI whose document is still opening, or with two (two
//!   buffers for one file, whose texts may differ).
//!
//! A URI that matches no document of the session is a file the server
//! reports on without the client having it open. There's no text to decode
//! against on the server's thread, so the publication leaves as it came
//! ([`LspEvent::FileDiagnostics`](crate::LspEvent::FileDiagnostics)), its
//! ranges for [`decode_file_diagnostics`] to decode against the file's text.
//!
//! Diagnostics already shown stay, anchored, until a decoded publication
//! replaces them.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ops::Range;
use std::path::PathBuf;
use std::task::{Context, Poll, Waker};

use fe_text::Text;
use lsp_types::{NumberOrString, PublishDiagnosticsParams, Url};

use crate::encoding::{PositionEncoding, from_lsp};

/// A diagnostic, its range in bytes of the buffer version it came with.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Diagnostic {
    pub range: Range<usize>,
    /// `None` when the server left it out; the client decides then.
    pub severity: Option<Severity>,
    pub message: String,
    /// What produced it, as `rustc` or `clippy`.
    pub source: Option<String>,
    /// A number is given as its decimal digits.
    pub code: Option<String>,
    pub tags: Vec<DiagnosticTag>,
    /// What the server attached, as received: it comes back with the
    /// diagnostic in a code action request.
    pub data: Option<serde_json::Value>,
    /// Secondary locations (`relatedInformation`), as rustc's "first borrow
    /// here".
    pub related: Vec<Related>,
    /// The `href` of `codeDescription`: a page describing the code.
    pub code_description: Option<Url>,
}

/// A location a diagnostic points to besides its own range.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Related {
    pub uri: Url,
    /// The file's path, for a `file` URI.
    pub path: Option<PathBuf>,
    /// The line the location starts on, which needs no encoding.
    pub row: u32,
    /// Its bytes in the buffer version the diagnostic came with, set only
    /// when the location is in that document.
    pub range: Option<Range<usize>>,
    pub message: String,
}

/// A diagnostic for a file the client doesn't have open: its range as the
/// server sent it, which [`decode_file_diagnostics`] turns into bytes of
/// the file's text.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileDiagnostic {
    pub range: lsp_types::Range,
    /// `None` when the server left it out; the client decides then.
    pub severity: Option<Severity>,
    pub message: String,
    /// What produced it, as `rustc` or `clippy`.
    pub source: Option<String>,
    /// A number is given as its decimal digits.
    pub code: Option<String>,
    pub tags: Vec<DiagnosticTag>,
}

impl FileDiagnostic {
    pub(crate) fn new(diagnostic: lsp_types::Diagnostic) -> Self {
        Self {
            range: diagnostic.range,
            severity: diagnostic.severity.and_then(severity),
            message: diagnostic.message,
            source: diagnostic.source,
            code: diagnostic.code.map(code),
            tags: (diagnostic.tags.into_iter().flatten())
                .filter_map(tag)
                .collect(),
        }
    }
}

/// `items` with their ranges decoded against `text`, the file's current
/// text, in the encoding the event named, ordered as `decode` orders them.
pub fn decode_file_diagnostics(
    text: &Text,
    items: &[FileDiagnostic],
    encoding: PositionEncoding,
) -> Vec<Diagnostic> {
    items
        .iter()
        .map(|item| {
            let (start, end) = byte_range(text, item.range, encoding);
            let mut diagnostic = Diagnostic::new(start..end, item.message.clone());
            diagnostic.severity = item.severity;
            diagnostic.source = item.source.clone();
            diagnostic.code = item.code.clone();
            diagnostic.tags = item.tags.clone();
            diagnostic
        })
        .collect()
}

/// `range` in bytes of `text`. A server's end before its start is read as
/// an empty range.
fn byte_range(text: &Text, range: lsp_types::Range, encoding: PositionEncoding) -> (usize, usize) {
    let start = from_lsp(text, range.start, encoding);
    let end = from_lsp(text, range.end, encoding).max(start);
    (start, end)
}

impl Diagnostic {
    /// A diagnostic over `range` saying `message`, with nothing else: the
    /// host sets the fields it has, to send it back in a request.
    pub fn new(range: Range<usize>, message: impl Into<String>) -> Self {
        Self {
            range,
            severity: None,
            message: message.into(),
            source: None,
            code: None,
            tags: Vec::new(),
            data: None,
            related: Vec::new(),
            code_description: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// The tags fe-lsp advertises (`tagSupport`); others are dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticTag {
    /// Unused or unreachable code, usually shown faded.
    Unnecessary,
    /// Usually shown struck through.
    Deprecated,
}

/// Publications the router has read and the actor hasn't decoded yet. Both
/// run on the server's thread.
#[derive(Default)]
pub(crate) struct Inbox {
    queue: RefCell<VecDeque<PublishDiagnosticsParams>>,
    /// The actor, waiting for one.
    waiting: Cell<Option<Waker>>,
}

impl Inbox {
    pub(crate) fn push(&self, params: PublishDiagnosticsParams) {
        self.queue.borrow_mut().push_back(params);
        if let Some(actor) = self.waiting.take() {
            actor.wake();
        }
    }

    /// Ready with every publication queued, once there's one.
    pub(crate) fn poll_take(&self, cx: &Context<'_>) -> Poll<Vec<PublishDiagnosticsParams>> {
        let mut queue = self.queue.borrow_mut();
        if queue.is_empty() {
            self.waiting.set(Some(cx.waker().clone()));
            return Poll::Pending;
        }
        Poll::Ready(queue.drain(..).collect())
    }
}

/// `diagnostics` with their positions decoded against `text`, the ring's
/// text for the version they were published for.
#[cfg(test)]
pub(crate) fn decode(
    text: &Text,
    encoding: PositionEncoding,
    diagnostics: Vec<lsp_types::Diagnostic>,
) -> Vec<Diagnostic> {
    decode_in(text, encoding, diagnostics, None)
}

/// [`decode`], also placing related locations in `document`, the URI the
/// diagnostics were published for.
pub(crate) fn decode_in(
    text: &Text,
    encoding: PositionEncoding,
    diagnostics: Vec<lsp_types::Diagnostic>,
    document: Option<&Url>,
) -> Vec<Diagnostic> {
    let document_path = document.and_then(file_path);
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let (start, end) = byte_range(text, diagnostic.range, encoding);
            Diagnostic {
                range: start..end,
                severity: diagnostic.severity.and_then(severity),
                message: diagnostic.message,
                source: diagnostic.source,
                code: diagnostic.code.map(code),
                tags: (diagnostic.tags.into_iter().flatten())
                    .filter_map(tag)
                    .collect(),
                data: diagnostic.data,
                related: (diagnostic.related_information.into_iter().flatten())
                    .map(|info| {
                        let location = info.location;
                        let path = file_path(&location.uri);
                        let here = document.is_some_and(|document| {
                            *document == location.uri
                                || (document_path.is_some() && document_path == path)
                        });
                        let range = here.then(|| {
                            let (start, end) = byte_range(text, location.range, encoding);
                            start..end
                        });
                        Related {
                            row: location.range.start.line,
                            uri: location.uri,
                            path,
                            range,
                            message: info.message,
                        }
                    })
                    .collect(),
                code_description: diagnostic.code_description.map(|c| c.href),
            }
        })
        .collect()
}

fn file_path(uri: &Url) -> Option<PathBuf> {
    if uri.scheme() == "file" {
        uri.to_file_path().ok()
    } else {
        None
    }
}

fn code(code: NumberOrString) -> String {
    match code {
        NumberOrString::Number(n) => n.to_string(),
        NumberOrString::String(s) => s,
    }
}

fn severity(severity: lsp_types::DiagnosticSeverity) -> Option<Severity> {
    use lsp_types::DiagnosticSeverity as S;
    match severity {
        S::ERROR => Some(Severity::Error),
        S::WARNING => Some(Severity::Warning),
        S::INFORMATION => Some(Severity::Information),
        S::HINT => Some(Severity::Hint),
        // Not in the protocol: as if left out.
        _ => None,
    }
}

fn tag(tag: lsp_types::DiagnosticTag) -> Option<DiagnosticTag> {
    use lsp_types::DiagnosticTag as T;
    match tag {
        T::UNNECESSARY => Some(DiagnosticTag::Unnecessary),
        T::DEPRECATED => Some(DiagnosticTag::Deprecated),
        _ => None,
    }
}
