//! Requests and notifications a host sends a server by method name, for the methods a server
//! defines beyond the LSP's own (rust-analyzer's `experimental/runnables`, say), used by plugins
//! (`lsp.request`, `lsp.notify`).
//!
//! - **Which session.** By server name and root ([`custom_request_at`](LspManager::custom_request_at))
//!   or by the server an open document is placed on ([`custom_request_for`](LspManager::custom_request_for)):
//!   never by name alone. None running there answers [`LspError::ServerDown`]; nothing is started.
//! - **Known limit: an allowlist.** async-lsp types every method at compile time, so only the
//!   methods in [`raw_requests!`] and [`raw_notifications!`] can be sent. Any other answers
//!   [`LspError::Unavailable`], without sending anything. Sending arbitrary methods needs an
//!   async-lsp change, a follow-up.
//! - **The reply** is the result as JSON, `null` included. The method's `lsp.timeouts` entry, if
//!   it has one, is its deadline; dropping the future cancels the request.

use std::path::Path;

use async_lsp::ServerSocket;
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use serde_json::Value;

use fe_buffer::BufferId;
use kiln_l10n::verbatim;

use crate::event::{LspError, ServerId, Unavailable};
use crate::manager::LspManager;
use crate::requests::{Issue, Reply, call, within};
use crate::server::Command;

/// Declares the requests whose params and results stay JSON, for [`request_call`] to look up by
/// method name.
macro_rules! raw_requests {
    ($($name:ident = $method:literal),* $(,)?) => {
        $(
            enum $name {}
            impl Request for $name {
                type Params = Value;
                type Result = Value;
                const METHOD: &'static str = $method;
            }
        )*

        // The request `method` with `params`, answering `answer`; none if it isn't one fe-lsp
        // sends.
        fn request_call(
            method: &str,
            answer: oneshot::Sender<Result<Value, LspError>>,
            params: Value,
        ) -> Option<Issue> {
            let wrap = |_, reply: Result<Reply<Value>, LspError>| reply.map(|reply| reply.result);
            match method {
                $($method => Some(call::<$name, _>(answer, params, wrap)),)*
                _ => None,
            }
        }
    };
}

raw_requests! {
    RunnablesRa = "experimental/runnables",
    MatchingBrace = "experimental/matchingBrace",
    ParentModule = "experimental/parentModule",
    RelatedTests = "experimental/relatedTests",
    ExternalDocs = "experimental/externalDocs",
    Ssr = "experimental/ssr",
    OnEnter = "experimental/onEnter",
    OpenCargoToml = "experimental/openCargoToml",
    ViewRecursiveMemoryLayout = "rust-analyzer/viewRecursiveMemoryLayout",
    AnalyzerStatus = "rust-analyzer/analyzerStatus",
    FetchDependencyList = "rust-analyzer/fetchDependencyList",
    ReloadWorkspace = "rust-analyzer/reloadWorkspace",
    RebuildProcMacros = "rust-analyzer/rebuildProcMacros",
    ViewSyntaxTree = "rust-analyzer/viewSyntaxTree",
    ViewHir = "rust-analyzer/viewHir",
    ViewMir = "rust-analyzer/viewMir",
    ViewFileText = "rust-analyzer/viewFileText",
    ViewItemTree = "rust-analyzer/viewItemTree",
    ViewCrateGraph = "rust-analyzer/viewCrateGraph",
    ExpandMacro = "rust-analyzer/expandMacro",
    MemoryUsage = "rust-analyzer/memoryUsage",
    InterpretFunction = "rust-analyzer/interpretFunction",
    DiscoverTest = "experimental/discoverTest",
    TestPluginCustom = "fastedit-test/custom",
}

/// Declares the notifications whose params stay JSON, for [`notice_sender`] to look up by method
/// name.
macro_rules! raw_notices {
    ($($name:ident = $method:literal),* $(,)?) => {
        $(
            enum $name {}
            impl Notification for $name {
                type Params = Value;
                const METHOD: &'static str = $method;
            }
        )*

        // What sends notification `method`; none if it isn't one fe-lsp sends.
        fn notice_sender(method: &str) -> Option<(&'static str, fn(&ServerSocket, Value) -> bool)> {
            match method {
                $($method => Some(($method, |socket, params| {
                    socket.notify::<$name>(params).is_ok()
                })),)*
                _ => None,
            }
        }
    };
}

