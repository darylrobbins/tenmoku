//! Requests in flight (docs/decisions/lsp-client.md §3.5): replies, per
//! method timeouts, cancellation and the orphan budget.
//!
//! - Every request is issued by its server's actor, which mirrors
//!   async-lsp's id for it (§2.1 gap 2). One whose caller has gone before
//!   it's sent is never sent.
//! - Its timeout (`lsp.timeouts`, on the injected clock) runs from the
//!   call, in the caller's future ([`within`]), so a request still waiting
//!   to be sent (behind a slow `initialize`, or a server that stopped
//!   reading) times out too. Its caller dropping the future, or timing
//!   out, sends `$/cancelRequest` with that id, and the caller gets
//!   [`LspError::Timeout`] at once (or nothing, having gone); one not yet
//!   sent is never sent. async-lsp frees a request's entry only when a reply comes, so
//!   the reply future isn't dropped: it becomes an **orphan**, which
//!   discards the late reply. More than [`ORPHANS_MAX`] orphans mean the
//!   server has stopped answering, and the actor recycles the session.
//! - A reply carries whether an empty result is final ([`Reply`]), and
//!   error codes -32800, -32801 and -32802 become `Cancelled`, `Retry` and
//!   `Retry` ([`LspError::from_lsp`]).

use std::marker::PhantomData;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::Poll;
use std::time::Duration;

use async_lsp::ServerSocket;
use fe_config::settings::LspTimeouts;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Either};
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt, select_biased};
use lsp_types::request::Request;

use crate::clock::Clock;
use crate::event::{LspError, not_open};
use crate::readiness::Readiness;
use crate::server::send_request;

/// The most abandoned requests a session may have unanswered; one more
/// recycles it.
pub const ORPHANS_MAX: usize = 64;

/// A server's answer to a request.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Reply<T> {
    pub result: T,
    /// Whether an empty `result` (no locations, no symbols) is final: only
    /// from a server in `server-status` mode that said `quiescent: true`
    /// before the request was sent and hasn't said otherwise since (§3.4).
    /// Otherwise an empty answer may come from a server still indexing,
    /// and something else (search) should answer instead. A non-empty
    /// result is always final.
    pub empty_is_final: bool,
}

/// How long formatting may take, in milliseconds: it has no `lsp.timeouts`
/// setting.
const FORMAT_TIMEOUT_MS: u64 = 3000;

/// How long a request with `method` may take, from `lsp.timeouts`; none
/// for methods it doesn't name.
pub(crate) fn timeout(method: &str, timeouts: &LspTimeouts) -> Option<Duration> {
    let ms = match method {
        "textDocument/formatting"
        | "textDocument/rangeFormatting"
        | "textDocument/onTypeFormatting" => FORMAT_TIMEOUT_MS,
        "textDocument/definition"
        | "textDocument/declaration"
        | "textDocument/typeDefinition"
        | "textDocument/implementation"
        | "textDocument/prepareCallHierarchy"
        | "textDocument/prepareTypeHierarchy" => timeouts.definition,
        "workspace/symbol" => timeouts.workspace_symbol,
        "textDocument/references"
        | "workspace/willRenameFiles"
        | "callHierarchy/incomingCalls"
        | "callHierarchy/outgoingCalls"
        | "typeHierarchy/supertypes"
        | "typeHierarchy/subtypes" => timeouts.references,
        "textDocument/hover"
        | "textDocument/documentHighlight"
        | "textDocument/linkedEditingRange"
        | "textDocument/documentSymbol"
        | "textDocument/foldingRange"
        | "textDocument/inlayHint"
        | "textDocument/codeLens"
        | "textDocument/documentLink"
        | "documentLink/resolve"
        | "textDocument/documentColor"
        | "textDocument/colorPresentation"
        | "textDocument/semanticTokens/full"
        | "textDocument/semanticTokens/full/delta" => timeouts.hover,
        "textDocument/codeAction" => timeouts.code_action,
        "textDocument/signatureHelp" => timeouts.signature_help,
        "textDocument/completion" => timeouts.completion,
        "completionItem/resolve"
        | "codeAction/resolve"
        | "inlayHint/resolve"
        | "codeLens/resolve" => timeouts.resolve,
        _ => return None,
    };
    Some(Duration::from_millis(ms.max(1)))
}

