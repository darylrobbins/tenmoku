//! Pull diagnostics (`textDocument/diagnostic`, docs/decisions/lsp-client.md
//! §7), for servers whose `InitializeResult` has `diagnosticProvider`.
//! Open documents only: no workspace diagnostics, related documents, result
//! ids or dynamic registration. A spec with
//! [`PullDiagnostics::TsserverRequest`] pulls through tsserver instead
//! (tsserver.rs), the same way otherwise.
//!
//! - **When.** A document of a pulling server is pulled after its
//!   `didOpen`, after each flushed `didChange` batch, after `didSave`, when
//!   the server's status turns `Ready`, and on `workspace/diagnostic/refresh`
//!   (every open document of the session). The triggers of one drain make
//!   one pull, sent behind the notifications they came with. At most one
//!   pull per document is in flight: a trigger meanwhile marks it wanted,
//!   and it's pulled once more when the reply lands.
//! - **Which version.** A reply belongs to the LSP version of the last
//!   `didOpen` or `didChange` sent for the document before the request,
//!   since the server takes notifications and requests in order. It's
//!   decoded as a versioned publication for that version (diagnostics.rs):
//!   against the ring's text, and dropped if that version has left the
//!   ring, if a newer pull reply was applied already, or if the incarnation
//!   it was sent for has ended (closed, reopened, restarted). A `full`
//!   report gives the items. An `unchanged` report (fe-lsp never sends a
//!   result id), a partial result and an error (`ServerCancelled`,
//!   `ContentModified`, [`PULL_TIMEOUT`]) are dropped, leaving the next
//!   trigger to pull again.
//! - **Pushed and pulled.** A server may do both (rust-analyzer pushes
//!   `cargo check`'s and offers its own by pull). Each incarnation keeps the
//!   latest accepted pushed list and pulled list ([`Lists`]), each at its LSP
//!   version, each with its own older-than-newest rule. Whenever either
//!   changes, one [`LspEvent::Diagnostics`](crate::LspEvent::Diagnostics)
//!   holds both, at the newer of the two versions. The older list's ranges
//!   are mapped forward through the ring's edits (start biased right, end
//!   biased left, as fe-editor's `DiagnosticSet::apply` maps them); if its
//!   version has left the ring, it's left out until it's replaced.

use std::collections::VecDeque;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use fe_buffer::{Bias, BufferId, Edit, Version};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use lsp_types::request::{DocumentDiagnosticRequest, Request};
use lsp_types::{
    DiagnosticServerCapabilities, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    ExecuteCommandOptions, ServerCapabilities, Url,
};
use serde_json::{Value, json};

use crate::clock::Clock;
use crate::diagnostics::{Diagnostic, decode_in};
use crate::document::SentVersion;
use crate::encoding::PositionEncoding;
use crate::event::LspError;
use crate::identity::{IncarnationId, SessionId};
use crate::launcher::PullDiagnostics;
use crate::requests::{Issue, Reply, call, within};
use crate::tsserver::{self, Released};

/// How long a pull may take before it's cancelled and dropped.
pub(crate) const PULL_TIMEOUT: Duration = Duration::from_secs(30);

/// What a server's `diagnosticProvider` says to pull with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Provider {
    /// Sent back as the request's `identifier`.
    identifier: Option<String>,
}

impl Provider {
    /// The server's provider, if it advertises one (options or
    /// registration options).
    pub(crate) fn from_capabilities(capabilities: &ServerCapabilities) -> Option<Self> {
        let options = match capabilities.diagnostic_provider.as_ref()? {
            DiagnosticServerCapabilities::Options(options) => options,
            DiagnosticServerCapabilities::RegistrationOptions(registration) => {
                &registration.diagnostic_options
            }
        };
        Some(Self {
            identifier: options.identifier.clone(),
        })
    }
}

/// `textDocument/diagnostic` with its params as JSON: lsp-types 0.95.1
/// would send `"previousResultId": null` and `"identifier": null`, which
/// typescript-go refuses (`InvalidParams`), where the protocol has them
/// left out.
enum Pull {}

