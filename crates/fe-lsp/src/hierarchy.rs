//! Call hierarchy (`textDocument/prepareCallHierarchy`,
//! `callHierarchy/incomingCalls|outgoingCalls`) and type hierarchy
//! (`textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes|subtypes`).
//!
//! - **Provider.** A server without `callHierarchyProvider` (or
//!   `typeHierarchyProvider`) gets no prepare request: `Ok(None)`.
//!   lsp-types 0.95.1 has no field for the latter, so the session records
//!   it as `experimental.typeHierarchyProvider` ([`mark_type_hierarchy`]).
//! - **Items.** A [`HierarchyItem`] may be in a file that isn't open, so its
//!   ranges stay LSP positions in the session's encoding and are converted
//!   against whatever text the host has ([`HierarchyItem::selection_in`]).
//!   Items whose uri isn't a `file:` path are dropped.
//! - **Follow-ups.** The raw item goes back unchanged to the server of the
//!   buffer it came from, only while that session has the buffer open as it
//!   was when the item came: otherwise `Unavailable("stale")`.
//! - **Failures.** A `null` reply or a timeout is empty. Results are capped
//!   at [`MAX_EDGES`].

use std::path::PathBuf;

use fe_buffer::{BufferId, Version};
use fe_nav::OutlineKind;
use fe_text::Text;
use futures::FutureExt;
use lsp_types::request::Request;
use lsp_types::{
    CallHierarchyPrepareParams, CallHierarchyServerCapability, Range, ServerCapabilities,
    SymbolKind, SymbolTag, TypeHierarchyPrepareParams, Url,
};
use serde::Deserialize;
use serde_json::Value;

use crate::document_symbol::outline_kind;
use crate::encoding::{PositionEncoding, from_lsp};
use crate::event::{LspError, NOT_OPEN};
use crate::identity::{IncarnationId, SessionId};
use crate::manager::LspManager;

/// The most edges (or items) one answer keeps.
pub const MAX_EDGES: usize = 2_000;

/// Where the session keeps that the server provides type hierarchy.
const TYPE_PROVIDER: &str = "typeHierarchyProvider";

/// A symbol in a call or type hierarchy.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct HierarchyItem {
    pub name: String,
    pub detail: Option<String>,
    pub kind: OutlineKind,
    pub path: PathBuf,
    /// The symbol's extent, as the server sent it.
    pub range: Range,
    /// What to select: usually the name.
    pub selection_range: Range,
    /// What `range` and `selection_range` count in.
    pub encoding: PositionEncoding,
    pub deprecated: bool,
    /// The item exactly as the server sent it (it carries `data`), sent
    /// back unchanged to ask for its calls or types.
    pub raw: Value,
    /// The document the item was asked for, as its session had it open.
    from: Option<(SessionId, IncarnationId)>,
}

impl HierarchyItem {
    /// `selection_range` in bytes of `text`.
    pub fn selection_in(&self, text: &Text) -> std::ops::Range<usize> {
        self.offsets(text, self.selection_range)
    }

    /// `range` in bytes of `text`.
    pub fn range_in(&self, text: &Text) -> std::ops::Range<usize> {
        self.offsets(text, self.range)
    }

    fn offsets(&self, text: &Text, range: Range) -> std::ops::Range<usize> {
        let start = from_lsp(text, range.start, self.encoding);
        let end = from_lsp(text, range.end, self.encoding);
        start.min(end)..end.max(start)
    }
}

/// A call between two symbols.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct CallEdge {
    /// The caller (incoming calls) or the callee (outgoing calls).
    pub item: HierarchyItem,
    /// The call sites: in the caller's file for incoming calls, in the item
    /// asked about for outgoing calls.
    pub from_ranges: Vec<Range>,
}

/// Records that the raw `initialize` result `capabilities` has a type
/// hierarchy provider, where `capabilities` (typed) can't say so.
pub(crate) fn mark_type_hierarchy(raw: &Value, capabilities: &mut ServerCapabilities) {
    let provided = match raw.get("capabilities").and_then(|c| c.get(TYPE_PROVIDER)) {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(_) => true,
    };
    if !provided {
        return;
    }
    match &mut capabilities.experimental {
        Some(Value::Object(map)) => {
            map.insert(TYPE_PROVIDER.into(), Value::Bool(true));
        }
        slot @ None => *slot = Some(serde_json::json!({ TYPE_PROVIDER: true })),
        // Not an object: kept, under a key beside the marker.
        Some(other) => {
            let kept = other.take();
            *other = serde_json::json!({ TYPE_PROVIDER: true, "kept": kept });
        }
    }
}

