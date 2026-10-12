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
//! - one for a URI with no current incarnation, or with two (two buffers
//!   for one file, whose texts may differ).
//!
//! Diagnostics already shown stay, anchored, until a decoded publication
//! replaces them.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ops::Range;
use std::task::{Context, Poll, Waker};

use fe_text::Text;
use lsp_types::{NumberOrString, PublishDiagnosticsParams};

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
pub(crate) fn decode(
    text: &Text,
    encoding: PositionEncoding,
    diagnostics: Vec<lsp_types::Diagnostic>,
) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let start = from_lsp(text, diagnostic.range.start, encoding);
            // A server's end before its start is read as an empty range.
            let end = from_lsp(text, diagnostic.range.end, encoding).max(start);
            Diagnostic {
                range: start..end,
                severity: diagnostic.severity.and_then(severity),
                message: diagnostic.message,
                source: diagnostic.source,
                code: diagnostic.code.map(|code| match code {
                    NumberOrString::Number(n) => n.to_string(),
                    NumberOrString::String(s) => s,
                }),
                tags: (diagnostic.tags.into_iter().flatten())
                    .filter_map(tag)
                    .collect(),
                data: diagnostic.data,
            }
        })
        .collect()
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
