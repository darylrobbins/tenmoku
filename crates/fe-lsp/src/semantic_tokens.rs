//! `textDocument/semanticTokens/full` and `full/delta`: the tokens a
//! server has for a whole document, by the names in its legend.
//!
//! - **Provider.** Only the document's provider is asked
//!   ([`Feature::SemanticTokens`](crate::Feature::SemanticTokens)): its
//!   lead if that has a `semanticTokensProvider` that does `full`, else
//!   the first add-on that does, as legends differ and a delta belongs to
//!   one server's last answer. With none, nothing is sent: `Ok(None)`.
//! - **Session.** The legend is read from the capabilities when the request
//!   is made, with the opening of the document they go with. A reply from
//!   another session or opening than the legend's is `Err(Retry)`, and so
//!   is a request made while the server is still initializing, once it
//!   has (nothing is sent): asked again, it is decoded by its own legend.
//! - **Delta.** Given the previous answer for the document, a server that
//!   does `full/delta` is asked for the edits since its `resultId`, while
//!   it still has open the document that answer came from. The edits are
//!   integers, applied to the previous stream sorted by start; a full reply
//!   replaces it. A delta that arrives for another opening of the document
//!   (it was closed and opened again before the request went) is not
//!   applied: `Err(Retry)`, and the next request is `full`.
//! - **Decoding.** The stream (relative line, relative start, length, type
//!   index, modifier bits) is decoded against the text the server had at
//!   the request's version, in the session's encoding, into bytes of that
//!   version. A token whose type isn't in the legend is skipped, modifier
//!   bits past the legend are ignored, and a token running past the end of
//!   its line is cut there.
//! - **Failures.** A request past `lsp.timeouts.hover` (3 s) is
//!   `Err(Retry)`, and cancels the request.

use std::ops::Range;
use std::sync::Arc;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use futures::future::BoxFuture;
use lsp_types::request::Request;
use lsp_types::{
    Position, SemanticTokenModifier, SemanticTokenType, SemanticTokensDeltaParams,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensServerCapabilities, ServerCapabilities, TextDocumentIdentifier,
};
use serde::{Deserialize, Serialize};

use crate::document::Admitted;
use crate::encoding::from_lsp;
use crate::event::{LspError, not_open};
use crate::identity::{IncarnationId, SessionId};
use crate::manager::LspManager;

/// One token of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SemanticToken {
    /// Its bytes, in the version the request was made on.
    pub range: Range<usize>,
    /// Its type, as the server's legend names it.
    pub token_type: Arc<str>,
    /// Its modifiers, as the server's legend names them, in bit order.
    pub modifiers: Vec<Arc<str>>,
}

/// The tokens a server has for a document, and what to ask for the next
/// ones as a delta of.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SemanticTokens {
    /// In the server's order: by position.
    pub tokens: Vec<SemanticToken>,
    /// What the server calls this answer, for a delta to start from.
    pub result_id: Option<String>,
    /// The server's integer stream, to apply a delta to.
    data: Vec<u32>,
    /// The opening of the document the tokens are for: a delta is only
    /// asked of a server that still has it open.
    from: (BufferId, (SessionId, IncarnationId)),
}

/// The names a server gives token types and modifiers, by index.
#[derive(Clone, Debug)]
struct Legend {
    types: Vec<Arc<str>>,
    modifiers: Vec<Arc<str>>,
}

/// The token types fastedit names (all the standard ones).
pub(crate) const TOKEN_TYPES: [SemanticTokenType; 25] = [
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::EVENT,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::MACRO,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::MODIFIER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::REGEXP,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::DECORATOR,
    SemanticTokenType::new("formatSpecifier"),
    SemanticTokenType::new("escapeSequence"),
];