fn calls(capabilities: &ServerCapabilities) -> bool {
    match &capabilities.call_hierarchy_provider {
        None | Some(CallHierarchyServerCapability::Simple(false)) => false,
        Some(_) => true,
    }
}

pub(crate) fn types(capabilities: &ServerCapabilities) -> bool {
    match (capabilities.experimental.as_ref()).and_then(|e| e.get(TYPE_PROVIDER)) {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(_) => true,
    }
}

/// Whether the server has the document `from` names open as it was.
fn has_open(opened: &crate::Opened, from: Identity, buffer: BufferId) -> bool {
    match from {
        None => true,
        Some(identity) => (opened.documents.iter())
            .any(|document| document.buffer == buffer && document.identity == identity),
    }
}

type Identity = Option<(SessionId, IncarnationId)>;

/// The fields of an item that fe-lsp reads, decoded from its JSON; the
/// item's other fields (`data` among them) stay in the JSON.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wire {
    name: String,
    kind: SymbolKind,
    #[serde(default)]
    tags: Option<Vec<SymbolTag>>,
    #[serde(default)]
    detail: Option<String>,
    uri: Url,
    range: Range,
    selection_range: Range,
}

/// `raw` as an item, or `None` if it's malformed or not in a file.
fn decode(raw: Value, encoding: PositionEncoding, from: Identity) -> Option<HierarchyItem> {
    let wire = Wire::deserialize(&raw).ok()?;
    Some(HierarchyItem {
        deprecated: (wire.tags.as_deref()).is_some_and(|t| t.contains(&SymbolTag::DEPRECATED)),
        path: wire.uri.to_file_path().ok()?,
        name: wire.name,
        detail: wire.detail,
        kind: outline_kind(wire.kind),
        range: wire.range,
        selection_range: wire.selection_range,
        encoding,
        raw,
        from,
    })
}

/// A call as the server sent it: `key` is `from` or `to`.
fn decode_call(
    mut call: Value,
    key: &str,
    encoding: PositionEncoding,
    from: Identity,
) -> Option<CallEdge> {
    let ranges = call.get("fromRanges").cloned().unwrap_or(Value::Null);
    let from_ranges = serde_json::from_value(ranges).unwrap_or_default();
    let item = decode(call.get_mut(key)?.take(), encoding, from)?;
    Some(CallEdge { item, from_ranges })
}

/// One of the six requests, with items as JSON so that none of an item's
/// fields is lost.
macro_rules! request {
    ($name:ident, $method:literal, $params:ty) => {
        enum $name {}
        impl Request for $name {
            type Params = $params;
            type Result = Option<Vec<Value>>;
            const METHOD: &'static str = $method;
        }
    };
}

request!(
    PrepareCalls,
    "textDocument/prepareCallHierarchy",
    CallHierarchyPrepareParams
);
request!(
    PrepareTypes,
    "textDocument/prepareTypeHierarchy",
    TypeHierarchyPrepareParams
);
request!(Incoming, "callHierarchy/incomingCalls", Value);
request!(Outgoing, "callHierarchy/outgoingCalls", Value);
request!(Supertypes, "typeHierarchy/supertypes", Value);
request!(Subtypes, "typeHierarchy/subtypes", Value);

