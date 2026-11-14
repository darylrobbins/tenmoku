//! Call hierarchy (`textDocument/prepareCallHierarchy`,
//! `callHierarchy/incomingCalls|outgoingCalls`) and type hierarchy
//! (`textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes|subtypes`).
//!
//! - **Servers.** Preparation is first-that-answers (§5): every server of
//!   the document with `callHierarchyProvider` (or `typeHierarchyProvider`)
//!   is asked at once, and the first non-empty answer in server order (the
//!   lead first) is taken. With none providing, nothing is sent:
//!   `Ok(None)`.
//!   lsp-types 0.95.1 has no field for the latter, so the session records
//!   it as `experimental.typeHierarchyProvider` ([`mark_type_hierarchy`]).
//! - **Items.** A [`HierarchyItem`] may be in a file that isn't open, so its
//!   ranges stay LSP positions in the session's encoding and are converted
//!   against whatever text the host has ([`HierarchyItem::selection_in`]).
//!   Items whose uri isn't a `file:` path are dropped.
//! - **Follow-ups.** The raw item goes back unchanged to the server that
//!   produced it ([`HierarchyItem::owner`]), and to no other, only while
//!   that session has the buffer open as it was when the item came:
//!   otherwise (the server stopped or restarted, or the document was
//!   reopened there) nothing is sent, and the answer is
//!   `Unavailable("stale")`.
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
use crate::event::{LspError, ServerId, not_open};
use crate::manager::{LspManager, Owner};
use crate::requests::{first_that_answers, rank_list};

/// The most edges (or items) one answer keeps.
pub const MAX_EDGES: usize = 2_000;

/// Where [`LspManager::caller_count`] stops counting: shown as "99+".
pub const CALLER_COUNT_CAP: usize = 99;

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
    /// The server that produced the item, and the document it was asked
    /// for as that server's session had it open: follow-ups go there alone.
    from: Option<Owner>,
}

impl HierarchyItem {
    /// The server that produced the item, which alone is asked for its
    /// calls or types, with the opening of the document it came from.
    pub fn owner(&self) -> Option<Owner> {
        self.from
    }

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

type Identity = Option<Owner>;

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
        let asks = self.ask_all(buffer, |server| {
            let reply = self.document_request_if_provided_on::<PrepareCalls>(
                server,
                buffer,
                version,
                "callHierarchy",
                calls,
                move |at| CallHierarchyPrepareParams {
                    text_document_position_params: at.position(offset),
                    work_done_progress_params: Default::default(),
                },
            );
            prepared("callHierarchy", server, reply.boxed())
        });
        first_that_answers(asks.into_iter().map(|(_, ask)| ask).collect(), rank_list)
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
        let asks = self.ask_all(buffer, |server| {
            let reply = self.document_request_if_provided_on::<PrepareTypes>(
                server,
                buffer,
                version,
                "typeHierarchy",
                types,
                move |at| TypeHierarchyPrepareParams {
                    text_document_position_params: at.position(offset),
                    work_done_progress_params: Default::default(),
                },
            );
            prepared("typeHierarchy", server, reply.boxed())
        });
        first_that_answers(asks.into_iter().map(|(_, ask)| ask).collect(), rank_list)
    }

    /// The calls into `item`, which came from open buffer `server_of`.
    pub fn incoming_calls(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> impl Future<Output = Result<Vec<CallEdge>, LspError>> + Send + 'static + use<> {
        self.call_edges::<Incoming>(server_of, item, "from")
    }

    /// How many symbols call `item`, which came from open buffer
    /// `server_of`, for a hover card's footer: its incoming calls counted,
    /// at most [`CALLER_COUNT_CAP`] (so 99 means "99 or more"); a timeout is
    /// an error, not zero. A host asks
    /// [`prepare_call_hierarchy`](Self::prepare_call_hierarchy) at the
    /// hovered symbol first; `Ok(None)` there (no call hierarchy) or no
    /// item means no count.
    pub fn caller_count(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
    ) -> impl Future<Output = Result<usize, LspError>> + Send + 'static + use<> {
        // A timeout stays one: an unknown count isn't zero.
        let sent = self.follow_up::<Incoming>(server_of, item, false);
        let (encoding, from) = (item.encoding, item.from);
        async move {
            let calls = (sent.await?.into_iter())
                .filter_map(|call| decode_call(call, "from", encoding, from))
                .take(CALLER_COUNT_CAP);
            Ok(calls.count())
        }
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
        let sent = self.follow_up::<R>(server_of, item, true);
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
        let sent = self.follow_up::<R>(server_of, item, true);
        async move {
            Ok((sent.await?.into_iter())
                .filter_map(|raw| decode(raw, encoding, from))
                .take(MAX_EDGES)
                .collect())
        }
    }

    /// Sends `item`'s raw JSON back, unchanged, to the server that produced
    /// it, if that session still has the buffer open as it was: otherwise
    /// `Unavailable("stale")`, with nothing sent. (An item with no owner
    /// goes to the lead of `server_of`.) `null` is an empty list, and so is a timeout if
    /// `timeout_is_empty`.
    fn follow_up<R: Request<Params = Value, Result = Option<Vec<Value>>> + 'static>(
        &self,
        server_of: BufferId,
        item: &HierarchyItem,
        timeout_is_empty: bool,
    ) -> futures::future::BoxFuture<'static, Result<Vec<Value>, LspError>> {
        let from = item.from;
        let server = match from {
            Some(owner) if !self.runs(owner.server) => {
                return async { Err(LspError::Unavailable(STALE.into())) }.boxed();
            }
            Some(owner) => owner.server,
            None => match self.server_of(server_of) {
                Some(server) => server,
                None => return async { Err(LspError::Unavailable(not_open())) }.boxed(),
            },
        };
        let params = serde_json::json!({ "item": item.raw });
        let reply = self
            .request_if::<R>(server, params, move |opened| {
                from.is_none_or(|owner| owner.has_open(opened))
            })
            .boxed();
        async move {
            match reply.await {
                Ok(Some(reply)) => Ok(reply.result.unwrap_or_default()),
                Ok(None) => Err(LspError::Unavailable(STALE.into())),
                Err(LspError::Timeout) if timeout_is_empty => Ok(Vec::new()),
                Err(err) => Err(err),
            }
        }
        .boxed()
    }
}

/// What `Unavailable` says when an item's session has ended.
const STALE: &str = "stale";

/// `server`'s answer to a prepare request: `Ok(None)` without the
/// provider.
fn prepared(
    feature: &'static str,
    server: ServerId,
    reply: futures::future::BoxFuture<'static, Prepared>,
) -> futures::future::BoxFuture<'static, Result<Option<Vec<HierarchyItem>>, LspError>> {
    reply
        .map(move |answer| match answer {
            Ok((reply, admitted)) => {
                let from = Some(Owner::of(server, &admitted));
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