impl Request for Pull {
    type Params = Value;
    type Result = DocumentDiagnosticReportResult;
    const METHOD: &'static str = DocumentDiagnosticRequest::METHOD;
}

/// A pull's reply, with what it was sent for.
pub(crate) struct Pulled {
    pub(crate) buffer: BufferId,
    pub(crate) identity: (SessionId, IncarnationId),
    /// The LSP version the server had when it got the request.
    pub(crate) lsp_version: i32,
    pub(crate) result: Result<Answer, LspError>,
}

impl Pulled {
    /// The encoding its positions are in, if not the session's: tsserver
    /// counts UTF-16 code units whatever was negotiated.
    pub(crate) fn encoding(&self) -> Option<PositionEncoding> {
        match self.result {
            Ok(Answer::Tsserver(_)) => Some(PositionEncoding::Utf16),
            _ => None,
        }
    }
}

/// What a pull got back.
pub(crate) enum Answer {
    /// The reply to `textDocument/diagnostic`.
    Report(DocumentDiagnosticReportResult),
    /// The diagnostics in tsserver's three replies, or why they're dropped
    /// (tsserver.rs).
    Tsserver(Result<Vec<lsp_types::Diagnostic>, String>),
}

/// How a session pulls.
enum Via {
    Diagnostic(Provider),
    Tsserver,
}

/// A session's pulls: whether its server pulls, and the replies on their
/// way back to the actor.
pub(crate) struct Puller {
    /// What the spec asks for.
    mode: PullDiagnostics,
    via: Option<Via>,
    replies: FuturesUnordered<BoxFuture<'static, Pulled>>,
    /// A tsserver pull's later requests, once its first is sent.
    released: Released,
    clock: Arc<dyn Clock>,
}

impl Puller {
    /// Pulls nothing until [`set_capabilities`](Self::set_capabilities).
    pub(crate) fn new(mode: PullDiagnostics, clock: Arc<dyn Clock>) -> Self {
        Self {
            mode,
            via: None,
            replies: FuturesUnordered::new(),
            released: Released::default(),
            clock,
        }
    }

    /// Pulls as `mode` asks, if the server offers it: `textDocument/diagnostic`
    /// if it advertises `diagnosticProvider`, or tsserver's requests if it
    /// registers `typescript.tsserverRequest`.
    pub(crate) fn set_capabilities(&mut self, capabilities: &ServerCapabilities) {
        self.via = match self.mode {
            PullDiagnostics::Advertised => {
                Provider::from_capabilities(capabilities).map(Via::Diagnostic)
            }
            PullDiagnostics::TsserverRequest => {
                let registered = (capabilities.execute_command_provider.as_ref()).is_some_and(
                    |ExecuteCommandOptions { commands, .. }| {
                        commands.iter().any(|command| command == tsserver::COMMAND)
                    },
                );
                registered.then_some(Via::Tsserver)
            }
        };
    }

    pub(crate) fn pulls(&self) -> bool {
        self.via.is_some()
    }

