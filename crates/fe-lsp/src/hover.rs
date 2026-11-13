//! `textDocument/hover` (docs/decisions/lsp-client.md §7, "Hover"): what a
//! server says about the text at an offset, as text for a popover.
//!
//! - **Contents.** Markdown [`MarkupContent`] is kept as it is, for the
//!   UI to show as text, with fenced code in the editor's font. Plain text
//!   is kept too. A [`MarkedString`] is markdown, or code in a language,
//!   which becomes a fenced block; an array of them is joined, a blank
//!   line apart. Contents with no text are no hover. [`Hover`] doesn't
//!   say which format it came in, so a UI reads plain text as markdown
//!   too: a plain-text line of three backticks starts a code block there.
//! - **Range.** The reply's range, if any, is decoded against the text the
//!   server had at the request's version, as bytes of that version.
//! - **Servers.** Merged (docs/decisions/q5j03ax2af-lsp-add-on-servers.md
//!   §5): every server of the document with `hoverProvider` is asked at
//!   once, at the one version (a lead that is the document's only server
//!   is asked whatever it advertises, as ever). Their contents are joined
//!   in server order (the lead first), a markdown rule apart, with
//!   contents equal to an earlier server's dropped; the range is the first
//!   server's that gives one.
//! - **Failures.** A timeout (3 s by default, `lsp.timeouts.hover`) is no
//!   hover, as §3.5 says: silently nothing. Other failures are errors,
//!   which a UI shows as nothing too. A server that fails or times out
//!   contributes nothing; the answer fails only if every server did.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::HoverRequest;
use lsp_types::{
    HoverContents, HoverParams, HoverProviderCapability, LanguageString, MarkedString,
    MarkupContent, ServerCapabilities,
};

use crate::document::Admitted;
use crate::event::LspError;
use crate::manager::LspManager;
use crate::requests::{Gate, merged};

/// What a server says about the text at an offset.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Hover {
    /// Markdown, or plain text, to show as text.
    pub text: String,
    /// What it's about, in bytes of the version the request was made on.
    pub range: Option<Range<usize>>,
}

impl Hover {
    pub fn new(text: impl Into<String>, range: Option<Range<usize>>) -> Self {
        Self {
            text: text.into(),
            range,
        }
    }
}

/// What `Unavailable` says when no server provides hovers.
const FEATURE: &str = "hover";

/// What separates two servers' contents: a markdown rule.
const RULE: &str = "\n\n---\n\n";

impl LspManager {
    /// The hover for byte `offset` of open buffer `buffer`, at its
    /// `version`, merged across its servers (see the [module](self)): each
    /// sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if no server has anything to say, or none says it within
    /// `lsp.timeouts.hover` (which also cancels the request). One server
    /// failing or timing out leaves the others' contents. Dropping the
    /// future cancels every request still out. It doesn't borrow the
    /// manager, so it may leave the manager's lock.
    pub fn hover(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> impl Future<Output = Result<Option<Hover>, LspError>> + Send + 'static + use<> {
        let asks = self.ask_each::<HoverRequest, _>(
            buffer,
            version,
            Gate::UnlessLoneLead,
            FEATURE,
            provides_hover,
            move |at| HoverParams {
                text_document_position_params: at.position(offset),
                work_done_progress_params: Default::default(),
            },
            |_, reply, admitted| reply.result.and_then(|hover| convert(hover, &admitted)),
        );
        merged(asks).map(|answer| match answer {
            Ok(parts) => Ok(merge(parts.into_iter().flatten().flatten())),
            Err(LspError::Timeout) => Ok(None),
            Err(err) => Err(err),
        })
    }
}

/// Servers' hovers as one, in server order: their contents a rule apart,
/// with a repeat of earlier contents dropped, and the first range given.
fn merge(hovers: impl IntoIterator<Item = Hover>) -> Option<Hover> {
    let mut texts: Vec<String> = Vec::new();
    let mut range = None;
    for hover in hovers {
        if !texts.contains(&hover.text) {
            texts.push(hover.text);
        }
        range = range.or(hover.range);
    }
    (!texts.is_empty()).then(|| Hover {
        text: texts.join(RULE),
        range,
    })
}

/// Whether the server provides hovers.
fn provides_hover(capabilities: &ServerCapabilities) -> bool {
    !matches!(
        capabilities.hover_provider,
        None | Some(HoverProviderCapability::Simple(false))
    )
}

/// A reply as a [`Hover`], if it has any text.
fn convert(hover: lsp_types::Hover, at: &Admitted) -> Option<Hover> {
    let text = match hover.contents {
        HoverContents::Markup(MarkupContent { value, .. }) => value,
        HoverContents::Scalar(marked) => marked_text(marked),
        HoverContents::Array(marked) => (marked.into_iter())
            .map(marked_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    if text.trim().is_empty() {
        return None;
    }
    let range = hover.range.map(|range| at.offsets(range));
    Some(Hover { text, range })
}

/// A marked string as markdown: code in a language is a fenced block.
fn marked_text(marked: MarkedString) -> String {
    match marked {
        MarkedString::String(text) => text,
        MarkedString::LanguageString(LanguageString { language, value }) => {
            // Longer than any run of backticks in the code, so it can't
            // close the fence early.
            let longest = (value.split(|c| c != '`')).map(str::len).max().unwrap_or(0);
            let fence = "`".repeat(longest.max(2) + 1);
            format!("{fence}{language}\n{value}\n{fence}")
        }
    }
}
