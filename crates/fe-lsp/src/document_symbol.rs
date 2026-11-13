//! `textDocument/documentSymbol`: the symbols of a document, as the flat
//! pre-order list the tree-sitter outline uses.
//!
//! - **Provider.** Only the document's provider is asked
//!   ([`Feature::DocumentSymbols`](crate::Feature::DocumentSymbols)): its
//!   lead if that has `documentSymbolProvider`, else the first add-on that
//!   does, for one outline. With none, nothing is sent: `Ok(None)`, and the
//!   caller falls back to the tree-sitter outline.
//! - **Shapes.** `DocumentSymbol[]` keeps the server's nesting;
//!   `SymbolInformation[]` is flat and is nested by range containment, its
//!   symbols of other documents dropped.
//! - **Ranges.** Decoded against the text the server had at the request's
//!   version, as bytes of that version. A symbol with an inverted range is
//!   dropped; its selection range is clipped into its range.
//! - **Failures.** A `null` or empty reply, or a timeout (3 s by default,
//!   `lsp.timeouts.hover`), is `Ok(Some([]))`.

use std::cmp::Reverse;
use std::ops::Range;

use fe_buffer::{BufferId, Version};
use fe_nav::OutlineKind;
use futures::FutureExt;
use lsp_types::request::DocumentSymbolRequest;
use lsp_types::{
    DocumentSymbolParams, DocumentSymbolResponse, OneOf, ServerCapabilities, SymbolKind, SymbolTag,
    TextDocumentIdentifier,
};

use crate::document::Admitted;
use crate::event::LspError;
use crate::manager::LspManager;

/// The most symbols kept, as many as the tree-sitter outline allows.
pub const MAX_SYMBOLS: usize = 20_000;

/// One symbol of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocumentSymbol {
    pub name: String,
    /// The server's detail (a signature, say), or empty.
    pub detail: String,
    pub kind: OutlineKind,
    /// The whole symbol, in bytes of the version the request was made on.
    pub range: Range<usize>,
    /// What to select when going to it: within `range`.
    pub selection_range: Range<usize>,
    /// 0 at the top.
    pub depth: u32,
    /// The index of the enclosing symbol in the list.
    pub parent: Option<u32>,
    pub deprecated: bool,
}

impl DocumentSymbol {
    /// A top-level symbol selecting its whole `range`, with no detail; the
    /// other fields are public to set.
    pub fn new(name: String, kind: OutlineKind, range: Range<usize>) -> Self {
        Self {
            name,
            detail: String::new(),
            kind,
            selection_range: range.clone(),
            range,
            depth: 0,
            parent: None,
            deprecated: false,
        }
    }
}

/// What `Unavailable` says when the server doesn't provide symbols.
const FEATURE: &str = "documentSymbol";

impl LspManager {
    /// The symbols of open buffer `buffer`, at its `version`, in pre-order:
    /// sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server doesn't provide symbols: nothing is sent.
    /// `Ok(Some(vec![]))` if it has none, or doesn't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels it too. It doesn't borrow the manager, so it may
    /// leave the manager's lock.
    pub fn document_symbols(
        &self,
        buffer: BufferId,
        version: Version,
    ) -> impl Future<Output = Result<Option<Vec<DocumentSymbol>>, LspError>> + Send + 'static + use<>
    {
        let reply = self.provider_request::<DocumentSymbolRequest>(
            buffer,
            version,
            FEATURE,
            provides_symbols,
            |at| DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        );
        // Boxed, as its type names the manager's borrow.
        reply
            .boxed()
            .map(|answer| match answer {
                Ok((reply, admitted, _)) => Ok(Some(match reply.result {
                    None => Vec::new(),
                    Some(DocumentSymbolResponse::Nested(symbols)) => nested(symbols, &admitted),
                    Some(DocumentSymbolResponse::Flat(symbols)) => flat(symbols, &admitted),
                })),
                Err(LspError::Timeout) => Ok(Some(Vec::new())),
                Err(LspError::Unavailable(why)) if why == FEATURE => Ok(None),
                Err(err) => Err(err),
            })
            .boxed()
    }
}

/// Whether the server provides document symbols.
pub(crate) fn provides_symbols(capabilities: &ServerCapabilities) -> bool {
    match &capabilities.document_symbol_provider {
        None | Some(OneOf::Left(false)) => false,
        Some(OneOf::Left(true) | OneOf::Right(_)) => true,
    }
}

fn inverted(range: &lsp_types::Range) -> bool {
    (range.start.line, range.start.character) > (range.end.line, range.end.character)
}

/// `selection` clipped into `range`.
fn clip(selection: Range<usize>, range: &Range<usize>) -> Range<usize> {
    let start = selection.start.clamp(range.start, range.end);
    let end = selection.end.clamp(range.start, range.end);
    start..end
}