    /// The requests that went out as their lead was sent, for the actor to
    /// send right behind it (tsserver.rs).
    pub(crate) fn released(&self) -> Vec<Issue> {
        std::mem::take(&mut *self.released.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The request pulling `uri`'s diagnostics, for the actor to send; its
    /// reply comes back through [`next`](Self::next). `None` if the server
    /// doesn't pull.
    pub(crate) fn request(
        &mut self,
        buffer: BufferId,
        uri: &Url,
        identity: (SessionId, IncarnationId),
        lsp_version: i32,
    ) -> Option<Issue> {
        let (issue, answered) = match self.via.as_ref()? {
            Via::Diagnostic(provider) => {
                let mut params = json!({ "textDocument": { "uri": uri } });
                if let Some(identifier) = &provider.identifier {
                    params["identifier"] = identifier.as_str().into();
                }
                let (answer, answered) = oneshot::channel();
                let issue = call::<Pull, _>(answer, params, |_, reply| {
                    reply.map(|Reply { result, .. }| Answer::Report(result))
                });
                let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
                (issue, answered.boxed())
            }
            Via::Tsserver => {
                let (issue, replies) = tsserver::pull(uri, &self.released);
                let uri = uri.clone();
                let answered = replies.map(move |replies| {
                    replies.map(|replies| Answer::Tsserver(tsserver::diagnostics(replies, &uri)))
                });
                (issue, answered.boxed())
            }
        };
        // Timing out drops `answered`, which cancels the requests.
        let replied = within(
            answered,
            Some(PULL_TIMEOUT),
            &*self.clock,
            Err(LspError::Timeout),
        );
        self.replies.push(
            replied
                .map(move |result| Pulled {
                    buffer,
                    identity,
                    lsp_version,
                    result,
                })
                .boxed(),
        );
        Some(issue)
    }

    /// The next reply; never, while none is on its way.
    pub(crate) async fn next(&mut self) -> Pulled {
        if self.replies.is_empty() {
            return futures::future::pending().await;
        }
        match futures::StreamExt::next(&mut self.replies).await {
            Some(pulled) => pulled,
            None => futures::future::pending().await,
        }
    }
}

/// The items of a pull reply worth decoding: a `full` report's, or why
/// it's dropped.
pub(crate) fn full_items(
    result: Result<Answer, LspError>,
) -> Result<Vec<lsp_types::Diagnostic>, String> {
    use DocumentDiagnosticReportResult as Got;
    match result {
        Ok(Answer::Report(Got::Report(DocumentDiagnosticReport::Full(full)))) => {
            Ok(full.full_document_diagnostic_report.items)
        }
        Ok(Answer::Report(Got::Report(DocumentDiagnosticReport::Unchanged(_)))) => {
            Err("an unchanged report".into())
        }
        Ok(Answer::Report(Got::Partial(_))) => Err("a partial result".into()),
        Ok(Answer::Tsserver(items)) => items,
        Err(err) => Err(format!("an error ({err})")),
    }
}

/// Where diagnostics came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Pushed,
    Pulled,
}

/// Diagnostics accepted for one LSP version, decoded in its text.
struct List {
    lsp_version: i32,
    items: Vec<Diagnostic>,
}

/// An incarnation's diagnostics and pulls.
#[derive(Default)]
pub(crate) struct Lists {
    pushed: Option<List>,
    pulled: Option<List>,
    /// A trigger came since the last pull was sent.
    wanted: bool,
    /// A pull is on its way.
    in_flight: bool,
}

impl Lists {
    /// Something happened that the document should be pulled for.
    pub(crate) fn want(&mut self) {
        self.wanted = true;
    }

    /// Whether to send a pull now: one is wanted and none is in flight.
    /// If so, it counts as sent.
    pub(crate) fn take_wanted(&mut self) -> bool {
        if !self.wanted || self.in_flight {
            return false;
        }
        self.wanted = false;
        self.in_flight = true;
        true
    }

    /// The pull in flight has its reply.
    pub(crate) fn replied(&mut self) {
        self.in_flight = false;
    }

    /// Accepts `diagnostics` from `source` for LSP version `version`,
    /// decoded against the ring's text for it. Returns the buffer version
    /// and items to report, holding both lists, or `None` if they're
    /// dropped: older than the newest accepted from `source`, or for a
    /// version not in the ring.
    pub(crate) fn accept(
        &mut self,
        source: Source,
        version: i32,
        diagnostics: Vec<lsp_types::Diagnostic>,
        ring: &VecDeque<SentVersion>,
        encoding: PositionEncoding,
        uri: &Url,
    ) -> Option<(Version, Vec<Diagnostic>)> {
        let slot = match source {
            Source::Pushed => &mut self.pushed,
            Source::Pulled => &mut self.pulled,
        };
        if let Some(newest) = slot.as_ref().map(|list| list.lsp_version)
            && version < newest
        {
            log::debug!(
                "{uri}: dropped {source:?} diagnostics for version {version}, older than {newest}"
            );
            return None;
        }
        let Some(sent) = ring.iter().find(|sent| sent.lsp_version == version) else {
            log::debug!(
                "{uri}: dropped {source:?} diagnostics for version {version}, not in the ring"
            );
            return None;
        };
        let items = decode_in(&sent.text, encoding, diagnostics, Some(uri));
        *slot = Some(List {
            lsp_version: version,
            items,
        });
        Some(self.merged(ring))
    }

