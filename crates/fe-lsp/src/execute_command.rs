//! `workspace/executeCommand` for a host, on the server running for a
//! root (debugger D21: jdtls's `vscode.java.startDebugSession`,
//! `vscode.java.resolveMainClass` and `vscode.java.resolveClasspath`).
//!
//! - **Which server.** The newest instance of the server named, running
//!   for the root given, whether [`open`](LspManager::open) routed
//!   documents to it or the host [`start`](LspManager::start)ed it. None
//!   running there answers [`LspError::ServerDown`].
//! - **Only its own commands.** A command the server doesn't list in its
//!   `executeCommandProvider.commands` is refused with `Unavailable`,
//!   without a request: at once if the server has initialized, else as
//!   the request would be sent, once it has, against the capabilities of
//!   the session it would go to. fe-lsp doesn't offer dynamic registration
//!   of `workspace/executeCommand`, so a server lists every command there
//!   (jdtls its bundles' too).
//! - **The reply** is the command's result as JSON, `null` included.
//!   It has no `lsp.timeouts` entry: dropping the future cancels it.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use futures::FutureExt;
use futures::channel::oneshot;
use lsp_types::request::{ExecuteCommand, Request};
use lsp_types::{ExecuteCommandOptions, ServerCapabilities};
use serde_json::{Value, json};

use crate::event::LspError;
use crate::manager::LspManager;
use crate::requests::{Reply, call, within};
use crate::server::{Command, WithDocuments};

/// `workspace/executeCommand` with its result as JSON: lsp-types reads a
/// `null` result as `None`.
enum Execute {}

impl Request for Execute {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = ExecuteCommand::METHOD;
}

impl LspManager {
    /// Has the server `name` running for `root` run `command` with
    /// `arguments` (`workspace/executeCommand`), answering with its result
    /// as JSON. See the module docs: `ServerDown` with no instance there,
    /// `Unavailable` for a command the server doesn't list in
    /// `executeCommandProvider.commands` (nothing is sent), and otherwise
    /// as [`request`](Self::request) answers. The future may be awaited
    /// anywhere; dropping it cancels the request.
    pub fn execute_command_at(
        &self,
        name: &str,
        root: &Path,
        command: &str,
        arguments: Vec<Value>,
    ) -> impl Future<Output = Result<Value, LspError>> + Send + 'static + use<> {
        let Some(server) = self.instance_at(name, root) else {
            return async { Err(LspError::ServerDown) }.boxed();
        };
        let Some(session) = self.shared_session(server) else {
            return async { Err(LspError::ServerDown) }.boxed();
        };
        let command = command.to_owned();
        if let Some(capabilities) = self.capabilities(server)
            && !lists(&capabilities, &command)
        {
            let err = not_listed(&command);
            return async move { Err(err) }.boxed();
        }
        let (answer, answered) = oneshot::channel();
        let admit: WithDocuments = Box::new(move |opened| {
            if let Err(err) = opened {
                let _ = answer.send(Err(err));
                return None;
            }
            // The session it would go to, which has initialized: commands
            // wait for that.
            let listed = (lock(&session).capabilities.as_ref())
                .is_some_and(|capabilities| lists(capabilities, &command));
            if !listed {
                let _ = answer.send(Err(not_listed(&command)));
                return None;
            }
            let params = json!({ "command": command, "arguments": arguments });
            Some(call::<Execute, _>(answer, params, |_, reply| {
                reply.map(|Reply { result, .. }| result)
            }))
        });
        // A server that has gone drops the command, and `answer` with it.
        self.send(server, Command::WithDocuments(admit));
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            self.timeout::<Execute>(),
            &*self.clock,
            Err(LspError::Timeout),
        )
        .boxed()
    }
}

/// Whether `capabilities` list `command` in `executeCommandProvider`.
fn lists(capabilities: &ServerCapabilities, command: &str) -> bool {
    (capabilities.execute_command_provider.as_ref())
        .is_some_and(|ExecuteCommandOptions { commands, .. }| commands.iter().any(|c| c == command))
}

fn not_listed(command: &str) -> LspError {
    LspError::Unavailable(
        kiln_l10n::tr!("lsp-not-a-server-command", command = command)
            .to_string()
            .into(),
    )
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
