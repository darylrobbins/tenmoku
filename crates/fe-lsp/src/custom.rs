//! Requests and notifications a host sends a server by method name, for the methods a server
//! defines beyond the LSP's own (rust-analyzer's `experimental/runnables`, say), used by plugins
//! (`lsp.request`, `lsp.notify`).
//!
//! - **Which session.** By server name and root ([`custom_request_at`](LspManager::custom_request_at))
//!   or by the server an open document is placed on ([`custom_request_for`](LspManager::custom_request_for)):
//!   never by name alone. None running there answers [`LspError::ServerDown`]; nothing is started.
//! - **Any method.** The method is sent as named, through async-lsp's untyped send
//!   (vendor/async-lsp/README.md). Which methods a plugin may send is fe-plugin's to judge
//!   (`is_routed`); fe-lsp adds no gate of its own.
//! - **In order.** A request is issued by the server's actor with the next mirrored id, and a
//!   notification rides `Command::Notify`, in order with the requests and document sync queued
//!   before it and inside the actor's window, consuming no request id.
//! - **The reply** is the result as JSON, `null` included. The caller's deadline is its own:
//!   dropping the future cancels the request with `$/cancelRequest` by its wire id.

use std::borrow::Cow;
use std::path::Path;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use serde_json::Value;

use fe_buffer::BufferId;

use crate::event::{LspError, ServerId};
use crate::manager::LspManager;
use crate::requests::{Reply, call_raw, within};
use crate::server::Command;

/// Whether the editor runs a server named `name` without any extension.
pub fn is_builtin_server(name: &str) -> bool {
    crate::registry::builtin_specs()
        .iter()
        .any(|spec| spec.name == name)
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
        let Some(server) = server else {
            return async { Err(LspError::ServerDown) }.boxed();
        };
        let (answer, answered) = oneshot::channel();
        let wrap = |_, reply: Result<Reply<Value>, LspError>| reply.map(|reply| reply.result);
        let issue = call_raw(answer, method.to_owned(), params, wrap);
        // A server that has gone drops the command, and `answer` with it.
        self.send(server, Command::Request(issue));
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            // No method a plugin may send has an `lsp.timeouts` entry (fe-plugin denies the
            // methods the table names): the host's own deadline drops the future, which cancels
            // the request.
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
        let server = server.ok_or(LspError::ServerDown)?;
        // In order with the requests and document sync queued before it, and consuming no
        // request id, as a notification does.
        let method = Cow::Owned(method.to_owned());
        if !self.try_send(server, Command::Notify { method, params }) {
            // The server's thread has ended, so the notification would be lost without a word.
            return Err(LspError::ServerDown);
        }
        Ok(())
    }
}