    /// Both lists at the newer one's version: its buffer version, then the
    /// pushed items and the pulled ones.
    fn merged(&self, ring: &VecDeque<SentVersion>) -> (Version, Vec<Diagnostic>) {
        let lists = [&self.pushed, &self.pulled];
        let newest = (lists.iter().filter_map(|list| list.as_ref()))
            .map(|list| list.lsp_version)
            .max()
            .expect("a list was just accepted");
        let to = (ring.iter())
            .position(|sent| sent.lsp_version == newest)
            .expect("the list just accepted is at or before the newest, in the ring");
        let mut items = Vec::new();
        for list in lists.into_iter().flatten() {
            if list.lsp_version == newest {
                items.extend(list.items.iter().cloned());
            } else if let Some(mapped) = map_forward(ring, list, to) {
                items.extend(mapped);
            }
        }
        (ring[to].version, items)
    }
}

/// `list`'s items mapped through the ring's edits up to `ring[to]`, or
/// `None` if its version has left the ring.
fn map_forward(ring: &VecDeque<SentVersion>, list: &List, to: usize) -> Option<Vec<Diagnostic>> {
    let from = ring
        .iter()
        .position(|sent| sent.lsp_version == list.lsp_version)?;
    let steps: Vec<&SentVersion> = ring.range(from + 1..=to).collect();
    if steps.iter().any(|sent| !sent.edits_complete) {
        // Batches went missing between them (document.rs): the ranges
        // can't be followed.
        return None;
    }
    let batches: Vec<&[Edit]> = (steps.iter())
        .flat_map(|sent| sent.edits.iter().map(|batch| &batch[..]))
        .collect();
    Some(
        list.items
            .iter()
            .map(|item| {
                let start = (batches.iter())
                    .fold(item.range.start, |at, batch| map(batch, at, Bias::Right));
                let end =
                    (batches.iter()).fold(item.range.end, |at, batch| map(batch, at, Bias::Left));
                let related = (item.related.iter())
                    .map(|related| {
                        let mut related = related.clone();
                        related.range = related.range.map(|range| {
                            let start = (batches.iter())
                                .fold(range.start, |at, batch| map(batch, at, Bias::Right));
                            let end = (batches.iter())
                                .fold(range.end, |at, batch| map(batch, at, Bias::Left));
                            start..end.max(start)
                        });
                        related
                    })
                    .collect();
                Diagnostic {
                    range: start..end.max(start),
                    related,
                    ..item.clone()
                }
            })
            .collect(),
    )
}

/// Maps `offset` through one batch, as an anchor with `bias` moves:
/// fe-buffer's mapping, which it doesn't export.
///
/// Offsets outside every edit shift by the length change of the edits
/// before them. An offset where text was inserted takes `bias`'s side; at
/// the start of a replaced range it stays at the start, at its end it
/// moves to the end of the new text, and strictly inside it takes
/// `bias`'s end.
pub(crate) fn map(edits: &[Edit], offset: usize, bias: Bias) -> usize {
    let mut ix = edits.partition_point(|edit| edit.old.end < offset);
    // A replacement ending at `offset` may be followed by an edit starting
    // there; the later edit decides.
    while ix + 1 < edits.len()
        && edits[ix].old.start < offset
        && edits[ix].old.end == offset
        && edits[ix + 1].old.start == offset
    {
        ix += 1;
    }
    let Some(edit) = edits.get(ix) else {
        return match edits.last() {
            Some(last) => offset + last.new.end - last.old.end,
            None => offset,
        };
    };
    if offset < edit.old.start {
        return offset + edit.new.start - edit.old.start;
    }
    let side = if edit.old.is_empty() {
        bias
    } else if offset == edit.old.start {
        Bias::Left
    } else if offset == edit.old.end {
        Bias::Right
    } else {
        bias
    };
    match side {
        Bias::Left => edit.new.start,
        Bias::Right => edit.new.end,
    }
}