impl LspManager {
    /// The call hierarchy items at byte `offset` of open buffer `buffer`, at
    /// its `version`. `Ok(None)` if the server doesn't provide call
    /// hierarchy: nothing is sent. Empty on `null` or a timeout.
    pub fn prepare_call_hierarchy(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> impl Future<Output = Result<Option<Vec<HierarchyItem>>, LspError>> + Send + 'static + use<>
    {
        let reply = self.document_request_if_provided::<PrepareCalls>(
            buffer,
            version,
            "callHierarchy",
            calls,
            move |at| CallHierarchyPrepareParams {
                text_document_position_params: at.position(offset),
                work_done_progress_params: Default::default(),
            },
        );
        prepared("callHierarchy", reply.boxed())
    }

    /// As [`prepare_call_hierarchy`](Self::prepare_call_hierarchy), for type
    /// hierarchy.
    pub fn prepare_type_hierarchy(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> impl Future<Output = Result<Option<Vec<HierarchyItem>>, LspError>> + Send + 'static + use<>
    {
        let reply = self.document_request_if_provided::<PrepareTypes>(
            buffer,
            version,
            "typeHierarchy",
            types,
            move |at| TypeHierarchyPrepareParams {
                text_document_position_params: at.position(offset),
                work_done_progress_params: Default::default(),
            },
        );
        prepared("typeHierarchy", reply.boxed())
    }

    /// The calls into `item`, which came from open buffer `server_of`.
    pub fn incoming_calls(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> impl Future<Output = Result<Vec<CallEdge>, LspError>> + Send + 'static + use<> {
        self.call_edges::<Incoming>(server_of, item, "from")
    }

    /// The calls out of `item`, which came from open buffer `server_of`.
    pub fn outgoing_calls(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> impl Future<Output = Result<Vec<CallEdge>, LspError>> + Send + 'static + use<> {
        self.call_edges::<Outgoing>(server_of, item, "to")
    }

    /// The supertypes of `item`, which came from open buffer `server_of`.
    pub fn supertypes(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> impl Future<Output = Result<Vec<HierarchyItem>, LspError>> + Send + 'static + use<> {
        self.type_items::<Supertypes>(server_of, item)
    }

    /// The subtypes of `item`, which came from open buffer `server_of`.
    pub fn subtypes(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> impl Future<Output = Result<Vec<HierarchyItem>, LspError>> + Send + 'static + use<> {
        self.type_items::<Subtypes>(server_of, item)
    }

    fn call_edges<R: Request<Params = Value, Result = Option<Vec<Value>>> + 'static>(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
        key: &'static str,
    ) -> impl Future<Output = Result<Vec<CallEdge>, LspError>> + Send + 'static + use<R> {
        let (encoding, from) = (item.encoding, item.from);
        let sent = self.follow_up::<R>(server_of, item);
        async move {
            Ok((sent.await?.into_iter())
                .filter_map(|call| decode_call(call, key, encoding, from))
                .take(MAX_EDGES)
                .collect())
        }
    }

    fn type_items<R: Request<Params = Value, Result = Option<Vec<Value>>> + 'static>(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> impl Future<Output = Result<Vec<HierarchyItem>, LspError>> + Send + 'static + use<R> {
        let (encoding, from) = (item.encoding, item.from);
        let sent = self.follow_up::<R>(server_of, item);
        async move {
            Ok((sent.await?.into_iter())
                .filter_map(|raw| decode(raw, encoding, from))
                .take(MAX_EDGES)
                .collect())
        }
    }

    /// Sends `item`'s raw JSON back, unchanged, to the server of
    /// `server_of`, if that session still has the buffer open as it was.
    /// `null` and a timeout are an empty list.
    fn follow_up<R: Request<Params = Value, Result = Option<Vec<Value>>> + 'static>(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> futures::future::BoxFuture<'static, Result<Vec<Value>, LspError>> {
        let Some(server) = self.server_of(server_of) else {
            return async { Err(LspError::Unavailable(NOT_OPEN.into())) }.boxed();
        };
        let from = item.from;
        let params = serde_json::json!({ "item": item.raw });
        let reply = self
            .request_if::<R>(server, params, move |opened| {
                has_open(opened, from, server_of)
            })
            .boxed();
        async move {
            match reply.await {
                Ok(Some(reply)) => Ok(reply.result.unwrap_or_default()),
                Ok(None) => Err(LspError::Unavailable("stale".into())),
                Err(LspError::Timeout) => Ok(Vec::new()),
                Err(err) => Err(err),
            }
        }
        .boxed()
    }
}

/// The answer to a prepare request: `Ok(None)` without the provider.
fn prepared(
    feature: &'static str,
    reply: futures::future::BoxFuture<'static, Prepared>,
) -> futures::future::BoxFuture<'static, Result<Option<Vec<HierarchyItem>>, LspError>> {
    reply
        .map(move |answer| match answer {
            Ok((reply, admitted)) => {
                let from = Some(admitted.document.identity);
                Ok(Some(
                    (reply.result.unwrap_or_default().into_iter())
                        .filter_map(|raw| decode(raw, admitted.encoding, from))
                        .take(MAX_EDGES)
                        .collect(),
                ))
            }
            Err(LspError::Timeout) => Ok(Some(Vec::new())),
            Err(LspError::Unavailable(why)) if why == feature => Ok(None),
            Err(err) => Err(err),
        })
        .boxed()
}

type Prepared = Result<(crate::Reply<Option<Vec<Value>>>, crate::Admitted), LspError>;

#[cfg(test)]
#[path = "hierarchy_tests.rs"]
mod tests;