raw_notices! {
    CancelFlycheck = "rust-analyzer/cancelFlycheck",
    RunFlycheck = "rust-analyzer/runFlycheck",
    ClearFlycheck = "rust-analyzer/clearFlycheck",
    TestPluginNotice = "fastedit-test/notice",
}

/// Sends notification `method`, one of [`raw_notices!`]'s, on `socket`; false if it isn't one.
/// [`notify::send`](crate::notify::send) falls back to it, so a custom notification takes the
/// actor's `Command::Notify` path and consumes no request id.
pub(crate) fn send_notice(socket: &ServerSocket, method: &str, params: Value) -> bool {
    match notice_sender(method) {
        // If the loop has gone, `loop_done` says so.
        Some((_, send)) => {
            send(socket, params);
            true
        }
        None => false,
    }
}

/// Whether the editor runs a server named `name` without any extension.
pub fn is_builtin_server(name: &str) -> bool {
    crate::registry::builtin_specs()
        .iter()
        .any(|spec| spec.name == name)
}

fn not_sendable(method: &str) -> LspError {
    LspError::Unavailable(Unavailable::Other(format!(
        verbatim!("{} is not a method fe-lsp can send"),
        method
    )))
}

impl LspManager {
    /// Sends request `method` to the server `name` running for `root`, with `params`, and answers
    /// with its result as JSON. See the module docs.
    pub fn custom_request_at(
        &self,
        name: &str,
        root: &Path,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value, LspError>> + Send + 'static + use<> {
        self.custom_request_to(self.instance_at(name, root), method, params)
    }

    /// As [`custom_request_at`](Self::custom_request_at), to the instance of server `name` that
    /// open document `buffer` is placed on.
    pub fn custom_request_for(
        &self,
        buffer: BufferId,
        name: &str,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value, LspError>> + Send + 'static + use<> {
        self.custom_request_to(self.placed_named(buffer, name), method, params)
    }

    /// The instance of server `name` that `buffer` is placed on.
    fn placed_named(&self, buffer: BufferId, name: &str) -> Option<ServerId> {
        (self.server_names(buffer).into_iter())
            .find(|(_, server)| server == name)
            .map(|(id, _)| id)
    }

    fn custom_request_to(
        &self,
        server: Option<ServerId>,
        method: &str,
        params: Value,
    ) -> BoxFuture<'static, Result<Value, LspError>> {
        let (answer, answered) = oneshot::channel();
        let Some(issue) = request_call(method, answer, params) else {
            let err = not_sendable(method);
            return async move { Err(err) }.boxed();
        };
        let Some(server) = server else {
            return async { Err(LspError::ServerDown) }.boxed();
        };
        // A server that has gone drops the command, and `answer` with it.
        self.send(server, Command::Request(issue));
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            // None of these methods has an `lsp.timeouts` entry: the host's own deadline
            // drops the future, which cancels the request.
            None,
            &*self.clock,
            Err(LspError::Timeout),
        )
        .boxed()
    }

    /// Sends notification `method` to the server `name` running for `root`. Queued in order with
    /// the requests before it; the answer says only whether a session was there.
    pub fn custom_notify_at(
        &self,
        name: &str,
        root: &Path,
        method: &str,
        params: Value,
    ) -> Result<(), LspError> {
        self.custom_notify_to(self.instance_at(name, root), method, params)
    }

    /// As [`custom_notify_at`](Self::custom_notify_at), to the instance `buffer` is placed on.
    pub fn custom_notify_for(
        &self,
        buffer: BufferId,
        name: &str,
        method: &str,
        params: Value,
    ) -> Result<(), LspError> {
        self.custom_notify_to(self.placed_named(buffer, name), method, params)
    }

    fn custom_notify_to(
        &self,
        server: Option<ServerId>,
        method: &str,
        params: Value,
    ) -> Result<(), LspError> {
        let Some((method, _)) = notice_sender(method) else {
            return Err(not_sendable(method));
        };
        let server = server.ok_or(LspError::ServerDown)?;
        // In order with the requests and document sync queued before it, and consuming no
        // request id, as a notification does.
        if !self.try_send(server, Command::Notify { method, params }) {
            // The server's thread has ended, so the notification would be lost without a word.
            return Err(LspError::ServerDown);
        }
        Ok(())
    }
}
