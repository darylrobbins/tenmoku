//! The client side's router: what fe-lsp answers when a server asks, and
//! what it does with the server's notifications (§3.6).
//!
//! Handled notifications: `$/progress` and `experimental/serverStatus`
//! (readiness.rs), `window/logMessage` (into the log ring) and
//! `window/showMessage` ([`LspEvent::ShowMessage`]), and
//! `textDocument/publishDiagnostics`, which goes to the actor to decode
//! (diagnostics.rs). `workspace/applyEdit` goes to the actor too, to decode
//! and hand to the host (workspace_edit.rs). Each is parsed here,
//! so one the client can't read is logged and ignored rather than ending
//! the main loop, as async-lsp's typed handlers would. Everything else is
//! logged and ignored too.

use std::cell::{Cell, RefCell};
use std::future::ready;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use async_lsp::router::Router;
use async_lsp::{AnyNotification, ServerSocket};
use futures::FutureExt;
use lsp_types::notification::{
    LogMessage, Notification, Progress, PublishDiagnostics, ShowMessage,
};
use lsp_types::request::{
    ApplyWorkspaceEdit, CodeLensRefresh, InlayHintRefreshRequest, RegisterCapability,
    SemanticTokensRefresh, ShowDocument, ShowMessageRequest, UnregisterCapability,
    WorkDoneProgressCreate, WorkspaceConfiguration, WorkspaceDiagnosticRefresh,
    WorkspaceFoldersRequest,
};
use lsp_types::{
    LogMessageParams, ProgressParams, ProgressParamsValue, ShowDocumentResult, ShowMessageParams,
    WorkspaceFolder,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clock::Clock;
use crate::diagnostics::Inbox;
use crate::event::{LspEvent, MessageReply, ServerId, SharedLog};
use crate::readiness::Readiness;
use crate::server::{Queue, Session, Sink, Taken};
use crate::workspace_edit::Edits;

/// rust-analyzer's `experimental/serverStatus`, sent once the client opts
/// in with `experimental.serverStatusNotification`.
pub(crate) enum ServerStatusNotification {}

impl Notification for ServerStatusNotification {
    type Params = ServerStatusParams;
    const METHOD: &'static str = "experimental/serverStatus";
}

/// Makes the session panic as the router reads it: for tests of what a
/// panicking actor leaves behind.
#[cfg(test)]
pub(crate) enum Panic {}

#[cfg(test)]
impl Notification for Panic {
    type Params = ();
    const METHOD: &'static str = "fe-lsp/test/panic";
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ServerStatusParams {
    #[serde(default)]
    pub(crate) health: String,
    /// Whether the server has nothing more to do (indexing, loading).
    pub(crate) quiescent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
}

/// Wakes the actor when the router changed what it watches (readiness).
#[derive(Default)]
pub(crate) struct Signal {
    raised: Cell<bool>,
    waiting: Cell<Option<Waker>>,
}

impl Signal {
    fn raise(&self) {
        self.raised.set(true);
        if let Some(actor) = self.waiting.take() {
            actor.wake();
        }
    }

    /// Ready once raised since it was last ready.
    pub(crate) fn poll_raised(&self, cx: &Context<'_>) -> Poll<()> {
        if self.raised.replace(false) {
            return Poll::Ready(());
        }
        self.waiting.set(Some(cx.waker().clone()));
        Poll::Pending
    }
}

/// What the router's handlers know about their server.
pub(crate) struct ClientState {
    /// The server's name, for logs.
    pub(crate) name: String,
    /// The single workspace folder.
    pub(crate) folder: WorkspaceFolder,
    /// The spec's `settings`, answering `workspace/configuration`; the
    /// actor replaces them when they change.
    pub(crate) settings: Rc<RefCell<Value>>,
    /// Where the loop counts the actor's requests it has taken.
    pub(crate) queue: Rc<Queue>,
    pub(crate) server: ServerId,
    pub(crate) sink: Sink,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) readiness: Arc<Mutex<Readiness>>,
    /// Raised when `readiness` changes.
    pub(crate) signal: Rc<Signal>,
    /// Raised at `workspace/diagnostic/refresh`, for the actor to pull
    /// every open document again (pull.rs).
    pub(crate) refresh: Rc<Signal>,
    pub(crate) log: SharedLog,
    /// Where publications wait for the actor to decode them.
    pub(crate) diagnostics: Rc<Inbox>,
    /// Where `workspace/applyEdit`s wait for the actor to decode them.
    pub(crate) edits: Rc<Edits>,
    /// Holds the session's registrations.
    pub(crate) session: Arc<Mutex<Session>>,
    /// Keeps the main loop's event channel open for as long as the loop
    /// lives: async-lsp panics if every sender is gone.
    pub(crate) _socket: ServerSocket,
}

pub(crate) fn client_router(state: ClientState) -> Router<ClientState> {
    let mut router = Router::new(state);
    router
        .request::<WorkDoneProgressCreate, _>(|st, params| {
            st.readiness().created(&params.token, st.clock.now());
            st.signal.raise();
            ready(Ok(()))
        })
        .request::<WorkspaceFoldersRequest, _>(|st, ()| ready(Ok(Some(vec![st.folder.clone()]))))
        .request::<WorkspaceConfiguration, _>(|st, params| {
            let settings = st.settings.borrow();
            let answers = params
                .items
                .iter()
                .map(|item| section(&settings, item.section.as_deref()))
                .collect();
            ready(Ok(answers))
        })
        .request::<ApplyWorkspaceEdit, _>(|st, params| st.edits.ask(params, &*st.clock).map(Ok))
        .request::<ShowMessageRequest, _>(|st, params| {
            log::info!("{}: {:?}: {}", st.name, params.typ, params.message);
            let actions = params.actions.unwrap_or_default();
            let titles = actions.iter().map(|action| action.title.clone()).collect();
            let (reply, answer) = MessageReply::new(actions);
            let mut sink = st.sink.lock().unwrap_or_else(PoisonError::into_inner);
            sink(LspEvent::ShowMessageRequest {
                server: st.server,
                name: st.name.clone(),
                typ: params.typ,
                message: params.message,
                actions: titles,
                reply,
            });
            // Every reply dropped unanswered cancels `answer`: dismissed.
            answer.map(|answer| Ok(answer.ok().flatten()))
        })
        .request::<ShowDocument, _>(|_, _| ready(Ok(ShowDocumentResult { success: false })))
        .request::<WorkspaceDiagnosticRefresh, _>(|st, ()| {
            st.refresh.raise();
            ready(Ok(()))
        })
        .request::<SemanticTokensRefresh, _>(|_, ()| ready(Ok(())))
        .request::<InlayHintRefreshRequest, _>(|_, ()| ready(Ok(())))
        .request::<CodeLensRefresh, _>(|_, ()| ready(Ok(())))
        .request::<RegisterCapability, _>(|st, params| {
            ready(st.session().registrations.register(params))
        })
        .request::<UnregisterCapability, _>(|st, params| {
            st.session().registrations.unregister(params);
            ready(Ok(()))
        })
        .event::<Taken>(|st, Taken| {
            st.queue.took();
            ControlFlow::Continue(())
        })
        // async-lsp's default stops the main loop on any notification it
        // has no handler for, unless its method starts with `$/`.
        .unhandled_notification(|st, notif| {
            st.notified(notif);
            ControlFlow::Continue(())
        });
    // Requests left unhandled get async-lsp's `MethodNotFound`.
    router
}

impl ClientState {
    fn session(&self) -> std::sync::MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn readiness(&self) -> std::sync::MutexGuard<'_, Readiness> {
        self.readiness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn notified(&self, notif: AnyNotification) {
        let method = notif.method.as_str();
        let parsed = match method {
            Progress::METHOD => parse(notif.params).map(|params| self.progress(params)),
            ServerStatusNotification::METHOD => {
                parse(notif.params).map(|params| self.server_status(&params))
            }
            LogMessage::METHOD => parse(notif.params).map(|params: LogMessageParams| {
                let line = format!("[{:?}] {}\n", params.typ, params.message);
                self.log
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(line.as_bytes());
            }),
            ShowMessage::METHOD => parse(notif.params).map(|params: ShowMessageParams| {
                let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
                sink(LspEvent::ShowMessage {
                    server: self.server,
                    name: self.name.clone(),
                    typ: params.typ,
                    message: params.message,
                });
            }),
            PublishDiagnostics::METHOD => {
                parse(notif.params).map(|params| self.diagnostics.push(params))
            }
            #[cfg(test)]
            Panic::METHOD => panic!("{}: a test asked the session to panic", self.name),
            _ => {
                log::info!("{}: ignored notification {method}", self.name);
                Ok(())
            }
        };
        if let Err(err) = parsed {
            log::info!("{}: ignored unreadable {method}: {err}", self.name);
        }
    }

    fn progress(&self, params: ProgressParams) {
        let ProgressParamsValue::WorkDone(value) = &params.value;
        self.readiness()
            .progress(&params.token, value, self.clock.now());
        self.signal.raise();
    }

    fn server_status(&self, params: &ServerStatusParams) {
        log::info!(
            "{}: status {} quiescent {}{}",
            self.name,
            params.health,
            params.quiescent,
            params
                .message
                .as_deref()
                .map(|m| format!(": {m}"))
                .unwrap_or_default(),
        );
        if self.readiness().status(params.quiescent, self.clock.now()) {
            self.signal.raise();
        } else {
            log::info!(
                "{}: in progress mode, so serverStatus is ignored",
                self.name
            );
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(params)
}

/// The value at a dotted `section` of `settings` ("a.b" is `settings.a.b`),
/// the whole of it for no section, or `null`.
fn section(settings: &Value, section: Option<&str>) -> Value {
    let Some(section) = section.filter(|s| !s.is_empty()) else {
        return settings.clone();
    };
    section
        .split('.')
        .try_fold(settings, |value, key| value.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}