/// `answered`, or `timed_out` once `timeout` has passed on `clock`,
/// counted from now. Timing out drops `answered`, which the actor sees as
/// the caller gone: it cancels the request, or never sends it.
pub(crate) fn within<A: Send + 'static>(
    answered: impl Future<Output = A> + Send + 'static,
    timeout: Option<Duration>,
    clock: &dyn Clock,
    timed_out: A,
) -> impl Future<Output = A> + Send + 'static {
    let timer = timeout.map(|timeout| clock.sleep(timeout));
    async move {
        let Some(timer) = timer else {
            return answered.await;
        };
        match futures::future::select(Box::pin(answered), timer).await {
            Either::Left((answer, _)) => answer,
            Either::Right(((), answered)) => {
                drop(answered);
                timed_out
            }
        }
    }
}

/// How one server's answer to a first-that-answers request counts (§5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rank {
    /// A non-empty answer: taken, once every earlier server has settled.
    Taken,
    /// An empty answer: the answer only if no server gives one.
    Empty,
    /// An error: the answer only if no server answers at all.
    Failed,
    /// The server doesn't provide the feature: the answer only if none
    /// does.
    Unprovided,
}

/// The first-that-answers of `asks` (§5), one request per server in
/// server order (the lead first), all already sent: the first answer that
/// `rank` takes *in that order*, waiting for each earlier server to
/// answer, fail or time out before taking a later one's, so the result
/// doesn't depend on which answered first. If none is taken, the earliest
/// of the best of the rest: an empty answer, else an error, else "not
/// provided". No servers at all is `Unavailable(not open)`.
///
/// Every request is polled at once. Those still out when the answer is
/// known are dropped, which cancels them, as is every one if the future
/// is dropped.
pub(crate) fn first_that_answers<T: Send + 'static>(
    asks: Vec<BoxFuture<'static, Result<T, LspError>>>,
    rank: impl Fn(&Result<T, LspError>) -> Rank + Send + 'static,
) -> impl Future<Output = Result<T, LspError>> + Send + 'static {
    let mut pending: Vec<Option<BoxFuture<'static, Result<T, LspError>>>> =
        asks.into_iter().map(Some).collect();
    let mut settled: Vec<Option<Result<T, LspError>>> = pending.iter().map(|_| None).collect();
    // Every server before this one has settled, and none was taken.
    let mut next = 0;
    futures::future::poll_fn(move |cx| {
        for (ask, answer) in pending.iter_mut().zip(settled.iter_mut()).skip(next) {
            if let Some(future) = ask
                && let Poll::Ready(answered) = future.poll_unpin(cx)
            {
                *answer = Some(answered);
                *ask = None;
            }
        }
        while let Some(answer) = settled.get(next) {
            match answer {
                None => return Poll::Pending,
                Some(answer) if rank(answer) == Rank::Taken => {
                    // The later ones are cancelled.
                    pending.clear();
                    return Poll::Ready(settled[next].take().expect("settled"));
                }
                Some(_) => next += 1,
            }
        }
        let best = (settled.iter().enumerate())
            .filter_map(|(at, answer)| Some((rank(answer.as_ref()?), at)))
            .min()
            .map(|(_, at)| at);
        Poll::Ready(match best {
            Some(at) => settled[at].take().expect("settled"),
            None => Err(LspError::Unavailable(not_open())),
        })
    })
}

/// How a list answer counts: a non-empty list is taken, an empty one is
/// empty, and none (`Ok(None)`) is a server without the provider.
pub(crate) fn rank_list<T>(answer: &Result<Option<Vec<T>>, LspError>) -> Rank {
    match answer {
        Ok(Some(found)) if !found.is_empty() => Rank::Taken,
        Ok(Some(_)) => Rank::Empty,
        Ok(None) => Rank::Unprovided,
        Err(_) => Rank::Failed,
    }
}

/// What a request in flight comes to, in the actor.
pub(crate) enum Settled {
    /// Its caller has the reply.
    Answered,
    /// Its caller went, or its timeout passed: cancel request `id`, and
    /// keep `orphan` until the server answers it.
    Abandoned {
        id: i32,
        orphan: BoxFuture<'static, ()>,
    },
}

