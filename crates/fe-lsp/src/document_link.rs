//! `textDocument/documentLink` and `documentLink/resolve`: the links a
//! server finds in a document (URLs and paths in comments and strings,
//! import paths), as ranges with the URI each one opens.
//!
//! - **Servers.** Merged (docs/decisions/q5j03ax2af-lsp-add-on-servers.md
//!   §5): every server of the document with `documentLinkProvider` is
//!   asked at once, at the one version. With none, nothing is sent:
//!   `Ok(None)`. Only one link can be clicked at a range, so where two
//!   servers' links have the same range the earlier server's (in server
//!   order, the lead first) wins; links at other ranges are all kept. A
//!   server that fails or times out contributes nothing; the answer fails
//!   only if every server did.
//! - **Ranges.** Decoded against the text the server had at the request's
//!   version, as bytes of that version, sorted by start. An empty range
//!   (nothing to click) is dropped.
//! - **Resolve.** Only a link without a `target` is resolved, only by the
//!   server that found it ([`DocumentLink::owner`]) and only if its
//!   provider has a `resolveProvider`, and only while that session has the
//!   document open as it had it, at the version the link was found in. A
//!   link whose server stopped or restarted since sends nothing, and is
//!   `Unavailable("the language server restarted")`.
//! - **Failures.** A request past `lsp.timeouts.hover` (3 s) is
//!   `Ok(Some([]))`, a resolve past it is the link as it was. Both cancel
//!   the request.

use std::ops::Range;
use std::path::PathBuf;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::{DocumentLinkRequest, DocumentLinkResolve};
use lsp_types::{DocumentLinkParams, ServerCapabilities, TextDocumentIdentifier};

use crate::document::Admitted;
use crate::encoding::to_lsp;
use crate::event::LspError;
use crate::manager::{LspManager, Owner};
use crate::requests::{Gate, by_precedence, merged};

/// One link a server has in a document.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct DocumentLink {
    /// What it covers, in bytes of the version the request was made on.
    pub range: Range<usize>,
    /// The URI it opens; none until
    /// [`resolve_document_link`](LspManager::resolve_document_link) fills
    /// it, if the server leaves it.
    pub target: Option<String>,
    /// What the server says about it on hover.
    pub tooltip: Option<String>,
    /// The server's own, kept for the resolve.
    pub data: Option<serde_json::Value>,
    /// The server that found it, and the document as its session had it
    /// open: only that session resolves it.
    owner: Option<Owner>,
}

impl DocumentLink {
    /// A link no server owns: it's resolved by the lead of the buffer it's
    /// in.
    pub fn new(range: Range<usize>, target: Option<String>) -> Self {
        Self {
            range,
            target,
            tooltip: None,
            data: None,
            owner: None,
        }
    }

    /// The server that found the link, with the opening of the document it
    /// was found in.
    pub fn owner(&self) -> Option<Owner> {
        self.owner
    }

    /// The scheme of the link's target, in lower case: `file`, `https`…
    /// None without a target, or with one that isn't a URI.
    pub fn scheme(&self) -> Option<String> {
        let url = lsp_types::Url::parse(self.target.as_deref()?).ok()?;
        Some(url.scheme().to_owned())
    }

    /// The file a `file:` target names, with the target's fragment (as
    /// `L12,5`), if it has one. None for any other target.
    pub fn file_target(&self) -> Option<(PathBuf, Option<String>)> {
        let url = lsp_types::Url::parse(self.target.as_deref()?).ok()?;
        if url.scheme() != "file" {
            return None;
        }
        let fragment = url.fragment().map(str::to_owned);
        Some((url.to_file_path().ok()?, fragment))
    }
}

/// What `Unavailable` says when the server doesn't provide links.
const FEATURE: &str = "documentLink";

/// What `Unavailable` says when the server doesn't resolve them.
const RESOLVE: &str = "documentLink/resolve";