/// The server's tree, flattened in pre-order, siblings by start.
fn nested(symbols: Vec<lsp_types::DocumentSymbol>, admitted: &Admitted) -> Vec<DocumentSymbol> {
    let mut out: Vec<DocumentSymbol> = Vec::new();
    // Symbols still to emit, last first, with their depth and parent. The
    // stack, not recursion, as a server's nesting is unbounded.
    let mut pending: Vec<(lsp_types::DocumentSymbol, u32, Option<u32>)> = Vec::new();
    let push_all =
        |pending: &mut Vec<_>, mut symbols: Vec<lsp_types::DocumentSymbol>, depth, parent| {
            symbols.retain(|symbol| !inverted(&symbol.range));
            symbols.sort_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character));
            pending.extend(symbols.into_iter().rev().map(|s| (s, depth, parent)));
        };
    push_all(&mut pending, symbols, 0, None);
    while let Some((mut symbol, depth, parent)) = pending.pop() {
        if out.len() >= MAX_SYMBOLS {
            break;
        }
        let range = admitted.offsets(symbol.range);
        #[allow(deprecated)]
        let flag = symbol.deprecated;
        let deprecated = deprecated(flag, symbol.tags.as_deref());
        let index = out.len() as u32;
        out.push(DocumentSymbol {
            selection_range: clip(admitted.offsets(symbol.selection_range), &range),
            range,
            name: symbol.name,
            detail: symbol.detail.unwrap_or_default(),
            kind: outline_kind(symbol.kind),
            depth,
            parent,
            deprecated,
        });
        if let Some(children) = symbol.children.take() {
            push_all(&mut pending, children, depth + 1, Some(index));
        }
    }
    out
}

/// A flat list, nested by containment.
fn flat(symbols: Vec<lsp_types::SymbolInformation>, admitted: &Admitted) -> Vec<DocumentSymbol> {
    let mut found: Vec<DocumentSymbol> = (symbols.into_iter())
        .filter(|symbol| {
            symbol.location.uri == admitted.document.uri && !inverted(&symbol.location.range)
        })
        .map(|symbol| {
            let range = admitted.offsets(symbol.location.range);
            #[allow(deprecated)]
            let flag = symbol.deprecated;
            DocumentSymbol {
                selection_range: range.clone(),
                range,
                name: symbol.name,
                detail: String::new(),
                kind: outline_kind(symbol.kind),
                depth: 0,
                parent: None,
                deprecated: deprecated(flag, symbol.tags.as_deref()),
            }
        })
        .collect();
    // By start, the larger first, so that a parent precedes its children.
    found.sort_by_key(|s| (s.range.start, Reverse(s.range.end)));
    found.truncate(MAX_SYMBOLS);
    // The indices of the symbols still open, innermost last.
    let mut open: Vec<usize> = Vec::new();
    for at in 0..found.len() {
        let range = found[at].range.clone();
        while open.last().is_some_and(|&top| {
            !(found[top].range.start <= range.start && range.end <= found[top].range.end)
        }) {
            open.pop();
        }
        found[at].depth = open.len() as u32;
        found[at].parent = open.last().map(|&top| top as u32);
        open.push(at);
    }
    found
}

fn deprecated(flag: Option<bool>, tags: Option<&[SymbolTag]>) -> bool {
    flag == Some(true) || tags.is_some_and(|tags| tags.contains(&SymbolTag::DEPRECATED))
}

/// fe-syntax's nearest kind for an LSP symbol kind.
pub(crate) fn outline_kind(kind: SymbolKind) -> OutlineKind {
    match kind {
        SymbolKind::FILE | SymbolKind::MODULE | SymbolKind::NAMESPACE | SymbolKind::PACKAGE => {
            OutlineKind::Module
        }
        SymbolKind::CLASS => OutlineKind::Class,
        SymbolKind::METHOD | SymbolKind::CONSTRUCTOR => OutlineKind::Method,
        SymbolKind::PROPERTY | SymbolKind::FIELD | SymbolKind::EVENT => OutlineKind::Field,
        SymbolKind::ENUM => OutlineKind::Enum,
        SymbolKind::INTERFACE => OutlineKind::Interface,
        SymbolKind::FUNCTION | SymbolKind::OPERATOR => OutlineKind::Function,
        SymbolKind::CONSTANT => OutlineKind::Constant,
        SymbolKind::ENUM_MEMBER => OutlineKind::Variant,
        SymbolKind::STRUCT => OutlineKind::Struct,
        SymbolKind::TYPE_PARAMETER => OutlineKind::Type,
        _ => OutlineKind::Variable,
    }
}

#[cfg(test)]
#[path = "document_symbol_tests.rs"]
mod tests;