/// What the actor issues a request with.
pub(crate) struct Out<'a> {
    pub(crate) socket: &'a ServerSocket,
    /// The id async-lsp gives it.
    pub(crate) id: i32,
    pub(crate) readiness: &'a Arc<Mutex<Readiness>>,
}

/// A request waiting for its actor to send it.
pub(crate) trait Issuer: Send {
    /// Its caller has gone: nothing need be sent.
    fn gone(&self) -> bool;
    #[cfg(test)]
    fn method(&self) -> &'static str;
    /// Sends it, and returns what answers its caller.
    fn issue(self: Box<Self>, out: Out<'_>) -> BoxFuture<'static, Settled>;
}

pub(crate) type Issue = Box<dyn Issuer>;

/// A request `R` with `params`, answering `answer` with `wrap` of the id
/// it went with and its reply.
pub(crate) fn call<R, A>(
    answer: oneshot::Sender<A>,
    params: R::Params,
    wrap: impl FnOnce(i32, Result<Reply<R::Result>, LspError>) -> A + Send + 'static,
) -> Issue
where
    R: Request,
    A: Send + 'static,
{
    Box::new(Call {
        answer,
        params,
        #[cfg(test)]
        method: R::METHOD,
        send: |socket: &ServerSocket, params| send_request::<R>(socket, params).boxed(),
        wrap,
        _result: PhantomData,
    })
}

struct Call<P, T, A, S, W> {
    answer: oneshot::Sender<A>,
    params: P,
    #[cfg(test)]
    method: &'static str,
    /// Sends the request with its params.
    send: S,
    wrap: W,
    _result: PhantomData<fn() -> T>,
}

impl<P, T, A, S, W> Issuer for Call<P, T, A, S, W>
where
    P: Send,
    T: Send + 'static,
    A: Send + 'static,
    S: FnOnce(&ServerSocket, P) -> BoxFuture<'static, async_lsp::Result<T>> + Send,
    W: FnOnce(i32, Result<Reply<T>, LspError>) -> A + Send + 'static,
{
    fn gone(&self) -> bool {
        self.answer.is_canceled()
    }

    #[cfg(test)]
    fn method(&self) -> &'static str {
        self.method
    }

    fn issue(self: Box<Self>, out: Out<'_>) -> BoxFuture<'static, Settled> {
        let Call {
            mut answer,
            params,
            send,
            wrap,
            ..
        } = *self;
        let id = out.id;
        let mut reply = send(out.socket, params);
        let readiness = out.readiness.clone();
        let mark = lock(&readiness).final_mark();
        Box::pin(async move {
            let then = {
                select_biased! {
                    result = (&mut reply).fuse() => Then::Replied(result),
                    () = answer.cancellation().fuse() => Then::Gone,
                }
            };
            let result = match then {
                Then::Replied(result) => result,
                Then::Gone => return abandoned(id, reply),
            };
            let reply = result.map_err(LspError::from_lsp).map(|result| Reply {
                result,
                empty_is_final: lock(&readiness).is_final(mark),
            });
            let _ = answer.send(wrap(id, reply));
            Settled::Answered
        })
    }
}

enum Then<T> {
    Replied(T),
    Gone,
}

fn abandoned<T: Send + 'static>(id: i32, reply: BoxFuture<'static, T>) -> Settled {
    Settled::Abandoned {
        id,
        orphan: reply.map(drop).boxed(),
    }
}

fn lock(readiness: &Mutex<Readiness>) -> std::sync::MutexGuard<'_, Readiness> {
    readiness.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Abandoned requests the server hasn't answered yet.
#[derive(Default)]
pub(crate) struct Orphans(FuturesUnordered<BoxFuture<'static, ()>>);

impl Orphans {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Keeps `orphan` until its reply comes. False, keeping nothing, if
    /// that would make more than [`ORPHANS_MAX`] still unanswered: those
    /// whose replies have come are let go first.
    pub(crate) fn adopt(&mut self, orphan: BoxFuture<'static, ()>) -> bool {
        while let Some(Some(())) = self.0.next().now_or_never() {}
        if self.0.len() >= ORPHANS_MAX {
            return false;
        }
        self.0.push(orphan);
        true
    }

    /// Resolves as each orphan's reply comes; never while there are none.
    pub(crate) async fn answered(&mut self) {
        if self.0.is_empty() {
            return futures::future::pending().await;
        }
        self.0.next().await;
    }
}