/// The token modifiers fastedit names (all the standard ones).
pub(crate) const TOKEN_MODIFIERS: [SemanticTokenModifier; 10] = [
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::ABSTRACT,
    SemanticTokenModifier::ASYNC,
    SemanticTokenModifier::MODIFICATION,
    SemanticTokenModifier::DOCUMENTATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

/// What `Unavailable` says when the server doesn't provide tokens.
const FEATURE: &str = "semanticTokens";

/// What `Unavailable` says once a server that was initializing has: never
/// sent.
const INITIALIZED: &str = "semanticTokens: initialized";

/// `textDocument/semanticTokens/full`, its stream kept as integers.
enum FullRequest {}

impl Request for FullRequest {
    type Params = SemanticTokensParams;
    type Result = Option<Full>;
    const METHOD: &'static str = lsp_types::request::SemanticTokensFullRequest::METHOD;
}

/// `textDocument/semanticTokens/full/delta`, its edits' integers kept as
/// they are: an edit may replace part of a token (lsp-types' own edits
/// take whole tokens).
enum DeltaRequest {}

impl Request for DeltaRequest {
    type Params = SemanticTokensDeltaParams;
    type Result = Option<DeltaReply>;
    const METHOD: &'static str = lsp_types::request::SemanticTokensFullDeltaRequest::METHOD;
}

/// A full reply, or a partial one (`data` alone).
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Full {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_id: Option<String>,
    data: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum DeltaReply {
    Full(Full),
    Delta(Delta),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Delta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_id: Option<String>,
    edits: Vec<Edit>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Edit {
    start: u32,
    delete_count: u32,
    #[serde(default)]
    data: Vec<u32>,
}

/// A reply, before it is checked against the request and decoded.
enum Answer {
    Full(Full),
    Delta(Delta),
}

impl LspManager {
    /// The semantic tokens of open buffer `buffer`, at its `version`: sent
    /// only if the server has that version, as any
    /// [`document_request`](Self::document_request) is. `previous`, the
    /// last answer for the buffer, makes it a `full/delta` request when
    /// the server can answer one.
    ///
    /// `Ok(None)` if the server doesn't provide full semantic tokens
    /// (nothing is sent). `Err(LspError::Retry)` if the server doesn't
    /// answer within `lsp.timeouts.hover` (which also cancels the request:
    /// a busy server answers a later ask, as rust-analyzer does once it has
    /// analysed), or the answer can't be decoded as asked: the server was
    /// initializing, or restarted or reopened the document before the
    /// request went. Asking again then works. Dropping the future cancels the request. It
    /// doesn't borrow the manager.
    pub fn semantic_tokens(
        &self,
        buffer: BufferId,
        version: Version,
        previous: Option<&SemanticTokens>,
    ) -> impl Future<Output = Result<Option<SemanticTokens>, LspError>> + Send + 'static + use<>
    {
        let full_params = |at: &Admitted| SemanticTokensParams {
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            text_document: TextDocumentIdentifier {
                uri: at.document.uri.clone(),
            },
        };
        // The provider (§5): the first server the buffer is placed on whose
        // capabilities have full tokens, with them and the opening they go
        // with: the same before and after them, as a new session opens the
        // document anew. One still initializing before it is waited for.
        let mut known = Err(None);
        for server in self.servers_of(buffer) {
            let opening = self.active_identity(buffer, server);
            let Some(capabilities) = self.capabilities(server) else {
                known = Err(Some(server));
                break;
            };
            if provides_full(&capabilities) {
                let same = self.active_identity(buffer, server) == opening;
                known = Ok((server, capabilities, opening.filter(|_| same)));
                break;
            }
        }
        let (server, capabilities, opening) = match known {
            Ok(known) => known,
            // None provides them: nothing is sent.
            Err(None) if self.is_served(buffer) => return async { Ok(None) }.boxed(),
            Err(None) => return async { Err(LspError::Unavailable(not_open())) }.boxed(),
            Err(Some(initializing)) => {
                // Waits for them, sends nothing, and has the caller ask again
                // (or says why it can't).
                return self
                    .document_request_if_provided_on::<FullRequest>(
                        initializing,
                        buffer,
                        version,
                        INITIALIZED,
                        |_| false,
                        full_params,
                    )
                    .boxed()
                    .map(|answer| match answer {
                        Ok(_) => Err(LspError::Retry),
                        Err(LspError::Unavailable(why)) if why == INITIALIZED => {
                            Err(LspError::Retry)
                        }
                        Err(LspError::Timeout) => Err(LspError::Retry),
                        Err(err) => Err(err),
                    })
                    .boxed();
            }
        };
        let legend = legend(&capabilities);
        let delta = previous.and_then(|previous| {
            let result_id = previous.result_id.clone()?;
            let same = previous.from.0 == buffer && opening == Some(previous.from.1);
            (same && provides_delta(&capabilities))
                .then(|| (result_id, (previous.from.1, previous.data.clone())))
        });
        // Boxed, as their types name the manager's borrow.
        let (reply, previous): (BoxFuture<'static, _>, _) = match delta {
            Some((previous_result_id, previous)) => (
                self.document_request_if_provided_on::<DeltaRequest>(
                    server,
                    buffer,
                    version,
                    FEATURE,
                    provides_delta,
                    move |at| SemanticTokensDeltaParams {
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        text_document: TextDocumentIdentifier {
                            uri: at.document.uri.clone(),
                        },
                        previous_result_id,
                    },
                )
                .boxed()
                .map(|answer| {
                    answer.map(|(reply, at)| {
                        let answer = match reply.result {
                            None => Answer::Full(Full::default()),
                            Some(DeltaReply::Full(full)) => Answer::Full(full),
                            Some(DeltaReply::Delta(delta)) => Answer::Delta(delta),
                        };
                        (answer, at)
                    })
                })
                .boxed(),
                Some(previous),
            ),
            None => (
                self.document_request_if_provided_on::<FullRequest>(
                    server,
                    buffer,
                    version,
                    FEATURE,
                    provides_full,
                    full_params,
                )
                .boxed()
                .map(|answer| {
                    answer.map(|(reply, at)| (Answer::Full(reply.result.unwrap_or_default()), at))
                })
                .boxed(),
                None,
            ),
        };
        reply
            .map(move |answer| {
                let (answer, at) = match answer {
                    Ok(answered) => answered,
                    Err(LspError::Timeout) => return Err(LspError::Retry),
                    Err(LspError::Unavailable(why)) if why == FEATURE => return Ok(None),
                    Err(err) => return Err(err),
                };
                let identity = at.document.identity;
                // The legend is another session's.
                if opening.map(|(session, _)| session) != Some(identity.0) {
                    return Err(LspError::Retry);
                }
                let Some(legend) = legend else {
                    return Ok(None);
                };
                let (result_id, data) = match answer {
                    Answer::Full(full) => (full.result_id, full.data),
                    Answer::Delta(delta) => match previous {
                        Some((from, previous)) if from == identity => {
                            (delta.result_id, apply(&previous, delta.edits))
                        }
                        // For another opening than the previous answer's.
                        _ => return Err(LspError::Retry),
                    },
                };
                Ok(Some(SemanticTokens {
                    tokens: decode(&data, &legend, &at),
                    result_id,
                    data,
                    from: (at.document.buffer, identity),
                }))
            })
            .boxed()
    }
}

/// `previous` with `edits`, each of which indexes its integers, applied
/// in order of their starts. An edit reaching past the end stops there.
fn apply(previous: &[u32], mut edits: Vec<Edit>) -> Vec<u32> {
    edits.sort_by_key(|edit| edit.start);
    let mut data = Vec::with_capacity(previous.len());
    let mut kept = 0;
    for edit in edits {
        let start = (edit.start as usize).clamp(kept, previous.len());
        data.extend_from_slice(&previous[kept..start]);
        data.extend(edit.data);
        kept = start
            .saturating_add(edit.delete_count as usize)
            .min(previous.len());
    }
    data.extend_from_slice(&previous[kept..]);
    data
}

/// `data`'s tokens, in bytes of the text the request was made on.
fn decode(data: &[u32], legend: &Legend, at: &Admitted) -> Vec<SemanticToken> {
    let text = &at.document.sent.text;
    let (mut line, mut start) = (0u32, 0u32);
    let mut tokens = Vec::new();
    for &[delta_line, delta_start, length, token_type, bits] in data.as_chunks::<5>().0 {
        if delta_line == 0 {
            start = start.saturating_add(delta_start);
        } else {
            line = line.saturating_add(delta_line);
            start = delta_start;
        }
        let Some(token_type) = legend.types.get(token_type as usize) else {
            continue;
        };
        // Past its line's end, `from_lsp` stops at it.
        let from = from_lsp(text, Position::new(line, start), at.encoding);
        let to = from_lsp(
            text,
            Position::new(line, start.saturating_add(length)),
            at.encoding,
        );
        if from >= to {
            continue;
        }
        let modifiers = (legend.modifiers.iter().enumerate())
            .take(u32::BITS as usize)
            .filter(|(bit, _)| bits & (1 << bit) != 0)
            .map(|(_, name)| name.clone())
            .collect();
        tokens.push(SemanticToken {
            range: from..to,
            token_type: token_type.clone(),
            modifiers,
        });
    }
    tokens
}

fn options(capabilities: &ServerCapabilities) -> Option<&SemanticTokensOptions> {
    match capabilities.semantic_tokens_provider.as_ref()? {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => Some(options),
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(options) => {
            Some(&options.semantic_tokens_options)
        }
    }
}

fn legend(capabilities: &ServerCapabilities) -> Option<Legend> {
    let legend = &options(capabilities)?.legend;
    Some(Legend {
        types: (legend.token_types.iter())
            .map(|name| name.as_str().into())
            .collect(),
        modifiers: (legend.token_modifiers.iter())
            .map(|name| name.as_str().into())
            .collect(),
    })
}

/// Whether the server provides the tokens of a whole document.
pub(crate) fn provides_full(capabilities: &ServerCapabilities) -> bool {
    options(capabilities).is_some_and(|options| {
        matches!(
            options.full,
            Some(SemanticTokensFullOptions::Bool(true) | SemanticTokensFullOptions::Delta { .. })
        )
    })
}

/// Whether the server provides them as a delta of the last ones.
fn provides_delta(capabilities: &ServerCapabilities) -> bool {
    options(capabilities).is_some_and(|options| {
        matches!(
            options.full,
            Some(SemanticTokensFullOptions::Delta { delta: Some(true) })
        )
    })
}
