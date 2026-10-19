//! Notifications to every server of a manager that concern no one
//! document (`workspace/didChangeWatchedFiles`, file operations).

use async_lsp::ServerSocket;
use lsp_types::ServerCapabilities;
use lsp_types::notification::Notification;
use serde_json::Value;

use crate::event::ServerId;
use crate::manager::LspManager;
use crate::server::Command;

/// Declares a notification whose params stay JSON, for [`send`] to look up
/// by method name (async-lsp types its notifications at compile time).
macro_rules! raw_notifications {
    ($($name:ident = $method:literal),* $(,)?) => {
        $(
            enum $name {}
            impl Notification for $name {
                type Params = Value;
                const METHOD: &'static str = $method;
            }
        )*

        /// Queues `method` on `socket`; false if it isn't one of those fe-lsp sends.
        pub(crate) fn send(socket: &ServerSocket, method: &str, params: Value) -> bool {
            match method {
                $($method => {
                    // If the loop has gone, `loop_done` says so.
                    let _ = socket.notify::<$name>(params);
                    true
                })*
                _ => {
                    log::warn!("no notification {method} to send");
                    false
                }
            }
        }
    };
}

raw_notifications! {
    DidChangeWatchedFiles = "workspace/didChangeWatchedFiles",
    DidCreateFiles = "workspace/didCreateFiles",
    DidRenameFiles = "workspace/didRenameFiles",
    DidDeleteFiles = "workspace/didDeleteFiles",
}

impl LspManager {
    /// Sends `method` to each running server of this manager, in id order,
    /// with the params `params_for` gives it (none: nothing is sent). The
    /// notification keeps its order with the requests and document sync
    /// queued before it. A server that hasn't initialized gets nothing.
    pub(crate) fn notify_sessions(
        &self,
        method: &'static str,
        params_for: impl Fn(ServerId, &ServerCapabilities) -> Option<Value>,
    ) {
        for id in self.server_ids() {
            let Some(capabilities) = self.capabilities(id) else {
                continue;
            };
            let Some(params) = params_for(id, &capabilities) else {
                continue;
            };
            // A server that has gone has nothing to be told.
            self.send(id, Command::Notify { method, params });
        }
    }
}