impl LspManager {
    /// The links in open buffer `buffer` at its `version`, merged across
    /// its servers with `documentLinkProvider` (see the [module](self)),
    /// each owned by the server that found it: each sent only if its
    /// server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if no server provides links: nothing is sent.
    /// `Ok(Some(vec![]))` if they have none, or don't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels every request still out. It doesn't borrow the
    /// manager.
    pub fn document_links(
        &self,
        buffer: BufferId,
        version: Version,
    ) -> impl Future<Output = Result<Option<Vec<DocumentLink>>, LspError>> + Send + 'static + use<>
    {
        let asks = self.ask_each::<DocumentLinkRequest, _>(
            buffer,
            version,
            Gate::Advertised,
            FEATURE,
            provides_links,
            |at| DocumentLinkParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
            |server, reply, admitted| {
                let owner = Some(Owner::of(server, &admitted));
                (reply.result.unwrap_or_default().into_iter())
                    .map(|link| convert(link, &admitted, owner))
                    .filter(|link| !link.range.is_empty())
                    .collect::<Vec<_>>()
            },
        );
        merged(asks).map(|answer| match answer {
            Ok(Some(parts)) => {
                let mut links = by_precedence(parts, |link| &link.range);
                links.sort_by_key(|link| (link.range.start, link.range.end));
                Ok(Some(links))
            }
            Ok(None) => Ok(None),
            Err(LspError::Timeout) => Ok(Some(Vec::new())),
            Err(err) => Err(err),
        })
    }

    /// `link`, found in open buffer `buffer` at its `version`, with the
    /// `target` its server resolves. Sent only to the link's
    /// [`owner`](DocumentLink::owner), while its session has the document
    /// open as it had it and at that version: otherwise nothing is sent,
    /// and the answer is `Unavailable("the language server restarted")`.
    /// A link with no owner goes to `buffer`'s lead.
    ///
    /// A link that has its target is as resolved as it gets, and is
    /// answered as it is without asking; one whose server has no
    /// `resolveProvider` is `Unavailable("documentLink/resolve")`. A server
    /// that doesn't answer within `lsp.timeouts.hover` (which also cancels
    /// the request) leaves the link as it was.
    pub fn resolve_document_link(
        &self,
        buffer: BufferId,
        version: Version,
        link: DocumentLink,
    ) -> impl Future<Output = Result<DocumentLink, LspError>> + Send + 'static + use<> {
        if link.target.is_some() {
            return async move { Ok(link) }.boxed();
        }
        let raw = link.clone();
        let owner = link.owner;
        let params = move |at: &Admitted| {
            let text = &at.document.sent.text;
            lsp_types::DocumentLink {
                range: lsp_types::Range {
                    start: to_lsp(text, raw.range.start, at.encoding),
                    end: to_lsp(text, raw.range.end, at.encoding),
                },
                target: None,
                tooltip: raw.tooltip,
                data: raw.data,
            }
        };
        let reply = match owner {
            Some(owner) => self
                .owner_request::<DocumentLinkResolve>(
                    owner,
                    version,
                    RESOLVE,
                    resolves_links,
                    params,
                )
                .boxed(),
            None => self
                .document_request_if_provided::<DocumentLinkResolve>(
                    buffer,
                    version,
                    RESOLVE,
                    resolves_links,
                    params,
                )
                .boxed(),
        };
        reply
            .map(move |answer| match answer {
                Ok((reply, admitted)) => Ok(convert(reply.result, &admitted, owner)),
                Err(LspError::Timeout) => Ok(link),
                Err(err) => Err(err),
            })
            .boxed()
    }
}

/// A reply's link, in bytes of the version asked about, owned by `owner`.
fn convert(link: lsp_types::DocumentLink, at: &Admitted, owner: Option<Owner>) -> DocumentLink {
    DocumentLink {
        range: at.offsets(link.range),
        target: link.target.map(String::from),
        tooltip: link.tooltip,
        data: link.data,
        owner,
    }
}

/// Whether the server provides document links.
fn provides_links(capabilities: &ServerCapabilities) -> bool {
    capabilities.document_link_provider.is_some()
}

/// Whether the server resolves the document links it gives.
fn resolves_links(capabilities: &ServerCapabilities) -> bool {
    (capabilities.document_link_provider.as_ref())
        .is_some_and(|provider| provider.resolve_provider == Some(true))
}

#[cfg(test)]
#[path = "document_link_tests.rs"]
mod tests;
